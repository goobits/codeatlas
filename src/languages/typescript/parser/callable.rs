use super::callable_effects::{collect_arrow_direct_effects, collect_direct_effects};
use super::format::{format_entity_name, format_ts_type};
use crate::domain::{
    CallableBody, CallableContract, CallableKind, CallableParameter, CallableSignature,
    Constructibility, ParameterRequirement, ParameterRole, ReceiverContract, ReceiverRequirement,
    SemanticField, SemanticLiteral, SemanticType, StringEncoding, TypeParameterContract,
    TypeParameterKind, TypeUnknownReason,
};
use std::collections::BTreeSet;
use swc_core::ecma::ast::*;

const MAX_TYPE_DEPTH: usize = 16;

pub(super) fn function_contract(
    function: &Function,
    kind: CallableKind,
    receiver: ReceiverContract,
) -> CallableContract {
    let generic_names = generic_names(function.type_params.as_deref());
    let signature = CallableSignature {
        kind,
        body: if function.body.is_some() {
            CallableBody::Present
        } else {
            CallableBody::DeclarationOnly
        },
        is_async: function.is_async,
        receiver,
        type_parameters: type_parameters(function.type_params.as_deref(), &generic_names),
        parameters: function
            .params
            .iter()
            .enumerate()
            .map(|(position, parameter)| {
                parameter_contract(&parameter.pat, position, &generic_names)
            })
            .collect(),
        result: result_type(function.return_type.as_deref(), &generic_names),
    };
    CallableContract::new(
        [signature],
        function
            .body
            .as_ref()
            .map(collect_direct_effects)
            .unwrap_or_default(),
    )
}

pub(super) fn arrow_contract(arrow: &ArrowExpr) -> CallableContract {
    let generic_names = generic_names(arrow.type_params.as_deref());
    let signature = CallableSignature {
        kind: CallableKind::Function,
        body: CallableBody::Present,
        is_async: arrow.is_async,
        receiver: ReceiverContract::none(),
        type_parameters: type_parameters(arrow.type_params.as_deref(), &generic_names),
        parameters: arrow
            .params
            .iter()
            .enumerate()
            .map(|(position, parameter)| parameter_contract(parameter, position, &generic_names))
            .collect(),
        result: result_type(arrow.return_type.as_deref(), &generic_names),
    };
    CallableContract::new([signature], collect_arrow_direct_effects(arrow))
}

pub(super) fn constructor_contract(constructor: &Constructor) -> CallableContract {
    let parameters = constructor
        .params
        .iter()
        .enumerate()
        .map(|(position, parameter)| match parameter {
            ParamOrTsParamProp::Param(parameter) => {
                parameter_contract(&parameter.pat, position, &BTreeSet::new())
            }
            ParamOrTsParamProp::TsParamProp(property) => match &property.param {
                TsParamPropParam::Ident(identifier) => {
                    identifier_parameter(identifier, position, &BTreeSet::new())
                }
                TsParamPropParam::Assign(assignment) => {
                    let mut parameter =
                        parameter_contract(&assignment.left, position, &BTreeSet::new());
                    parameter.requirement = ParameterRequirement::Defaulted;
                    parameter
                }
            },
        })
        .collect();
    CallableContract::new(
        [CallableSignature {
            kind: CallableKind::Constructor,
            body: if constructor.body.is_some() {
                CallableBody::Present
            } else {
                CallableBody::DeclarationOnly
            },
            is_async: false,
            receiver: ReceiverContract::none(),
            type_parameters: Vec::new(),
            parameters,
            result: SemanticType::Named {
                identity: "Self".to_string(),
                arguments: Vec::new(),
            },
        }],
        constructor
            .body
            .as_ref()
            .map(collect_direct_effects)
            .unwrap_or_default(),
    )
}

pub(super) fn method_signature_contract(method: &TsMethodSignature) -> CallableContract {
    let generic_names = generic_names(method.type_params.as_deref());
    let signature = CallableSignature {
        kind: CallableKind::Method,
        body: CallableBody::DeclarationOnly,
        is_async: false,
        receiver: instance_receiver(),
        type_parameters: type_parameters(method.type_params.as_deref(), &generic_names),
        parameters: method
            .params
            .iter()
            .enumerate()
            .map(|(position, parameter)| {
                function_type_parameter(parameter, position, &generic_names)
            })
            .collect(),
        result: result_type(method.type_ann.as_deref(), &generic_names),
    };
    CallableContract::new([signature], [])
}

pub(super) fn getter_signature_contract(getter: &TsGetterSignature) -> CallableContract {
    CallableContract::new(
        [CallableSignature {
            kind: CallableKind::Getter,
            body: CallableBody::DeclarationOnly,
            is_async: false,
            receiver: instance_receiver(),
            type_parameters: Vec::new(),
            parameters: Vec::new(),
            result: result_type(getter.type_ann.as_deref(), &BTreeSet::new()),
        }],
        [],
    )
}

pub(super) fn setter_signature_contract(setter: &TsSetterSignature) -> CallableContract {
    CallableContract::new(
        [CallableSignature {
            kind: CallableKind::Setter,
            body: CallableBody::DeclarationOnly,
            is_async: false,
            receiver: instance_receiver(),
            type_parameters: Vec::new(),
            parameters: vec![function_type_parameter(&setter.param, 0, &BTreeSet::new())],
            result: SemanticType::Unit,
        }],
        [],
    )
}

pub(super) fn receiver(is_static: bool) -> ReceiverContract {
    if is_static {
        ReceiverContract::none()
    } else {
        instance_receiver()
    }
}

fn instance_receiver() -> ReceiverContract {
    ReceiverContract {
        requirement: ReceiverRequirement::Instance,
        constructibility: Constructibility::RequiresFactory,
    }
}

fn parameter_contract(
    pattern: &Pat,
    position: usize,
    generic_names: &BTreeSet<String>,
) -> CallableParameter {
    match pattern {
        Pat::Ident(identifier) => identifier_parameter(identifier, position, generic_names),
        Pat::Assign(assignment) => {
            let mut parameter = parameter_contract(&assignment.left, position, generic_names);
            parameter.requirement = ParameterRequirement::Defaulted;
            parameter
        }
        Pat::Rest(rest) => {
            let semantic_type = rest.type_ann.as_deref().map_or_else(
                || pattern_semantic_type(&rest.arg, generic_names),
                |annotation| semantic_type(&annotation.type_ann, generic_names, 0),
            );
            CallableParameter {
                position,
                name: pattern_name(&rest.arg),
                role: ParameterRole::VariadicPositional,
                requirement: ParameterRequirement::Optional,
                constructibility: semantic_type.constructibility(),
                semantic_type,
            }
        }
        Pat::Array(array) => pattern_parameter(
            array.type_ann.as_deref(),
            array.optional,
            position,
            generic_names,
            "array-pattern",
        ),
        Pat::Object(object) => pattern_parameter(
            object.type_ann.as_deref(),
            object.optional,
            position,
            generic_names,
            "object-pattern",
        ),
        Pat::Invalid(_) | Pat::Expr(_) => unknown_parameter(position, "parameter-pattern"),
    }
}

fn identifier_parameter(
    identifier: &BindingIdent,
    position: usize,
    generic_names: &BTreeSet<String>,
) -> CallableParameter {
    let semantic_type = identifier.type_ann.as_deref().map_or_else(
        || {
            SemanticType::unknown(
                TypeUnknownReason::MissingAnnotation,
                identifier.id.sym.to_string(),
            )
        },
        |annotation| semantic_type(&annotation.type_ann, generic_names, 0),
    );
    CallableParameter {
        position,
        name: Some(identifier.id.sym.to_string()),
        role: ParameterRole::Positional,
        requirement: if identifier.optional {
            ParameterRequirement::Optional
        } else {
            ParameterRequirement::Required
        },
        constructibility: semantic_type.constructibility(),
        semantic_type,
    }
}

fn pattern_parameter(
    annotation: Option<&TsTypeAnn>,
    optional: bool,
    position: usize,
    generic_names: &BTreeSet<String>,
    display: &str,
) -> CallableParameter {
    let semantic_type = annotation.map_or_else(
        || SemanticType::unknown(TypeUnknownReason::UnsupportedPattern, display),
        |annotation| semantic_type(&annotation.type_ann, generic_names, 0),
    );
    CallableParameter {
        position,
        name: None,
        role: ParameterRole::Positional,
        requirement: if optional {
            ParameterRequirement::Optional
        } else {
            ParameterRequirement::Required
        },
        constructibility: semantic_type.constructibility(),
        semantic_type,
    }
}

fn function_type_parameter(
    parameter: &TsFnParam,
    position: usize,
    generic_names: &BTreeSet<String>,
) -> CallableParameter {
    match parameter {
        TsFnParam::Ident(identifier) => identifier_parameter(identifier, position, generic_names),
        TsFnParam::Rest(rest) => {
            let semantic_type = rest.type_ann.as_deref().map_or_else(
                || pattern_semantic_type(&rest.arg, generic_names),
                |annotation| semantic_type(&annotation.type_ann, generic_names, 0),
            );
            CallableParameter {
                position,
                name: pattern_name(&rest.arg),
                role: ParameterRole::VariadicPositional,
                requirement: ParameterRequirement::Optional,
                constructibility: semantic_type.constructibility(),
                semantic_type,
            }
        }
        TsFnParam::Array(array) => pattern_parameter(
            array.type_ann.as_deref(),
            array.optional,
            position,
            generic_names,
            "array-pattern",
        ),
        TsFnParam::Object(object) => pattern_parameter(
            object.type_ann.as_deref(),
            object.optional,
            position,
            generic_names,
            "object-pattern",
        ),
    }
}

fn unknown_parameter(position: usize, display: &str) -> CallableParameter {
    let semantic_type = SemanticType::unknown(TypeUnknownReason::UnsupportedPattern, display);
    CallableParameter {
        position,
        name: None,
        role: ParameterRole::Positional,
        requirement: ParameterRequirement::Required,
        constructibility: semantic_type.constructibility(),
        semantic_type,
    }
}

fn pattern_semantic_type(pattern: &Pat, generic_names: &BTreeSet<String>) -> SemanticType {
    match pattern {
        Pat::Ident(identifier) => identifier.type_ann.as_deref().map_or_else(
            || {
                SemanticType::unknown(
                    TypeUnknownReason::MissingAnnotation,
                    identifier.id.sym.to_string(),
                )
            },
            |annotation| semantic_type(&annotation.type_ann, generic_names, 0),
        ),
        Pat::Array(array) => array.type_ann.as_deref().map_or_else(
            || SemanticType::unknown(TypeUnknownReason::UnsupportedPattern, "array-pattern"),
            |annotation| semantic_type(&annotation.type_ann, generic_names, 0),
        ),
        Pat::Object(object) => object.type_ann.as_deref().map_or_else(
            || SemanticType::unknown(TypeUnknownReason::UnsupportedPattern, "object-pattern"),
            |annotation| semantic_type(&annotation.type_ann, generic_names, 0),
        ),
        Pat::Assign(assignment) => pattern_semantic_type(&assignment.left, generic_names),
        Pat::Rest(rest) => rest.type_ann.as_deref().map_or_else(
            || pattern_semantic_type(&rest.arg, generic_names),
            |annotation| semantic_type(&annotation.type_ann, generic_names, 0),
        ),
        Pat::Invalid(_) | Pat::Expr(_) => {
            SemanticType::unknown(TypeUnknownReason::UnsupportedPattern, "parameter-pattern")
        }
    }
}

fn pattern_name(pattern: &Pat) -> Option<String> {
    match pattern {
        Pat::Ident(identifier) => Some(identifier.id.sym.to_string()),
        Pat::Assign(assignment) => pattern_name(&assignment.left),
        Pat::Rest(rest) => pattern_name(&rest.arg),
        _ => None,
    }
}

fn result_type(annotation: Option<&TsTypeAnn>, generic_names: &BTreeSet<String>) -> SemanticType {
    annotation.map_or_else(
        || SemanticType::unknown(TypeUnknownReason::MissingAnnotation, "return"),
        |annotation| semantic_type(&annotation.type_ann, generic_names, 0),
    )
}

fn semantic_type(
    value_type: &TsType,
    generic_names: &BTreeSet<String>,
    depth: usize,
) -> SemanticType {
    if depth >= MAX_TYPE_DEPTH {
        return SemanticType::unknown(
            TypeUnknownReason::UnboundedRecursive,
            format_ts_type(value_type),
        );
    }
    match value_type {
        TsType::TsKeywordType(keyword) => semantic_keyword(keyword.kind),
        TsType::TsTypeRef(reference) => {
            let identity = format_entity_name(&reference.type_name);
            if reference.type_params.is_none() && generic_names.contains(&identity) {
                return SemanticType::TypeParameter { name: identity };
            }
            let arguments = reference
                .type_params
                .as_deref()
                .map(|parameters| {
                    parameters
                        .params
                        .iter()
                        .map(|value| semantic_type(value, generic_names, depth + 1))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            semantic_reference(identity, arguments)
        }
        TsType::TsArrayType(array) => SemanticType::List {
            value: Box::new(semantic_type(&array.elem_type, generic_names, depth + 1)),
            min_items: None,
            max_items: None,
        },
        TsType::TsTupleType(tuple) => SemanticType::Tuple {
            values: tuple
                .elem_types
                .iter()
                .map(|element| semantic_type(&element.ty, generic_names, depth + 1))
                .collect(),
        },
        TsType::TsOptionalType(optional) => SemanticType::Optional {
            value: Box::new(semantic_type(&optional.type_ann, generic_names, depth + 1)),
        },
        TsType::TsRestType(rest) => SemanticType::List {
            value: Box::new(semantic_type(&rest.type_ann, generic_names, depth + 1)),
            min_items: None,
            max_items: None,
        },
        TsType::TsUnionOrIntersectionType(TsUnionOrIntersectionType::TsUnionType(union)) => {
            SemanticType::union(
                union
                    .types
                    .iter()
                    .map(|value| semantic_type(value, generic_names, depth + 1)),
            )
        }
        TsType::TsTypeLit(literal) => semantic_record(&literal.members, generic_names, depth + 1),
        TsType::TsLitType(literal) => semantic_literal(&literal.lit),
        TsType::TsParenthesizedType(parenthesized) => {
            semantic_type(&parenthesized.type_ann, generic_names, depth + 1)
        }
        TsType::TsTypeOperator(operator) if operator.op == TsTypeOperatorOp::ReadOnly => {
            semantic_type(&operator.type_ann, generic_names, depth + 1)
        }
        TsType::TsThisType(_) => SemanticType::Named {
            identity: "this".to_string(),
            arguments: Vec::new(),
        },
        _ => SemanticType::unknown(TypeUnknownReason::Unsupported, format_ts_type(value_type)),
    }
}

fn semantic_keyword(kind: TsKeywordTypeKind) -> SemanticType {
    match kind {
        TsKeywordTypeKind::TsBooleanKeyword => SemanticType::Boolean,
        TsKeywordTypeKind::TsNumberKeyword => SemanticType::Float {
            bits: Some(64),
            allows_special: true,
        },
        TsKeywordTypeKind::TsBigIntKeyword => SemanticType::Integer {
            signed: Some(true),
            bits: None,
        },
        TsKeywordTypeKind::TsStringKeyword => SemanticType::String {
            encoding: StringEncoding::Utf16,
            max_length: None,
        },
        TsKeywordTypeKind::TsNullKeyword | TsKeywordTypeKind::TsUndefinedKeyword => {
            SemanticType::Null
        }
        TsKeywordTypeKind::TsVoidKeyword => SemanticType::Unit,
        _ => SemanticType::unknown(TypeUnknownReason::Unsupported, format!("{kind:?}")),
    }
}

fn semantic_reference(identity: String, arguments: Vec<SemanticType>) -> SemanticType {
    let base = identity.rsplit('.').next().unwrap_or(identity.as_str());
    match base {
        "Array" | "ReadonlyArray" if arguments.len() == 1 => SemanticType::List {
            value: Box::new(arguments.into_iter().next().expect("one array argument")),
            min_items: None,
            max_items: None,
        },
        "Set" | "ReadonlySet" if arguments.len() == 1 => SemanticType::Set {
            value: Box::new(arguments.into_iter().next().expect("one set argument")),
            max_items: None,
        },
        "Map" | "ReadonlyMap" | "Record" if arguments.len() == 2 => {
            let mut arguments = arguments.into_iter();
            SemanticType::Map {
                key: Box::new(arguments.next().expect("map key argument")),
                value: Box::new(arguments.next().expect("map value argument")),
                max_items: None,
            }
        }
        _ => SemanticType::Named {
            identity,
            arguments,
        },
    }
}

fn semantic_record(
    members: &[TsTypeElement],
    generic_names: &BTreeSet<String>,
    depth: usize,
) -> SemanticType {
    let mut fields = Vec::new();
    for member in members {
        let TsTypeElement::TsPropertySignature(property) = member else {
            return SemanticType::unknown(TypeUnknownReason::Unsupported, "type-literal-member");
        };
        let Some(name) = property
            .key
            .as_ident()
            .map(|identifier| identifier.sym.to_string())
        else {
            return SemanticType::unknown(TypeUnknownReason::Unsupported, "computed-record-field");
        };
        let semantic_type = property.type_ann.as_deref().map_or_else(
            || SemanticType::unknown(TypeUnknownReason::MissingAnnotation, name.clone()),
            |annotation| semantic_type(&annotation.type_ann, generic_names, depth + 1),
        );
        fields.push(SemanticField {
            name,
            required: !property.optional,
            semantic_type,
        });
    }
    SemanticType::Record { fields }
}

fn semantic_literal(literal: &TsLit) -> SemanticType {
    let value = match literal {
        TsLit::Str(value) => SemanticLiteral::String(value.value.to_string()),
        TsLit::Number(value) => SemanticLiteral::Float(value.value.to_string()),
        TsLit::Bool(value) => SemanticLiteral::Boolean(value.value),
        TsLit::BigInt(value) => SemanticLiteral::Integer(value.value.to_string()),
        TsLit::Tpl(_) => {
            return SemanticType::unknown(TypeUnknownReason::Unsupported, "template-literal")
        }
    };
    SemanticType::Literal { value }
}

fn generic_names(parameters: Option<&TsTypeParamDecl>) -> BTreeSet<String> {
    parameters
        .into_iter()
        .flat_map(|parameters| &parameters.params)
        .map(|parameter| parameter.name.sym.to_string())
        .collect()
}

fn type_parameters(
    parameters: Option<&TsTypeParamDecl>,
    generic_names: &BTreeSet<String>,
) -> Vec<TypeParameterContract> {
    parameters
        .into_iter()
        .flat_map(|parameters| &parameters.params)
        .map(|parameter| TypeParameterContract {
            name: parameter.name.sym.to_string(),
            kind: TypeParameterKind::Type,
            constraints: parameter
                .constraint
                .as_deref()
                .map(|constraint| vec![semantic_type(constraint, generic_names, 0)])
                .unwrap_or_default(),
            default: parameter
                .default
                .as_deref()
                .map(|default| semantic_type(default, generic_names, 0)),
        })
        .collect()
}

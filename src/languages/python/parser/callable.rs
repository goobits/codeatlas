use super::callable_effects::collect_direct_effects;
use super::format_py_expr;
use crate::domain::{
    CallableBody, CallableContract, CallableKind, CallableParameter, CallableSignature,
    Constructibility, ParameterRequirement, ParameterRole, ReceiverContract, ReceiverRequirement,
    SemanticLiteral, SemanticType, StringEncoding, TypeParameterContract, TypeParameterKind,
    TypeUnknownReason,
};
use rustpython_parser::ast;
use std::collections::BTreeSet;

const MAX_TYPE_DEPTH: usize = 16;

pub(super) struct PythonCallable<'a> {
    pub args: &'a ast::Arguments,
    pub returns: &'a Option<Box<ast::Expr>>,
    pub type_parameters: &'a [ast::TypeParam],
    pub decorators: &'a [ast::Expr],
    pub body: &'a [ast::Stmt],
    pub is_async: bool,
    pub is_class_member: bool,
    pub is_declaration_file: bool,
}

pub(super) fn contract(callable: PythonCallable<'_>) -> CallableContract {
    let generic_names = callable
        .type_parameters
        .iter()
        .map(type_parameter_name)
        .collect::<BTreeSet<_>>();
    let (kind, mut receiver, consumes_receiver) = receiver_contract(&callable);
    let mut parameters = Vec::new();
    let mut receiver_consumed = false;
    for argument in &callable.args.posonlyargs {
        if consumes_receiver && !receiver_consumed {
            receiver_consumed = true;
            continue;
        }
        push_parameter(
            &mut parameters,
            &argument.def,
            ParameterRole::PositionalOnly,
            argument.default.is_some(),
            &generic_names,
        );
    }
    for argument in &callable.args.args {
        if consumes_receiver && !receiver_consumed {
            receiver_consumed = true;
            continue;
        }
        push_parameter(
            &mut parameters,
            &argument.def,
            ParameterRole::PositionalOrNamed,
            argument.default.is_some(),
            &generic_names,
        );
    }
    if consumes_receiver && !receiver_consumed {
        receiver.requirement = ReceiverRequirement::Unknown;
        receiver.constructibility = Constructibility::Unknown;
    }
    if let Some(argument) = &callable.args.vararg {
        push_variadic_parameter(
            &mut parameters,
            argument,
            ParameterRole::VariadicPositional,
            &generic_names,
        );
    }
    for argument in &callable.args.kwonlyargs {
        push_parameter(
            &mut parameters,
            &argument.def,
            ParameterRole::NamedOnly,
            argument.default.is_some(),
            &generic_names,
        );
    }
    if let Some(argument) = &callable.args.kwarg {
        push_variadic_parameter(
            &mut parameters,
            argument,
            ParameterRole::VariadicNamed,
            &generic_names,
        );
    }
    let result = callable.returns.as_deref().map_or_else(
        || SemanticType::unknown(TypeUnknownReason::MissingAnnotation, "return"),
        |value| semantic_type(value, &generic_names, 0),
    );
    let signature = CallableSignature {
        kind,
        body: if callable.is_declaration_file || is_declaration_body(callable.body) {
            CallableBody::DeclarationOnly
        } else {
            CallableBody::Present
        },
        is_async: callable.is_async,
        receiver,
        type_parameters: callable
            .type_parameters
            .iter()
            .map(|parameter| type_parameter(parameter, &generic_names))
            .collect(),
        parameters,
        result,
    };
    CallableContract::new([signature], collect_direct_effects(callable.body))
}

fn receiver_contract(callable: &PythonCallable<'_>) -> (CallableKind, ReceiverContract, bool) {
    if !callable.is_class_member {
        return (CallableKind::Function, ReceiverContract::none(), false);
    }
    let decorators = callable
        .decorators
        .iter()
        .filter_map(qualified_name)
        .collect::<BTreeSet<_>>();
    if decorators.contains("staticmethod") {
        return (CallableKind::Method, ReceiverContract::none(), false);
    }
    let kind = if decorators.contains("property") {
        CallableKind::Getter
    } else if decorators.iter().any(|name| name.ends_with(".setter")) {
        CallableKind::Setter
    } else {
        CallableKind::Method
    };
    if decorators.contains("classmethod") {
        return (
            kind,
            ReceiverContract {
                requirement: ReceiverRequirement::Type,
                constructibility: Constructibility::Direct,
            },
            true,
        );
    }
    (
        kind,
        ReceiverContract {
            requirement: ReceiverRequirement::Instance,
            constructibility: Constructibility::RequiresFactory,
        },
        true,
    )
}

fn push_parameter(
    parameters: &mut Vec<CallableParameter>,
    argument: &ast::Arg,
    role: ParameterRole,
    has_default: bool,
    generic_names: &BTreeSet<String>,
) {
    let semantic_type = argument.annotation.as_deref().map_or_else(
        || SemanticType::unknown(TypeUnknownReason::MissingAnnotation, argument.arg.as_str()),
        |value| semantic_type(value, generic_names, 0),
    );
    parameters.push(CallableParameter {
        position: parameters.len(),
        name: Some(argument.arg.as_str().to_string()),
        role,
        requirement: if has_default {
            ParameterRequirement::Defaulted
        } else {
            ParameterRequirement::Required
        },
        constructibility: semantic_type.constructibility(),
        semantic_type,
    });
}

fn push_variadic_parameter(
    parameters: &mut Vec<CallableParameter>,
    argument: &ast::Arg,
    role: ParameterRole,
    generic_names: &BTreeSet<String>,
) {
    let semantic_type = argument.annotation.as_deref().map_or_else(
        || SemanticType::unknown(TypeUnknownReason::MissingAnnotation, argument.arg.as_str()),
        |value| semantic_type(value, generic_names, 0),
    );
    parameters.push(CallableParameter {
        position: parameters.len(),
        name: Some(argument.arg.as_str().to_string()),
        role,
        requirement: ParameterRequirement::Optional,
        constructibility: semantic_type.constructibility(),
        semantic_type,
    });
}

fn semantic_type(
    expression: &ast::Expr,
    generic_names: &BTreeSet<String>,
    depth: usize,
) -> SemanticType {
    if depth >= MAX_TYPE_DEPTH {
        return SemanticType::unknown(
            TypeUnknownReason::UnboundedRecursive,
            format_py_expr(expression),
        );
    }
    match expression {
        ast::Expr::Name(name) => semantic_name(name.id.as_str(), generic_names),
        ast::Expr::Attribute(_) => SemanticType::Named {
            identity: qualified_name(expression).unwrap_or_else(|| format_py_expr(expression)),
            arguments: Vec::new(),
        },
        ast::Expr::Subscript(subscript) => semantic_subscript(subscript, generic_names, depth + 1),
        ast::Expr::BinOp(binary) if binary.op == ast::Operator::BitOr => SemanticType::union([
            semantic_type(&binary.left, generic_names, depth + 1),
            semantic_type(&binary.right, generic_names, depth + 1),
        ]),
        ast::Expr::Tuple(tuple) => SemanticType::Tuple {
            values: tuple
                .elts
                .iter()
                .map(|value| semantic_type(value, generic_names, depth + 1))
                .collect(),
        },
        ast::Expr::Constant(constant) => semantic_constant(&constant.value),
        _ => SemanticType::unknown(TypeUnknownReason::Unsupported, format_py_expr(expression)),
    }
}

fn semantic_name(name: &str, generic_names: &BTreeSet<String>) -> SemanticType {
    if generic_names.contains(name) {
        return SemanticType::TypeParameter {
            name: name.to_string(),
        };
    }
    match name {
        "None" => SemanticType::Null,
        "bool" => SemanticType::Boolean,
        "int" => SemanticType::Integer {
            signed: Some(true),
            bits: None,
        },
        "float" => SemanticType::Float {
            bits: Some(64),
            allows_special: true,
        },
        "str" => SemanticType::String {
            encoding: StringEncoding::Unicode,
            max_length: None,
        },
        "bytes" | "bytearray" => SemanticType::Bytes { max_length: None },
        "Any" => SemanticType::unknown(TypeUnknownReason::Unsupported, name),
        "Never" | "NoReturn" | "Callable" => {
            SemanticType::unknown(TypeUnknownReason::Unsupported, name)
        }
        _ => SemanticType::Named {
            identity: name.to_string(),
            arguments: Vec::new(),
        },
    }
}

fn semantic_subscript(
    subscript: &ast::ExprSubscript,
    generic_names: &BTreeSet<String>,
    depth: usize,
) -> SemanticType {
    let identity =
        qualified_name(&subscript.value).unwrap_or_else(|| format_py_expr(&subscript.value));
    let arguments = expression_arguments(&subscript.slice)
        .into_iter()
        .map(|value| semantic_type(value, generic_names, depth + 1))
        .collect::<Vec<_>>();
    let base = identity.rsplit('.').next().unwrap_or(identity.as_str());
    match base {
        "Optional" if arguments.len() == 1 => SemanticType::Optional {
            value: Box::new(arguments.into_iter().next().expect("one optional argument")),
        },
        "Union" => SemanticType::union(arguments),
        "Literal" => SemanticType::union(arguments),
        "list" | "List" if arguments.len() == 1 => SemanticType::List {
            value: Box::new(arguments.into_iter().next().expect("one list argument")),
            min_items: None,
            max_items: None,
        },
        "set" | "Set" | "frozenset" | "FrozenSet" if arguments.len() == 1 => SemanticType::Set {
            value: Box::new(arguments.into_iter().next().expect("one set argument")),
            max_items: None,
        },
        "dict" | "Dict" if arguments.len() == 2 => {
            let mut arguments = arguments.into_iter();
            SemanticType::Map {
                key: Box::new(arguments.next().expect("map key argument")),
                value: Box::new(arguments.next().expect("map value argument")),
                max_items: None,
            }
        }
        "tuple" | "Tuple" => SemanticType::Tuple { values: arguments },
        "Annotated" if !arguments.is_empty() => {
            arguments.into_iter().next().expect("annotated value")
        }
        "Callable" => SemanticType::unknown(TypeUnknownReason::Unsupported, identity),
        _ => SemanticType::Named {
            identity,
            arguments,
        },
    }
}

fn semantic_constant(value: &ast::Constant) -> SemanticType {
    match value {
        ast::Constant::None => SemanticType::Null,
        ast::Constant::Bool(value) => SemanticType::Literal {
            value: SemanticLiteral::Boolean(*value),
        },
        ast::Constant::Str(value) => SemanticType::Literal {
            value: SemanticLiteral::String(value.clone()),
        },
        ast::Constant::Int(value) => SemanticType::Literal {
            value: SemanticLiteral::Integer(value.to_string()),
        },
        ast::Constant::Float(value) => SemanticType::Literal {
            value: SemanticLiteral::Float(value.to_string()),
        },
        _ => SemanticType::unknown(TypeUnknownReason::Unsupported, "literal"),
    }
}

fn expression_arguments(expression: &ast::Expr) -> Vec<&ast::Expr> {
    match expression {
        ast::Expr::Tuple(tuple) => tuple.elts.iter().collect(),
        _ => vec![expression],
    }
}

fn type_parameter(
    parameter: &ast::TypeParam,
    generic_names: &BTreeSet<String>,
) -> TypeParameterContract {
    match parameter {
        ast::TypeParam::TypeVar(parameter) => TypeParameterContract {
            name: parameter.name.as_str().to_string(),
            kind: TypeParameterKind::Type,
            constraints: parameter
                .bound
                .as_deref()
                .map(|bound| vec![semantic_type(bound, generic_names, 0)])
                .unwrap_or_default(),
            default: None,
        },
        ast::TypeParam::ParamSpec(parameter) => TypeParameterContract {
            name: parameter.name.as_str().to_string(),
            kind: TypeParameterKind::ParameterSpec,
            constraints: Vec::new(),
            default: None,
        },
        ast::TypeParam::TypeVarTuple(parameter) => TypeParameterContract {
            name: parameter.name.as_str().to_string(),
            kind: TypeParameterKind::Variadic,
            constraints: Vec::new(),
            default: None,
        },
    }
}

fn type_parameter_name(parameter: &ast::TypeParam) -> String {
    match parameter {
        ast::TypeParam::TypeVar(parameter) => parameter.name.as_str().to_string(),
        ast::TypeParam::ParamSpec(parameter) => parameter.name.as_str().to_string(),
        ast::TypeParam::TypeVarTuple(parameter) => parameter.name.as_str().to_string(),
    }
}

pub(super) fn qualified_name(expression: &ast::Expr) -> Option<String> {
    match expression {
        ast::Expr::Name(name) => Some(name.id.as_str().to_string()),
        ast::Expr::Attribute(attribute) => Some(format!(
            "{}.{}",
            qualified_name(&attribute.value)?,
            attribute.attr.as_str()
        )),
        ast::Expr::Call(call) => qualified_name(&call.func),
        _ => None,
    }
}

fn is_declaration_body(body: &[ast::Stmt]) -> bool {
    match body {
        [ast::Stmt::Pass(_)] => true,
        [ast::Stmt::Expr(expression)] => matches!(
            &*expression.value,
            ast::Expr::Constant(value) if value.value == ast::Constant::Ellipsis
        ),
        _ => false,
    }
}

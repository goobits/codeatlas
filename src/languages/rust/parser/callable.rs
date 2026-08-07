use super::callable_effects::collect_direct_effects;
use super::signatures::format_type;
use codeatlas_domain::{
    CallableBody, CallableContract, CallableKind, CallableParameter, CallableSignature,
    Constructibility, ParameterRequirement, ParameterRole, ReceiverContract, ReceiverRequirement,
    SemanticLiteral, SemanticType, StringEncoding, TypeParameterContract, TypeParameterKind,
    TypeUnknownReason,
};
use std::collections::BTreeSet;
use syn::{FnArg, GenericArgument, GenericParam, PathArguments, ReturnType, Type, TypeParamBound};

const MAX_TYPE_DEPTH: usize = 16;

pub(super) fn contract(
    signature: &syn::Signature,
    kind: CallableKind,
    body: CallableBody,
    block: Option<&syn::Block>,
) -> CallableContract {
    let generic_names = signature
        .generics
        .type_params()
        .map(|parameter| parameter.ident.to_string())
        .collect::<BTreeSet<_>>();
    let mut receiver = ReceiverContract::none();
    let mut parameters = Vec::new();
    for argument in &signature.inputs {
        match argument {
            FnArg::Receiver(value) => {
                receiver = ReceiverContract {
                    requirement: if value.mutability.is_some() {
                        ReceiverRequirement::MutableInstance
                    } else {
                        ReceiverRequirement::Instance
                    },
                    constructibility: Constructibility::RequiresFactory,
                };
            }
            FnArg::Typed(value) => {
                let semantic_type = semantic_type(&value.ty, &generic_names, 0);
                parameters.push(CallableParameter {
                    position: parameters.len(),
                    name: match &*value.pat {
                        syn::Pat::Ident(identifier) => Some(identifier.ident.to_string()),
                        _ => None,
                    },
                    role: ParameterRole::Positional,
                    requirement: ParameterRequirement::Required,
                    constructibility: semantic_type.constructibility(),
                    semantic_type,
                });
            }
        }
    }
    let result = match &signature.output {
        ReturnType::Default => SemanticType::Unit,
        ReturnType::Type(_, value_type) => semantic_type(value_type, &generic_names, 0),
    };
    let signature = CallableSignature {
        kind,
        body,
        is_async: signature.asyncness.is_some(),
        receiver,
        type_parameters: type_parameters(&signature.generics, &generic_names),
        parameters,
        result,
    };
    CallableContract::new([signature], collect_direct_effects(block))
}

fn type_parameters(
    generics: &syn::Generics,
    generic_names: &BTreeSet<String>,
) -> Vec<TypeParameterContract> {
    generics
        .params
        .iter()
        .map(|parameter| match parameter {
            GenericParam::Type(parameter) => TypeParameterContract {
                name: parameter.ident.to_string(),
                kind: TypeParameterKind::Type,
                constraints: parameter
                    .bounds
                    .iter()
                    .filter_map(|bound| match bound {
                        TypeParamBound::Trait(bound) => Some(SemanticType::Named {
                            identity: path_identity(&bound.path),
                            arguments: path_type_arguments(&bound.path, generic_names, 0),
                        }),
                        TypeParamBound::Lifetime(_) => None,
                        _ => Some(SemanticType::unknown(
                            TypeUnknownReason::Unsupported,
                            "generic-bound",
                        )),
                    })
                    .collect(),
                default: parameter
                    .default
                    .as_ref()
                    .map(|value| semantic_type(value, generic_names, 0)),
            },
            GenericParam::Const(parameter) => TypeParameterContract {
                name: parameter.ident.to_string(),
                kind: TypeParameterKind::Const,
                constraints: vec![semantic_type(&parameter.ty, generic_names, 0)],
                default: parameter.default.as_ref().map(semantic_const),
            },
            GenericParam::Lifetime(parameter) => TypeParameterContract {
                name: parameter.lifetime.ident.to_string(),
                kind: TypeParameterKind::Lifetime,
                constraints: Vec::new(),
                default: None,
            },
        })
        .collect()
}

fn semantic_type(
    value_type: &Type,
    generic_names: &BTreeSet<String>,
    depth: usize,
) -> SemanticType {
    if depth >= MAX_TYPE_DEPTH {
        return SemanticType::unknown(
            TypeUnknownReason::UnboundedRecursive,
            format_type(value_type),
        );
    }
    match value_type {
        Type::Reference(reference) => semantic_type(&reference.elem, generic_names, depth + 1),
        Type::Slice(slice) => {
            let value = semantic_type(&slice.elem, generic_names, depth + 1);
            if is_u8(&value) {
                SemanticType::Bytes { max_length: None }
            } else {
                SemanticType::List {
                    value: Box::new(value),
                    min_items: None,
                    max_items: None,
                }
            }
        }
        Type::Array(array) => {
            let value = semantic_type(&array.elem, generic_names, depth + 1);
            let length = integer_literal(&array.len).and_then(|value| value.parse::<u64>().ok());
            if is_u8(&value) {
                SemanticType::Bytes { max_length: length }
            } else {
                SemanticType::List {
                    value: Box::new(value),
                    min_items: length,
                    max_items: length,
                }
            }
        }
        Type::Tuple(tuple) if tuple.elems.is_empty() => SemanticType::Unit,
        Type::Tuple(tuple) => SemanticType::Tuple {
            values: tuple
                .elems
                .iter()
                .map(|value| semantic_type(value, generic_names, depth + 1))
                .collect(),
        },
        Type::Path(type_path) if type_path.qself.is_none() => {
            semantic_path(&type_path.path, generic_names, depth + 1)
        }
        Type::Paren(parenthesized) => semantic_type(&parenthesized.elem, generic_names, depth + 1),
        Type::Group(group) => semantic_type(&group.elem, generic_names, depth + 1),
        Type::Never(_) => {
            SemanticType::unknown(TypeUnknownReason::Unsupported, format_type(value_type))
        }
        Type::Infer(_) => SemanticType::unknown(
            TypeUnknownReason::MissingAnnotation,
            format_type(value_type),
        ),
        Type::TraitObject(_) | Type::ImplTrait(_) | Type::BareFn(_) | Type::Ptr(_) => {
            SemanticType::unknown(TypeUnknownReason::Unsupported, format_type(value_type))
        }
        _ => SemanticType::unknown(TypeUnknownReason::Unresolved, format_type(value_type)),
    }
}

fn semantic_path(path: &syn::Path, generic_names: &BTreeSet<String>, depth: usize) -> SemanticType {
    let Some(segment) = path.segments.last() else {
        return SemanticType::unknown(TypeUnknownReason::Unresolved, "empty-path");
    };
    let name = segment.ident.to_string();
    if path.segments.len() == 1 && generic_names.contains(&name) {
        return SemanticType::TypeParameter { name };
    }
    let arguments = segment_type_arguments(segment, generic_names, depth);
    match name.as_str() {
        "bool" => SemanticType::Boolean,
        "i8" => integer(true, 8),
        "i16" => integer(true, 16),
        "i32" => integer(true, 32),
        "i64" => integer(true, 64),
        "i128" => integer(true, 128),
        "isize" => SemanticType::Integer {
            signed: Some(true),
            bits: None,
        },
        "u8" => integer(false, 8),
        "u16" => integer(false, 16),
        "u32" => integer(false, 32),
        "u64" => integer(false, 64),
        "u128" => integer(false, 128),
        "usize" => SemanticType::Integer {
            signed: Some(false),
            bits: None,
        },
        "f32" => float(32),
        "f64" => float(64),
        "String" | "str" => SemanticType::String {
            encoding: StringEncoding::Utf8,
            max_length: None,
        },
        "char" => SemanticType::String {
            encoding: StringEncoding::Unicode,
            max_length: Some(1),
        },
        "Option" if arguments.len() == 1 => SemanticType::Optional {
            value: Box::new(arguments.into_iter().next().expect("one option argument")),
        },
        "Result" if arguments.len() == 2 => {
            let mut arguments = arguments.into_iter();
            SemanticType::Result {
                ok: Box::new(arguments.next().expect("result ok argument")),
                error: Box::new(arguments.next().expect("result error argument")),
            }
        }
        "Vec" if arguments.len() == 1 => {
            let value = arguments.into_iter().next().expect("one vector argument");
            if is_u8(&value) {
                SemanticType::Bytes { max_length: None }
            } else {
                SemanticType::List {
                    value: Box::new(value),
                    min_items: None,
                    max_items: None,
                }
            }
        }
        "HashSet" | "BTreeSet" if arguments.len() == 1 => SemanticType::Set {
            value: Box::new(arguments.into_iter().next().expect("one set argument")),
            max_items: None,
        },
        "HashMap" | "BTreeMap" if arguments.len() == 2 => {
            let mut arguments = arguments.into_iter();
            SemanticType::Map {
                key: Box::new(arguments.next().expect("map key argument")),
                value: Box::new(arguments.next().expect("map value argument")),
                max_items: None,
            }
        }
        _ => SemanticType::Named {
            identity: path_identity(path),
            arguments,
        },
    }
}

fn path_type_arguments(
    path: &syn::Path,
    generic_names: &BTreeSet<String>,
    depth: usize,
) -> Vec<SemanticType> {
    path.segments
        .last()
        .map(|segment| segment_type_arguments(segment, generic_names, depth))
        .unwrap_or_default()
}

fn segment_type_arguments(
    segment: &syn::PathSegment,
    generic_names: &BTreeSet<String>,
    depth: usize,
) -> Vec<SemanticType> {
    match &segment.arguments {
        PathArguments::AngleBracketed(arguments) => arguments
            .args
            .iter()
            .filter_map(|argument| match argument {
                GenericArgument::Type(value) => {
                    Some(semantic_type(value, generic_names, depth + 1))
                }
                GenericArgument::Const(value) => Some(semantic_const(value)),
                GenericArgument::Lifetime(_) => None,
                _ => Some(SemanticType::unknown(
                    TypeUnknownReason::Unsupported,
                    "associated-type-argument",
                )),
            })
            .collect(),
        PathArguments::Parenthesized(_) => vec![SemanticType::unknown(
            TypeUnknownReason::Unsupported,
            "callable-type-argument",
        )],
        PathArguments::None => Vec::new(),
    }
}

fn semantic_const(expression: &syn::Expr) -> SemanticType {
    match expression {
        syn::Expr::Lit(literal) => match &literal.lit {
            syn::Lit::Int(integer) => SemanticType::Literal {
                value: SemanticLiteral::Integer(integer.base10_digits().to_string()),
            },
            syn::Lit::Bool(value) => SemanticType::Literal {
                value: SemanticLiteral::Boolean(value.value),
            },
            syn::Lit::Str(value) => SemanticType::Literal {
                value: SemanticLiteral::String(value.value()),
            },
            _ => SemanticType::unknown(TypeUnknownReason::Unsupported, "const-expression"),
        },
        _ => SemanticType::unknown(TypeUnknownReason::Unresolved, "const-expression"),
    }
}

fn integer_literal(expression: &syn::Expr) -> Option<String> {
    match expression {
        syn::Expr::Lit(literal) => match &literal.lit {
            syn::Lit::Int(integer) => Some(integer.base10_digits().to_string()),
            _ => None,
        },
        _ => None,
    }
}

fn integer(signed: bool, bits: u16) -> SemanticType {
    SemanticType::Integer {
        signed: Some(signed),
        bits: Some(bits),
    }
}

fn float(bits: u16) -> SemanticType {
    SemanticType::Float {
        bits: Some(bits),
        allows_special: true,
    }
}

fn is_u8(value: &SemanticType) -> bool {
    matches!(
        value,
        SemanticType::Integer {
            signed: Some(false),
            bits: Some(8)
        }
    )
}

pub(super) fn path_identity(path: &syn::Path) -> String {
    path.segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect::<Vec<_>>()
        .join("::")
}

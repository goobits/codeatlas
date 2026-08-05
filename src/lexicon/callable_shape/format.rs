use crate::domain::{
    CallableKind, CallableSignature, Constructibility, ParameterRequirement, ParameterRole,
    ReceiverRequirement, SemanticLiteral, SemanticType, StringEncoding, TypeParameterKind,
    TypeUnknownReason,
};

pub(super) fn format_callable_shape(signatures: &[CallableSignature]) -> String {
    signatures
        .iter()
        .map(format_signature)
        .collect::<Vec<_>>()
        .join(" | ")
}

fn format_signature(signature: &CallableSignature) -> String {
    let async_prefix = if signature.is_async { "async " } else { "" };
    let declaration = match signature.body {
        crate::domain::CallableBody::Present => "body",
        crate::domain::CallableBody::DeclarationOnly => "declaration",
    };
    let type_parameters = if signature.type_parameters.is_empty() {
        String::new()
    } else {
        format!(
            "<{}>",
            signature
                .type_parameters
                .iter()
                .map(|parameter| {
                    let constraints = if parameter.constraints.is_empty() {
                        String::new()
                    } else {
                        format!(
                            ":{}",
                            parameter
                                .constraints
                                .iter()
                                .map(format_semantic_type)
                                .collect::<Vec<_>>()
                                .join("+")
                        )
                    };
                    let default = parameter
                        .default
                        .as_ref()
                        .map(|value| format!("={}", format_semantic_type(value)))
                        .unwrap_or_default();
                    format!(
                        "{}:{}{constraints}{default}",
                        parameter.name,
                        format_type_parameter_kind(parameter.kind)
                    )
                })
                .collect::<Vec<_>>()
                .join(",")
        )
    };
    let parameters = signature
        .parameters
        .iter()
        .map(|parameter| {
            let name = parameter
                .name
                .clone()
                .unwrap_or_else(|| format!("$arg{}", parameter.position));
            format!(
                "{}:{}:{}/{}/{}:{}",
                parameter.position,
                name,
                format_parameter_role(parameter.role),
                format_parameter_requirement(parameter.requirement),
                format_constructibility(parameter.constructibility),
                format_semantic_type(&parameter.semantic_type)
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "{async_prefix}{}{type_parameters}[{declaration};receiver={}/{}]({parameters}) -> {}",
        format_callable_kind(signature.kind),
        format_receiver_requirement(signature.receiver.requirement),
        format_constructibility(signature.receiver.constructibility),
        format_semantic_type(&signature.result)
    )
}

fn format_semantic_type(semantic_type: &SemanticType) -> String {
    match semantic_type {
        SemanticType::Unknown { reason, .. } => {
            format!("unknown({})", format_unknown_reason(*reason))
        }
        SemanticType::Unit => "unit".to_string(),
        SemanticType::Boolean => "boolean".to_string(),
        SemanticType::Integer { signed, bits } => match (signed, bits) {
            (Some(true), Some(bits)) => format!("i{bits}"),
            (Some(false), Some(bits)) => format!("u{bits}"),
            (Some(true), None) => "signed_integer".to_string(),
            (Some(false), None) => "unsigned_integer".to_string(),
            (None, Some(bits)) => format!("integer<{bits}>"),
            (None, None) => "integer".to_string(),
        },
        SemanticType::Float {
            bits,
            allows_special,
        } => {
            let width = bits.map_or_else(|| "float".to_string(), |bits| format!("f{bits}"));
            if *allows_special {
                format!("{width}<special>")
            } else {
                width
            }
        }
        SemanticType::String {
            encoding,
            max_length,
        } => format_bounded_type("string", format_string_encoding(*encoding), *max_length),
        SemanticType::Bytes { max_length } => {
            format_bounded_type("bytes", String::new(), *max_length)
        }
        SemanticType::Null => "null".to_string(),
        SemanticType::Literal { value } => format_literal(value),
        SemanticType::Optional { value } => {
            format!("optional<{}>", format_semantic_type(value))
        }
        SemanticType::Union { variants } => format!(
            "union<{}>",
            variants
                .iter()
                .map(format_semantic_type)
                .collect::<Vec<_>>()
                .join("|")
        ),
        SemanticType::List {
            value,
            min_items,
            max_items,
        } => format!(
            "list<{};{}>",
            format_semantic_type(value),
            format_bounds(*min_items, *max_items)
        ),
        SemanticType::Tuple { values } => format!(
            "tuple<{}>",
            values
                .iter()
                .map(format_semantic_type)
                .collect::<Vec<_>>()
                .join(",")
        ),
        SemanticType::Set { value, max_items } => format!(
            "set<{};max={}>",
            format_semantic_type(value),
            format_bound(*max_items)
        ),
        SemanticType::Map {
            key,
            value,
            max_items,
        } => format!(
            "map<{},{};max={}>",
            format_semantic_type(key),
            format_semantic_type(value),
            format_bound(*max_items)
        ),
        SemanticType::Record { fields } => format!(
            "record<{}>",
            fields
                .iter()
                .map(|field| format!(
                    "{}{}:{}",
                    field.name,
                    if field.required { "!" } else { "?" },
                    format_semantic_type(&field.semantic_type)
                ))
                .collect::<Vec<_>>()
                .join(",")
        ),
        SemanticType::Result { ok, error } => format!(
            "result<{},{}>",
            format_semantic_type(ok),
            format_semantic_type(error)
        ),
        SemanticType::Named {
            identity,
            arguments,
        } => {
            if arguments.is_empty() {
                format!("named<{identity}>")
            } else {
                format!(
                    "named<{identity};{}>",
                    arguments
                        .iter()
                        .map(format_semantic_type)
                        .collect::<Vec<_>>()
                        .join(",")
                )
            }
        }
        SemanticType::TypeParameter { name } => format!("type_parameter<{name}>"),
    }
}

fn format_bounded_type(kind: &str, qualifier: String, max: Option<u64>) -> String {
    let mut values = Vec::new();
    if !qualifier.is_empty() {
        values.push(qualifier);
    }
    if let Some(max) = max {
        values.push(format!("max={max}"));
    }
    if values.is_empty() {
        kind.to_string()
    } else {
        format!("{kind}<{}>", values.join(","))
    }
}

fn format_bounds(min: Option<u64>, max: Option<u64>) -> String {
    format!("min={},max={}", format_bound(min), format_bound(max))
}

fn format_bound(value: Option<u64>) -> String {
    value.map_or_else(|| "?".to_string(), |value| value.to_string())
}

fn format_literal(literal: &SemanticLiteral) -> String {
    match literal {
        SemanticLiteral::Boolean(value) => format!("literal<boolean:{value}>"),
        SemanticLiteral::Integer(value) => format!("literal<integer:{value}>"),
        SemanticLiteral::Float(value) => format!("literal<float:{value}>"),
        SemanticLiteral::String(value) => {
            format!(
                "literal<string:{}>",
                serde_json::to_string(value).expect("string literal")
            )
        }
        SemanticLiteral::Null => "literal<null>".to_string(),
    }
}

fn format_callable_kind(kind: CallableKind) -> &'static str {
    match kind {
        CallableKind::Function => "function",
        CallableKind::Method => "method",
        CallableKind::Constructor => "constructor",
        CallableKind::Getter => "getter",
        CallableKind::Setter => "setter",
    }
}

fn format_receiver_requirement(requirement: ReceiverRequirement) -> &'static str {
    match requirement {
        ReceiverRequirement::None => "none",
        ReceiverRequirement::Instance => "instance",
        ReceiverRequirement::MutableInstance => "mutable_instance",
        ReceiverRequirement::Type => "type",
        ReceiverRequirement::Unknown => "unknown",
    }
}

fn format_constructibility(constructibility: Constructibility) -> &'static str {
    match constructibility {
        Constructibility::Direct => "direct",
        Constructibility::RequiresFactory => "requires_factory",
        Constructibility::Unsupported => "unsupported",
        Constructibility::Unknown => "unknown",
    }
}

fn format_parameter_role(role: ParameterRole) -> &'static str {
    match role {
        ParameterRole::Positional => "positional",
        ParameterRole::PositionalOnly => "positional_only",
        ParameterRole::PositionalOrNamed => "positional_or_named",
        ParameterRole::NamedOnly => "named_only",
        ParameterRole::VariadicPositional => "variadic_positional",
        ParameterRole::VariadicNamed => "variadic_named",
    }
}

fn format_parameter_requirement(requirement: ParameterRequirement) -> &'static str {
    match requirement {
        ParameterRequirement::Required => "required",
        ParameterRequirement::Optional => "optional",
        ParameterRequirement::Defaulted => "defaulted",
    }
}

fn format_type_parameter_kind(kind: TypeParameterKind) -> &'static str {
    match kind {
        TypeParameterKind::Type => "type",
        TypeParameterKind::Const => "const",
        TypeParameterKind::Lifetime => "lifetime",
        TypeParameterKind::ParameterSpec => "parameter_spec",
        TypeParameterKind::Variadic => "variadic",
    }
}

fn format_unknown_reason(reason: TypeUnknownReason) -> &'static str {
    match reason {
        TypeUnknownReason::MissingAnnotation => "missing_annotation",
        TypeUnknownReason::Unresolved => "unresolved",
        TypeUnknownReason::Unsupported => "unsupported",
        TypeUnknownReason::UnboundedRecursive => "unbounded_recursive",
        TypeUnknownReason::UnsupportedPattern => "unsupported_pattern",
    }
}

fn format_string_encoding(encoding: StringEncoding) -> String {
    match encoding {
        StringEncoding::Utf8 => "utf8",
        StringEncoding::Utf16 => "utf16",
        StringEncoding::Unicode => "unicode",
        StringEncoding::Unknown => "unknown",
    }
    .to_string()
}

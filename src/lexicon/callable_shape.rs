mod format;

use crate::domain::{CallableContract, CallableSignature, SemanticType, TypeUnknownReason};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct CallableShape {
    signatures: Vec<CallableSignature>,
    has_type_evidence: bool,
}

impl CallableShape {
    pub(super) fn has_type_evidence(&self) -> bool {
        self.has_type_evidence
    }

    pub(super) fn format_shape(&self) -> String {
        format::format_callable_shape(&self.signatures)
    }
}

pub(super) fn project_callable_shape_exact(contract: &CallableContract) -> CallableShape {
    project_callable_shape(contract, false)
}

pub(super) fn project_callable_shape_semantic_roles(contract: &CallableContract) -> CallableShape {
    project_callable_shape(contract, true)
}

fn project_callable_shape(contract: &CallableContract, normalize_bindings: bool) -> CallableShape {
    let has_type_evidence = contract.signatures.iter().any(has_signature_type_evidence);
    let mut signatures = contract.signatures.clone();
    if normalize_bindings {
        for signature in &mut signatures {
            normalize_signature_bindings(signature);
        }
        signatures.sort();
        signatures.dedup();
    }
    CallableShape {
        signatures,
        has_type_evidence,
    }
}

fn normalize_signature_bindings(signature: &mut CallableSignature) {
    let type_parameter_names = signature
        .type_parameters
        .iter()
        .enumerate()
        .map(|(position, parameter)| (parameter.name.clone(), format!("$type{position}")))
        .collect::<BTreeMap<_, _>>();
    for parameter in &mut signature.type_parameters {
        parameter.name = type_parameter_names[&parameter.name].clone();
        for constraint in &mut parameter.constraints {
            normalize_semantic_type(constraint, &type_parameter_names);
        }
        if let Some(default) = &mut parameter.default {
            normalize_semantic_type(default, &type_parameter_names);
        }
    }
    for parameter in &mut signature.parameters {
        parameter.name = None;
        normalize_semantic_type(&mut parameter.semantic_type, &type_parameter_names);
    }
    normalize_semantic_type(&mut signature.result, &type_parameter_names);
}

fn normalize_semantic_type(
    semantic_type: &mut SemanticType,
    type_parameter_names: &BTreeMap<String, String>,
) {
    match semantic_type {
        SemanticType::Unknown { display, .. } => *display = None,
        SemanticType::Optional { value }
        | SemanticType::List { value, .. }
        | SemanticType::Set { value, .. } => {
            normalize_semantic_type(value, type_parameter_names);
        }
        SemanticType::Union { variants } | SemanticType::Tuple { values: variants } => {
            for variant in variants {
                normalize_semantic_type(variant, type_parameter_names);
            }
        }
        SemanticType::Map { key, value, .. } => {
            normalize_semantic_type(key, type_parameter_names);
            normalize_semantic_type(value, type_parameter_names);
        }
        SemanticType::Record { fields } => {
            for field in fields {
                normalize_semantic_type(&mut field.semantic_type, type_parameter_names);
            }
        }
        SemanticType::Result { ok, error } => {
            normalize_semantic_type(ok, type_parameter_names);
            normalize_semantic_type(error, type_parameter_names);
        }
        SemanticType::Named { arguments, .. } => {
            for argument in arguments {
                normalize_semantic_type(argument, type_parameter_names);
            }
        }
        SemanticType::TypeParameter { name } => {
            if let Some(normalized) = type_parameter_names.get(name) {
                *name = normalized.clone();
            }
        }
        SemanticType::Unit
        | SemanticType::Boolean
        | SemanticType::Integer { .. }
        | SemanticType::Float { .. }
        | SemanticType::String { .. }
        | SemanticType::Bytes { .. }
        | SemanticType::Null
        | SemanticType::Literal { .. } => {}
    }
}

fn has_signature_type_evidence(signature: &CallableSignature) -> bool {
    signature
        .type_parameters
        .iter()
        .flat_map(|parameter| parameter.constraints.iter().chain(parameter.default.iter()))
        .chain(
            signature
                .parameters
                .iter()
                .map(|parameter| &parameter.semantic_type),
        )
        .chain(std::iter::once(&signature.result))
        .any(has_semantic_type_evidence)
}

fn has_semantic_type_evidence(semantic_type: &SemanticType) -> bool {
    !matches!(
        semantic_type,
        SemanticType::Unit
            | SemanticType::Unknown {
                reason: TypeUnknownReason::MissingAnnotation,
                ..
            }
    )
}

#[cfg(test)]
mod tests {
    use super::{project_callable_shape_exact, project_callable_shape_semantic_roles};
    use crate::domain::{
        CallableBody, CallableContract, CallableKind, CallableParameter, CallableSignature,
        ParameterRequirement, ParameterRole, ReceiverContract, SemanticType, TypeUnknownReason,
    };

    fn contract(name: &str, semantic_type: SemanticType) -> CallableContract {
        let result = semantic_type.clone();
        CallableContract::new(
            [CallableSignature {
                kind: CallableKind::Function,
                body: CallableBody::Present,
                is_async: false,
                receiver: ReceiverContract::none(),
                type_parameters: Vec::new(),
                parameters: vec![CallableParameter {
                    position: 0,
                    name: Some(name.to_string()),
                    role: ParameterRole::Positional,
                    requirement: ParameterRequirement::Required,
                    constructibility: semantic_type.constructibility(),
                    semantic_type,
                }],
                result,
            }],
            [],
        )
    }

    #[test]
    fn semantic_role_shape_ignores_binding_spelling_but_exact_shape_does_not() {
        let left = contract("path", SemanticType::Boolean);
        let right = contract("value", SemanticType::Boolean);

        assert_ne!(
            project_callable_shape_exact(&left),
            project_callable_shape_exact(&right)
        );
        let left = project_callable_shape_semantic_roles(&left);
        let right = project_callable_shape_semantic_roles(&right);
        assert_eq!(left, right);
        assert!(left.format_shape().contains("$arg0"));
    }

    #[test]
    fn missing_annotations_remain_distinct_from_type_evidence() {
        let untyped = project_callable_shape_semantic_roles(&contract(
            "value",
            SemanticType::unknown(TypeUnknownReason::MissingAnnotation, "value"),
        ));
        let typed = project_callable_shape_semantic_roles(&contract(
            "value",
            SemanticType::unknown(TypeUnknownReason::Unsupported, "Callback"),
        ));

        assert!(!untyped.has_type_evidence());
        assert!(typed.has_type_evidence());
    }

    #[test]
    fn implicit_unit_result_is_not_discriminating_type_evidence() {
        let contract = CallableContract::new(
            [CallableSignature {
                kind: CallableKind::Function,
                body: CallableBody::Present,
                is_async: false,
                receiver: ReceiverContract::none(),
                type_parameters: Vec::new(),
                parameters: Vec::new(),
                result: SemanticType::Unit,
            }],
            [],
        );

        assert!(!project_callable_shape_semantic_roles(&contract).has_type_evidence());
    }
}

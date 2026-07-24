use super::diagnostic::{ArchitectureError, Diagnostic};
use super::yaml::{parse, ParseLimits};
use serde_json::{Map, Value};

const COMMON_SCHEMA: &str =
    include_str!("../../spec/architecture/v0.1/schemas/common.v0.1.schema.yaml");
const MODULE_SCHEMA: &str =
    include_str!("../../spec/architecture/v0.1/schemas/architecture-module.v0.1.schema.yaml");
const POLICY_SCHEMA: &str =
    include_str!("../../spec/architecture/v0.1/schemas/architecture-policy.v0.1.schema.yaml");
const VOCABULARY_SCHEMA: &str =
    include_str!("../../spec/architecture/v0.1/schemas/architecture-vocabulary.v0.1.schema.yaml");
const CHANGE_SCHEMA: &str =
    include_str!("../../spec/architecture/v0.1/schemas/architecture-change.v0.1.schema.yaml");
const OBSERVATION_SCHEMA: &str =
    include_str!("../../spec/architecture/v0.1/schemas/architecture-observation.v0.1.schema.yaml");
const CONFORMANCE_SCHEMA: &str =
    include_str!("../../spec/architecture/v0.1/schemas/architecture-conformance.v0.1.schema.yaml");

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DocumentKind {
    Module,
    Policy,
    Vocabulary,
    Change,
    Observation,
    Conformance,
}

impl DocumentKind {
    pub(crate) fn from_document(document: &Value) -> Result<Self, ArchitectureError> {
        match document.get("kind").and_then(Value::as_str) {
            Some("ArchitectureModule") => Ok(Self::Module),
            Some("ArchitecturePolicy") => Ok(Self::Policy),
            Some("ArchitectureVocabulary") => Ok(Self::Vocabulary),
            Some("ArchitectureChange") => Ok(Self::Change),
            Some("ArchitectureObservation") => Ok(Self::Observation),
            Some("ArchitectureConformance") => Ok(Self::Conformance),
            Some(kind) => Err(ArchitectureError::new(
                "schema.unknown-document-kind",
                format!("unknown document kind: {kind}"),
            )),
            None => Err(ArchitectureError::new(
                "schema.missing-document-kind",
                "document kind is required",
            )),
        }
    }

    fn source(self) -> &'static str {
        match self {
            Self::Module => MODULE_SCHEMA,
            Self::Policy => POLICY_SCHEMA,
            Self::Vocabulary => VOCABULARY_SCHEMA,
            Self::Change => CHANGE_SCHEMA,
            Self::Observation => OBSERVATION_SCHEMA,
            Self::Conformance => CONFORMANCE_SCHEMA,
        }
    }
}

pub(crate) fn validate(document: &Value) -> Vec<Diagnostic> {
    let kind = match DocumentKind::from_document(document) {
        Ok(kind) => kind,
        Err(error) => return vec![*error.diagnostic],
    };
    let schema = match bundled_schema(kind) {
        Ok(schema) => schema,
        Err(error) => return vec![*error.diagnostic],
    };
    let validator = match jsonschema::validator_for(&schema) {
        Ok(validator) => validator,
        Err(error) => {
            return vec![Diagnostic::error(
                "schema.definition-invalid",
                format!("cannot compile {kind:?} schema: {error}"),
            )];
        }
    };
    let mut diagnostics = validator
        .iter_errors(document)
        .map(|error| {
            let path = error.instance_path().as_str();
            Diagnostic::error(
                "schema.document-invalid",
                if path.is_empty() {
                    error.to_string()
                } else {
                    format!("{path}: {error}")
                },
            )
        })
        .collect::<Vec<_>>();
    diagnostics.sort_by(|left, right| left.message.cmp(&right.message));
    diagnostics
}

fn bundled_schema(kind: DocumentKind) -> Result<Value, ArchitectureError> {
    let common = parse_schema(COMMON_SCHEMA, "common")?;
    let mut schema = parse_schema(kind.source(), &format!("{kind:?}"))?;
    let common_definitions = common
        .get("$defs")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            ArchitectureError::new(
                "schema.common-definitions-missing",
                "common schema must contain $defs",
            )
        })?
        .clone();
    let object = schema.as_object_mut().ok_or_else(|| {
        ArchitectureError::new(
            "schema.definition-invalid",
            "document schema root must be an object",
        )
    })?;
    let definitions = object
        .entry("$defs")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| {
            ArchitectureError::new("schema.definition-invalid", "$defs must be an object")
        })?;
    for (name, definition) in common_definitions {
        if definitions.insert(name.clone(), definition).is_some() {
            return Err(ArchitectureError::new(
                "schema.definition-collision",
                format!("common and document schemas both define $defs/{name}"),
            ));
        }
    }
    rewrite_common_references(&mut schema);
    Ok(schema)
}

fn parse_schema(source: &str, name: &str) -> Result<Value, ArchitectureError> {
    parse(source.as_bytes(), ParseLimits::default())
        .map(|document| document.value)
        .map_err(|error| {
            ArchitectureError::new(
                "schema.source-invalid",
                format!("{name} schema is invalid restricted YAML: {error}"),
            )
        })
}

fn rewrite_common_references(value: &mut Value) {
    match value {
        Value::Array(values) => {
            for value in values {
                rewrite_common_references(value);
            }
        }
        Value::Object(values) => {
            for value in values.values_mut() {
                rewrite_common_references(value);
            }
        }
        Value::String(reference) if reference.starts_with("common.v0.1.schema.yaml#/$defs/") => {
            *reference = reference.replacen("common.v0.1.schema.yaml", "", 1);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::{bundled_schema, validate, DocumentKind};
    use serde_json::json;

    #[test]
    fn all_static_schemas_are_valid_draft_2020_12() {
        for kind in [
            DocumentKind::Module,
            DocumentKind::Policy,
            DocumentKind::Vocabulary,
            DocumentKind::Change,
            DocumentKind::Observation,
            DocumentKind::Conformance,
        ] {
            let schema = bundled_schema(kind).expect("bundled schema");
            jsonschema::meta::validate(&schema).expect("valid Draft 2020-12 schema");
        }
    }

    #[test]
    fn schema_rejects_unknown_fields_and_change_outcomes_as_types() {
        let mut document = json!({
            "apiVersion": "atlas.codeatlas.dev/v0.1",
            "kind": "ArchitectureChange",
            "metadata": {
                "id": "codeatlas.change.example",
                "name": "Example",
                "architectureVersion": 1
            },
            "vocabulary": {
                "id": "codeatlas.architecture.core",
                "version": 1,
                "digest": format!("sha256:{}", "a".repeat(64))
            },
            "baseGraphDigest": format!("sha256:{}", "b".repeat(64)),
            "change": {"type": "INTRODUCE"},
            "decision": {
                "status": "proposed",
                "authority": {"governing": [], "supporting": []}
            },
            "approval": {"status": "required"},
            "changeControl": {"policy": "owner_approval_required"},
            "affectedIds": ["codeatlas.capability.example"],
            "currentOwner": null,
            "intendedOwner": "codeatlas.package.example",
            "expectedEffects": {"adds": [], "removes": [], "requires": []},
            "migrationPlan": [],
            "removalPlan": [],
            "verificationPlan": [],
            "targetGraphDigest": null,
            "surprise": true
        });
        assert!(validate(&document)
            .iter()
            .any(|diagnostic| diagnostic.message.contains("surprise")));

        document.as_object_mut().expect("change").remove("surprise");
        document["change"]["type"] = json!("REJECT");
        assert!(!validate(&document).is_empty());
    }
}

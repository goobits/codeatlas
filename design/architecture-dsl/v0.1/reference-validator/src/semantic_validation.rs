use crate::{digest_value, validate_document_schema, Diagnostic, DigestKind, TypedDigest};
use serde::Deserialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ValueType {
    String,
    Integer,
    Boolean,
    Identifier,
    Digest,
    StringList,
    IdentifierList,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectKindDefinition {
    pub required_attributes: BTreeMap<String, ValueType>,
    pub optional_attributes: BTreeMap<String, ValueType>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PredicateDefinition {
    pub subject_kinds: BTreeSet<String>,
    pub object_kinds: BTreeSet<String>,
    pub cycles: CyclePolicy,
    pub inverse: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum CyclePolicy {
    Allowed,
    Forbidden,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConstraintRuleDefinition {
    pub version: u64,
    pub required_arguments: BTreeMap<String, ValueType>,
    pub optional_arguments: BTreeMap<String, ValueType>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BindingAdapterVersion {
    pub required_selector: BTreeMap<String, ValueType>,
    pub optional_selector: BTreeMap<String, ValueType>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct BindingAdapterDefinition {
    pub versions: BTreeMap<String, BindingAdapterVersion>,
}

#[derive(Clone, Debug)]
pub struct Vocabulary {
    pub id: String,
    pub version: u64,
    pub digest: TypedDigest,
    pub authority_kinds: BTreeSet<String>,
    pub object_kinds: BTreeMap<String, ObjectKindDefinition>,
    pub predicates: BTreeMap<String, PredicateDefinition>,
    pub constraint_rules: BTreeMap<String, ConstraintRuleDefinition>,
    pub binding_adapters: BTreeMap<String, BindingAdapterDefinition>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct VocabularyDocument {
    metadata: VocabularyMetadata,
    authority_kinds: BTreeSet<String>,
    object_kinds: BTreeMap<String, ObjectKindDefinition>,
    predicates: BTreeMap<String, PredicateDefinition>,
    constraint_rules: BTreeMap<String, ConstraintRuleDefinition>,
    binding_adapters: BTreeMap<String, BindingAdapterDefinition>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct VocabularyMetadata {
    id: String,
    architecture_version: u64,
}

impl Vocabulary {
    pub fn from_document(document: &Value) -> Result<Self, Vec<Diagnostic>> {
        let mut diagnostics = validate_document_schema(document);
        if document.get("kind").and_then(Value::as_str) != Some("ArchitectureVocabulary") {
            diagnostics.push(Diagnostic::error(
                "vocabulary.wrong-document-kind",
                "vocabulary source must be ArchitectureVocabulary",
            ));
        }
        if !diagnostics.is_empty() {
            return Err(diagnostics);
        }

        let parsed: VocabularyDocument = match serde_json::from_value(document.clone()) {
            Ok(parsed) => parsed,
            Err(error) => {
                return Err(vec![Diagnostic::error(
                    "vocabulary.decode-failed",
                    error.to_string(),
                )])
            }
        };
        let digest = match digest_value(DigestKind::CanonicalModule, document) {
            Ok(digest) => digest,
            Err(error) => return Err(vec![*error.diagnostic]),
        };

        let vocabulary = Self {
            id: parsed.metadata.id,
            version: parsed.metadata.architecture_version,
            digest,
            authority_kinds: parsed.authority_kinds,
            object_kinds: parsed.object_kinds,
            predicates: parsed.predicates,
            constraint_rules: parsed.constraint_rules,
            binding_adapters: parsed.binding_adapters,
        };
        let diagnostics = vocabulary.validate_definitions();
        if diagnostics.is_empty() {
            Ok(vocabulary)
        } else {
            Err(diagnostics)
        }
    }

    pub fn validate_document(&self, document: &Value) -> Vec<Diagnostic> {
        let mut diagnostics = validate_document_schema(document);
        if !diagnostics.is_empty() {
            return diagnostics;
        }

        if document.get("kind").and_then(Value::as_str) != Some("ArchitectureVocabulary") {
            diagnostics.extend(self.validate_vocabulary_reference(document));
        }
        diagnostics.extend(self.validate_lifecycle(
            document,
            document_id(document).unwrap_or("<unknown-document>"),
        ));

        match document.get("kind").and_then(Value::as_str) {
            Some("ArchitectureModule") => {
                diagnostics.extend(self.validate_module(document));
            }
            Some("ArchitecturePolicy") => {
                diagnostics.extend(self.validate_policy(document));
            }
            Some("ArchitectureChange") => {
                diagnostics.extend(self.validate_change(document));
            }
            Some("ArchitectureObservation") => {
                diagnostics.extend(self.validate_observation(document));
            }
            Some("ArchitectureConformance") | Some("ArchitectureVocabulary") => {}
            _ => {}
        }

        diagnostics.sort_by(|left, right| {
            left.code
                .cmp(&right.code)
                .then_with(|| left.message.cmp(&right.message))
        });
        diagnostics
    }

    pub fn validate_object(&self, id: &str, declaration: &Value) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        let Some(kind) = declaration.get("kind").and_then(Value::as_str) else {
            return vec![Diagnostic::error(
                "object.kind-missing",
                format!("{id} has no object kind"),
            )];
        };
        let Some(definition) = self.object_kinds.get(kind) else {
            return vec![Diagnostic::error(
                "vocabulary.unknown-object-kind",
                format!("{id} uses unknown object kind {kind}"),
            )];
        };
        let attributes = declaration
            .get("attributes")
            .and_then(Value::as_object)
            .expect("schema validated object attributes");

        for (name, value_type) in &definition.required_attributes {
            match attributes.get(name) {
                Some(value) if matches_value_type(value, *value_type) => {}
                Some(_) => diagnostics.push(type_mismatch("object", id, name, *value_type)),
                None => diagnostics.push(Diagnostic::error(
                    "object.required-attribute-missing",
                    format!("{id} is missing required attribute {name}"),
                )),
            }
        }
        for (name, value) in attributes {
            let value_type = definition
                .required_attributes
                .get(name)
                .or_else(|| definition.optional_attributes.get(name));
            match value_type {
                Some(value_type) if matches_value_type(value, *value_type) => {}
                Some(value_type) => {
                    diagnostics.push(type_mismatch("object", id, name, *value_type));
                }
                None => diagnostics.push(Diagnostic::error(
                    "object.unknown-attribute",
                    format!("{id} has unknown {kind} attribute {name}"),
                )),
            }
        }
        diagnostics.extend(self.validate_lifecycle(declaration, id));
        diagnostics
    }

    pub fn validate_relation(
        &self,
        id: &str,
        declaration: &Value,
        object_kinds: &BTreeMap<String, String>,
    ) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        let predicate = declaration["predicate"]
            .as_str()
            .expect("schema validated predicate");
        let subject = declaration["subject"]
            .as_str()
            .expect("schema validated subject");
        let object = declaration["object"]
            .as_str()
            .expect("schema validated object");
        let Some(definition) = self.predicates.get(predicate) else {
            return vec![Diagnostic::error(
                "vocabulary.unknown-predicate",
                format!("{id} uses unknown predicate {predicate}"),
            )];
        };

        match object_kinds.get(subject) {
            Some(kind) if definition.subject_kinds.contains(kind) => {}
            Some(kind) => diagnostics.push(Diagnostic::error(
                "relation.invalid-subject-kind",
                format!("{id} predicate {predicate} does not allow subject kind {kind}"),
            )),
            None => diagnostics.push(Diagnostic::error(
                "relation.subject-unresolved",
                format!("{id} references unknown subject {subject}"),
            )),
        }
        match object_kinds.get(object) {
            Some(kind) if definition.object_kinds.contains(kind) => {}
            Some(kind) => diagnostics.push(Diagnostic::error(
                "relation.invalid-object-kind",
                format!("{id} predicate {predicate} does not allow object kind {kind}"),
            )),
            None => diagnostics.push(Diagnostic::error(
                "relation.object-unresolved",
                format!("{id} references unknown object {object}"),
            )),
        }
        if definition.cycles == CyclePolicy::Forbidden && subject == object {
            diagnostics.push(Diagnostic::error(
                "relation.self-cycle-forbidden",
                format!("{id} creates a forbidden self-cycle"),
            ));
        }
        diagnostics.extend(self.validate_lifecycle(declaration, id));
        diagnostics
    }

    pub fn validate_binding(&self, id: &str, declaration: &Value) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        let kind = declaration["adapter"]["kind"]
            .as_str()
            .expect("schema validated adapter kind");
        let version = declaration["adapter"]["version"]
            .as_u64()
            .expect("schema validated adapter version")
            .to_string();
        let Some(adapter) = self.binding_adapters.get(kind) else {
            return vec![Diagnostic::error(
                "vocabulary.unknown-binding-adapter",
                format!("{id} uses unknown adapter {kind}"),
            )];
        };
        let Some(definition) = adapter.versions.get(&version) else {
            return vec![Diagnostic::error(
                "binding.unsupported-adapter-version",
                format!("{id} uses unsupported adapter {kind} version {version}"),
            )];
        };
        let selector = declaration["selector"]
            .as_object()
            .expect("schema validated selector");
        diagnostics.extend(validate_typed_map(
            "binding",
            id,
            selector,
            &definition.required_selector,
            &definition.optional_selector,
        ));
        diagnostics.extend(self.validate_lifecycle(declaration, id));
        diagnostics
    }

    pub fn validate_constraint(&self, id: &str, declaration: &Value) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        let rule = declaration["rule"].as_str().expect("schema validated rule");
        let Some(definition) = self.constraint_rules.get(rule) else {
            return vec![Diagnostic::error(
                "vocabulary.unknown-constraint-rule",
                format!("{id} uses unknown constraint rule {rule}"),
            )];
        };
        let arguments = declaration["arguments"]
            .as_object()
            .expect("schema validated arguments");
        diagnostics.extend(validate_typed_map(
            "constraint",
            id,
            arguments,
            &definition.required_arguments,
            &definition.optional_arguments,
        ));
        diagnostics.extend(self.validate_lifecycle(declaration, id));
        diagnostics
    }

    fn validate_definitions(&self) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        for (predicate, definition) in &self.predicates {
            for kind in definition
                .subject_kinds
                .iter()
                .chain(definition.object_kinds.iter())
            {
                if !self.object_kinds.contains_key(kind) {
                    diagnostics.push(Diagnostic::error(
                        "vocabulary.predicate-kind-unknown",
                        format!("{predicate} references unknown object kind {kind}"),
                    ));
                }
            }
            if let Some(inverse) = &definition.inverse {
                if !self.predicates.contains_key(inverse) {
                    diagnostics.push(Diagnostic::error(
                        "vocabulary.inverse-predicate-unknown",
                        format!("{predicate} names unknown inverse {inverse}"),
                    ));
                }
            }
        }
        diagnostics
    }

    fn validate_vocabulary_reference(&self, document: &Value) -> Vec<Diagnostic> {
        let reference = &document["vocabulary"];
        let mut diagnostics = Vec::new();
        if reference["id"].as_str() != Some(&self.id) {
            diagnostics.push(Diagnostic::error(
                "vocabulary.identity-mismatch",
                format!("document does not reference vocabulary {}", self.id),
            ));
        }
        if reference["version"].as_u64() != Some(self.version) {
            diagnostics.push(Diagnostic::error(
                "vocabulary.version-mismatch",
                format!(
                    "document does not reference vocabulary version {}",
                    self.version
                ),
            ));
        }
        if reference["digest"].as_str() != Some(self.digest.as_str()) {
            diagnostics.push(Diagnostic::error(
                "vocabulary.digest-mismatch",
                "document vocabulary digest does not match loaded vocabulary",
            ));
        }
        diagnostics
    }

    fn validate_module(&self, document: &Value) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        for (id, declaration) in map(document, "objects") {
            diagnostics.extend(self.validate_object(id, declaration));
        }
        for (id, declaration) in map(document, "bindings") {
            diagnostics.extend(self.validate_binding(id, declaration));
        }
        for (id, declaration) in map(document, "constraints") {
            diagnostics.extend(self.validate_constraint(id, declaration));
        }
        diagnostics
    }

    fn validate_policy(&self, document: &Value) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        for (id, declaration) in map(document, "rules") {
            diagnostics.extend(self.validate_constraint(id, declaration));
        }
        for (id, exception) in map(document, "exceptions") {
            diagnostics.extend(self.validate_lifecycle(exception, id));
            if exception["scope"]["module"].as_str() == Some("*") {
                diagnostics.push(Diagnostic::error(
                    "policy.wildcard-scope",
                    format!("{id} uses a prohibited wildcard scope"),
                ));
            }
            if exception["affectedIds"]
                .as_array()
                .is_some_and(Vec::is_empty)
            {
                diagnostics.push(Diagnostic::error(
                    "policy.affected-ids-empty",
                    format!("{id} must identify at least one affected ID"),
                ));
            }
            if exception["removalPlan"]
                .as_array()
                .is_some_and(Vec::is_empty)
            {
                diagnostics.push(Diagnostic::error(
                    "policy.removal-plan-missing",
                    format!("{id} must include a removal plan"),
                ));
            }
        }
        diagnostics
    }

    fn validate_change(&self, document: &Value) -> Vec<Diagnostic> {
        let change_type = document["change"]["type"]
            .as_str()
            .expect("schema validated change type");
        if change_type == "REPLACE"
            && document["removalPlan"]
                .as_array()
                .is_some_and(Vec::is_empty)
        {
            return vec![Diagnostic::error(
                "change.removal-plan-missing",
                "REPLACE requires a non-empty removal plan",
            )];
        }
        Vec::new()
    }

    fn validate_observation(&self, document: &Value) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        if document["metadata"]["generated"].as_bool() != Some(true) {
            diagnostics.push(Diagnostic::error(
                "generated.metadata-required",
                "ArchitectureObservation metadata.generated must be true",
            ));
        }
        for (id, fact) in map(document, "facts") {
            let mode = fact["mode"].as_str().expect("schema validated fact mode");
            if mode == "inferred" && fact.get("confidenceBasisPoints").is_none() {
                diagnostics.push(Diagnostic::error(
                    "observation.inferred-confidence-missing",
                    format!("{id} is inferred but has no confidenceBasisPoints"),
                ));
            }
        }
        diagnostics
    }

    fn validate_lifecycle(&self, value: &Value, id: &str) -> Vec<Diagnostic> {
        let Some(decision) = value.get("decision") else {
            return Vec::new();
        };
        let mut diagnostics = Vec::new();
        let status = decision["status"]
            .as_str()
            .expect("schema validated decision status");
        for authority_group in ["governing", "supporting"] {
            for authority in decision["authority"][authority_group]
                .as_array()
                .into_iter()
                .flatten()
            {
                let kind = authority["kind"]
                    .as_str()
                    .expect("schema validated authority kind");
                if !self.authority_kinds.contains(kind) {
                    diagnostics.push(Diagnostic::error(
                        "authority.kind-unknown",
                        format!("{id} uses unknown authority kind {kind}"),
                    ));
                }
            }
        }
        if status == "accepted"
            && decision["authority"]["governing"]
                .as_array()
                .is_some_and(Vec::is_empty)
        {
            diagnostics.push(Diagnostic::error(
                "authority.governing-required",
                format!("{id} is accepted without governing authority"),
            ));
        }
        if status == "proposed"
            && decision["authority"]["supporting"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|authority| authority["kind"].as_str() == Some("owner-direction"))
        {
            // Owner direction supports the proposal but intentionally leaves it proposed.
        }

        let approval = value
            .get("approval")
            .and_then(|approval| approval["status"].as_str());
        let change_control = value
            .get("changeControl")
            .and_then(|control| control["policy"].as_str());
        if status == "accepted"
            && change_control != Some("open_review")
            && !matches!(approval, Some("granted" | "not_required"))
        {
            diagnostics.push(Diagnostic::error(
                "approval.required-for-accepted",
                format!("{id} is accepted without required approval"),
            ));
        }
        diagnostics
    }
}

pub fn is_qualified_identifier(value: &str) -> bool {
    if value.len() > 200 {
        return false;
    }
    let segments = value.split('.').collect::<Vec<_>>();
    segments.len() >= 3
        && segments.iter().all(|segment| {
            !segment.is_empty()
                && segment
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
                && segment
                    .as_bytes()
                    .first()
                    .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        })
}

fn validate_typed_map(
    category: &str,
    id: &str,
    values: &serde_json::Map<String, Value>,
    required: &BTreeMap<String, ValueType>,
    optional: &BTreeMap<String, ValueType>,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for (name, value_type) in required {
        match values.get(name) {
            Some(value) if matches_value_type(value, *value_type) => {}
            Some(_) => diagnostics.push(type_mismatch(category, id, name, *value_type)),
            None => diagnostics.push(Diagnostic::error(
                format!("{category}.required-field-missing"),
                format!("{id} is missing required field {name}"),
            )),
        }
    }
    for (name, value) in values {
        let value_type = required.get(name).or_else(|| optional.get(name));
        match value_type {
            Some(value_type) if matches_value_type(value, *value_type) => {}
            Some(value_type) => {
                diagnostics.push(type_mismatch(category, id, name, *value_type));
            }
            None => diagnostics.push(Diagnostic::error(
                format!("{category}.unknown-field"),
                format!("{id} has unknown field {name}"),
            )),
        }
    }
    diagnostics
}

fn type_mismatch(category: &str, id: &str, name: &str, expected: ValueType) -> Diagnostic {
    Diagnostic::error(
        format!("{category}.field-type-mismatch"),
        format!("{id} field {name} must be {expected:?}"),
    )
}

fn matches_value_type(value: &Value, value_type: ValueType) -> bool {
    match value_type {
        ValueType::String => value.is_string(),
        ValueType::Integer => value.as_i64().is_some(),
        ValueType::Boolean => value.is_boolean(),
        ValueType::Identifier => value.as_str().is_some_and(is_qualified_identifier),
        ValueType::Digest => value.as_str().is_some_and(|value| {
            value.strip_prefix("sha256:").is_some_and(|hex| {
                hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
            })
        }),
        ValueType::StringList => value
            .as_array()
            .is_some_and(|values| values.iter().all(Value::is_string)),
        ValueType::IdentifierList => value.as_array().is_some_and(|values| {
            values
                .iter()
                .all(|value| value.as_str().is_some_and(is_qualified_identifier))
        }),
    }
}

fn map<'a>(document: &'a Value, field: &str) -> &'a serde_json::Map<String, Value> {
    document[field]
        .as_object()
        .expect("schema validated mapping")
}

fn document_id(document: &Value) -> Option<&str> {
    document
        .get("metadata")
        .and_then(|metadata| metadata.get("id"))
        .and_then(Value::as_str)
}

#[cfg(test)]
mod tests {
    use super::{is_qualified_identifier, Vocabulary};
    use crate::{parse_restricted_yaml, ParseLimits};

    fn core_vocabulary() -> Vocabulary {
        let document = parse_restricted_yaml(
            include_bytes!("../../vocabularies/core.v0.1.atlas.yaml"),
            ParseLimits::default(),
        )
        .expect("parse core vocabulary");
        Vocabulary::from_document(&document.value).expect("valid core vocabulary")
    }

    #[test]
    fn qualified_identifiers_are_strict_and_stable() {
        assert!(is_qualified_identifier("goobits.app.tabby"));
        assert!(is_qualified_identifier(
            "codeatlas.capability.context-slice"
        ));
        assert!(!is_qualified_identifier("Tabby"));
        assert!(!is_qualified_identifier("goobits.Tabby.app"));
        assert!(!is_qualified_identifier("goobits.app"));
    }

    #[test]
    fn core_vocabulary_is_closed_and_self_consistent() {
        let vocabulary = core_vocabulary();
        assert!(vocabulary.object_kinds.contains_key("capability"));
        assert!(vocabulary.predicates.contains_key("consumes"));
        assert!(vocabulary.constraint_rules.contains_key("no_path"));
        assert!(vocabulary.binding_adapters.contains_key("npm.package"));
    }
}

use super::diagnostic::Diagnostic;
use super::digest::{digest_value, DigestKind, TypedDigest};
use super::model::VocabularyIdentity;
use super::schema;
use super::yaml::{parse, ParseLimits};
use serde::Deserialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

const CORE_VOCABULARY: &[u8] =
    include_bytes!("../../spec/architecture/v0.1/vocabularies/core.v0.1.atlas.yaml");

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
enum ValueType {
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
struct ObjectKindDefinition {
    required_attributes: BTreeMap<String, ValueType>,
    optional_attributes: BTreeMap<String, ValueType>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CyclePolicy {
    Allowed,
    Forbidden,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PredicateDefinition {
    subject_kinds: BTreeSet<String>,
    object_kinds: BTreeSet<String>,
    pub(crate) cycles: CyclePolicy,
    #[allow(dead_code)]
    inverse: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConstraintRuleDefinition {
    #[allow(dead_code)]
    version: u64,
    required_arguments: BTreeMap<String, ValueType>,
    optional_arguments: BTreeMap<String, ValueType>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BindingAdapterVersion {
    required_selector: BTreeMap<String, ValueType>,
    optional_selector: BTreeMap<String, ValueType>,
}

#[derive(Clone, Debug, Deserialize)]
struct BindingAdapterDefinition {
    versions: BTreeMap<String, BindingAdapterVersion>,
}

#[derive(Clone, Debug)]
pub(crate) struct Vocabulary {
    pub(crate) id: String,
    pub(crate) version: u64,
    pub(crate) digest: TypedDigest,
    authority_kinds: BTreeSet<String>,
    object_kinds: BTreeMap<String, ObjectKindDefinition>,
    pub(crate) predicates: BTreeMap<String, PredicateDefinition>,
    constraint_rules: BTreeMap<String, ConstraintRuleDefinition>,
    binding_adapters: BTreeMap<String, BindingAdapterDefinition>,
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
    pub(crate) fn bundled() -> Result<Self, Vec<Diagnostic>> {
        let parsed = parse(CORE_VOCABULARY, ParseLimits::default())
            .map_err(|error| vec![*error.diagnostic])?;
        Self::from_document(&parsed.value)
    }

    fn from_document(document: &Value) -> Result<Self, Vec<Diagnostic>> {
        let mut diagnostics = schema::validate(document);
        if document.get("kind").and_then(Value::as_str) != Some("ArchitectureVocabulary") {
            diagnostics.push(Diagnostic::error(
                "vocabulary.wrong-document-kind",
                "vocabulary source must be ArchitectureVocabulary",
            ));
        }
        if !diagnostics.is_empty() {
            return Err(diagnostics);
        }
        let parsed: VocabularyDocument =
            serde_json::from_value(document.clone()).map_err(|error| {
                vec![Diagnostic::error(
                    "vocabulary.decode-failed",
                    error.to_string(),
                )]
            })?;
        let digest = digest_value(DigestKind::CanonicalModule, document)
            .map_err(|error| vec![*error.diagnostic])?;
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

    pub(crate) fn validate_document(&self, document: &Value) -> Vec<Diagnostic> {
        let mut diagnostics = schema::validate(document);
        if !diagnostics.is_empty() {
            return diagnostics;
        }
        if document.get("kind").and_then(Value::as_str) != Some("ArchitectureVocabulary") {
            diagnostics.extend(self.validate_reference(document));
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
            Some("ArchitectureConformance") => {
                diagnostics.extend(self.validate_conformance(document));
            }
            _ => {}
        }
        diagnostics.sort_by(|left, right| {
            left.code
                .cmp(&right.code)
                .then_with(|| left.message.cmp(&right.message))
        });
        diagnostics
    }

    pub(crate) fn identity(&self) -> VocabularyIdentity {
        VocabularyIdentity {
            id: self.id.clone(),
            version: self.version,
            digest: self.digest.clone(),
        }
    }

    pub(crate) fn validate_relation(
        &self,
        id: &str,
        declaration: &Value,
        object_kinds: &BTreeMap<String, String>,
    ) -> Vec<Diagnostic> {
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
        let mut diagnostics = Vec::new();
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

    pub(crate) fn object_reference_ids(&self, declaration: &Value) -> Vec<String> {
        let Some(kind) = declaration["kind"].as_str() else {
            return Vec::new();
        };
        let Some(definition) = self.object_kinds.get(kind) else {
            return Vec::new();
        };
        let Some(attributes) = declaration["attributes"].as_object() else {
            return Vec::new();
        };
        let mut references = Vec::new();
        for (name, value_type) in definition
            .required_attributes
            .iter()
            .chain(definition.optional_attributes.iter())
        {
            let Some(value) = attributes.get(name) else {
                continue;
            };
            match value_type {
                ValueType::Identifier => {
                    if let Some(reference) = value.as_str() {
                        references.push(reference.to_owned());
                    }
                }
                ValueType::IdentifierList => {
                    references.extend(
                        value
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(Value::as_str)
                            .map(str::to_owned),
                    );
                }
                _ => {}
            }
        }
        references.sort();
        references.dedup();
        references
    }

    pub(crate) fn has_object_kind(&self, kind: &str) -> bool {
        self.object_kinds.contains_key(kind)
    }

    pub(crate) fn has_predicate(&self, predicate: &str) -> bool {
        self.predicates.contains_key(predicate)
    }

    fn validate_object(&self, id: &str, declaration: &Value) -> Vec<Diagnostic> {
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
        let mut diagnostics = Vec::new();
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
            match definition
                .required_attributes
                .get(name)
                .or_else(|| definition.optional_attributes.get(name))
            {
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

    fn validate_binding(&self, id: &str, declaration: &Value) -> Vec<Diagnostic> {
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
        let mut diagnostics = validate_typed_map(
            "binding",
            id,
            declaration["selector"]
                .as_object()
                .expect("schema validated selector"),
            &definition.required_selector,
            &definition.optional_selector,
        );
        diagnostics.extend(self.validate_lifecycle(declaration, id));
        diagnostics
    }

    fn validate_constraint(&self, id: &str, declaration: &Value) -> Vec<Diagnostic> {
        let rule = declaration["rule"].as_str().expect("schema validated rule");
        let Some(definition) = self.constraint_rules.get(rule) else {
            return vec![Diagnostic::error(
                "vocabulary.unknown-constraint-rule",
                format!("{id} uses unknown constraint rule {rule}"),
            )];
        };
        let mut diagnostics = validate_typed_map(
            "constraint",
            id,
            declaration["arguments"]
                .as_object()
                .expect("schema validated arguments"),
            &definition.required_arguments,
            &definition.optional_arguments,
        );
        diagnostics.extend(self.validate_lifecycle(declaration, id));
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
        if document["change"]["type"].as_str() == Some("REPLACE")
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
            if fact["mode"].as_str() == Some("inferred")
                && fact.get("confidenceBasisPoints").is_none()
            {
                diagnostics.push(Diagnostic::error(
                    "observation.inferred-confidence-missing",
                    format!("{id} is inferred but has no confidenceBasisPoints"),
                ));
            }
        }
        diagnostics
    }

    fn validate_conformance(&self, document: &Value) -> Vec<Diagnostic> {
        if document["metadata"]["generated"].as_bool() == Some(true) {
            Vec::new()
        } else {
            vec![Diagnostic::error(
                "generated.metadata-required",
                "ArchitectureConformance metadata.generated must be true",
            )]
        }
    }

    fn validate_reference(&self, document: &Value) -> Vec<Diagnostic> {
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
                format!("document does not pin vocabulary digest {}", self.digest),
            ));
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
        for group in ["governing", "supporting"] {
            for authority in decision["authority"][group]
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
                format!("{id} is missing required {category} field {name}"),
            )),
        }
    }
    for (name, value) in values {
        match required.get(name).or_else(|| optional.get(name)) {
            Some(value_type) if matches_value_type(value, *value_type) => {}
            Some(value_type) => diagnostics.push(type_mismatch(category, id, name, *value_type)),
            None => diagnostics.push(Diagnostic::error(
                format!("{category}.unknown-field"),
                format!("{id} has unknown {category} field {name}"),
            )),
        }
    }
    diagnostics
}

fn type_mismatch(category: &str, id: &str, name: &str, expected: ValueType) -> Diagnostic {
    Diagnostic::error(
        format!("{category}.field-type-mismatch"),
        format!("{id} field {name} does not match {expected:?}"),
    )
}

fn matches_value_type(value: &Value, value_type: ValueType) -> bool {
    match value_type {
        ValueType::String => value.is_string(),
        ValueType::Integer => value.as_i64().is_some(),
        ValueType::Boolean => value.is_boolean(),
        ValueType::Identifier => value.as_str().is_some_and(is_qualified_identifier),
        ValueType::Digest => value.as_str().is_some_and(valid_digest),
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

fn valid_digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

pub(crate) fn is_qualified_identifier(value: &str) -> bool {
    value.len() <= 200
        && value.split('.').count() >= 3
        && value.split('.').all(|segment| {
            segment
                .as_bytes()
                .first()
                .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        })
        && value.split('.').all(|segment| {
            segment
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        })
}

fn map<'a>(document: &'a Value, field: &str) -> &'a serde_json::Map<String, Value> {
    document[field]
        .as_object()
        .expect("schema validated mapping")
}

fn document_id(document: &Value) -> Option<&str> {
    document["metadata"]["id"].as_str()
}

#[cfg(test)]
mod tests;

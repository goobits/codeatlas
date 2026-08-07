mod manifests;

use super::digest::{digest_value, DigestKind, TypedDigest};
use super::graph::CompiledGraph;
use super::model::{
    valid_source_commit, valid_timestamp, GeneratedMetadata, GeneratorIdentity, RepositoryIdentity,
    SourceLocation, VocabularyIdentity,
};
use super::vocabulary::{is_qualified_identifier, Vocabulary};
use super::yaml::{parse, ParseLimits};
use super::{Diagnostic, ARCHITECTURE_API_VERSION};
use manifests::{ManifestIndex, ManifestMatch};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const OBSERVER_ID: &str = "codeatlas.tool.architecture-observer";
const OBSERVER_VERSION: &str = "0.1";

#[derive(Clone, Debug)]
pub(crate) struct ObserveRequest {
    pub repository_root: PathBuf,
    pub repository_id: String,
    pub observation_id: String,
    pub source_commit: String,
    pub observed_at: String,
    pub source_inputs: Vec<String>,
}

#[derive(schemars::JsonSchema, Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CoverageStatus {
    Complete,
    Partial,
    Unsupported,
    Unknown,
}

#[derive(schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Coverage {
    pub repository_scope: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    pub feature: String,
    pub status: CoverageStatus,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub included_roots: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub excluded_roots: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub limitations: Vec<String>,
}

#[derive(schemars::JsonSchema, Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ObservationMode {
    Deterministic,
    Inferred,
}

#[derive(schemars::JsonSchema, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ObservationFact {
    pub target: String,
    pub observed_kind: String,
    pub attributes: Value,
    pub extractor: GeneratorIdentity,
    pub mode: ObservationMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence_basis_points: Option<u64>,
    pub source_locations: Vec<SourceLocation>,
    pub coverage_ids: Vec<String>,
}

#[derive(schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ObservationDigests {
    pub observation_content_digest: TypedDigest,
    pub observation_envelope_digest: TypedDigest,
}

#[derive(schemars::JsonSchema, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ArchitectureObservation {
    pub api_version: String,
    pub kind: String,
    pub metadata: GeneratedMetadata,
    pub vocabulary: VocabularyIdentity,
    pub repository: RepositoryIdentity,
    pub source_commit: String,
    pub coverage: BTreeMap<String, Coverage>,
    pub facts: BTreeMap<String, ObservationFact>,
    pub digests: ObservationDigests,
}

pub(crate) fn observe(
    graph: &CompiledGraph,
    request: &ObserveRequest,
) -> Result<ArchitectureObservation, Vec<Diagnostic>> {
    let vocabulary = Vocabulary::bundled()?;
    let mut diagnostics = validate_request(request);
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    let manifests = ManifestIndex::scan(&request.repository_root)?;
    let mut coverage = BTreeMap::new();
    let mut facts = BTreeMap::new();
    for (binding_id, binding) in &graph.bindings {
        let declaration = &binding.declaration;
        let adapter = declaration["adapter"]["kind"]
            .as_str()
            .expect("validated adapter");
        let selector_name = declaration["selector"]["name"]
            .as_str()
            .expect("validated manifest selector");
        let (coverage_id, matches, extractor_id, language) = match adapter {
            "npm.package" => (
                "codeatlas.coverage.npm-package-v1",
                manifests.npm(selector_name),
                "codeatlas.extractor.npm-package",
                "package-json",
            ),
            "rust.crate" => (
                "codeatlas.coverage.rust-crate-v1",
                manifests.rust(selector_name),
                "codeatlas.extractor.rust-crate",
                "toml",
            ),
            _ => {
                diagnostics.push(Diagnostic::error(
                    "observation.adapter-unsupported",
                    format!("{binding_id} uses unsupported observation adapter {adapter}"),
                ));
                continue;
            }
        };
        coverage
            .entry(coverage_id.to_owned())
            .or_insert_with(|| manifest_coverage(adapter, language));
        let target = declaration["target"]
            .as_str()
            .expect("validated binding target");
        let Some(target_object) = graph.objects.get(target) else {
            diagnostics.push(Diagnostic::error(
                "observation.binding-target-missing",
                format!("{binding_id} targets missing object {target}"),
            ));
            continue;
        };
        let observed_kind = target_object.declaration["kind"]
            .as_str()
            .expect("validated object kind");
        let context = ManifestFactContext {
            binding_id,
            target,
            observed_kind,
            adapter,
            selector_name,
            coverage_id,
            extractor_id,
        };
        add_manifest_facts(&mut facts, &context, matches);
    }
    if !diagnostics.is_empty() {
        diagnostics.sort_by(|left, right| {
            left.code
                .cmp(&right.code)
                .then_with(|| left.message.cmp(&right.message))
        });
        return Err(diagnostics);
    }

    let mut source_inputs = request.source_inputs.clone();
    source_inputs.sort();
    source_inputs.dedup();
    let generator = GeneratorIdentity {
        id: OBSERVER_ID.to_owned(),
        version: OBSERVER_VERSION.to_owned(),
    };
    let content_digest = digest_value(
        DigestKind::ObservationContent,
        &json!({"coverage": &coverage, "facts": &facts}),
    )
    .map_err(|error| vec![*error.diagnostic])?;
    let generation_command = "codeatlas scan architecture".to_owned();
    let repository = RepositoryIdentity {
        id: request.repository_id.clone(),
    };
    let envelope_digest = digest_value(
        DigestKind::ObservationEnvelope,
        &json!({
            "repository": &repository,
            "sourceCommit": &request.source_commit,
            "observationContentDigest": &content_digest,
            "generator": &generator,
            "generatedAt": &request.observed_at,
            "sourceInputs": &source_inputs,
            "generationCommand": &generation_command,
        }),
    )
    .map_err(|error| vec![*error.diagnostic])?;
    let observation = ArchitectureObservation {
        api_version: ARCHITECTURE_API_VERSION.to_owned(),
        kind: "ArchitectureObservation".to_owned(),
        metadata: GeneratedMetadata {
            id: request.observation_id.clone(),
            name: format!("{} architecture observation", request.repository_id),
            architecture_version: 1,
            generated: true,
            generator,
            generated_at: request.observed_at.clone(),
            source_inputs,
            generation_command,
            manual_editing: "prohibited".to_owned(),
        },
        vocabulary: vocabulary.identity(),
        repository,
        source_commit: request.source_commit.clone(),
        coverage,
        facts,
        digests: ObservationDigests {
            observation_content_digest: content_digest,
            observation_envelope_digest: envelope_digest,
        },
    };
    validate(&observation, &vocabulary)?;
    Ok(observation)
}

pub(super) fn load(path: &Path) -> Result<ArchitectureObservation, Vec<Diagnostic>> {
    let bytes = fs::read(path).map_err(|error| {
        vec![Diagnostic::error(
            "observation.read-failed",
            format!("{}: {error}", path.display()),
        )
        .at_path(path)]
    })?;
    let document = parse(&bytes, ParseLimits::default())
        .map_err(|error| vec![error.diagnostic.at_path(path)])?
        .value;
    let vocabulary = Vocabulary::bundled()?;
    let diagnostics = validate_document(&document, &vocabulary);
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    let observation: ArchitectureObservation =
        serde_json::from_value(document).map_err(|error| {
            vec![Diagnostic::error("observation.decode-failed", error.to_string()).at_path(path)]
        })?;
    validate_semantics(&observation)?;
    Ok(observation)
}

fn validate(
    observation: &ArchitectureObservation,
    vocabulary: &Vocabulary,
) -> Result<(), Vec<Diagnostic>> {
    let document = serde_json::to_value(observation).map_err(|error| {
        vec![Diagnostic::error(
            "observation.serialization-failed",
            error.to_string(),
        )]
    })?;
    let mut diagnostics = validate_document(&document, vocabulary);
    if let Err(mut semantic_diagnostics) = validate_semantics(observation) {
        diagnostics.append(&mut semantic_diagnostics);
    }
    if diagnostics.is_empty() {
        Ok(())
    } else {
        Err(diagnostics)
    }
}

fn validate_semantics(observation: &ArchitectureObservation) -> Result<(), Vec<Diagnostic>> {
    let mut diagnostics = Vec::new();
    if !is_qualified_identifier(&observation.metadata.id)
        || !is_qualified_identifier(&observation.repository.id)
    {
        diagnostics.push(Diagnostic::error(
            "observation.identifier-invalid",
            "observation and repository IDs must be qualified architecture identifiers",
        ));
    }
    if !valid_source_commit(&observation.source_commit) {
        diagnostics.push(Diagnostic::error(
            "observation.source-commit-invalid",
            "sourceCommit must contain 7 to 64 lowercase hexadecimal characters",
        ));
    }
    if !valid_timestamp(&observation.metadata.generated_at) {
        diagnostics.push(Diagnostic::error(
            "observation.timestamp-invalid",
            "generatedAt must use RFC 3339 UTC seconds",
        ));
    }
    if diagnostics.is_empty() {
        Ok(())
    } else {
        Err(diagnostics)
    }
}

fn validate_document(document: &Value, vocabulary: &Vocabulary) -> Vec<Diagnostic> {
    let mut diagnostics = vocabulary.validate_document(document);
    if !diagnostics.is_empty() {
        return diagnostics;
    }
    match digest_value(
        DigestKind::ObservationContent,
        &json!({
            "coverage": &document["coverage"],
            "facts": &document["facts"],
        }),
    ) {
        Ok(content_digest)
            if document["digests"]["observationContentDigest"].as_str()
                != Some(content_digest.as_str()) =>
        {
            diagnostics.push(Diagnostic::error(
                "observation.content-digest-mismatch",
                "observationContentDigest does not match the semantic facts and coverage",
            ));
        }
        Err(error) => diagnostics.push(*error.diagnostic),
        _ => {}
    }
    match digest_value(
        DigestKind::ObservationEnvelope,
        &json!({
            "repository": &document["repository"],
            "sourceCommit": &document["sourceCommit"],
            "observationContentDigest": &document["digests"]["observationContentDigest"],
            "generator": &document["metadata"]["generator"],
            "generatedAt": &document["metadata"]["generatedAt"],
            "sourceInputs": &document["metadata"]["sourceInputs"],
            "generationCommand": &document["metadata"]["generationCommand"],
        }),
    ) {
        Ok(envelope_digest)
            if document["digests"]["observationEnvelopeDigest"].as_str()
                != Some(envelope_digest.as_str()) =>
        {
            diagnostics.push(Diagnostic::error(
                "observation.envelope-digest-mismatch",
                "observationEnvelopeDigest does not match its provenance envelope",
            ));
        }
        Err(error) => diagnostics.push(*error.diagnostic),
        _ => {}
    }
    diagnostics
}

fn validate_request(request: &ObserveRequest) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for (field, value) in [
        ("repository ID", request.repository_id.as_str()),
        ("observation ID", request.observation_id.as_str()),
    ] {
        if !is_qualified_identifier(value) {
            diagnostics.push(Diagnostic::error(
                "observation.identifier-invalid",
                format!("{field} is not a qualified architecture identifier: {value}"),
            ));
        }
    }
    if !valid_source_commit(&request.source_commit) {
        diagnostics.push(Diagnostic::error(
            "observation.source-commit-invalid",
            "source commit must contain 7 to 64 lowercase hexadecimal characters",
        ));
    }
    if !valid_timestamp(&request.observed_at) {
        diagnostics.push(Diagnostic::error(
            "observation.timestamp-invalid",
            "observed-at must use RFC 3339 UTC seconds, for example 2026-07-23T00:00:00Z",
        ));
    }
    diagnostics
}

fn manifest_coverage(adapter: &str, language: &str) -> Coverage {
    Coverage {
        repository_scope: ".".to_owned(),
        language: Some(language.to_owned()),
        feature: adapter.to_owned(),
        status: CoverageStatus::Complete,
        included_roots: vec![".".to_owned()],
        excluded_roots: vec![
            ".git".to_owned(),
            "build".to_owned(),
            "coverage".to_owned(),
            "dist".to_owned(),
            "node_modules".to_owned(),
            "target".to_owned(),
            "tests".to_owned(),
        ],
        limitations: vec![
            "Generated, dependency, build, and test-fixture directories are outside maintained manifest coverage."
                .to_owned(),
        ],
    }
}

struct ManifestFactContext<'a> {
    binding_id: &'a str,
    target: &'a str,
    observed_kind: &'a str,
    adapter: &'a str,
    selector_name: &'a str,
    coverage_id: &'a str,
    extractor_id: &'a str,
}

fn add_manifest_facts(
    output: &mut BTreeMap<String, ObservationFact>,
    context: &ManifestFactContext<'_>,
    matches: &[ManifestMatch],
) {
    for (index, manifest) in matches.iter().enumerate() {
        let fact_id = format!("{}.fact-{}", context.binding_id, index + 1);
        output.insert(
            fact_id,
            ObservationFact {
                target: context.target.to_owned(),
                observed_kind: context.observed_kind.to_owned(),
                attributes: json!({
                    "adapter": context.adapter,
                    "bindingId": context.binding_id,
                    "matchedPath": &manifest.location.path,
                    "name": &manifest.name,
                    "selector": context.selector_name,
                    "version": &manifest.version,
                }),
                extractor: GeneratorIdentity {
                    id: context.extractor_id.to_owned(),
                    version: "1".to_owned(),
                },
                mode: ObservationMode::Deterministic,
                confidence_basis_points: None,
                source_locations: vec![manifest.location.clone()],
                coverage_ids: vec![context.coverage_id.to_owned()],
            },
        );
    }
}

pub(crate) fn source_input_paths(modules: &[PathBuf], source_root: &Path) -> Vec<String> {
    modules
        .iter()
        .map(|module| {
            let path = if module.is_absolute() {
                module.clone()
            } else {
                source_root.join(module)
            };
            crate::paths::normalize_relative_path(&path, source_root)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{observe, ObserveRequest};
    use crate::architecture::compiler::{compile, CompileRequest};
    use crate::architecture::graph::CompileMode;
    use std::path::PathBuf;

    #[test]
    fn observes_manifest_bindings_deterministically() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let compiled = compile(&CompileRequest {
            roots: vec![root.join(
                "spec/architecture/v0.1/examples/workshop-codeatlas/architecture.atlas.yaml",
            )],
            allowed_root: root.clone(),
            mode: CompileMode::Governing,
        })
        .expect("compile");
        let request = ObserveRequest {
            repository_root: root,
            repository_id: "codeatlas.repository.source".to_owned(),
            observation_id: "codeatlas.observation.source".to_owned(),
            source_commit: "0123456789abcdef".to_owned(),
            observed_at: "2026-07-23T00:00:00Z".to_owned(),
            source_inputs: vec!["workshop-codeatlas.atlas.yaml".to_owned()],
        };
        let first = observe(&compiled.report.graph, &request).expect("observe");
        let second = observe(&compiled.report.graph, &request).expect("observe");
        assert_eq!(first, second);
        assert!(first
            .facts
            .values()
            .any(|fact| fact.attributes["name"] == "codeatlas"));
    }
}

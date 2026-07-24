use super::compiler::CompileResult;
use super::diagnostic::{Diagnostic, Severity};
use super::digest::{digest_value, DigestKind, TypedDigest};
use super::graph::{CompileMode, GraphDeclaration};
use super::model::{valid_timestamp, GeneratedMetadata, GeneratorIdentity, VocabularyIdentity};
use super::observation::{
    self, ArchitectureObservation, CoverageStatus, ObservationFact, ObservationMode,
};
use super::policy::{evaluate_exception, ExceptionContext, ExceptionDisposition, PolicySet};
use super::vocabulary::{is_qualified_identifier, Vocabulary};
use super::ARCHITECTURE_API_VERSION;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

const VALIDATOR_ID: &str = "codeatlas.tool.architecture-conformance";
const VALIDATOR_VERSION: &str = "0.1";

pub(crate) struct ConformanceRequest {
    pub policy_roots: Vec<PathBuf>,
    pub policy_allowed_root: PathBuf,
    pub observation_path: PathBuf,
    pub conformance_id: String,
    pub as_of: String,
    pub source_inputs: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConformanceInputs {
    pub governing_graph_digest: TypedDigest,
    pub architecture_closure_digest: TypedDigest,
    pub policy_closure_digest: TypedDigest,
    pub observation_content_digest: TypedDigest,
    pub vocabulary_digest: TypedDigest,
    pub validator_version: String,
    pub as_of: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ConformanceState {
    Matched,
    Partial,
    Absent,
    Conflicting,
    Unexpected,
    Unobserved,
    Ambiguous,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConformanceEvidence {
    pub fact_ids: Vec<String>,
    pub coverage_ids: Vec<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct ExceptionDispositions {
    pub applied: Vec<String>,
    pub stale: Vec<String>,
    pub expired: Vec<String>,
    pub irrelevant: Vec<String>,
    pub rejected: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConformanceResult {
    pub declaration_id: String,
    pub state: ConformanceState,
    pub severity: Severity,
    pub reason_code: String,
    pub evidence: ConformanceEvidence,
    pub exceptions: ExceptionDispositions,
    pub explanation: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ArchitectureConformance {
    pub api_version: String,
    pub kind: String,
    pub metadata: GeneratedMetadata,
    pub vocabulary: VocabularyIdentity,
    pub conformance_inputs: ConformanceInputs,
    pub results: BTreeMap<String, ConformanceResult>,
    pub conformance_result_digest: TypedDigest,
}

impl ArchitectureConformance {
    pub(crate) fn has_errors(&self) -> bool {
        self.results
            .values()
            .any(|result| result.severity == Severity::Error)
    }
}

pub(crate) fn conform(
    compilation: &CompileResult,
    request: &ConformanceRequest,
) -> Result<ArchitectureConformance, Vec<Diagnostic>> {
    let mut diagnostics = validate_request(compilation, request);
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    let vocabulary = Vocabulary::bundled()?;
    let observation = observation::load(&request.observation_path)?;
    if observation.vocabulary != compilation.report.vocabulary {
        return Err(vec![Diagnostic::error(
            "conformance.vocabulary-mismatch",
            "the observation and governing graph use different vocabularies",
        )]);
    }
    let policies = PolicySet::load(
        &request.policy_roots,
        &request.policy_allowed_root,
        &vocabulary,
    )?;
    let mut results = evaluate(compilation, &observation);
    apply_exceptions(&mut results, compilation, &policies, request.as_of.as_str());
    diagnostics.extend(validate_evidence(&results, &observation));
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }

    let inputs = ConformanceInputs {
        governing_graph_digest: compilation.report.graph_digest.clone(),
        architecture_closure_digest: compilation.report.architecture_closure_digest.clone(),
        policy_closure_digest: policies.digest,
        observation_content_digest: observation.digests.observation_content_digest.clone(),
        vocabulary_digest: vocabulary.identity().digest,
        validator_version: format!("{VALIDATOR_ID}/{VALIDATOR_VERSION}"),
        as_of: request.as_of.clone(),
    };
    let result_digest = digest_value(
        DigestKind::ConformanceResult,
        &json!({
            "conformanceInputs": &inputs,
            "result": &results,
        }),
    )
    .map_err(|error| vec![*error.diagnostic])?;
    let mut source_inputs = request.source_inputs.clone();
    source_inputs.sort();
    source_inputs.dedup();
    let conformance = ArchitectureConformance {
        api_version: ARCHITECTURE_API_VERSION.to_owned(),
        kind: "ArchitectureConformance".to_owned(),
        metadata: GeneratedMetadata {
            id: request.conformance_id.clone(),
            name: format!("{} architecture conformance", observation.repository.id),
            architecture_version: 1,
            generated: true,
            generator: GeneratorIdentity {
                id: VALIDATOR_ID.to_owned(),
                version: VALIDATOR_VERSION.to_owned(),
            },
            generated_at: request.as_of.clone(),
            source_inputs,
            generation_command: "codeatlas architecture conform".to_owned(),
            manual_editing: "prohibited".to_owned(),
        },
        vocabulary: vocabulary.identity(),
        conformance_inputs: inputs,
        results,
        conformance_result_digest: result_digest,
    };
    let document = serde_json::to_value(&conformance).map_err(|error| {
        vec![Diagnostic::error(
            "conformance.serialization-failed",
            error.to_string(),
        )]
    })?;
    let diagnostics = vocabulary.validate_document(&document);
    if diagnostics.is_empty() {
        Ok(conformance)
    } else {
        Err(diagnostics)
    }
}

fn validate_request(compilation: &CompileResult, request: &ConformanceRequest) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    if compilation.report.mode != CompileMode::Governing {
        diagnostics.push(Diagnostic::error(
            "conformance.governing-graph-required",
            "conformance can evaluate only a governing graph",
        ));
    }
    if !is_qualified_identifier(&request.conformance_id) {
        diagnostics.push(Diagnostic::error(
            "conformance.identifier-invalid",
            "conformance ID must be a qualified architecture identifier",
        ));
    }
    if !valid_timestamp(&request.as_of) {
        diagnostics.push(Diagnostic::error(
            "conformance.timestamp-invalid",
            "as-of must use RFC 3339 UTC seconds",
        ));
    }
    diagnostics
}

fn evaluate(
    compilation: &CompileResult,
    observation: &ArchitectureObservation,
) -> BTreeMap<String, ConformanceResult> {
    let graph = &compilation.report.graph;
    let mut results = BTreeMap::new();
    let mut evaluated_targets = BTreeSet::new();
    for (binding_id, binding) in &graph.bindings {
        let target = binding.declaration["target"]
            .as_str()
            .expect("validated binding target");
        evaluated_targets.insert(target.to_owned());
        results.insert(
            format!("{binding_id}.conformance"),
            evaluate_binding(
                binding_id,
                binding,
                target,
                graph.objects[target].declaration["kind"]
                    .as_str()
                    .expect("validated target kind"),
                observation,
            ),
        );
    }
    for (id, declaration) in &graph.objects {
        if !evaluated_targets.contains(id) {
            results.insert(
                format!("{id}.conformance"),
                evaluate_unbound_object(id, declaration, observation),
            );
        }
    }
    for (id, declaration) in &graph.constraints {
        results.insert(
            format!("{id}.conformance"),
            evaluate_unobserved_constraint(id, declaration),
        );
    }
    let declared = graph
        .objects
        .keys()
        .chain(graph.relations.keys())
        .chain(graph.constraints.keys())
        .collect::<BTreeSet<_>>();
    for (fact_id, fact) in &observation.facts {
        if !declared.contains(&fact.target) {
            results.insert(
                format!("{fact_id}.conformance"),
                evaluate_unexpected(fact_id, fact, observation),
            );
        }
    }
    results
}

fn evaluate_binding(
    binding_id: &str,
    binding: &GraphDeclaration,
    target: &str,
    expected_kind: &str,
    observation: &ArchitectureObservation,
) -> ConformanceResult {
    let adapter = binding.declaration["adapter"]["kind"]
        .as_str()
        .expect("validated adapter");
    let cardinality = binding.declaration["cardinality"]
        .as_str()
        .expect("validated cardinality");
    let facts = observation
        .facts
        .iter()
        .filter(|(_, fact)| {
            fact.target == target
                && fact.attributes["bindingId"]
                    .as_str()
                    .is_none_or(|observed_binding| observed_binding == binding_id)
        })
        .map(|(id, fact)| (id.as_str(), fact))
        .collect::<Vec<_>>();
    let coverage = observation
        .coverage
        .iter()
        .filter(|(_, coverage)| coverage.feature == adapter)
        .map(|(id, coverage)| (id.as_str(), coverage))
        .collect::<Vec<_>>();
    let evidence = evidence(&facts, &coverage);
    let deterministic = facts
        .iter()
        .all(|(_, fact)| fact.mode == ObservationMode::Deterministic);
    let complete = !coverage.is_empty()
        && coverage
            .iter()
            .all(|(_, coverage)| coverage.status == CoverageStatus::Complete);
    let partial = coverage
        .iter()
        .any(|(_, coverage)| coverage.status == CoverageStatus::Partial);
    let kind_conflict = facts
        .iter()
        .any(|(_, fact)| fact.observed_kind != expected_kind);
    let count = facts.len();
    let cardinality_matches = match cardinality {
        "exactly_one" => count == 1,
        "at_most_one" => count <= 1,
        "one_or_more" => count >= 1,
        "any" => true,
        _ => unreachable!("validated cardinality"),
    };
    let required = matches!(cardinality, "exactly_one" | "one_or_more");

    if facts
        .iter()
        .any(|(_, fact)| fact.mode == ObservationMode::Inferred)
    {
        return result(
            target,
            ConformanceState::Ambiguous,
            Severity::Advisory,
            "evidence.inferred-review-only",
            evidence,
            "Inferred evidence cannot establish an accepted deterministic binding.",
        );
    }
    if coverage.is_empty()
        || coverage.iter().any(|(_, coverage)| {
            matches!(
                coverage.status,
                CoverageStatus::Unsupported | CoverageStatus::Unknown
            )
        })
    {
        return result(
            target,
            ConformanceState::Unobserved,
            Severity::Advisory,
            "coverage.unsupported",
            evidence,
            "No supported complete extractor coverage evaluates this binding.",
        );
    }
    if partial {
        return result(
            target,
            if facts.is_empty() {
                ConformanceState::Unobserved
            } else {
                ConformanceState::Partial
            },
            Severity::Advisory,
            "coverage.partial",
            evidence,
            "Partial extractor coverage cannot establish the complete binding.",
        );
    }
    if complete && deterministic && kind_conflict {
        return result(
            target,
            ConformanceState::Conflicting,
            Severity::Error,
            "binding.kind-conflict",
            evidence,
            "Deterministic evidence reports a construct with the wrong declared kind.",
        );
    }
    if complete && deterministic && !cardinality_matches {
        let (state, reason, explanation) = if required && facts.is_empty() {
            (
                ConformanceState::Absent,
                "binding.required-absent",
                "Complete deterministic coverage found no required construct.",
            )
        } else {
            (
                ConformanceState::Conflicting,
                "binding.cardinality-conflict",
                "Complete deterministic evidence violates the declared binding cardinality.",
            )
        };
        return result(
            target,
            state,
            Severity::Error,
            reason,
            evidence,
            explanation,
        );
    }
    result(
        target,
        ConformanceState::Matched,
        Severity::Advisory,
        "binding.exact-match",
        evidence,
        "The accepted binding matched deterministic implementation evidence.",
    )
}

fn evaluate_unbound_object(
    declaration_id: &str,
    _declaration: &GraphDeclaration,
    observation: &ArchitectureObservation,
) -> ConformanceResult {
    let facts = observation
        .facts
        .iter()
        .filter(|(_, fact)| fact.target == declaration_id)
        .map(|(id, fact)| (id.as_str(), fact))
        .collect::<Vec<_>>();
    let coverage = coverage_for_facts(&facts, observation);
    if facts.is_empty() {
        return result(
            declaration_id,
            ConformanceState::Unobserved,
            Severity::Advisory,
            "binding.missing",
            evidence(&facts, &coverage),
            "No accepted binding maps this declaration to observable implementation.",
        );
    }
    result(
        declaration_id,
        ConformanceState::Ambiguous,
        Severity::Advisory,
        "binding.missing-for-evidence",
        evidence(&facts, &coverage),
        "Implementation evidence exists, but no accepted binding establishes its mapping.",
    )
}

fn evaluate_unobserved_constraint(
    declaration_id: &str,
    _declaration: &GraphDeclaration,
) -> ConformanceResult {
    result(
        declaration_id,
        ConformanceState::Unobserved,
        Severity::Advisory,
        "coverage.unsupported",
        ConformanceEvidence {
            fact_ids: Vec::new(),
            coverage_ids: Vec::new(),
        },
        "No observed source-graph coverage evaluates this architecture constraint.",
    )
}

fn evaluate_unexpected(
    fact_id: &str,
    fact: &ObservationFact,
    observation: &ArchitectureObservation,
) -> ConformanceResult {
    let facts = vec![(fact_id, fact)];
    let coverage = coverage_for_facts(&facts, observation);
    let complete = !coverage.is_empty()
        && coverage
            .iter()
            .all(|(_, coverage)| coverage.status == CoverageStatus::Complete);
    if fact.mode == ObservationMode::Deterministic && complete {
        result(
            &fact.target,
            ConformanceState::Unexpected,
            Severity::Error,
            "declaration.unexpected",
            evidence(&facts, &coverage),
            "Complete deterministic evidence found a governed construct with no declaration.",
        )
    } else {
        result(
            &fact.target,
            ConformanceState::Ambiguous,
            Severity::Advisory,
            "declaration.possibly-unexpected",
            evidence(&facts, &coverage),
            "Incomplete or inferred evidence cannot establish an unexpected construct.",
        )
    }
}

fn coverage_for_facts<'a>(
    facts: &[(&str, &'a ObservationFact)],
    observation: &'a ArchitectureObservation,
) -> Vec<(&'a str, &'a super::observation::Coverage)> {
    let ids = facts
        .iter()
        .flat_map(|(_, fact)| fact.coverage_ids.iter())
        .collect::<BTreeSet<_>>();
    ids.into_iter()
        .filter_map(|id| {
            observation
                .coverage
                .get_key_value(id)
                .map(|(id, coverage)| (id.as_str(), coverage))
        })
        .collect()
}

fn evidence(
    facts: &[(&str, &ObservationFact)],
    coverage: &[(&str, &super::observation::Coverage)],
) -> ConformanceEvidence {
    ConformanceEvidence {
        fact_ids: facts.iter().map(|(id, _)| (*id).to_owned()).collect(),
        coverage_ids: coverage.iter().map(|(id, _)| (*id).to_owned()).collect(),
    }
}

fn result(
    declaration_id: &str,
    state: ConformanceState,
    severity: Severity,
    reason_code: &str,
    evidence: ConformanceEvidence,
    explanation: &str,
) -> ConformanceResult {
    ConformanceResult {
        declaration_id: declaration_id.to_owned(),
        state,
        severity,
        reason_code: reason_code.to_owned(),
        evidence,
        exceptions: ExceptionDispositions::default(),
        explanation: explanation.to_owned(),
    }
}

fn apply_exceptions(
    results: &mut BTreeMap<String, ConformanceResult>,
    compilation: &CompileResult,
    policies: &PolicySet,
    as_of: &str,
) {
    for result in results.values_mut() {
        let Some(declaring_module) =
            declaring_constraint_module(compilation, &result.declaration_id)
        else {
            continue;
        };
        let Some(closure) = compilation
            .lockfile
            .documents
            .iter()
            .find(|document| document.module_id == declaring_module)
            .map(|document| document.import_closure_digest.as_str())
        else {
            continue;
        };
        let context = ExceptionContext {
            constraint_id: &result.declaration_id,
            constraint_version: 1,
            declaration_id: &result.declaration_id,
            declaring_module,
            affected_closure_digest: closure,
            as_of,
        };
        for exception in &policies.exceptions {
            let Some(disposition) = evaluate_exception(exception, &context) else {
                continue;
            };
            let output = match disposition {
                ExceptionDisposition::Applied => {
                    if result.severity == Severity::Advisory {
                        &mut result.exceptions.irrelevant
                    } else {
                        result.severity = Severity::Advisory;
                        &mut result.exceptions.applied
                    }
                }
                ExceptionDisposition::Stale => &mut result.exceptions.stale,
                ExceptionDisposition::Expired => &mut result.exceptions.expired,
                ExceptionDisposition::Irrelevant => &mut result.exceptions.irrelevant,
                ExceptionDisposition::Rejected => &mut result.exceptions.rejected,
            };
            output.push(exception.id.clone());
        }
    }
}

fn declaring_constraint_module<'a>(
    compilation: &'a CompileResult,
    declaration_id: &str,
) -> Option<&'a str> {
    compilation
        .report
        .graph
        .constraints
        .get(declaration_id)
        .map(|declaration| declaration.module.as_str())
}

fn validate_evidence(
    results: &BTreeMap<String, ConformanceResult>,
    observation: &ArchitectureObservation,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for (result_id, result) in results {
        let deterministic = result.evidence.fact_ids.iter().all(|id| {
            observation
                .facts
                .get(id)
                .is_some_and(|fact| fact.mode == ObservationMode::Deterministic)
        });
        let complete = !result.evidence.coverage_ids.is_empty()
            && result.evidence.coverage_ids.iter().all(|id| {
                observation
                    .coverage
                    .get(id)
                    .is_some_and(|coverage| coverage.status == CoverageStatus::Complete)
            });
        if result.severity == Severity::Error
            && matches!(
                result.state,
                ConformanceState::Absent
                    | ConformanceState::Conflicting
                    | ConformanceState::Unexpected
            )
            && (!complete
                || (matches!(
                    result.state,
                    ConformanceState::Conflicting | ConformanceState::Unexpected
                ) && !deterministic))
        {
            diagnostics.push(Diagnostic::error(
                "conformance.hard-gate-without-complete-deterministic-evidence",
                format!("{result_id} cannot be an error without complete deterministic evidence"),
            ));
        }
        if result.state == ConformanceState::Matched
            && result.evidence.fact_ids.iter().any(|id| {
                observation
                    .facts
                    .get(id)
                    .is_some_and(|fact| fact.mode == ObservationMode::Inferred)
            })
        {
            diagnostics.push(Diagnostic::error(
                "conformance.inferred-evidence-cannot-match",
                format!("{result_id} reports inferred evidence as matched"),
            ));
        }
    }
    diagnostics
}

pub(crate) fn source_inputs(
    modules: &[PathBuf],
    policies: &[PathBuf],
    observation: &Path,
    source_root: &Path,
) -> Vec<String> {
    let mut paths = modules.iter().chain(policies).cloned().collect::<Vec<_>>();
    paths.push(observation.to_path_buf());
    paths
        .iter()
        .map(|path| {
            let path = if path.is_absolute() {
                path.clone()
            } else {
                source_root.join(path)
            };
            crate::paths::normalize_relative_path(&path, source_root)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{conform, evaluate, ConformanceRequest, ConformanceState};
    use crate::architecture::compiler::{compile, CompileRequest};
    use crate::architecture::graph::CompileMode;
    use crate::architecture::observation::{
        observe, CoverageStatus, ObservationMode, ObserveRequest,
    };
    use std::fs;
    use std::path::{Path, PathBuf};

    fn root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    fn temporary_file(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "codeatlas-conformance-{}-{name}",
            std::process::id()
        ))
    }

    fn remove_existing(path: &Path) {
        if path.exists() {
            fs::remove_file(path).expect("remove old temporary file");
        }
    }

    fn compilation_and_observation() -> (
        crate::architecture::CompileResult,
        crate::architecture::observation::ArchitectureObservation,
    ) {
        let root = root();
        let module =
            root.join("spec/architecture/v0.1/examples/workshop-codeatlas/architecture.atlas.yaml");
        let compilation = compile(&CompileRequest {
            roots: vec![module.clone()],
            allowed_root: root.clone(),
            mode: CompileMode::Governing,
        })
        .expect("compile");
        let observation = observe(
            &compilation.report.graph,
            &ObserveRequest {
                repository_root: root.clone(),
                repository_id: "codeatlas.repository.source".to_owned(),
                observation_id: "codeatlas.observation.source".to_owned(),
                source_commit: "0123456789abcdef".to_owned(),
                observed_at: "2026-07-23T00:00:00Z".to_owned(),
                source_inputs: vec!["architecture.atlas.yaml".to_owned()],
            },
        )
        .expect("observation");
        (compilation, observation)
    }

    #[test]
    fn deterministic_manifest_observation_matches_reproducibly() {
        let root = root();
        let (compilation, observation) = compilation_and_observation();
        let observation_path = temporary_file("observation.json");
        remove_existing(&observation_path);
        fs::write(
            &observation_path,
            serde_json::to_vec_pretty(&observation).expect("serialize observation"),
        )
        .expect("write observation");
        let request = ConformanceRequest {
            policy_roots: Vec::new(),
            policy_allowed_root: root,
            observation_path: observation_path.clone(),
            conformance_id: "codeatlas.conformance.source".to_owned(),
            as_of: "2026-07-23T00:00:00Z".to_owned(),
            source_inputs: vec!["architecture.atlas.yaml".to_owned()],
        };
        let report = conform(&compilation, &request).expect("conformance");
        let repeated = conform(&compilation, &request).expect("repeated conformance");
        assert_eq!(report, repeated);
        assert_eq!(
            report.results["codeatlas.binding.provider-crate.conformance"].state,
            ConformanceState::Matched
        );
        remove_existing(&observation_path);
    }

    #[test]
    fn uncertainty_never_becomes_a_false_hard_gate() {
        let (compilation, mut observation) = compilation_and_observation();
        observation.facts.clear();
        let absent = evaluate(&compilation, &observation);
        assert_eq!(
            absent["codeatlas.binding.provider-crate.conformance"].state,
            ConformanceState::Absent
        );

        observation
            .coverage
            .values_mut()
            .for_each(|coverage| coverage.status = CoverageStatus::Unsupported);
        let unsupported = evaluate(&compilation, &observation);
        assert_eq!(
            unsupported["codeatlas.binding.provider-crate.conformance"].state,
            ConformanceState::Unobserved
        );

        let (_, mut inferred) = compilation_and_observation();
        inferred.facts.values_mut().for_each(|fact| {
            fact.mode = ObservationMode::Inferred;
            fact.confidence_basis_points = Some(7_200);
        });
        let inferred_results = evaluate(&compilation, &inferred);
        assert_eq!(
            inferred_results["codeatlas.binding.provider-crate.conformance"].state,
            ConformanceState::Ambiguous
        );
    }

    #[test]
    fn complete_deterministic_unknown_targets_are_unexpected() {
        let (compilation, mut observation) = compilation_and_observation();
        let mut unexpected = observation
            .facts
            .values()
            .next()
            .expect("manifest fact")
            .clone();
        unexpected.target = "codeatlas.provider.undeclared".to_owned();
        unexpected.attributes["bindingId"] = serde_json::Value::Null;
        observation
            .facts
            .insert("codeatlas.fact.undeclared".to_owned(), unexpected);
        let results = evaluate(&compilation, &observation);
        assert_eq!(
            results["codeatlas.fact.undeclared.conformance"].state,
            ConformanceState::Unexpected
        );
    }

    #[test]
    fn accepted_reference_observation_remains_loadable() {
        let path = root().join(
            "spec/architecture/v0.1/examples/observation/architecture-observation.generated.yaml",
        );
        crate::architecture::observation::load(&path).expect("accepted observation");
    }
}

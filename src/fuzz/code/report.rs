use super::corpus::{map_signature, CorpusMappingIssue};
#[cfg(test)]
use crate::analysis::reachability::render_diagnostics;
use crate::analysis::reachability::Reachability;
use crate::domain::source_graph::{
    ContextScope, NodeId, SourceGraph, SourceLanguage, SourceNode, SourceVisibility,
};
use crate::domain::{CallableBlockReason, CallableContract, FuzzPolicyEvidence};
use crate::fuzz::corpus::{select_pairwise, CorpusDimension};
use crate::fuzz::FuzzFailureKind;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub(crate) const CODE_FUZZ_INVENTORY_SCHEMA_VERSION: &str = "codeatlas.code-fuzz-inventory/v1";
pub(crate) const CODE_FUZZ_REPORT_SCHEMA_VERSION: &str = "codeatlas.code-fuzz-report/v1";

#[derive(schemars::JsonSchema, Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CodeFuzzReportArtifactKind {
    Report,
}

#[derive(schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CodeFuzzReport {
    pub schema_version: String,
    pub kind: CodeFuzzReportArtifactKind,
    pub id: String,
    pub content_digest: String,
    pub plan_id: String,
    pub plan_content_digest: String,
    #[serde(flatten)]
    pub body: CodeFuzzReportBody,
}

#[derive(schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CodeFuzzReportBody {
    pub tool_version: String,
    pub target_id: String,
    pub callable_id: String,
    pub language: SourceLanguage,
    pub seed: String,
    pub deterministic_prefix_digest: String,
    pub deterministic_cases: u64,
    pub adaptive_cases: u64,
    pub alternate_behavior: bool,
    pub failures: Vec<CodeFuzzFailure>,
}

#[derive(schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CodeFuzzFailure {
    pub kind: FuzzFailureKind,
    pub minimized: bool,
    pub reproducer: crate::execution::ArtifactLink,
}

#[derive(Serialize)]
struct CodeFuzzReportIdentity<'a> {
    schema_version: &'static str,
    kind: CodeFuzzReportArtifactKind,
    plan_id: &'a str,
    plan_content_digest: &'a str,
    #[serde(flatten)]
    body: &'a CodeFuzzReportBody,
}

impl CodeFuzzReport {
    pub(crate) fn new(
        plan: &crate::execution::ExecutionPlan,
        body: CodeFuzzReportBody,
    ) -> Result<Self> {
        validate_code_fuzz_report_body(&body)?;
        let content_digest = digest_code_fuzz_report(&plan.id, &plan.content_digest, &body)?;
        let id = format!(
            "report_{}",
            crate::execution::artifact::validate_digest(&content_digest)
                .expect("fresh CodeAtlas report digest is valid")
        );
        Ok(Self {
            schema_version: CODE_FUZZ_REPORT_SCHEMA_VERSION.to_string(),
            kind: CodeFuzzReportArtifactKind::Report,
            id,
            content_digest,
            plan_id: plan.id.clone(),
            plan_content_digest: plan.content_digest.clone(),
            body,
        })
    }
}

impl crate::execution::artifact::ManagedArtifact for CodeFuzzReport {
    const DIRECTORY: &'static str = "reports";
    const PREFIX: &'static str = "report";
    const LABEL: &'static str = "code fuzz report";

    fn artifact_id(&self) -> &str {
        &self.id
    }

    fn verify_identity(&self) -> Result<()> {
        if self.schema_version != CODE_FUZZ_REPORT_SCHEMA_VERSION
            || self.kind != CodeFuzzReportArtifactKind::Report
        {
            anyhow::bail!("Unsupported code fuzz report artifact identity");
        }
        crate::execution::artifact::validate_artifact_id(Self::PREFIX, &self.id)?;
        crate::execution::artifact::validate_artifact_id("plan", &self.plan_id)?;
        crate::execution::artifact::validate_digest(&self.plan_content_digest)?;
        validate_code_fuzz_report_body(&self.body)?;
        let expected =
            digest_code_fuzz_report(&self.plan_id, &self.plan_content_digest, &self.body)?;
        let expected_id = format!(
            "report_{}",
            crate::execution::artifact::validate_digest(&expected)
                .expect("fresh CodeAtlas report digest is valid")
        );
        if self.id != expected_id || self.content_digest != expected {
            anyhow::bail!("Code fuzz report identity does not match its canonical body");
        }
        Ok(())
    }
}

fn digest_code_fuzz_report(
    plan_id: &str,
    plan_content_digest: &str,
    body: &CodeFuzzReportBody,
) -> Result<String> {
    crate::execution::artifact::digest_value(
        "atlas.codeatlas.dev/code-fuzz-report/v1",
        &CodeFuzzReportIdentity {
            schema_version: CODE_FUZZ_REPORT_SCHEMA_VERSION,
            kind: CodeFuzzReportArtifactKind::Report,
            plan_id,
            plan_content_digest,
            body,
        },
    )
}

fn validate_code_fuzz_report_body(body: &CodeFuzzReportBody) -> Result<()> {
    for (label, value) in [
        ("tool version", body.tool_version.as_str()),
        ("target ID", body.target_id.as_str()),
        ("callable ID", body.callable_id.as_str()),
        ("seed", body.seed.as_str()),
    ] {
        if value.trim() != value || value.is_empty() || value.chars().any(char::is_control) {
            anyhow::bail!("Code fuzz report {label} is invalid");
        }
    }
    body.seed
        .parse::<u128>()
        .map_err(|_| anyhow::anyhow!("Code fuzz report seed is invalid"))?;
    crate::execution::artifact::validate_digest(&body.deterministic_prefix_digest)?;
    crate::execution::artifact::validate_artifact_links(
        &body
            .failures
            .iter()
            .map(|failure| failure.reproducer.clone())
            .collect::<Vec<_>>(),
    )?;
    if body
        .failures
        .iter()
        .any(|failure| failure.reproducer.kind != "reproducer")
    {
        anyhow::bail!("Code fuzz report failure must link a reproducer");
    }
    Ok(())
}

#[derive(schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CodeFuzzInventory {
    pub schema_version: String,
    pub tool_version: String,
    pub max_cases: u64,
    pub contracts: Vec<CodeFuzzContract>,
}

#[derive(schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CodeFuzzContract {
    pub target: NodeId,
    pub project: String,
    pub path: String,
    pub symbol: String,
    pub language: SourceLanguage,
    pub public_contexts: Vec<String>,
    pub callable: CallableContract,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_policy: Option<FuzzPolicyEvidence>,
    pub signatures: Vec<CodeFuzzSignatureCorpus>,
    pub oracle_evidence: Vec<CodeFuzzOracleEvidence>,
    pub callable_block_reasons: Vec<CallableBlockReason>,
    pub fuzz_block_reasons: Vec<CodeFuzzBlockReason>,
    pub status: CodeFuzzability,
}

impl CodeFuzzContract {
    pub(crate) fn selector(&self) -> String {
        format!("{}#{}", self.path, self.symbol)
    }
}

#[derive(schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CodeFuzzSignatureCorpus {
    pub signature: usize,
    pub dimensions: Vec<CorpusDimension>,
    pub deterministic_cases: Vec<Vec<usize>>,
    pub pairwise_complete: bool,
    pub mapping_issues: Vec<CorpusMappingIssue>,
}

#[derive(schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CodeFuzzability {
    ReadyForPlanning,
    Blocked,
}

#[derive(
    schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize,
)]
#[serde(deny_unknown_fields)]
pub(crate) struct CodeFuzzBlockReason {
    pub kind: CodeFuzzBlockKind,
    pub subject: String,
}

#[derive(
    schemars::JsonSchema, Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CodeFuzzBlockKind {
    BlockedByPolicy,
    MalformedDirective,
    NotPubliclyReachable,
    UnsupportedLanguage,
    UnsupportedCorpus,
}

impl CodeFuzzBlockKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::BlockedByPolicy => "blocked_by_policy",
            Self::MalformedDirective => "malformed_directive",
            Self::NotPubliclyReachable => "not_publicly_reachable",
            Self::UnsupportedLanguage => "unsupported_language",
            Self::UnsupportedCorpus => "unsupported_corpus",
        }
    }
}

#[derive(
    schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize,
)]
#[serde(deny_unknown_fields)]
pub(crate) struct CodeFuzzOracleEvidence {
    pub kind: CodeFuzzOracleKind,
    pub source: String,
}

#[derive(
    schemars::JsonSchema, Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CodeFuzzOracleKind {
    ResultShape,
}

#[cfg(test)]
pub(crate) fn build_inventory(
    graph: &SourceGraph,
    exclusions: &[String],
    max_cases: u64,
) -> Result<CodeFuzzInventory> {
    let reachability = Reachability::analyze(graph).map_err(render_diagnostics)?;
    build_inventory_with_reachability(graph, &reachability, exclusions, max_cases)
}

pub(crate) fn build_inventory_with_reachability(
    graph: &SourceGraph,
    reachability: &Reachability,
    exclusions: &[String],
    max_cases: u64,
) -> Result<CodeFuzzInventory> {
    let max_cases_limit = max_cases;
    let max_cases = usize::try_from(max_cases)
        .map_err(|_| anyhow::anyhow!("Code fuzz case limit does not fit this platform"))?;
    let excluded = resolve_exclusions(graph, exclusions)?;
    let mut contracts = Vec::new();

    for (target, node) in &graph.nodes {
        let SourceNode::Symbol(symbol) = node else {
            continue;
        };
        let Some(callable) = &symbol.callable else {
            continue;
        };
        if symbol.visibility != SourceVisibility::Public {
            continue;
        }
        let Some(SourceNode::File(file)) = graph.nodes.get(&symbol.file) else {
            continue;
        };
        let public_contexts = reachability
            .contexts(target)
            .into_iter()
            .filter_map(|context| {
                graph.contexts.get(&context).and_then(|context| {
                    (context.scope == ContextScope::PublicSurface).then(|| context.name.clone())
                })
            })
            .collect::<Vec<_>>();
        let signatures = callable
            .signatures
            .iter()
            .enumerate()
            .map(|(signature, contract)| {
                let (dimensions, mapping_issues) = map_signature(contract);
                let selection = select_pairwise(&dimensions, max_cases)?;
                Ok(CodeFuzzSignatureCorpus {
                    signature,
                    dimensions,
                    deterministic_cases: selection.cases,
                    pairwise_complete: selection.complete,
                    mapping_issues,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let mut fuzz_block_reasons = Vec::new();
        if excluded.contains(target)
            || symbol
                .fuzz_policy
                .as_ref()
                .is_some_and(|policy| policy.denial.is_some())
        {
            fuzz_block_reasons.push(block_reason(
                CodeFuzzBlockKind::BlockedByPolicy,
                if excluded.contains(target) {
                    "config"
                } else {
                    "source_directive"
                },
            ));
        }
        if symbol
            .fuzz_policy
            .as_ref()
            .is_some_and(|policy| !policy.issues.is_empty())
        {
            fuzz_block_reasons.push(block_reason(
                CodeFuzzBlockKind::MalformedDirective,
                "source_directive",
            ));
        }
        if public_contexts.is_empty() {
            fuzz_block_reasons.push(block_reason(
                CodeFuzzBlockKind::NotPubliclyReachable,
                "public_surface",
            ));
        }
        if !matches!(
            file.language,
            SourceLanguage::Rust
                | SourceLanguage::Python
                | SourceLanguage::JavaScript
                | SourceLanguage::TypeScript
        ) {
            fuzz_block_reasons.push(block_reason(
                CodeFuzzBlockKind::UnsupportedLanguage,
                "language",
            ));
        }
        if signatures
            .iter()
            .any(|signature| !signature.mapping_issues.is_empty())
        {
            fuzz_block_reasons.push(block_reason(CodeFuzzBlockKind::UnsupportedCorpus, "inputs"));
        }
        if signatures.len() != 1 {
            fuzz_block_reasons.push(block_reason(
                CodeFuzzBlockKind::UnsupportedCorpus,
                "signatures",
            ));
        }
        fuzz_block_reasons.sort();
        fuzz_block_reasons.dedup();
        let callable_block_reasons = callable.block_reasons.clone();
        let status = if callable_block_reasons.is_empty() && fuzz_block_reasons.is_empty() {
            CodeFuzzability::ReadyForPlanning
        } else {
            CodeFuzzability::Blocked
        };
        let result_shape_supported = !callable
            .block_reasons
            .iter()
            .any(|reason| reason.subject.ends_with(":result"));
        contracts.push(CodeFuzzContract {
            target: target.clone(),
            project: symbol.project.0.clone(),
            path: file.path.clone(),
            symbol: selector_symbol(target, &symbol.name),
            language: file.language,
            public_contexts,
            callable: callable.clone(),
            source_policy: symbol.fuzz_policy.clone(),
            signatures,
            oracle_evidence: result_shape_supported
                .then(|| CodeFuzzOracleEvidence {
                    kind: CodeFuzzOracleKind::ResultShape,
                    source: "callable.result".to_string(),
                })
                .into_iter()
                .collect(),
            callable_block_reasons,
            fuzz_block_reasons,
            status,
        });
    }
    contracts.sort_by(|left, right| left.target.cmp(&right.target));
    let discovered = contracts
        .iter()
        .map(|contract| contract.target.clone())
        .collect::<BTreeSet<_>>();
    if let Some(target) = excluded.difference(&discovered).next() {
        anyhow::bail!(
            "Code fuzz exclusion {target} does not identify a discovered public callable"
        );
    }
    Ok(CodeFuzzInventory {
        schema_version: CODE_FUZZ_INVENTORY_SCHEMA_VERSION.to_string(),
        tool_version: env!("CARGO_PKG_VERSION").to_string(),
        max_cases: max_cases_limit,
        contracts,
    })
}

pub(crate) fn select_contract<'a>(
    inventory: &'a CodeFuzzInventory,
    project: &str,
    language: SourceLanguage,
    requested: Option<&str>,
) -> Result<&'a CodeFuzzContract> {
    let candidates = inventory
        .contracts
        .iter()
        .filter(|contract| contract.project == project && contract.language == language)
        .collect::<Vec<_>>();
    match requested {
        Some(selector) => candidates
            .into_iter()
            .find(|contract| contract.selector() == selector)
            .with_context(|| {
                format!("Code fuzz target has no public callable with exact selector {selector:?}")
            }),
        None if candidates.len() == 1 => Ok(candidates[0]),
        None if candidates.is_empty() => anyhow::bail!(
            "Code fuzz target has no discovered public callables for {project:?} {language:?}"
        ),
        None => anyhow::bail!(
            "Code fuzz target has {} public callables; select one exact path#symbol with --symbol",
            candidates.len()
        ),
    }
}

pub(crate) fn select_contract_id<'a>(
    inventory: &'a CodeFuzzInventory,
    project: &str,
    language: SourceLanguage,
    callable_id: &str,
) -> Result<&'a CodeFuzzContract> {
    inventory
        .contracts
        .iter()
        .find(|contract| {
            contract.project == project
                && contract.language == language
                && contract.target.0 == callable_id
        })
        .with_context(|| {
            format!("Code fuzz target no longer contains callable identity {callable_id:?}")
        })
}

fn resolve_exclusions(graph: &SourceGraph, exclusions: &[String]) -> Result<BTreeSet<NodeId>> {
    let mut resolved = BTreeSet::new();
    for exclusion in exclusions {
        let target = crate::context_slice::resolve_target(graph, exclusion)?;
        if target.nodes.len() != 1 {
            anyhow::bail!(
                "Code fuzz exclusion {exclusion:?} resolved to {} targets; expected exactly one",
                target.nodes.len()
            );
        }
        resolved.insert(target.nodes[0].clone());
    }
    Ok(resolved)
}

fn block_reason(kind: CodeFuzzBlockKind, subject: &str) -> CodeFuzzBlockReason {
    CodeFuzzBlockReason {
        kind,
        subject: subject.to_string(),
    }
}

fn selector_symbol(target: &NodeId, fallback: &str) -> String {
    target
        .0
        .rsplit_once('#')
        .map_or_else(|| fallback.to_string(), |(_, symbol)| symbol.to_string())
}

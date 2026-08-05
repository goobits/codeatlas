use super::corpus::{map_signature, CorpusMappingIssue};
#[cfg(test)]
use crate::analysis::reachability::render_diagnostics;
use crate::analysis::reachability::Reachability;
use crate::domain::source_graph::{
    ContextScope, NodeId, SourceGraph, SourceLanguage, SourceNode, SourceVisibility,
};
use crate::domain::{CallableBlockReason, CallableContract, FuzzPolicyEvidence};
use crate::fuzz::corpus::{select_pairwise, CorpusDimension};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub(crate) const CODE_FUZZ_INVENTORY_SCHEMA_VERSION: &str = "codeatlas.code-fuzz-inventory/v1";

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

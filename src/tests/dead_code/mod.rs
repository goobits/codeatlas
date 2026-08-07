mod ecmascript;
mod python;
mod rust;
mod source_policy;
mod workspace;

use crate::config::{ProjectConfig, RepositoryScope};
use crate::dead_code::{DeadCodeFinding, DeadCodeFindingKind};
use crate::{dead_code, languages, outputs};
use codeatlas_domain::source_graph::{
    AnalysisBoundary, AnalysisCompleteness, BoundaryKind, ContextId, ContextRole, ContextScope,
    EdgeTarget, FindingConfidence, NodeId, ProjectId, SourceContext, SourceEdge, SourceEdgeKind,
    SourceEvidence, SourceFile, SourceGraph, SourceLanguage, SourceNode, SourceProject,
    SourceSymbol, SourceSymbolKind, SourceVisibility,
};
use codeatlas_domain::{EvidenceClass, SourceDisposition};
use std::collections::BTreeSet;
use std::path::PathBuf;

fn fixture_root(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("dead-code")
        .join(path)
}

fn analyze_fixture(path: &str) -> crate::dead_code::DeadCodeReport {
    let graph = source_graph_fixture(path);
    dead_code::analyze(&graph).expect("dead-code report")
}

fn source_graph_fixture(path: &str) -> codeatlas_domain::source_graph::SourceGraph {
    let root = fixture_root(path);
    let config_path = root.join("codeatlas.json");
    let project = ProjectConfig::load(&root, Some(&config_path)).expect("fixture configuration");
    let projects = project.analysis_projects().expect("analysis projects");
    languages::reachability::build_source_graph(&projects).expect("source graph")
}

fn finding<'a>(
    findings: &'a [DeadCodeFinding],
    kind: DeadCodeFindingKind,
    path: &str,
    symbol: Option<&str>,
) -> &'a DeadCodeFinding {
    findings
        .iter()
        .find(|finding| {
            finding.kind == kind && finding.path == path && finding.symbol.as_deref() == symbol
        })
        .unwrap_or_else(|| panic!("missing {kind:?} for {path} {symbol:?}"))
}

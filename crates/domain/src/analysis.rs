use crate::source_graph::{ContextRole, ContextScope, ProjectId};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize)]
pub struct ResolvedAnalysisProject {
    pub id: ProjectId,
    pub root: PathBuf,
    pub report_root: String,
    pub languages: Vec<String>,
    pub contexts: BTreeMap<String, AnalysisContext>,
    pub assume_reachable: Vec<String>,
    pub require_complete: bool,
    pub no_default_ignore: bool,
    pub rust: RustAnalysisOptions,
    pub workspace_member: bool,
    pub excluded_roots: Vec<PathBuf>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AnalysisContext {
    pub role: ContextRole,
    pub scope: ContextScope,
    pub entrypoints: Vec<String>,
    pub subjects: Vec<TestSubject>,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct RustAnalysisOptions {
    pub all_features: bool,
    pub features: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum TestSubject {
    Project(String),
    Source(String),
}

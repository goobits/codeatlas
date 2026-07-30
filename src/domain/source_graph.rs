//! Language-neutral facts observed from source code.
//!
//! This graph is shared by source-analysis domains. Language adapters own the
//! semantics that produce its facts. Feature domains, such as dead-code
//! analysis, own the judgments derived from those facts.
//!
//! This is intentionally not the Atlas Architecture DSL graph. Declared
//! architecture and observed source reachability have different identities,
//! authority, and lifecycle rules.

use super::{Span, Visibility};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

pub(crate) const SOURCE_GRAPH_SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct SourceGraph {
    pub schema_version: u32,
    pub projects: BTreeMap<ProjectId, SourceProject>,
    pub nodes: BTreeMap<NodeId, SourceNode>,
    pub edges: BTreeSet<SourceEdge>,
    pub contexts: BTreeMap<ContextId, SourceContext>,
    pub boundaries: BTreeSet<AnalysisBoundary>,
}

impl Default for SourceGraph {
    fn default() -> Self {
        Self {
            schema_version: SOURCE_GRAPH_SCHEMA_VERSION,
            projects: BTreeMap::new(),
            nodes: BTreeMap::new(),
            edges: BTreeSet::new(),
            contexts: BTreeMap::new(),
            boundaries: BTreeSet::new(),
        }
    }
}

impl SourceGraph {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn add_project(&mut self, project: SourceProject) -> Result<(), GraphError> {
        if self.projects.insert(project.id.clone(), project).is_some() {
            return Err(GraphError::Project);
        }
        Ok(())
    }

    pub(crate) fn add_node(&mut self, id: NodeId, node: SourceNode) -> Result<(), GraphError> {
        if self.nodes.insert(id, node).is_some() {
            return Err(GraphError::Node);
        }
        Ok(())
    }

    pub(crate) fn add_context(&mut self, context: SourceContext) -> Result<(), GraphError> {
        if self.contexts.insert(context.id.clone(), context).is_some() {
            return Err(GraphError::Context);
        }
        Ok(())
    }

    pub(crate) fn record_boundary(
        &mut self,
        project: &ProjectId,
        node: Option<NodeId>,
        kind: BoundaryKind,
        effect: AnalysisCompleteness,
        message: impl Into<String>,
        evidence: SourceEvidence,
    ) {
        if let Some(source_project) = self.projects.get_mut(project) {
            source_project.completeness = source_project.completeness.worst(effect);
        }
        self.boundaries.insert(AnalysisBoundary {
            project: project.clone(),
            node,
            kind,
            effect,
            message: message.into(),
            evidence,
        });
    }

    pub(crate) fn validate(&self) -> Result<(), Vec<GraphDiagnostic>> {
        let mut diagnostics = Vec::new();

        for (node_id, node) in &self.nodes {
            if !self.projects.contains_key(node.project()) {
                diagnostics.push(GraphDiagnostic::new(
                    "source_graph.unknown_project",
                    format!("{node_id} references unknown project {}", node.project()),
                ));
            }
            if let SourceNode::Symbol(symbol) = node {
                match self.nodes.get(&symbol.file) {
                    Some(SourceNode::File(file)) if file.project == symbol.project => {}
                    Some(SourceNode::File(_)) => diagnostics.push(GraphDiagnostic::new(
                        "source_graph.cross_project_symbol",
                        format!(
                            "{node_id} and file {} belong to different projects",
                            symbol.file
                        ),
                    )),
                    Some(SourceNode::Symbol(_)) => diagnostics.push(GraphDiagnostic::new(
                        "source_graph.symbol_file_not_file",
                        format!("{node_id} references symbol {} as its file", symbol.file),
                    )),
                    None => diagnostics.push(GraphDiagnostic::new(
                        "source_graph.unknown_symbol_file",
                        format!("{node_id} references unknown file {}", symbol.file),
                    )),
                }
            }
        }

        for edge in &self.edges {
            if !self.nodes.contains_key(&edge.from) {
                diagnostics.push(GraphDiagnostic::new(
                    "source_graph.unknown_edge_source",
                    format!("edge source {} does not exist", edge.from),
                ));
            }
            if let EdgeTarget::Node(target) = &edge.to {
                if !self.nodes.contains_key(target) {
                    diagnostics.push(GraphDiagnostic::new(
                        "source_graph.unknown_edge_target",
                        format!("edge target {target} does not exist"),
                    ));
                }
            }
        }

        for context in self.contexts.values() {
            if !self.projects.contains_key(&context.project) {
                diagnostics.push(GraphDiagnostic::new(
                    "source_graph.unknown_context_project",
                    format!(
                        "context {} references unknown project {}",
                        context.id, context.project
                    ),
                ));
            }
            for root in &context.roots {
                match self.nodes.get(root) {
                    Some(node) if node.project() == &context.project => {}
                    Some(_) => diagnostics.push(GraphDiagnostic::new(
                        "source_graph.cross_project_context_root",
                        format!(
                            "context {} has root {root} from another project",
                            context.id
                        ),
                    )),
                    None => diagnostics.push(GraphDiagnostic::new(
                        "source_graph.unknown_context_root",
                        format!("context {} references unknown root {root}", context.id),
                    )),
                }
            }
        }

        for boundary in &self.boundaries {
            if !self.projects.contains_key(&boundary.project) {
                diagnostics.push(GraphDiagnostic::new(
                    "source_graph.unknown_boundary_project",
                    format!("boundary references unknown project {}", boundary.project),
                ));
            }
            if let Some(node) = &boundary.node {
                if !self.nodes.contains_key(node) {
                    diagnostics.push(GraphDiagnostic::new(
                        "source_graph.unknown_boundary_node",
                        format!("boundary references unknown node {node}"),
                    ));
                }
            }
        }

        diagnostics.sort();
        if diagnostics.is_empty() {
            Ok(())
        } else {
            Err(diagnostics)
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(transparent)]
pub(crate) struct ProjectId(pub String);

impl fmt::Display for ProjectId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(transparent)]
pub(crate) struct NodeId(pub String);

impl NodeId {
    pub(crate) fn file(project: &ProjectId, path: &str) -> Self {
        Self(format!(
            "file/{}/{}",
            escape_id_segment(&project.0),
            escape_id_segment(path)
        ))
    }

    pub(crate) fn symbol(file: &Self, local_id: &str) -> Self {
        Self(format!(
            "symbol/{}/{}",
            escape_id_segment(&file.0),
            escape_id_segment(local_id)
        ))
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(transparent)]
pub(crate) struct ContextId(pub String);

impl ContextId {
    pub(crate) fn new(project: &ProjectId, name: &str) -> Self {
        Self(format!(
            "context/{}/{}",
            escape_id_segment(&project.0),
            escape_id_segment(name)
        ))
    }
}

impl fmt::Display for ContextId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct SourceProject {
    pub id: ProjectId,
    pub root: String,
    pub languages: BTreeSet<SourceLanguage>,
    pub completeness: AnalysisCompleteness,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum SourceNode {
    File(SourceFile),
    Symbol(SourceSymbol),
}

impl SourceNode {
    pub(crate) fn project(&self) -> &ProjectId {
        match self {
            Self::File(file) => &file.project,
            Self::Symbol(symbol) => &symbol.project,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct SourceFile {
    pub project: ProjectId,
    pub path: String,
    pub language: SourceLanguage,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct SourceSymbol {
    pub project: ProjectId,
    pub file: NodeId,
    pub name: String,
    pub symbol_kind: SourceSymbolKind,
    pub visibility: SourceVisibility,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span: Option<Span>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum SourceLanguage {
    #[serde(rename = "javascript")]
    JavaScript,
    #[serde(rename = "typescript")]
    TypeScript,
    #[serde(rename = "svelte")]
    Svelte,
    #[serde(rename = "python")]
    Python,
    #[serde(rename = "rust")]
    Rust,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SourceSymbolKind {
    Module,
    Class,
    Function,
    Method,
    Variable,
    Constant,
    Interface,
    Struct,
    Enum,
    Trait,
    TypeAlias,
    Property,
    Other,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SourceVisibility {
    Public,
    Internal,
    Private,
    Unknown,
}

impl From<Visibility> for SourceVisibility {
    fn from(visibility: Visibility) -> Self {
        match visibility {
            Visibility::Public => Self::Public,
            Visibility::Internal => Self::Internal,
            Visibility::Private => Self::Private,
            Visibility::Unknown => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AnalysisCompleteness {
    Complete,
    Partial,
    Unsupported,
}

impl AnalysisCompleteness {
    pub(crate) fn worst(self, other: Self) -> Self {
        use AnalysisCompleteness::{Complete, Partial, Unsupported};
        match (self, other) {
            (Unsupported, _) | (_, Unsupported) => Unsupported,
            (Partial, _) | (_, Partial) => Partial,
            (Complete, Complete) => Complete,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FindingConfidence {
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct SourceContext {
    pub id: ContextId,
    pub project: ProjectId,
    pub name: String,
    pub role: ContextRole,
    pub roots: BTreeSet<NodeId>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ContextRole {
    Production,
    Test,
    Tooling,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct SourceEdge {
    pub from: NodeId,
    pub to: EdgeTarget,
    pub kind: SourceEdgeKind,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bindings: Vec<SourceBinding>,
    pub evidence: SourceEvidence,
}

impl SourceEdge {
    pub(crate) fn traversable_target(&self) -> Option<&NodeId> {
        if self.kind == SourceEdgeKind::Contains {
            return None;
        }
        match &self.to {
            EdgeTarget::Node(target) => Some(target),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub(crate) enum EdgeTarget {
    Node(NodeId),
    External(String),
    UnresolvedInternal(String),
    DynamicUnknown(String),
    Unsupported(String),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SourceEdgeKind {
    Contains,
    ModuleDependency,
    Import,
    ReExport,
    DynamicImport,
    GlobImport,
    Require,
    LexicalReference,
    AssumeReachable,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct SourceBinding {
    pub imported: String,
    pub local: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exported: Option<String>,
    #[serde(default)]
    pub namespace: bool,
    #[serde(default)]
    pub type_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct SourceEvidence {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span: Option<Span>,
    pub extractor: String,
}

impl SourceEvidence {
    pub(crate) fn new(
        path: impl Into<String>,
        span: Option<Span>,
        extractor: impl Into<String>,
    ) -> Self {
        Self {
            path: path.into(),
            span,
            extractor: extractor.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct AnalysisBoundary {
    pub project: ProjectId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node: Option<NodeId>,
    pub kind: BoundaryKind,
    pub effect: AnalysisCompleteness,
    pub message: String,
    pub evidence: SourceEvidence,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub(crate) enum BoundaryKind {
    DynamicImport,
    Reflection,
    MacroExpansion,
    ConditionalCompilation,
    UnresolvedInternal,
    UnsupportedDependency,
    UnsupportedSyntax,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct GraphDiagnostic {
    pub code: &'static str,
    pub message: String,
}

impl GraphDiagnostic {
    fn new(code: &'static str, message: String) -> Self {
        Self { code, message }
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum GraphError {
    #[error("project ID already exists")]
    Project,
    #[error("node ID already exists")]
    Node,
    #[error("context ID already exists")]
    Context,
}

fn escape_id_segment(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}

#[cfg(test)]
mod tests {
    use super::{
        AnalysisCompleteness, BoundaryKind, ContextId, NodeId, ProjectId, SourceEvidence,
        SourceGraph, SourceLanguage, SourceProject, SourceVisibility,
    };
    use crate::domain::Visibility;
    use std::collections::BTreeSet;

    #[test]
    fn node_and_context_ids_escape_path_boundaries_deterministically() {
        let project = ProjectId("workspace/root".to_string());
        let file = NodeId::file(&project, "src/a~b.ts");
        let symbol = NodeId::symbol(&file, "function/value");
        let context = ContextId::new(&project, "unit/tests");

        assert_eq!(file.0, "file/workspace~1root/src~1a~0b.ts");
        assert_eq!(
            symbol.0,
            "symbol/file~1workspace~01root~1src~01a~00b.ts/function~1value"
        );
        assert_eq!(context.0, "context/workspace~1root/unit~1tests");
    }

    #[test]
    fn boundary_recording_preserves_the_worst_completeness() {
        let project = ProjectId("example".to_string());
        let mut graph = SourceGraph::new();
        graph
            .add_project(SourceProject {
                id: project.clone(),
                root: ".".to_string(),
                languages: BTreeSet::from([SourceLanguage::Python]),
                completeness: AnalysisCompleteness::Unsupported,
            })
            .expect("project");

        graph.record_boundary(
            &project,
            None,
            BoundaryKind::Reflection,
            AnalysisCompleteness::Partial,
            "dynamic registration",
            SourceEvidence::new("src/plugin.py", None, "test"),
        );

        assert_eq!(
            graph.projects[&project].completeness,
            AnalysisCompleteness::Unsupported
        );
        assert_eq!(graph.boundaries.len(), 1);
    }

    #[test]
    fn source_visibility_uses_the_canonical_domain_mapping() {
        assert_eq!(
            SourceVisibility::from(Visibility::Internal),
            SourceVisibility::Internal
        );
    }
}

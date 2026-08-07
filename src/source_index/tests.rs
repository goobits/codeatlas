use super::SourceIndex;
use codeatlas_domain::source_graph::{
    AnalysisCompleteness, ContextRole, ContextScope, EdgeTarget, NodeId, ProjectId, SourceEdge,
    SourceEdgeKind, SourceEvidence, SourceFile, SourceGraph, SourceLanguage, SourceNode,
    SourceProject,
};
use codeatlas_domain::{AnalysisContext, ResolvedAnalysisProject};
use std::cell::Cell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

struct TemporaryDirectory(PathBuf);

impl TemporaryDirectory {
    fn new(label: &str) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "codeatlas-source-index-{label}-{}-{timestamp}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).expect("temporary directory");
        Self(root)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TemporaryDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn project(root: &Path) -> ResolvedAnalysisProject {
    ResolvedAnalysisProject {
        id: ProjectId("fixture".to_string()),
        root: root.to_path_buf(),
        report_root: ".".to_string(),
        languages: vec!["ts".to_string()],
        contexts: BTreeMap::from([(
            "application".to_string(),
            AnalysisContext {
                role: ContextRole::Production,
                scope: ContextScope::Runtime,
                entrypoints: vec!["src/index.ts".to_string()],
                subjects: Vec::new(),
            },
        )]),
        assume_reachable: Vec::new(),
        require_complete: false,
        no_default_ignore: false,
        rust: Default::default(),
        workspace_member: false,
        excluded_roots: Vec::new(),
    }
}

fn write_source(root: &Path, relative: &str, source: &str) -> PathBuf {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().expect("source parent")).expect("source directory");
    std::fs::write(&path, source).expect("source file");
    path
}

#[test]
fn unchanged_file_facts_are_reused_and_content_changes_miss() {
    let temporary = TemporaryDirectory::new("facts");
    let repository = temporary.path().join("repository");
    let cache = temporary.path().join("cache");
    let source = write_source(&repository, "src/index.ts", "export const value = 1\n");
    let projects = [project(&repository)];
    let index = SourceIndex::open_at(cache.clone(), 16 * 1024 * 1024, &projects).expect("index");
    let parses = Cell::new(0);
    let first = index
        .parse_file("test-parser-v1", &source, &repository, |source| {
            parses.set(parses.get() + 1);
            Ok(source.to_uppercase())
        })
        .expect("first parse");
    let second = index
        .parse_file("test-parser-v1", &source, &repository, |source| {
            parses.set(parses.get() + 1);
            Ok(source.to_uppercase())
        })
        .expect("cached parse");
    assert_eq!(first, second);
    assert_eq!(parses.get(), 1);

    std::fs::write(&source, "export const value = 2\n").expect("changed source");
    let changed = SourceIndex::open_at(cache, 16 * 1024 * 1024, &projects).expect("changed index");
    let changed_value = changed
        .parse_file("test-parser-v1", &source, &repository, |source| {
            parses.set(parses.get() + 1);
            Ok(source.to_uppercase())
        })
        .expect("changed parse");
    assert!(changed_value.contains("VALUE = 2"));
    assert_eq!(parses.get(), 2);
}

#[test]
fn graph_cache_is_byte_identical_and_invalidates_on_source_content() {
    let temporary = TemporaryDirectory::new("graph");
    let repository = temporary.path().join("repository");
    let cache = temporary.path().join("cache");
    let source = write_source(&repository, "src/index.ts", "export const value = 1\n");
    let projects = [project(&repository)];
    let cold = SourceIndex::open_at(cache.clone(), 16 * 1024 * 1024, &projects).expect("cold");
    let project_id = ProjectId("fixture".to_string());
    let file_id = NodeId::file(&project_id, "src/index.ts");
    let mut graph = SourceGraph::new();
    graph
        .add_project(SourceProject {
            id: project_id.clone(),
            root: ".".to_string(),
            languages: [SourceLanguage::TypeScript].into_iter().collect(),
            completeness: AnalysisCompleteness::Partial,
        })
        .expect("graph project");
    graph
        .add_node(
            file_id.clone(),
            SourceNode::File(SourceFile {
                project: project_id,
                path: "src/index.ts".to_string(),
                language: SourceLanguage::TypeScript,
            }),
        )
        .expect("graph file");
    graph.edges.insert(SourceEdge {
        from: file_id,
        to: EdgeTarget::DynamicUnknown("./plugin".to_string()),
        kind: SourceEdgeKind::DynamicImport,
        bindings: Vec::new(),
        evidence: SourceEvidence::new("src/index.ts", None, "test-parser"),
    });
    cold.store_graph(&graph);

    let warm = SourceIndex::open_at(cache.clone(), 16 * 1024 * 1024, &projects).expect("warm");
    let cached = warm.load_graph().expect("cached graph");
    assert_eq!(
        serde_json::to_vec(&graph).expect("cold graph JSON"),
        serde_json::to_vec(&cached).expect("warm graph JSON")
    );

    let graph_path = std::fs::read_dir(cache.join("graphs"))
        .expect("graph directory")
        .next()
        .expect("graph entry")
        .expect("graph path")
        .path();
    std::fs::write(&graph_path, b"invalid cache entry").expect("corrupt graph cache");
    let corrupt = SourceIndex::open_at(cache.clone(), 16 * 1024 * 1024, &projects)
        .expect("corrupt cache index");
    assert!(corrupt.load_graph().is_none());
    assert!(!graph_path.exists());
    corrupt.store_graph(&graph);
    let recovered = SourceIndex::open_at(cache.clone(), 16 * 1024 * 1024, &projects)
        .expect("recovered cache index");
    assert_eq!(recovered.load_graph().as_ref(), Some(&graph));

    std::fs::write(&source, "export const value = 2\n").expect("changed source");
    let changed = SourceIndex::open_at(cache, 16 * 1024 * 1024, &projects).expect("changed");
    assert!(changed.load_graph().is_none());
}

#[test]
fn pruning_enforces_the_declared_cache_size_limit() {
    let temporary = TemporaryDirectory::new("bounded");
    let repository = temporary.path().join("repository");
    let cache = temporary.path().join("cache");
    let mut sources = Vec::new();
    for index in 0..12 {
        sources.push(write_source(
            &repository,
            &format!("src/file{index}.ts"),
            &format!("export const value{index} = '{}';\n", "x".repeat(512)),
        ));
    }
    let projects = [project(&repository)];
    let index = SourceIndex::open_at(cache.clone(), 2_048, &projects).expect("bounded index");
    for source in sources {
        index
            .parse_file("test-parser-v1", &source, &repository, |source| {
                Ok(source.to_string())
            })
            .expect("cached fact");
    }
    index.finish("miss", Duration::ZERO);
    let bytes = walkdir::WalkDir::new(&cache)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_file())
        .filter_map(|entry| entry.metadata().ok().map(|metadata| metadata.len()))
        .sum::<u64>();
    assert!(bytes <= 2_048, "bounded cache retained {bytes} bytes");
}

#[test]
fn checkout_local_cache_roots_are_rejected() {
    let temporary = TemporaryDirectory::new("external");
    let repository = temporary.path().join("repository");
    write_source(&repository, "src/index.ts", "export const value = 1\n");
    let projects = [project(&repository)];
    let error =
        super::environment::validate_external_root(&repository.join(".cache/codeatlas"), &projects)
            .expect_err("checkout-local cache should fail");
    assert!(error.to_string().contains("disjoint"));
}

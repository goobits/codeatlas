use super::diagnostic::Diagnostic;
use super::digest::{digest_value, DigestKind, TypedDigest};
use super::documents::DocumentSet;
use super::graph::{self, CompileMode, CompiledGraph};
use super::model::{GeneratorIdentity, VocabularyIdentity};
use super::vocabulary::Vocabulary;
use super::{ARCHITECTURE_API_VERSION, ARCHITECTURE_SCHEMA_VERSION};
use serde::Serialize;
use serde_json::json;
use std::collections::BTreeMap;
use std::path::PathBuf;

const COMPILER_VERSION: &str = "codeatlas-architecture-compiler/0.1";

#[derive(Clone, Debug)]
pub(crate) struct CompileRequest {
    pub roots: Vec<PathBuf>,
    pub allowed_root: PathBuf,
    pub mode: CompileMode,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CompileResult {
    pub report: CompilationReport,
    pub lockfile: ArchitectureLockfile,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CompilationReport {
    pub schema_version: u32,
    pub api_version: &'static str,
    pub tool_version: String,
    pub compiler_version: &'static str,
    pub mode: CompileMode,
    pub architecture_closure_digest: TypedDigest,
    pub graph_digest: TypedDigest,
    pub vocabulary: VocabularyIdentity,
    pub roots: Vec<String>,
    pub graph: CompiledGraph,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ArchitectureLockfile {
    pub schema_version: u32,
    pub api_version: &'static str,
    pub generated: bool,
    pub manual_editing: &'static str,
    pub generator: GeneratorIdentity,
    pub roots: Vec<String>,
    pub vocabulary: VocabularyIdentity,
    pub documents: Vec<LockDocument>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LockDocument {
    pub module_id: String,
    pub architecture_version: u64,
    pub source: String,
    pub source_document_digest: TypedDigest,
    pub canonical_module_digest: TypedDigest,
    pub import_closure_digest: TypedDigest,
}

pub(crate) fn compile(request: &CompileRequest) -> Result<CompileResult, Vec<Diagnostic>> {
    let vocabulary = Vocabulary::bundled()?;
    let loaded = DocumentSet::load(
        &request.roots,
        &request.allowed_root,
        "ArchitectureModule",
        &vocabulary,
    )?;
    let documents = loaded.values();
    let graph = graph::compile(&documents, &vocabulary, request.mode)?;

    let mut lock_documents = Vec::with_capacity(loaded.documents.len());
    let mut closure_digests = BTreeMap::new();
    for (module_id, document) in &loaded.documents {
        let digest = loaded.import_closure_digest(module_id)?;
        closure_digests.insert(module_id.clone(), digest.clone());
        lock_documents.push(LockDocument {
            module_id: module_id.clone(),
            architecture_version: document.value["metadata"]["architectureVersion"]
                .as_u64()
                .expect("validated architecture version"),
            source: document.portable_path.clone(),
            source_document_digest: document.source_digest.clone(),
            canonical_module_digest: document.canonical_digest.clone(),
            import_closure_digest: digest,
        });
    }
    lock_documents.sort_by(|left, right| left.module_id.cmp(&right.module_id));

    let vocabulary_identity = vocabulary.identity();
    let roots = loaded.roots;
    let architecture_closure_digest = digest_value(
        DigestKind::ArchitectureClosure,
        &json!({
            "roots": roots.iter().map(|id| {
                json!({
                    "moduleId": id,
                    "importClosureDigest": closure_digests[id],
                })
            }).collect::<Vec<_>>(),
            "vocabulary": {
                "id": vocabulary.id,
                "version": vocabulary.version,
                "digest": vocabulary.digest,
            },
            "compilerVersion": COMPILER_VERSION,
        }),
    )
    .map_err(|error| vec![*error.diagnostic])?;
    let graph_digest = graph.digest().map_err(|error| vec![*error.diagnostic])?;

    Ok(CompileResult {
        report: CompilationReport {
            schema_version: ARCHITECTURE_SCHEMA_VERSION,
            api_version: ARCHITECTURE_API_VERSION,
            tool_version: env!("CARGO_PKG_VERSION").to_owned(),
            compiler_version: COMPILER_VERSION,
            mode: request.mode,
            architecture_closure_digest,
            graph_digest,
            vocabulary: vocabulary_identity.clone(),
            roots: roots.clone(),
            graph,
        },
        lockfile: ArchitectureLockfile {
            schema_version: ARCHITECTURE_SCHEMA_VERSION,
            api_version: ARCHITECTURE_API_VERSION,
            generated: true,
            manual_editing: "prohibited",
            generator: GeneratorIdentity {
                id: "codeatlas.tool.architecture-compiler".to_owned(),
                version: env!("CARGO_PKG_VERSION").to_owned(),
            },
            roots,
            vocabulary: vocabulary_identity,
            documents: lock_documents,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::{compile, CompileRequest};
    use crate::architecture::digest::{digest_value, DigestKind};
    use crate::architecture::documents::confined_path;
    use crate::architecture::graph::CompileMode;
    use crate::architecture::yaml::{parse, ParseLimits};
    use serde_json::json;
    use std::fs;
    use std::path::{Path, PathBuf};

    fn crate_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    fn fixture_root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "codeatlas-architecture-compiler-{}-{name}",
            std::process::id()
        ))
    }

    fn remove_existing(path: &Path) {
        if path.is_dir() {
            fs::remove_dir_all(path).expect("remove stale fixture directory");
        } else if path.exists() {
            fs::remove_file(path).expect("remove stale fixture file");
        }
    }

    #[test]
    fn accepted_example_compiles_deterministically() {
        let root = crate_root();
        let request = CompileRequest {
            roots: vec![
                root.join("spec/architecture/v0.1/examples/tabby-shelly/architecture.atlas.yaml")
            ],
            allowed_root: root.clone(),
            mode: CompileMode::Governing,
        };
        let first = compile(&request).expect("first");
        let second = compile(&request).expect("second");
        assert_eq!(
            first.report.graph_digest, second.report.graph_digest,
            "graph identity must be deterministic"
        );
        assert_eq!(first.lockfile.documents.len(), 1);
    }

    #[test]
    fn architecture_change_cannot_compile_as_current_architecture() {
        let root = crate_root();
        let request = CompileRequest {
            roots: vec![root.join(
                "spec/architecture/v0.1/examples/tabby-cutover-change/architecture-change.atlas.yaml",
            )],
            allowed_root: root.clone(),
            mode: CompileMode::Governing,
        };
        let diagnostics = compile(&request).expect_err("change is non-governing");
        assert!(diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "document.root-kind"));
    }

    #[test]
    fn local_imports_are_exact_digest_pinned_and_locked() {
        let directory = fixture_root("imports");
        remove_existing(&directory);
        fs::create_dir_all(&directory).expect("fixture directory");
        let base_path = directory.join("base.atlas.yaml");
        let root_path = directory.join("root.atlas.yaml");
        let base_source = include_str!(
            "../../spec/architecture/v0.1/examples/tabby-shelly/architecture.atlas.yaml"
        );
        let base = parse(base_source.as_bytes(), ParseLimits::default())
            .expect("base")
            .value;
        let base_digest = digest_value(DigestKind::CanonicalModule, &base).expect("base digest");
        fs::write(&base_path, base_source).expect("write base");

        let mut root = base.clone();
        root["metadata"]["id"] = json!("goobits.product.root-architecture");
        root["imports"] = json!([{
            "module": "goobits.product.tabby-shelly",
            "architectureVersion": 1,
            "digest": base_digest,
            "source": "base.atlas.yaml"
        }]);
        root["exports"] = json!({
            "objects": [],
            "relations": [],
            "bindings": [],
            "constraints": []
        });
        root["objects"] = json!({});
        root["relations"] = json!({});
        root["bindings"] = json!({});
        root["constraints"] = json!({});
        root["retired"] = json!({});
        fs::write(
            &root_path,
            serde_yaml::to_string(&root).expect("serialize root"),
        )
        .expect("write root");

        let request = CompileRequest {
            roots: vec![root_path.clone()],
            allowed_root: directory.clone(),
            mode: CompileMode::Governing,
        };
        let result = compile(&request).expect("compile import closure");
        assert_eq!(result.lockfile.documents.len(), 2);
        assert_eq!(
            result.report.roots,
            vec!["goobits.product.root-architecture"]
        );

        root["imports"][0]["digest"] = json!(format!("sha256:{}", "0".repeat(64)));
        fs::write(
            &root_path,
            serde_yaml::to_string(&root).expect("serialize mismatch"),
        )
        .expect("write mismatch");
        let diagnostics = compile(&request).expect_err("digest mismatch");
        assert!(diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "import.digest-mismatch"));
        fs::remove_dir_all(directory).expect("clean fixture");
    }

    #[test]
    fn import_security_boundary_rejects_path_and_symlink_escapes() {
        let directory = fixture_root("security");
        let outside = directory.with_extension("outside.yaml");
        remove_existing(&directory);
        remove_existing(&outside);
        fs::create_dir_all(&directory).expect("fixture directory");
        fs::write(&outside, b"outside").expect("outside file");
        let root = fs::canonicalize(&directory).expect("canonical root");

        let error = confined_path(&root, &outside, &root).expect_err("path escape");
        assert_eq!(error.diagnostic.code, "import.path-escape");

        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let link = directory.join("escaped.atlas.yaml");
            symlink(&outside, &link).expect("symlink");
            let error = confined_path(&root, &link, &root).expect_err("symlink escape");
            assert_eq!(error.diagnostic.code, "import.path-escape");
        }

        fs::remove_dir_all(directory).expect("clean fixture");
        fs::remove_file(outside).expect("clean outside fixture");
    }
}

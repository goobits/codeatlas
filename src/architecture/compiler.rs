use super::diagnostic::{sort_diagnostics, ArchitectureError, Diagnostic};
use super::digest::{digest_value, DigestKind, TypedDigest};
use super::graph::{self, CompileMode, CompiledGraph};
use super::vocabulary::Vocabulary;
use super::yaml::{parse, ParseLimits};
use super::{ARCHITECTURE_API_VERSION, ARCHITECTURE_SCHEMA_VERSION};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::path::{Component, Path, PathBuf};

const COMPILER_VERSION: &str = "codeatlas-architecture-compiler/0.1";
const MAX_TOTAL_BYTES: usize = 64 * 1024 * 1024;
const MAX_IMPORT_DEPTH: usize = 32;
const MAX_DOCUMENTS: usize = 4096;

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
pub(crate) struct GeneratorIdentity {
    pub id: &'static str,
    pub version: String,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct VocabularyIdentity {
    pub id: String,
    pub version: u64,
    pub digest: TypedDigest,
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

struct LoadedDocument {
    portable_path: String,
    value: Value,
    source_digest: TypedDigest,
    canonical_digest: TypedDigest,
}

pub(crate) fn compile(request: &CompileRequest) -> Result<CompileResult, Vec<Diagnostic>> {
    if request.roots.is_empty() {
        return Err(vec![Diagnostic::error(
            "compile.roots-empty",
            "at least one root ArchitectureModule is required",
        )]);
    }
    let vocabulary = Vocabulary::bundled()?;
    let allowed_root = fs::canonicalize(&request.allowed_root).map_err(|error| {
        vec![Diagnostic::error(
            "import.root-unavailable",
            format!("{}: {error}", request.allowed_root.display()),
        )]
    })?;
    let mut diagnostics = Vec::new();
    let mut loaded = BTreeMap::<PathBuf, LoadedDocument>::new();
    let mut pending = VecDeque::new();
    for root in &request.roots {
        match confined_path(&allowed_root, root, &allowed_root) {
            Ok(path) => pending.push_back((path, 0usize)),
            Err(error) => diagnostics.push(*error.diagnostic),
        }
    }

    let mut total_bytes = 0usize;
    while let Some((path, depth)) = pending.pop_front() {
        if loaded.contains_key(&path) {
            continue;
        }
        if depth > MAX_IMPORT_DEPTH {
            diagnostics.push(
                Diagnostic::error(
                    "resource.import-depth",
                    format!("import depth exceeds {MAX_IMPORT_DEPTH}"),
                )
                .at_path(&path),
            );
            continue;
        }
        if loaded.len() >= MAX_DOCUMENTS {
            diagnostics.push(Diagnostic::error(
                "resource.document-count",
                format!("document count exceeds {MAX_DOCUMENTS}"),
            ));
            break;
        }
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) => {
                diagnostics.push(
                    Diagnostic::error(
                        "document.read-failed",
                        format!("{}: {error}", path.display()),
                    )
                    .at_path(&path),
                );
                continue;
            }
        };
        total_bytes = match total_bytes.checked_add(bytes.len()) {
            Some(total) if total <= MAX_TOTAL_BYTES => total,
            _ => {
                diagnostics.push(Diagnostic::error(
                    "resource.total-source-bytes",
                    format!("total source bytes exceed {MAX_TOTAL_BYTES}"),
                ));
                break;
            }
        };
        let parsed = match parse(&bytes, ParseLimits::default()) {
            Ok(parsed) => parsed,
            Err(error) => {
                diagnostics.push(error.diagnostic.at_path(&path));
                continue;
            }
        };
        let document_id = parsed.value["metadata"]["id"].as_str().map(str::to_owned);
        let mut document_diagnostics = vocabulary.validate_document(&parsed.value);
        for diagnostic in &mut document_diagnostics {
            diagnostic.source_path = Some(path.clone());
            diagnostic.document_id.clone_from(&document_id);
        }
        if parsed.value["kind"].as_str() != Some("ArchitectureModule") {
            document_diagnostics.push(
                Diagnostic::error(
                    "compile.root-kind",
                    "architecture compilation accepts ArchitectureModule documents only",
                )
                .at_path(&path),
            );
        }
        diagnostics.extend(document_diagnostics);
        let canonical_digest = match digest_value(DigestKind::CanonicalModule, &parsed.value) {
            Ok(digest) => digest,
            Err(error) => {
                diagnostics.push(error.diagnostic.at_path(&path));
                continue;
            }
        };
        if let Some(imports) = parsed.value["imports"].as_array() {
            let parent = path.parent().unwrap_or(&allowed_root);
            for import in imports {
                let Some(source) = import["source"].as_str() else {
                    continue;
                };
                match confined_path(&allowed_root, Path::new(source), parent) {
                    Ok(import_path) => pending.push_back((import_path, depth + 1)),
                    Err(error) => diagnostics.push((*error.diagnostic).at_path(&path)),
                }
            }
        }
        loaded.insert(
            path.clone(),
            LoadedDocument {
                portable_path: portable_path(&path, &allowed_root),
                value: parsed.value,
                source_digest: parsed.source_document_digest,
                canonical_digest,
            },
        );
    }
    if !diagnostics.is_empty() {
        sort_diagnostics(&mut diagnostics);
        return Err(diagnostics);
    }

    let documents = loaded
        .values()
        .map(|document| document.value.clone())
        .collect::<Vec<_>>();
    let graph = graph::compile(&documents, &vocabulary, request.mode)?;
    let by_id = loaded
        .values()
        .map(|document| {
            (
                document.value["metadata"]["id"]
                    .as_str()
                    .expect("validated module ID")
                    .to_owned(),
                document,
            )
        })
        .collect::<BTreeMap<_, _>>();
    let root_ids = request
        .roots
        .iter()
        .map(|root| confined_path(&allowed_root, root, &allowed_root))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| vec![*error.diagnostic])?
        .into_iter()
        .map(|root| {
            loaded[&root].value["metadata"]["id"]
                .as_str()
                .expect("validated root ID")
                .to_owned()
        })
        .collect::<BTreeSet<_>>();

    let mut lock_documents = Vec::with_capacity(by_id.len());
    let mut closure_digests = BTreeMap::new();
    for (module_id, document) in &by_id {
        let digest = import_closure_digest(module_id, &by_id)?;
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

    let vocabulary_identity = VocabularyIdentity {
        id: vocabulary.id.clone(),
        version: vocabulary.version,
        digest: vocabulary.digest.clone(),
    };
    let roots = root_ids.into_iter().collect::<Vec<_>>();
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
                id: "codeatlas.tool.architecture-compiler",
                version: env!("CARGO_PKG_VERSION").to_owned(),
            },
            roots,
            vocabulary: vocabulary_identity,
            documents: lock_documents,
        },
    })
}

fn confined_path(
    allowed_root: &Path,
    source: &Path,
    parent: &Path,
) -> Result<PathBuf, ArchitectureError> {
    let source_text = source.to_string_lossy();
    if source_text.contains("://") {
        return Err(ArchitectureError::new(
            "import.network-source-prohibited",
            "network import sources are prohibited",
        ));
    }
    if source_text.contains('\0') {
        return Err(ArchitectureError::new(
            "import.nul-prohibited",
            "import source contains a NUL byte",
        ));
    }
    if source
        .components()
        .any(|component| matches!(component, Component::Prefix(_)))
    {
        return Err(ArchitectureError::new(
            "import.platform-prefix-prohibited",
            "platform-prefixed import sources are prohibited",
        ));
    }
    let candidate = if source.is_absolute() {
        source.to_path_buf()
    } else {
        parent.join(source)
    };
    let canonical = fs::canonicalize(&candidate).map_err(|error| {
        ArchitectureError::new(
            "import.source-unavailable",
            format!("{}: {error}", candidate.display()),
        )
    })?;
    if !canonical.starts_with(allowed_root) {
        return Err(ArchitectureError::new(
            "import.path-escape",
            format!(
                "{} resolves outside {}",
                candidate.display(),
                allowed_root.display()
            ),
        ));
    }
    Ok(canonical)
}

fn portable_path(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn import_closure_digest(
    root: &str,
    documents: &BTreeMap<String, &LoadedDocument>,
) -> Result<TypedDigest, Vec<Diagnostic>> {
    let mut pending = vec![root.to_owned()];
    let mut visited = BTreeSet::new();
    let mut members = Vec::new();
    while let Some(id) = pending.pop() {
        if !visited.insert(id.clone()) {
            continue;
        }
        let Some(document) = documents.get(&id) else {
            return Err(vec![Diagnostic::error(
                "import.module-unresolved",
                format!("import closure references missing module {id}"),
            )]);
        };
        members.push(json!({
            "moduleId": id,
            "architectureVersion": document.value["metadata"]["architectureVersion"],
            "canonicalModuleDigest": document.canonical_digest,
        }));
        for import in document.value["imports"]
            .as_array()
            .expect("validated imports")
        {
            pending.push(
                import["module"]
                    .as_str()
                    .expect("validated import ID")
                    .to_owned(),
            );
        }
    }
    members.sort_by(|left, right| left["moduleId"].as_str().cmp(&right["moduleId"].as_str()));
    digest_value(DigestKind::ImportClosure, &Value::Array(members))
        .map_err(|error| vec![*error.diagnostic])
}

#[cfg(test)]
mod tests {
    use super::{compile, confined_path, CompileRequest};
    use crate::architecture::digest::{digest_value, DigestKind};
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
            .any(|diagnostic| diagnostic.code == "compile.root-kind"));
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

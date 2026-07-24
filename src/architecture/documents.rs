use super::diagnostic::{sort_diagnostics, ArchitectureError, Diagnostic};
use super::digest::{digest_value, DigestKind, TypedDigest};
use super::vocabulary::Vocabulary;
use super::yaml::{parse, ParseLimits};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::path::{Component, Path, PathBuf};

const MAX_TOTAL_BYTES: usize = 64 * 1024 * 1024;
const MAX_IMPORT_DEPTH: usize = 32;
const MAX_DOCUMENTS: usize = 4096;

pub(super) struct LoadedDocument {
    pub portable_path: String,
    pub canonical_path: PathBuf,
    pub value: Value,
    pub source_digest: TypedDigest,
    pub canonical_digest: TypedDigest,
}

pub(super) struct DocumentSet {
    pub roots: Vec<String>,
    pub documents: BTreeMap<String, LoadedDocument>,
}

impl DocumentSet {
    pub fn load(
        roots: &[PathBuf],
        allowed_root: &Path,
        expected_kind: &str,
        vocabulary: &Vocabulary,
    ) -> Result<Self, Vec<Diagnostic>> {
        if roots.is_empty() {
            return Err(vec![Diagnostic::error(
                "document.roots-empty",
                format!("at least one root {expected_kind} is required"),
            )]);
        }
        let allowed_root = fs::canonicalize(allowed_root).map_err(|error| {
            vec![Diagnostic::error(
                "import.root-unavailable",
                format!("{}: {error}", allowed_root.display()),
            )]
        })?;
        let mut diagnostics = Vec::new();
        let mut loaded_by_path = BTreeMap::<PathBuf, LoadedDocument>::new();
        let mut pending = VecDeque::new();
        for root in roots {
            match confined_path(&allowed_root, root, &allowed_root) {
                Ok(path) => pending.push_back((path, 0usize)),
                Err(error) => diagnostics.push(*error.diagnostic),
            }
        }

        let mut total_bytes = 0usize;
        while let Some((path, depth)) = pending.pop_front() {
            if loaded_by_path.contains_key(&path) {
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
            if loaded_by_path.len() >= MAX_DOCUMENTS {
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
            if parsed.value["kind"].as_str() != Some(expected_kind) {
                document_diagnostics.push(
                    Diagnostic::error(
                        "document.root-kind",
                        format!("expected {expected_kind} document"),
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
            loaded_by_path.insert(
                path.clone(),
                LoadedDocument {
                    portable_path: portable_path(&path, &allowed_root),
                    canonical_path: path,
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

        let mut documents = BTreeMap::new();
        for document in loaded_by_path.into_values() {
            let id = document.value["metadata"]["id"]
                .as_str()
                .expect("validated document ID")
                .to_owned();
            if documents.insert(id.clone(), document).is_some() {
                diagnostics.push(Diagnostic::error(
                    "document.duplicate-id",
                    format!("duplicate document ID {id}"),
                ));
            }
        }
        diagnostics.extend(validate_imports(&documents, &allowed_root));
        diagnostics.extend(validate_import_cycles(&documents));

        let mut root_ids = Vec::new();
        for root in roots {
            let root_path = match confined_path(&allowed_root, root, &allowed_root) {
                Ok(path) => path,
                Err(error) => {
                    diagnostics.push(*error.diagnostic);
                    continue;
                }
            };
            match documents
                .values()
                .find(|document| document.canonical_path == root_path)
            {
                Some(document) => root_ids.push(
                    document.value["metadata"]["id"]
                        .as_str()
                        .expect("validated document ID")
                        .to_owned(),
                ),
                None => diagnostics.push(Diagnostic::error(
                    "document.root-unresolved",
                    format!("root {} was not loaded", root.display()),
                )),
            }
        }
        root_ids.sort();
        root_ids.dedup();
        if diagnostics.is_empty() {
            Ok(Self {
                roots: root_ids,
                documents,
            })
        } else {
            sort_diagnostics(&mut diagnostics);
            Err(diagnostics)
        }
    }

    pub fn values(&self) -> Vec<Value> {
        self.documents
            .values()
            .map(|document| document.value.clone())
            .collect()
    }

    pub fn import_closure_digest(&self, root: &str) -> Result<TypedDigest, Vec<Diagnostic>> {
        let mut pending = vec![root.to_owned()];
        let mut visited = BTreeSet::new();
        let mut members = Vec::new();
        while let Some(id) = pending.pop() {
            if !visited.insert(id.clone()) {
                continue;
            }
            let Some(document) = self.documents.get(&id) else {
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
}

fn validate_imports(
    documents: &BTreeMap<String, LoadedDocument>,
    allowed_root: &Path,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for (document_id, document) in documents {
        let parent = document.canonical_path.parent().unwrap_or(allowed_root);
        for import in document.value["imports"]
            .as_array()
            .expect("validated imports")
        {
            let imported_id = import["module"].as_str().expect("validated import ID");
            let Some(imported) = documents.get(imported_id) else {
                diagnostics.push(Diagnostic::error(
                    "import.module-unresolved",
                    format!("{document_id} imports missing document {imported_id}"),
                ));
                continue;
            };
            let expected_path = import["source"]
                .as_str()
                .and_then(|source| confined_path(allowed_root, Path::new(source), parent).ok());
            if expected_path.as_ref() != Some(&imported.canonical_path) {
                diagnostics.push(Diagnostic::error(
                    "import.source-identity-mismatch",
                    format!(
                        "{document_id} import source does not declare expected ID {imported_id}"
                    ),
                ));
            }
            if import["architectureVersion"].as_u64()
                != imported.value["metadata"]["architectureVersion"].as_u64()
            {
                diagnostics.push(Diagnostic::error(
                    "import.architecture-version-mismatch",
                    format!("{document_id} pins the wrong version of {imported_id}"),
                ));
            }
            if import["digest"].as_str() != Some(imported.canonical_digest.as_str()) {
                diagnostics.push(Diagnostic::error(
                    "import.digest-mismatch",
                    format!("{document_id} pins the wrong digest for {imported_id}"),
                ));
            }
        }
    }
    diagnostics
}

fn validate_import_cycles(documents: &BTreeMap<String, LoadedDocument>) -> Vec<Diagnostic> {
    fn visit(
        id: &str,
        documents: &BTreeMap<String, LoadedDocument>,
        active: &mut BTreeSet<String>,
        complete: &mut BTreeSet<String>,
    ) -> Result<(), String> {
        if complete.contains(id) {
            return Ok(());
        }
        if !active.insert(id.to_owned()) {
            return Err(id.to_owned());
        }
        let document = &documents[id];
        for import in document.value["imports"]
            .as_array()
            .expect("validated imports")
        {
            let imported_id = import["module"].as_str().expect("validated import ID");
            if documents.contains_key(imported_id) {
                visit(imported_id, documents, active, complete)?;
            }
        }
        active.remove(id);
        complete.insert(id.to_owned());
        Ok(())
    }

    let mut active = BTreeSet::new();
    let mut complete = BTreeSet::new();
    for id in documents.keys() {
        if let Err(cycle) = visit(id, documents, &mut active, &mut complete) {
            return vec![Diagnostic::error(
                "import.cycle",
                format!("import graph contains a cycle through {cycle}"),
            )];
        }
    }
    Vec::new()
}

pub(super) fn confined_path(
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

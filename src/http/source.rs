use super::model::{
    HttpConfidence, HttpSkippedFile, HttpSourceCompleteness, HttpSourceEvidence,
    HttpSourceInventory, HttpSourceOperation,
};
use super::openapi::{normalize_path, operation_key};
use anyhow::{Context, Result};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

mod ecmascript;
mod python;
mod rust;
mod sveltekit;

pub(super) fn inventory(
    source_roots: &[PathBuf],
    repository_root: &Path,
    complete: bool,
) -> Result<HttpSourceInventory> {
    let mut operations = Vec::new();
    let mut skipped_files = Vec::new();
    let mut visited = BTreeSet::new();

    for source_root in source_roots {
        let source_root = source_root.canonicalize().with_context(|| {
            format!("HTTP source root does not exist: {}", source_root.display())
        })?;
        let mut builder = ignore::WalkBuilder::new(&source_root);
        builder
            .hidden(false)
            .git_ignore(true)
            .git_global(false)
            .git_exclude(true);
        for entry in builder.build() {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    skipped_files.push(HttpSkippedFile {
                        path: source_root.display().to_string(),
                        reason: error.to_string(),
                    });
                    continue;
                }
            };
            let path = entry.path();
            if !entry.file_type().is_some_and(|kind| kind.is_file()) || !is_source_file(path) {
                continue;
            }
            let canonical = match path.canonicalize() {
                Ok(path) => path,
                Err(error) => {
                    skipped_files.push(HttpSkippedFile {
                        path: display_path(path, repository_root),
                        reason: error.to_string(),
                    });
                    continue;
                }
            };
            if !visited.insert(canonical) {
                continue;
            }
            let source = match std::fs::read_to_string(path) {
                Ok(source) => source,
                Err(error) => {
                    skipped_files.push(HttpSkippedFile {
                        path: display_path(path, repository_root),
                        reason: error.to_string(),
                    });
                    continue;
                }
            };
            detect_file(path, repository_root, &source, &mut operations);
        }
    }

    operations.sort_by(|left, right| {
        left.key
            .cmp(&right.key)
            .then_with(|| left.evidence.path.cmp(&right.evidence.path))
            .then_with(|| left.evidence.line.cmp(&right.evidence.line))
    });
    operations.dedup_by(|left, right| {
        left.key == right.key
            && left.evidence.path == right.evidence.path
            && left.evidence.line == right.evidence.line
    });
    skipped_files.sort_by(|left, right| left.path.cmp(&right.path));

    Ok(HttpSourceInventory {
        completeness: if complete {
            HttpSourceCompleteness::Complete
        } else {
            HttpSourceCompleteness::Partial
        },
        reason: if complete {
            "Project configuration asserts that all runtime HTTP operations use supported static declarations.".to_string()
        } else {
            "Static source detection is intentionally partial; dynamic registration, computed paths, and runtime mounting may be absent.".to_string()
        },
        operations,
        skipped_files,
    })
}

fn is_source_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|extension| extension.to_str()),
        Some("ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" | "py" | "rs" | "svelte")
    )
}

fn detect_file(
    path: &Path,
    repository_root: &Path,
    source: &str,
    output: &mut Vec<HttpSourceOperation>,
) {
    let extension = path.extension().and_then(|value| value.to_str());
    if sveltekit::is_server(path) {
        sveltekit::detect(path, repository_root, source, output);
    }
    match extension {
        Some("ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" | "svelte") => {
            ecmascript::detect(path, repository_root, source, output);
        }
        Some("py") => python::detect(path, repository_root, source, output),
        Some("rs") => rust::detect(path, repository_root, source, output),
        _ => {}
    }
}

#[allow(clippy::too_many_arguments)]
fn push_operation(
    output: &mut Vec<HttpSourceOperation>,
    method: &str,
    path: &str,
    detector: &str,
    confidence: HttpConfidence,
    file_path: &Path,
    repository_root: &Path,
    line: u32,
) {
    let method = method.to_uppercase();
    let path = normalize_path(path);
    output.push(HttpSourceOperation {
        key: operation_key(&method, &path),
        method,
        path,
        detector: detector.to_string(),
        confidence,
        evidence: HttpSourceEvidence {
            path: display_path(file_path, repository_root),
            line,
        },
    });
}

fn display_path(path: &Path, repository_root: &Path) -> String {
    crate::paths::normalize_relative_path(path, repository_root)
}

fn line_at(source: &str, offset: usize) -> u32 {
    source[..offset.min(source.len())]
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count() as u32
        + 1
}

#[cfg(test)]
mod tests {
    use super::{detect_file, ecmascript::object_literals_after};
    use std::path::Path;

    #[test]
    fn detects_create_route_descriptors_and_direct_typescript_routes() {
        let source = r#"
const createWidget = createRoute({
  method: 'post',
  path: '/widgets/{id}',
  request: { body: { content: {} } },
})
store.get('/ordinary')
app.get("/health", handler)
"#;
        let mut operations = Vec::new();
        detect_file(
            Path::new("/repo/src/routes.ts"),
            Path::new("/repo"),
            source,
            &mut operations,
        );
        let keys = operations
            .iter()
            .map(|operation| operation.key.as_str())
            .collect::<Vec<_>>();
        assert!(keys.contains(&"POST /widgets/{id}"));
        assert!(keys.contains(&"GET /health"));
        assert!(!keys.contains(&"GET /ordinary"));
        assert_eq!(object_literals_after(source, "createRoute").len(), 1);
    }

    #[test]
    fn detects_python_and_rust_routes() {
        let mut python = Vec::new();
        detect_file(
            Path::new("/repo/api.py"),
            Path::new("/repo"),
            "@app.post('/widgets')\ndef create(): pass\n",
            &mut python,
        );
        assert_eq!(python[0].key, "POST /widgets");

        let mut rust = Vec::new();
        detect_file(
            Path::new("/repo/api.rs"),
            Path::new("/repo"),
            "#[get(\"/widgets/{id}\")]\nasync fn get_widget() {}\n",
            &mut rust,
        );
        assert_eq!(rust[0].key, "GET /widgets/{id}");
    }

    #[test]
    fn detects_sveltekit_server_methods() {
        let mut operations = Vec::new();
        detect_file(
            Path::new("/repo/src/routes/(api)/users/[id]/+server.ts"),
            Path::new("/repo"),
            "export async function GET() {}\nexport const DELETE = handler\n",
            &mut operations,
        );
        let keys = operations
            .iter()
            .map(|operation| operation.key.as_str())
            .collect::<Vec<_>>();
        assert!(keys.contains(&"GET /users/{id}"));
        assert!(keys.contains(&"DELETE /users/{id}"));
    }
}

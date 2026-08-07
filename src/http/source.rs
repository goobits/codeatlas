use super::model::{
    HttpConfidence, HttpSkippedFile, HttpSourceCompleteness, HttpSourceEvidence,
    HttpSourceInventory, HttpSourceOperation, HttpSourceOperationKind,
};
use super::openapi::{normalize_path, operation_key};
use anyhow::{Context, Result};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

mod ecmascript;
mod medusa;
mod node;
mod node_regex;
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
        let boundary_root = source_root.clone();
        builder
            .hidden(false)
            .git_ignore(true)
            .git_global(false)
            .git_exclude(true)
            .filter_entry(move |entry| {
                entry.path() == boundary_root
                    || !entry.file_type().is_some_and(|kind| kind.is_dir())
                    || !is_nested_project_root(entry.path())
            });
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
            if !entry.file_type().is_some_and(|kind| kind.is_file())
                || !is_source_file(path)
                || is_test_source(path, &source_root)
            {
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
            && ((left.kind == HttpSourceOperationKind::Page
                && right.kind == HttpSourceOperationKind::Page)
                || (left.evidence.path == right.evidence.path
                    && left.evidence.line == right.evidence.line))
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
    if sveltekit::is_route(path) {
        return true;
    }
    matches!(
        path.extension().and_then(|extension| extension.to_str()),
        Some("ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" | "py" | "rs" | "svelte")
    )
}

fn is_nested_project_root(path: &Path) -> bool {
    ["package.json", "Cargo.toml", "pyproject.toml", "go.mod"]
        .iter()
        .any(|manifest| path.join(manifest).is_file())
}

fn is_test_source(path: &Path, source_root: &Path) -> bool {
    codeatlas_source::source_policy::is_conventional_test_source(
        path.strip_prefix(source_root).unwrap_or(path),
    )
}

fn detect_file(
    path: &Path,
    repository_root: &Path,
    source: &str,
    output: &mut Vec<HttpSourceOperation>,
) {
    let extension = path.extension().and_then(|value| value.to_str());
    detect_annotations(path, repository_root, source, output);
    if sveltekit::is_route(path) {
        sveltekit::detect(path, repository_root, source, output);
    }
    if medusa::is_route(path) {
        medusa::detect(path, repository_root, source, output);
    }
    match extension {
        Some("ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" | "svelte") => {
            ecmascript::detect(path, repository_root, source, output);
            node::detect(path, repository_root, source, output);
        }
        Some("py") => python::detect(path, repository_root, source, output),
        Some("rs") => rust::detect(path, repository_root, source, output),
        _ => {}
    }
}

fn detect_annotations(
    path: &Path,
    repository_root: &Path,
    source: &str,
    output: &mut Vec<HttpSourceOperation>,
) {
    use regex::Regex;
    use std::sync::OnceLock;

    static HTTP_ANNOTATION: OnceLock<Regex> = OnceLock::new();
    let annotation = HTTP_ANNOTATION.get_or_init(|| {
        Regex::new(
            r#"(?mi)@codeatlas-http[ \t]+(GET|PUT|POST|DELETE|OPTIONS|HEAD|PATCH|TRACE)[ \t]+(/[^\s*]*)"#,
        )
        .expect("CodeAtlas HTTP annotation detector")
    });
    for_each_comment(path, source, |offset, comment| {
        for captures in annotation.captures_iter(comment) {
            let (Some(method), Some(route)) = (captures.get(1), captures.get(2)) else {
                continue;
            };
            push_operation(
                output,
                method.as_str(),
                route.as_str(),
                "codeatlas_http_annotation",
                HttpConfidence::High,
                path,
                repository_root,
                line_at(source, offset.saturating_add(method.start())),
            );
        }
    });
}

fn for_each_comment(path: &Path, source: &str, mut visit: impl FnMut(usize, &str)) {
    let extension = path.extension().and_then(|value| value.to_str());
    let is_python = extension == Some("py");
    let is_rust = extension == Some("rs");
    let is_svelte = extension == Some("svelte");
    let bytes = source.as_bytes();
    let mut index = 0;

    while index < bytes.len() {
        if is_python && bytes[index] == b'#' {
            let start = index + 1;
            let end = source[start..]
                .find('\n')
                .map_or(bytes.len(), |relative| start + relative);
            visit(start, &source[start..end]);
            index = end;
            continue;
        }
        if !is_python && bytes[index..].starts_with(b"//") {
            let start = index + 2;
            let end = source[start..]
                .find('\n')
                .map_or(bytes.len(), |relative| start + relative);
            visit(start, &source[start..end]);
            index = end;
            continue;
        }
        if !is_python && bytes[index..].starts_with(b"/*") {
            let start = index + 2;
            let mut cursor = start;
            let mut depth = 1_u32;
            while cursor < bytes.len() && depth > 0 {
                if is_rust && bytes[cursor..].starts_with(b"/*") {
                    depth = depth.saturating_add(1);
                    cursor += 2;
                } else if bytes[cursor..].starts_with(b"*/") {
                    depth -= 1;
                    if depth == 0 {
                        visit(start, &source[start..cursor]);
                    }
                    cursor += 2;
                } else {
                    cursor += 1;
                }
            }
            index = cursor;
            continue;
        }
        if is_svelte && bytes[index..].starts_with(b"<!--") {
            let start = index + 4;
            let end = source[start..]
                .find("-->")
                .map_or(bytes.len(), |relative| start + relative);
            visit(start, &source[start..end]);
            index = (end + 3).min(bytes.len());
            continue;
        }
        if is_rust {
            if let Some(end) = rust_char_literal_end(source, index) {
                index = end;
                continue;
            }
            if let Some(end) = rust_raw_string_end(source, index) {
                index = end;
                continue;
            }
        }

        let quote = bytes[index];
        let is_quote = if is_rust {
            quote == b'"'
        } else if is_python {
            matches!(quote, b'\'' | b'"')
        } else {
            matches!(quote, b'\'' | b'"' | b'`')
        };
        if !is_quote {
            index += 1;
            continue;
        }

        let delimiter_len = if is_python && bytes[index..].starts_with(&[quote, quote, quote]) {
            3
        } else {
            1
        };
        index += delimiter_len;
        while index < bytes.len() {
            if bytes[index] == b'\\' {
                index = (index + 2).min(bytes.len());
            } else if (delimiter_len == 1 && bytes[index] == quote)
                || (delimiter_len == 3 && bytes[index..].starts_with(&[quote, quote, quote]))
            {
                index += delimiter_len;
                break;
            } else {
                index += 1;
            }
        }
    }
}

fn rust_char_literal_end(source: &str, start: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let quote = if bytes.get(start) == Some(&b'b') && bytes.get(start + 1) == Some(&b'\'') {
        start + 1
    } else if bytes.get(start) == Some(&b'\'') {
        start
    } else {
        return None;
    };
    let mut cursor = quote + 1;
    while cursor < bytes.len() && bytes[cursor] != b'\n' {
        if bytes[cursor] == b'\\' {
            cursor = (cursor + 2).min(bytes.len());
        } else if bytes[cursor] == b'\'' {
            return Some(cursor + 1);
        } else {
            cursor += 1;
        }
    }
    None
}

fn rust_raw_string_end(source: &str, start: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut cursor = start;
    if bytes.get(cursor) == Some(&b'b') {
        cursor += 1;
    }
    if bytes.get(cursor) != Some(&b'r') {
        return None;
    }
    cursor += 1;
    let hashes_start = cursor;
    while bytes.get(cursor) == Some(&b'#') {
        cursor += 1;
    }
    if bytes.get(cursor) != Some(&b'"') {
        return None;
    }
    let hash_count = cursor - hashes_start;
    cursor += 1;
    while cursor < bytes.len() {
        if bytes[cursor] == b'"'
            && bytes
                .get(cursor + 1..cursor + 1 + hash_count)
                .is_some_and(|suffix| suffix.iter().all(|byte| *byte == b'#'))
        {
            return Some(cursor + 1 + hash_count);
        }
        cursor += 1;
    }
    Some(bytes.len())
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
        kind: HttpSourceOperationKind::Endpoint,
        schema_missing: true,
        path_pattern: None,
        detector: detector.to_string(),
        confidence,
        evidence: HttpSourceEvidence {
            path: display_path(file_path, repository_root),
            line,
        },
    });
}

#[allow(clippy::too_many_arguments)]
fn push_pattern_operation(
    output: &mut Vec<HttpSourceOperation>,
    method: &str,
    path: &str,
    path_pattern: &str,
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
        kind: HttpSourceOperationKind::Endpoint,
        schema_missing: true,
        path_pattern: Some(path_pattern.to_string()),
        detector: detector.to_string(),
        confidence,
        evidence: HttpSourceEvidence {
            path: display_path(file_path, repository_root),
            line,
        },
    });
}

fn push_page(
    output: &mut Vec<HttpSourceOperation>,
    path: &str,
    path_pattern: &str,
    file_path: &Path,
    repository_root: &Path,
) {
    let path = normalize_path(path);
    output.push(HttpSourceOperation {
        key: operation_key("PAGE", &path),
        method: "PAGE".to_string(),
        path,
        kind: HttpSourceOperationKind::Page,
        schema_missing: false,
        path_pattern: Some(path_pattern.to_string()),
        detector: "sveltekit_page".to_string(),
        confidence: HttpConfidence::High,
        evidence: HttpSourceEvidence {
            path: display_path(file_path, repository_root),
            line: 1,
        },
    });
}

fn display_path(path: &Path, repository_root: &Path) -> String {
    codeatlas_source::paths::normalize_relative_path(path, repository_root)
}

fn line_at(source: &str, offset: usize) -> u32 {
    source[..offset.min(source.len())]
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count() as u32
        + 1
}

fn exported_http_methods(source: &str) -> impl Iterator<Item = regex::Match<'_>> {
    use regex::Regex;
    use std::sync::OnceLock;

    static EXPORT: OnceLock<Regex> = OnceLock::new();
    let export = EXPORT.get_or_init(|| {
		Regex::new(
			r#"(?m)\bexport\s+(?:(?:async\s+)?function|const|let|var)\s+(GET|PUT|POST|DELETE|OPTIONS|HEAD|PATCH)\b"#,
		)
		.expect("filesystem HTTP method detector")
	});
    export
        .captures_iter(source)
        .filter_map(|captures| captures.get(1))
}

#[cfg(test)]
mod tests {
    use super::{detect_file, ecmascript::object_literals_after, is_test_source};
    use std::path::Path;

    #[test]
    fn sveltekit_routes_under_test_fixtures_are_not_runtime_operations() {
        assert!(is_test_source(
            Path::new("/repo/tests/fixtures/app/src/routes/+page.svelte"),
            Path::new("/repo")
        ));
        assert!(!is_test_source(
            Path::new("/repo/src/routes/+page.svelte"),
            Path::new("/repo")
        ));
    }

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
// @codeatlas-http GET /exports/{id}
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
        assert!(keys.contains(&"GET /exports/{id}"));
        assert!(!keys.contains(&"GET /ordinary"));
        assert_eq!(object_literals_after(source, "createRoute").len(), 1);
    }

    #[test]
    fn annotations_attach_only_to_language_comments() {
        for (path, source) in [
            (
                "/repo/src/fixture.rs",
                "const FIXTURE: &str = r#\"// @codeatlas-http GET /rust-fixture\"#;\n",
            ),
            (
                "/repo/src/fixture.ts",
                "const fixture = `/* @codeatlas-http GET /typescript-fixture */`;\n",
            ),
            (
                "/repo/src/fixture.py",
                "fixture = '''# @codeatlas-http GET /python-fixture'''\n",
            ),
        ] {
            let mut operations = Vec::new();
            detect_file(Path::new(path), Path::new("/repo"), source, &mut operations);
            assert!(operations.is_empty(), "{path}: {operations:?}");
        }

        for (path, source, expected) in [
            (
                "/repo/src/route.rs",
                "/// @codeatlas-http GET /rust-comment\n",
                "GET /rust-comment",
            ),
            (
                "/repo/src/route.ts",
                "/** @codeatlas-http POST /typescript-comment */\n",
                "POST /typescript-comment",
            ),
            (
                "/repo/src/route.py",
                "# @codeatlas-http PATCH /python-comment\n",
                "PATCH /python-comment",
            ),
        ] {
            let mut operations = Vec::new();
            detect_file(Path::new(path), Path::new("/repo"), source, &mut operations);
            assert_eq!(
                operations
                    .iter()
                    .map(|operation| operation.key.as_str())
                    .collect::<Vec<_>>(),
                [expected],
                "{path}"
            );
        }
    }

    #[test]
    fn annotations_ignore_embedded_fixtures_in_this_rust_source() {
        let mut operations = Vec::new();
        detect_file(
            Path::new("/repo/src/http/source.rs"),
            Path::new("/repo"),
            include_str!("source.rs"),
            &mut operations,
        );
        assert!(
            operations
                .iter()
                .all(|operation| operation.key != "GET /exports/{id}"),
            "{operations:?}"
        );
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
            "export function GET() {}\nexport async function POST() {}\nexport const DELETE = handler\n",
            &mut operations,
        );
        detect_file(
            Path::new("/repo/src/routes/api/release/routes/+server.ts"),
            Path::new("/repo"),
            "export async function POST() {}\n",
            &mut operations,
        );
        let keys = operations
            .iter()
            .map(|operation| operation.key.as_str())
            .collect::<Vec<_>>();
        assert!(keys.contains(&"GET /users/{id}"));
        assert!(keys.contains(&"POST /users/{id}"));
        assert!(keys.contains(&"DELETE /users/{id}"));
        assert!(keys.contains(&"POST /api/release/routes"));
    }

    #[test]
    fn detects_sveltekit_pages_and_bounded_node_routes() {
        let mut page = Vec::new();
        detect_file(
            Path::new("/repo/src/routes/[[lang=lang]]/(site)/users/[id]/+page.svelte"),
            Path::new("/repo"),
            "<h1>User</h1>",
            &mut page,
        );
        let keys = page
            .iter()
            .map(|operation| operation.key.as_str())
            .collect::<Vec<_>>();
        assert!(keys.contains(&"PAGE /users/{id}"));
        assert!(keys.contains(&"PAGE /{lang}/users/{id}"));

        let mut node = Vec::new();
        detect_file(
            Path::new("/repo/src/server.ts"),
            Path::new("/repo"),
            r#"
if (req.method === 'GET' && url.pathname === '/health') {}
if (request.method === 'GET' && request.url === '/readinessz') {}
if (request.url === '/render') {
    if (request.method !== 'POST') return
    render()
}
if (req.method === 'GET' && req.url?.startsWith('/tmp-assets/')) {}
if (req.method === 'GET' && requestUrl.pathname.startsWith('/downloads/')) {}
const documentMatch = url.pathname.match(/^\/documents\/([^/]+)$/)
if (request.method === 'DELETE' && documentMatch) {}
"#,
            &mut node,
        );
        let keys = node
            .iter()
            .map(|operation| operation.key.as_str())
            .collect::<Vec<_>>();
        assert!(keys.contains(&"GET /health"));
        assert!(keys.contains(&"GET /readinessz"));
        assert!(keys.contains(&"POST /render"));
        assert!(keys.contains(&"GET /tmp-assets/{path}"));
        assert!(keys.contains(&"GET /downloads/{path}"));
        assert!(keys.contains(&"DELETE /documents/{segment1}"));
    }

    #[test]
    fn detects_cloudflare_fetch_path_guards_without_inventing_gets_for_post_routes() {
        let mut operations = Vec::new();
        detect_file(
            Path::new("/repo/src/worker.ts"),
            Path::new("/repo"),
            r#"
export default {
    async fetch(request: Request): Promise<Response> {
        const url = new URL(request.url)
        if (url.pathname === '/' || url.pathname === '/status') return new Response('ok')
        if (url.pathname === '/api/check' && request.method === 'POST') return Response.json({})
        if (url.pathname === '/upload') {
            if (request.method !== 'POST') return new Response('wrong method', { status: 405 })
            return new Response('upload')
        }
        return new Response('missing', { status: 404 })
    }
}
"#,
            &mut operations,
        );
        let keys = operations
            .iter()
            .map(|operation| operation.key.as_str())
            .collect::<Vec<_>>();
        assert!(keys.contains(&"GET /"));
        assert!(keys.contains(&"GET /status"));
        assert!(keys.contains(&"POST /api/check"), "{keys:?}");
        assert!(!keys.contains(&"GET /api/check"));
        assert!(!keys.contains(&"GET /upload"));
    }
}

use crate::domain::ScanConfig;
use crate::{analysis, languages, outputs, package};
use std::path::PathBuf;

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("docs")
}

#[test]
fn package_exports_map_declaration_outputs_back_to_source() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("docs-dist");
    let package = package::discover(&root)
        .expect("package manifest")
        .expect("package metadata");

    assert_eq!(package.exports.len(), 1);
    assert_eq!(package.exports[0].public_path, ".");
    assert_eq!(package.exports[0].source_path, "src/index.ts");
}

#[test]
fn package_docs_follow_public_exports_and_jsdoc() {
    let root = fixture_root();
    let config = ScanConfig {
        include_types: true,
        include_private: false,
        entrypoints: None,
        suggest: false,
        imports: false,
        no_default_ignore: false,
    };
    let mut report = languages::scan_all(&root, &config, languages::get_scanners_auto(&root));
    let package = package::discover(&root)
        .expect("package manifest")
        .expect("package metadata");
    package::annotate(&mut report, &root, package, false);
    analysis::annotate_docs(&mut report, &root);

    let create = report
        .symbols
        .iter()
        .find(|symbol| symbol.name == "createThing")
        .expect("createThing symbol");
    let docs = create.docs.as_ref().expect("createThing docs");
    assert_eq!(create.export_paths, ["@example/docs"]);
    assert_eq!(docs.summary, "Create a thing.");
    assert_eq!(
        docs.params.get("options").map(String::as_str),
        Some("Thing options.")
    );
    assert_eq!(docs.returns.as_deref(), Some("The created label."));
    assert_eq!(docs.examples, ["createThing({ label: 'demo' })"]);

    for expected in [
        "createThingArrow",
        "DEFAULT_LABEL",
        "ThingId",
        "ThingOptions",
        "ThingStore",
    ] {
        assert!(
            report.symbols.iter().any(|symbol| symbol.name == expected),
            "missing {expected}"
        );
    }

    let store = report
        .symbols
        .iter()
        .find(|symbol| symbol.name == "ThingStore")
        .expect("ThingStore symbol");
    assert_eq!(store.export_paths, ["@example/docs"]);
    for expected in ["constructor", "category", "name", "size", "find"] {
        assert!(
            store.children.iter().any(|child| child.name == expected),
            "missing ThingStore.{expected}"
        );
    }
    assert!(!store.children.iter().any(|child| child.name == "#reset"));
    assert_eq!(
        store
            .children
            .iter()
            .find(|child| child.name == "constructor")
            .map(|child| child.signature.as_str()),
        Some("constructor(name: string, public readonly category: string)")
    );
    assert!(store
        .children
        .iter()
        .find(|child| child.name == "find")
        .is_some_and(|child| child.id.ends_with("#ThingStore.find")));
    let add = report
        .symbols
        .iter()
        .find(|symbol| symbol.name == "add")
        .expect("add symbol");
    assert_eq!(add.export_paths, ["@example/docs/math"]);

    let markdown = outputs::markdown::render(&report, None);
    assert!(markdown.contains("# @example/docs API Reference"));
    assert!(markdown.contains("## `@example/docs/math`"));
    assert!(markdown.contains("Create a thing."));
    assert!(markdown.contains("**Members**"));
    assert!(
        markdown.contains("| `label` | `label: string` | Deprecated: Use `name`. Legacy label. |")
    );
    assert!(!markdown.contains("secret"));
    assert!(!markdown.contains("#### `label`"));
    assert!(!markdown.contains("internalOnly"));
    assert_eq!(markdown, outputs::markdown::render(&report, None));

    let html = outputs::html::render(&report, None);
    assert!(html.starts_with("<!doctype html>"));
    assert!(html.contains("Search public API"));
    assert!(html.contains("@example/docs API Reference"));
    assert!(html.contains("Create a thing."));
    assert!(html.contains("<th>Member</th>"));
    assert!(html.contains("Deprecated: Use <code>name</code>. Legacy label."));
    assert!(!html.contains("secret"));
    assert!(!html.contains("internalOnly"));
    assert_eq!(html, outputs::html::render(&report, None));

    let json = outputs::json::render(&report).expect("JSON report");
    assert!(json.contains("\"schema_version\": 1"));
    assert!(json.contains("\"package\""));
    assert!(json.contains("\"export_paths\""));
    let mut reordered = report.clone();
    reordered.symbols.reverse();
    assert_eq!(
        json,
        outputs::json::render(&reordered).expect("canonical JSON report")
    );
}

#[test]
fn legacy_json_reports_deserialize_with_schema_defaults() {
    let legacy = r#"{
        "stats":{"files_scanned":1,"files_skipped":0,"symbols_found":1,"routes_found":0},
        "symbols":[{
            "id":"ts:src/index.ts:fn#create",
            "name":"create",
            "kind":"Function",
            "visibility":"Public",
            "language":"TypeScript",
            "file_path":"src/index.ts",
            "span":null,
            "signature":"function create()",
            "children":[]
        }],
        "routes":[],
        "skipped_files":[],
        "imports":[],
        "unused_public":[]
    }"#;
    let report: crate::domain::ScanReport =
        serde_json::from_str(legacy).expect("legacy report should remain readable");

    assert_eq!(report.schema_version, 1);
    assert!(!report.tool_version.is_empty());
    assert!(report.symbols[0].docs.is_none());
    assert!(report.symbols[0].export_paths.is_empty());
}

#[test]
fn typescript_signatures_preserve_common_public_types() {
    let source = r#"
export function listNames(): readonly string[] { return [] }
export function isName(value: unknown): value is string { return true }
export type NameKey = `name:${string}`
"#;
    let report = crate::languages::typescript::parser::parse_source(source, "src/types.ts")
        .expect("TypeScript source");
    let signature = |name: &str| {
        report
            .symbols
            .iter()
            .find(|symbol| symbol.name == name)
            .map(|symbol| symbol.signature.as_str())
    };

    assert_eq!(
        signature("listNames"),
        Some("function listNames() -> readonly string[]")
    );
    assert_eq!(
        signature("isName"),
        Some("function isName(value: unknown) -> value is string")
    );
    assert_eq!(
        signature("NameKey"),
        Some("type NameKey = `name:${string}`")
    );
}

#[test]
fn typescript_overloads_share_one_stable_symbol() {
    let source = r#"
export function parse(value: string): string
export function parse(value: number): number
export function parse(value: string | number): string | number { return value }
"#;
    let report = crate::languages::typescript::parser::parse_source(source, "src/overloads.ts")
        .expect("TypeScript overloads");
    let overloads = report
        .symbols
        .iter()
        .filter(|symbol| symbol.name == "parse")
        .collect::<Vec<_>>();

    assert_eq!(overloads.len(), 1);
    assert_eq!(overloads[0].signature.lines().count(), 3);
    assert_eq!(overloads[0].id, "ts:src/overloads.ts:fn#parse");
}

#[test]
fn extracts_python_and_rust_source_docs() {
    for (fixture, symbol_name, summary) in [
        ("py", "public_func", "Return the public fixture value."),
        ("rs", "used", "Return the used fixture value."),
    ] {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join(fixture);
        let config = ScanConfig {
            include_types: true,
            include_private: false,
            entrypoints: None,
            suggest: false,
            imports: false,
            no_default_ignore: false,
        };
        let mut report = languages::scan_all(&root, &config, languages::get_scanners_auto(&root));
        analysis::annotate_docs(&mut report, &root);
        let symbol = report
            .symbols
            .iter()
            .find(|symbol| symbol.name == symbol_name)
            .expect("documented symbol");
        assert_eq!(
            symbol.docs.as_ref().map(|docs| docs.summary.as_str()),
            Some(summary)
        );
    }
}

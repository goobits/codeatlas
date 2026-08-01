use crate::config::ProjectConfig;
use crate::domain::ScanConfig;
use crate::{analysis, commands, languages, outputs, package};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("docs")
}

fn fixture_report(include_private: bool) -> crate::domain::ScanReport {
    let root = fixture_root();
    let config = ScanConfig {
        include_types: true,
        include_private,
        entrypoints: None,
        no_default_ignore: false,
    };
    let mut report = languages::scan_all(&root, &config, languages::get_scanners_auto(&root));
    let package = package::discover(&root)
        .expect("package manifest")
        .expect("package metadata");
    analysis::annotate_package_exports(&mut report, &root, package, false);
    analysis::annotate_docs(&mut report, &root);
    report
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
fn declaration_contract_docs_follow_the_shipped_types_target() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("docs-dist");
    let package = package::discover_for_docs(&root, true)
        .expect("package manifest")
        .expect("package metadata");

    assert_eq!(package.exports.len(), 1);
    assert_eq!(package.exports[0].source_path, "dist/types/index.d.ts");
}

#[test]
fn declaration_contract_scans_reachable_files_inside_ignored_dist() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("docs-dist");
    let config = ScanConfig {
        include_types: true,
        include_private: false,
        entrypoints: Some(vec!["dist/types/index.d.ts".to_string()]),
        no_default_ignore: false,
    };
    let report = languages::scan_all(
        &root,
        &config,
        languages::get_scanners(Some(vec!["ts".to_string()])),
    );

    let symbol = report
        .symbols
        .iter()
        .find(|symbol| symbol.name == "createPublicValue")
        .expect("public declaration alias");
    assert_eq!(symbol.visibility, crate::domain::Visibility::Public);
    assert_eq!(
        symbol.signature,
        "function createPublicValue(options: PublicValueOptions) -> string"
    );
    assert!(report
        .symbols
        .iter()
        .any(|symbol| symbol.name == "PublicValueOptions"));
    assert!(
        !report
            .symbols
            .iter()
            .any(|symbol| symbol.name == "createValue"),
        "declaration contracts must expose the shipped alias, not its private name"
    );
}

#[test]
fn declaration_contract_rejects_empty_public_export_scans() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("docs-dist");
    let project =
        ProjectConfig::load(&root, Some(&root.join("codeatlas.json"))).expect("project config");
    let mut report = crate::domain::ScanReport::default();

    let error = commands::annotate_report(&mut report, &project)
        .expect_err("empty declaration contract must fail");
    assert!(error.to_string().contains("no scanned symbols"));
}

#[test]
fn declaration_contract_rejects_missing_configured_entrypoints() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("docs-dist");
    let mut project =
        ProjectConfig::load(&root, Some(&root.join("codeatlas.json"))).expect("project config");
    project.config.entrypoints = vec!["dist/types/missing.d.ts".to_string()];

    let error = commands::build_scan_config(&project, false, None)
        .expect_err("missing declaration entrypoint must fail");
    assert!(error.to_string().contains("dist/types/missing.d.ts"));
    assert!(error.to_string().contains("Build the package declarations"));
}

#[test]
fn declaration_contract_rejects_missing_package_type_targets() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "codeatlas-missing-declaration-target-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(&root).expect("temporary package");
    fs::write(
        root.join("package.json"),
        r#"{
            "name": "@example/missing-declarations",
            "exports": { ".": { "types": "./dist/index.d.ts" } }
        }"#,
    )
    .expect("package manifest");

    let error = package::discover_for_docs(&root, true)
        .expect_err("missing package declaration target must fail");
    assert!(error
        .to_string()
        .contains("no existing TypeScript declaration export targets"));
    fs::remove_dir_all(root).expect("remove temporary package");
}

#[cfg(unix)]
#[test]
fn declaration_contract_scans_an_entrypoint_through_a_managed_directory_symlink() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "codeatlas-symlinked-declaration-root-{}-{nonce}",
        std::process::id()
    ));
    let output = std::env::temp_dir().join(format!(
        "codeatlas-symlinked-declaration-output-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(&root).expect("temporary package");
    fs::create_dir_all(&output).expect("managed declaration output");
    fs::write(
        root.join("package.json"),
        r#"{
            "name": "@example/symlinked-declarations",
            "exports": { ".": { "types": "./dist/index.d.ts" } }
        }"#,
    )
    .expect("package manifest");
    fs::write(
        root.join("codeatlas.json"),
        r#"{
            "entrypoints": ["dist/index.d.ts"],
            "languages": ["ts"],
            "docs": { "declaration_contract": true }
        }"#,
    )
    .expect("CodeAtlas config");
    fs::write(
        output.join("index.d.ts"),
        "/** Public managed declaration. */\nexport interface ManagedPublicAPI { ready: boolean }\n",
    )
    .expect("managed declaration");
    std::os::unix::fs::symlink(&output, root.join("dist")).expect("managed output symlink");

    let project =
        ProjectConfig::load(&root, Some(&root.join("codeatlas.json"))).expect("project config");
    let config = commands::build_scan_config(&project, false, None).expect("scan config");
    let mut report = commands::scan_project(&project, &config).expect("declaration scan");
    commands::annotate_report(&mut report, &project).expect("annotated declaration report");

    assert!(report
        .symbols
        .iter()
        .any(|symbol| symbol.name == "ManagedPublicAPI"));
    fs::remove_dir_all(root).expect("remove temporary package");
    fs::remove_dir_all(output).expect("remove managed declaration output");
}

#[test]
fn declaration_contract_includes_transitive_referenced_types() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "codeatlas-declaration-references-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(root.join("dist")).expect("declaration output directory");
    fs::write(
        root.join("package.json"),
        r#"{
            "name": "@example/declarations",
            "exports": { ".": { "types": "./dist/index.d.ts" } }
        }"#,
    )
    .expect("package manifest");
    fs::write(
        root.join("codeatlas.json"),
        r#"{
            "languages": ["ts"],
            "package_exports": true,
            "docs": { "declaration_contract": true }
        }"#,
    )
    .expect("CodeAtlas config");
    fs::write(
        root.join("dist/index.d.ts"),
        r#"
/** Deep value required by a supporting declaration. */
interface Detail {
    /** Stable detail id. */
    id: string
}

/** Input required by the public API. */
interface Support {
    /** Nested detail. */
    detail: Detail
}

/** Declaration that is unrelated to the public contract. */
interface Unused {
    ignored: boolean
}

/** Directly importable public API. */
interface PublicAPI {
    /** Load one detail. */
    load(input: Support): Detail
}

export { PublicAPI }
"#,
    )
    .expect("declaration contract");

    let project =
        ProjectConfig::load(&root, Some(&root.join("codeatlas.json"))).expect("project config");
    let config = commands::build_scan_config(&project, false, None).expect("scan config");
    let mut report = commands::scan_project(&project, &config).expect("declaration scan");
    commands::annotate_report(&mut report, &project).expect("annotated declaration report");

    let public = report
        .symbols
        .iter()
        .find(|symbol| symbol.name == "PublicAPI")
        .expect("direct public symbol");
    assert!(!public.referenced);
    assert_eq!(public.export_paths, ["@example/declarations"]);
    for expected in ["Support", "Detail"] {
        let symbol = report
            .symbols
            .iter()
            .find(|symbol| symbol.name == expected)
            .unwrap_or_else(|| panic!("missing {expected}"));
        assert!(symbol.referenced, "{expected} must be marked as referenced");
        assert!(symbol.export_paths.is_empty());
    }
    assert!(!report.symbols.iter().any(|symbol| symbol.name == "Unused"));

    let markdown = outputs::markdown::render(&report, None, false, Some("Example SDK"));
    assert!(markdown.contains("## `Example SDK`"));
    assert!(markdown.contains("## `Supporting types`"));
    assert!(markdown.contains("#### `Support`"));
    assert!(markdown.contains("#### `Detail`"));

    fs::remove_dir_all(root).expect("remove declaration fixture");
}

#[test]
fn package_docs_follow_public_exports_and_jsdoc() {
    let report = fixture_report(false);

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
    let size = store
        .children
        .iter()
        .find(|child| child.name == "size")
        .expect("ThingStore.size getter");
    assert_eq!(size.kind, crate::domain::SymbolKind::Property);
    assert_eq!(size.signature, "get size() -> number");
    let add = report
        .symbols
        .iter()
        .find(|symbol| symbol.name == "add")
        .expect("add symbol");
    assert_eq!(add.export_paths, ["@example/docs/math"]);

    let markdown = outputs::markdown::render(&report, None, false, None);
    assert!(markdown.contains("# @example/docs API Reference"));
    assert!(markdown.contains("## `@example/docs/math`"));
    assert!(markdown.contains("### Classes"));
    assert!(markdown.contains("### Functions"));
    assert!(markdown.contains("Create a thing."));
    assert!(markdown.contains("**Members**"));
    assert!(
        markdown.contains("| `label` | `label: string` | Deprecated: Use `name`. Legacy label. |")
    );
    assert!(!markdown.contains("secret"));
    assert!(!markdown.contains("#### `label`"));
    assert!(!markdown.contains("internalOnly"));
    assert_eq!(
        markdown,
        outputs::markdown::render(&report, None, false, None)
    );

    let html = outputs::html::render(&report, None, false);
    assert!(html.starts_with("<!doctype html>"));
    assert!(html.contains("Search public API"));
    assert!(html.contains("@example/docs API Reference"));
    assert!(html.contains("Create a thing."));
    assert!(html.contains("<th>Member</th>"));
    assert!(html.contains("Deprecated: Use <code>name</code>. Legacy label."));
    assert!(html.contains("Skip to API reference"));
    assert!(html.contains("Browse API"));
    assert!(html.contains("class=\"atlas-kind-section__title\">Classes</h3>"));
    assert!(html.contains("class=\"atlas-nav__kind\">Functions</p>"));
    assert!(html.contains("class=\"atlas-permalink\""));
    assert!(html.contains("http-equiv=\"Content-Security-Policy\""));
    assert!(html.contains("script-src &#39;sha256-"));
    assert!(!html.contains("unsafe-inline"));
    assert!(html.contains("thingstore.find"));
    assert!(!html.contains("secret"));
    assert!(!html.contains("internalOnly"));
    assert_eq!(html, outputs::html::render(&report, None, false));

    let options = crate::config::DocsConfig {
        canonical_url: Some("https://example.com/api".to_string()),
        description: Some("Example API documentation".to_string()),
        home_url: Some("https://example.com".to_string()),
        public_name: Some("Example Browser SDK".to_string()),
        theme: crate::config::DocsThemeConfig {
            dark: crate::config::DocsThemePalette {
                accent: Some("#c4b5fd".to_string()),
                ..crate::config::DocsThemePalette::default()
            },
            light: crate::config::DocsThemePalette {
                accent: Some("#6c3aed".to_string()),
                background: Some("#fafafa".to_string()),
                ..crate::config::DocsThemePalette::default()
            },
        },
        ..crate::config::DocsConfig::default()
    };
    let configured_html = outputs::html::render_with_options(&report, None, false, &options);
    assert!(configured_html.contains("rel=\"canonical\" href=\"https://example.com/api\""));
    assert!(configured_html.contains("content=\"Example API documentation\""));
    assert!(configured_html.contains("property=\"og:title\""));
    assert!(configured_html.contains("property=\"og:url\" content=\"https://example.com/api\""));
    assert!(configured_html.contains("name=\"twitter:card\" content=\"summary\""));
    assert!(configured_html.contains("class=\"atlas-brand\" href=\"https://example.com\""));
    assert!(configured_html.contains("class=\"atlas-type-link\""));
    assert!(configured_html.contains("Example Browser SDK"));
    assert!(!configured_html.contains("@example/docs/math"));
    assert!(configured_html.contains("@media (prefers-color-scheme: light)"));
    assert!(configured_html.contains("--atlas-accent: #6c3aed"));
    assert!(configured_html.contains("--atlas-bg: #fafafa"));
    assert!(configured_html.contains("@media (prefers-color-scheme: dark)"));
    assert!(configured_html.contains("--atlas-accent: #c4b5fd"));

    let json = outputs::json::render(&report).expect("JSON report");
    assert!(json.contains("\"schema_version\": 2"));
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
fn configured_entrypoints_limit_reported_package_exports() {
    let root = fixture_root();
    let mut project =
        ProjectConfig::load(&root, Some(&root.join("codeatlas.json"))).expect("project config");
    project.config.entrypoints = vec!["src/index.ts".to_string()];
    let config = ScanConfig {
        include_types: true,
        include_private: false,
        entrypoints: Some(project.config.entrypoints.clone()),
        no_default_ignore: false,
    };
    let mut report = languages::scan_all(&root, &config, languages::get_scanners_auto(&root));
    commands::annotate_report(&mut report, &project).expect("annotated entrypoint report");

    let package = report.package.expect("package metadata");
    assert_eq!(package.exports.len(), 1);
    assert_eq!(package.exports[0].public_path, ".");
    assert!(report.symbols.iter().all(|symbol| !symbol
        .export_paths
        .contains(&"@example/docs/math".to_string())));
}

#[test]
fn configured_reports_follow_reachable_dependency_types() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "codeatlas-dependency-docs-{}-{nonce}",
        std::process::id()
    ));
    let dependency = root.join("node_modules/@example/contracts");
    let vendor = root.join("node_modules/vendor-types");
    fs::create_dir_all(root.join("src")).expect("root source directory");
    fs::create_dir_all(dependency.join("src")).expect("dependency source directory");
    fs::create_dir_all(vendor.join("src")).expect("vendor source directory");
    fs::write(
        root.join("package.json"),
        r#"{
            "name": "@example/app",
            "dependencies": {
                "@example/contracts": "workspace:*",
                "vendor-types": "^1.0.0"
            },
            "exports": { ".": { "source": "./src/index.ts" } }
        }"#,
    )
    .expect("root package manifest");
    fs::write(
        root.join("codeatlas.json"),
        r#"{
            "languages": ["ts"],
            "include_types": true,
            "package_exports": true,
            "docs": { "include_dependency_types": true }
        }"#,
    )
    .expect("root CodeAtlas config");
    fs::write(
        root.join("src/index.ts"),
        r#"
import type { ExternalAPI } from '@example/contracts/public'
import type { VendorAPI } from 'vendor-types'

/** Public application API. */
export interface PublicAPI {
    /** Dependency-owned operations. */
    readonly external: ExternalAPI

    /** Vendor-owned operations remain a referenced external type. */
    readonly vendor: VendorAPI
}
"#,
    )
    .expect("root source");
    fs::write(
        dependency.join("package.json"),
        r#"{
            "name": "@example/contracts",
            "exports": { "./public": { "source": "./src/public.ts" } }
        }"#,
    )
    .expect("dependency package manifest");
    fs::write(
        dependency.join("src/public.ts"),
        r#"
export * from './contracts.ts'
"#,
    )
    .expect("dependency entrypoint");
    fs::write(
        dependency.join("src/contracts.ts"),
        r#"
/** Dependency operations exposed through the application API. */
export interface ExternalAPI {
    /** Load one external result. */
    load(options: ExternalOptions): Promise<ExternalResult>
}

/** Options accepted by `ExternalAPI.load`. */
export interface ExternalOptions {
    /** Stable item identifier. */
    id: string
}

/** Result returned by `ExternalAPI.load`. */
export interface ExternalResult {
    /** Loaded item label. */
    label: string
}

/** Unrelated dependency contract. */
export interface UnrelatedAPI {
    clear(): void
}
"#,
    )
    .expect("dependency contracts");
    fs::write(
        vendor.join("package.json"),
        r#"{
            "name": "vendor-types",
            "version": "1.0.0",
            "exports": { ".": { "source": "./src/index.ts" } }
        }"#,
    )
    .expect("vendor package manifest");
    fs::write(
        vendor.join("src/index.ts"),
        "/** Third-party API that should not be copied into local docs. */\nexport interface VendorAPI {}\n",
    )
    .expect("vendor source");

    let config = ScanConfig {
        include_types: true,
        include_private: false,
        entrypoints: Some(vec!["src/index.ts".to_string()]),
        no_default_ignore: false,
    };
    let mut report = languages::scan_all(&root, &config, languages::get_scanners_auto(&root));
    let project =
        ProjectConfig::load(&root, Some(&root.join("codeatlas.json"))).expect("project config");
    commands::annotate_report(&mut report, &project).expect("reachable dependency types");

    for expected in ["ExternalAPI", "ExternalOptions", "ExternalResult"] {
        assert!(
            report.symbols.iter().any(|symbol| symbol.name == expected),
            "missing {expected}"
        );
    }
    assert!(!report
        .symbols
        .iter()
        .any(|symbol| symbol.name == "UnrelatedAPI"));
    assert!(!report
        .symbols
        .iter()
        .any(|symbol| symbol.name == "VendorAPI"));
    let external = report
        .symbols
        .iter()
        .find(|symbol| symbol.name == "ExternalAPI")
        .expect("ExternalAPI symbol");
    assert_eq!(external.package.as_deref(), Some("@example/contracts"));
    assert_eq!(external.export_paths, ["@example/contracts/public"]);
    assert!(external.referenced);
    assert!(external.file_path.starts_with("@example/contracts/"));
    assert_eq!(
        external
            .children
            .iter()
            .find(|child| child.name == "load")
            .and_then(|child| child.docs.as_ref())
            .map(|docs| docs.summary.as_str()),
        Some("Load one external result.")
    );
    let markdown = outputs::markdown::render(&report, Some("Application API"), false, None);
    assert!(markdown.contains("## `Supporting types`"));
    assert!(!markdown.contains("## `@example/contracts/public`"));
    assert!(markdown.contains("Options accepted by `ExternalAPI.load`."));
    assert!(!markdown.contains("Unrelated dependency contract."));

    let public_options = crate::config::DocsConfig {
        public_name: Some("Application Browser SDK".to_string()),
        ..crate::config::DocsConfig::default()
    };
    let public_html = outputs::html::render_with_options(&report, None, false, &public_options);
    assert!(public_html.contains("Application Browser SDK"));
    assert!(public_html.contains("ExternalAPI"));
    assert!(!public_html.contains("@example/contracts"));
    assert!(!public_html.contains("ts:src/"));

    fs::remove_dir_all(&root).expect("remove dependency docs fixture");
}

#[test]
fn internal_interface_members_follow_include_private() {
    let public_report = fixture_report(false);
    let options = public_report
        .symbols
        .iter()
        .find(|symbol| symbol.name == "ThingOptions")
        .expect("ThingOptions symbol");
    let secret = options
        .children
        .iter()
        .find(|child| child.name == "secret")
        .expect("ThingOptions.secret member");

    assert!(secret.docs.as_ref().is_some_and(|docs| docs.internal));
    assert!(!outputs::markdown::render(&public_report, None, false, None).contains("secret"));
    assert!(!outputs::html::render(&public_report, None, false).contains("secret"));

    let private_report = fixture_report(true);
    let private_markdown = outputs::markdown::render(&private_report, None, true, None);
    assert!(private_markdown
        .contains("| `secret` | `secret: string` | Parser-only implementation marker. |"));
    assert!(private_markdown.contains("##### `#reset`"));
    assert!(!private_markdown.contains("@internal"));

    let private_html = outputs::html::render(&private_report, None, true);
    assert!(private_html.contains("<td><code>secret</code></td>"));
    assert!(private_html.contains("<code>#reset</code>"));
    assert!(!private_html.contains("@internal"));
}

#[test]
fn scan_reports_require_an_explicit_schema_contract() {
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
    assert!(serde_json::from_str::<crate::domain::ScanReport>(legacy).is_err());
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
fn typescript_signatures_preserve_generics_and_object_aliases() {
    let source = r#"
export interface Result<TValue extends object = Record<string, unknown>> {
    map<TNext = TValue>(callback: (value: TValue) => TNext): Result<TNext>
    readonly save: <TFormat extends string = "blob">(value: TValue) => Promise<TFormat>
}
export type Options<TValue = string> = {
    readonly value: TValue
    optional?: number
    transform<TNext>(value: TValue): TNext
}
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
        signature("Result"),
        Some("interface Result<TValue extends object = Record<string, unknown>>")
    );
    assert_eq!(
        signature("Options"),
        Some(
            "type Options<TValue = string> = { readonly value: TValue; optional?: number; transform<TNext>(value: TValue): TNext }"
        )
    );
    let result = report
        .symbols
        .iter()
        .find(|symbol| symbol.name == "Result")
        .expect("Result interface");
    assert_eq!(
        result
            .children
            .first()
            .map(|child| child.signature.as_str()),
        Some("map<TNext = TValue>(callback: (value: TValue) => TNext) -> Result<TNext>")
    );
    assert_eq!(
        result.children.get(1).map(|child| child.signature.as_str()),
        Some(
            "readonly save: <TFormat extends string = \"blob\">(value: TValue) => Promise<TFormat>"
        )
    );
}

#[test]
fn typescript_accessors_are_documented_as_properties() {
    let source = r#"
export interface Store {
    get current(): string
    set current(value: string)
}
export class StoreImpl {
    get current(): string { return "" }
    set current(value: string) {}
}
"#;
    let report = crate::languages::typescript::parser::parse_source(source, "src/accessors.ts")
        .expect("TypeScript accessors");

    for owner in ["Store", "StoreImpl"] {
        let symbol = report
            .symbols
            .iter()
            .find(|symbol| symbol.name == owner)
            .expect("accessor owner");
        let accessors = symbol
            .children
            .iter()
            .filter(|child| child.name == "current")
            .collect::<Vec<_>>();
        assert_eq!(accessors.len(), 1);
        assert_eq!(accessors[0].kind, crate::domain::SymbolKind::Property);
        assert!(accessors[0]
            .signature
            .lines()
            .any(|signature| signature.starts_with("get current")));
        assert!(accessors[0]
            .signature
            .lines()
            .any(|signature| signature.starts_with("set current")));
    }
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

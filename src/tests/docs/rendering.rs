use super::*;

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
    assert!(json.contains("\"schema_version\": 3"));
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

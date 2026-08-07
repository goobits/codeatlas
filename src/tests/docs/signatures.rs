use super::*;

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
    assert!(serde_json::from_str::<codeatlas_domain::ScanReport>(legacy).is_err());
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
        assert_eq!(accessors[0].kind, codeatlas_domain::SymbolKind::Property);
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

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("py");
    let config = ScanConfig {
        include_types: true,
        include_private: false,
        entrypoints: None,
        no_default_ignore: false,
    };
    let mut report = languages::scan_all(&root, &config, languages::get_scanners_auto(&root));
    analysis::annotate_docs(&mut report, &root);
    let constant = report
        .symbols
        .iter()
        .find(|symbol| symbol.name == "PUBLIC_LABEL")
        .expect("Python constant");
    assert!(constant.docs.is_none());
}

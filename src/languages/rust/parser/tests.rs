use super::{parse_module_info, reachability::format_capture_identifiers};
use std::collections::BTreeSet;
use std::path::Path;

#[test]
fn reachability_tracks_attributes_methods_and_embedded_sources() {
    let source = r#"
            struct Config {
                #[serde(default = "fallback")]
                value: u32,
            }

            fn fallback() -> u32 {
                1
            }

            enum Mode {
                One,
            }

            impl From<u32> for Mode {
                fn from(_: u32) -> Self {
                    Self::One
                }
            }

            const HOOK: &str = include_str!("hooks.py");
        "#;
    let info =
        parse_module_info(Path::new("src/lib.rs"), Path::new("."), source).expect("Rust facts");
    assert!(info
        .reachability
        .symbol_paths
        .get("Config")
        .is_some_and(|paths| paths.contains(&vec!["fallback".to_string()])));
    let mode = info
        .symbols
        .iter()
        .find(|symbol| symbol.name == "Mode")
        .expect("enum symbol");
    assert!(mode.children.iter().any(|symbol| symbol.name == "from"));
    assert!(!info.symbols.iter().any(|symbol| symbol.name == "Mode.from"));
    assert!(info
        .reachability
        .embedded_sources
        .iter()
        .any(|source| { source.owner.as_deref() == Some("HOOK") && source.path == "hooks.py" }));
}

#[test]
fn reachability_understands_tauri_command_registration() {
    let source = r#"
            #[derive(Serialize, schemars::JsonSchema)]
            #[serde(rename_all = "camelCase")]
            struct Payload {
                value: String,
            }

            #[tauri::command]
            fn local_command() {}

            fn main() {
                let _ = tauri::generate_handler![
                    commands::dialog::open_file,
                    commands::fs::read,
                ];
                let _ = tauri::generate_context!();
            }
        "#;
    let info =
        parse_module_info(Path::new("src/main.rs"), Path::new("."), source).expect("Rust facts");
    let main_paths = info
        .reachability
        .symbol_paths
        .get("main")
        .expect("main references");
    assert!(main_paths.contains(&vec![
        "commands".to_string(),
        "dialog".to_string(),
        "open_file".to_string(),
    ]));
    assert!(main_paths.contains(&vec![
        "commands".to_string(),
        "fs".to_string(),
        "read".to_string(),
    ]));
    let uncertainty = info
        .reachability
        .uncertainties
        .iter()
        .map(|item| item.expression.as_str())
        .collect::<Vec<_>>();
    assert!(!uncertainty
        .iter()
        .any(|item| item.contains("generate_context")));
    assert!(!uncertainty
        .iter()
        .any(|item| item.contains("generate_handler")));
    assert!(!uncertainty
        .iter()
        .any(|item| item.contains("tauri :: command")));
    assert!(!uncertainty.iter().any(|item| item.contains("serde")));
    assert!(!uncertainty.iter().any(|item| item.contains("JsonSchema")));
}

#[test]
fn reachability_keeps_receiver_calls_owned_by_the_calling_method() {
    let source = r#"
            struct Worker;

            impl Worker {
                fn run(&self) {
                    self.finish();
                    format!("{}", helper());
                }

                fn finish(&self) {}
            }

            fn helper() {}

            #[cfg(test)]
            mod tests {
                struct TestHelper;

                impl TestHelper {
                    fn prepare() {}
                }

                #[test]
                fn smoke() {
                    TestHelper::prepare();
                }
            }
        "#;
    let info =
        parse_module_info(Path::new("src/lib.rs"), Path::new("."), source).expect("Rust facts");

    assert_eq!(
        info.reachability.symbol_method_calls["Worker.run"],
        BTreeSet::from(["finish".to_string()])
    );
    assert!(info.reachability.symbol_paths["Worker.run"].contains(&vec!["helper".to_string()]));
    assert!(info
        .modules
        .iter()
        .any(|module| module.name == "tests" && module.inline && module.test_only));
    assert!(info.reachability.symbol_paths["smoke"]
        .contains(&vec!["TestHelper".to_string(), "prepare".to_string()]));
}

#[test]
fn reachability_recognizes_qualified_test_attributes() {
    let source = r#"
            #[test]
            fn synchronous_test() {}

            #[tokio::test]
            async fn asynchronous_test() {}

            #[rstest]
            fn parameterized_test() {}

            fn helper() {}
        "#;
    let info = parse_module_info(Path::new("tests/runtime.rs"), Path::new("."), source)
        .expect("Rust facts");

    assert_eq!(
        info.reachability.test_symbols,
        BTreeSet::from([
            "asynchronous_test".to_string(),
            "parameterized_test".to_string(),
            "synchronous_test".to_string(),
        ])
    );
}

#[test]
fn reachability_tracks_implicit_format_string_captures() {
    let source = r#"
            const TOKEN_PREFIX: &str = "access.v1";

            fn token(width: usize) -> String {
                format!("{TOKEN_PREFIX:>width$} {{escaped}}")
            }
        "#;
    let info =
        parse_module_info(Path::new("src/lib.rs"), Path::new("."), source).expect("Rust facts");
    let paths = &info.reachability.symbol_paths["token"];
    assert!(paths.contains(&vec!["TOKEN_PREFIX".to_string()]));
    assert!(paths.contains(&vec!["width".to_string()]));
    assert!(!paths.contains(&vec!["escaped".to_string()]));

    assert_eq!(
        format_capture_identifiers("{value:?} {value:>width$} {{literal}} {0}"),
        BTreeSet::from(["value".to_string(), "width".to_string()])
    );
}

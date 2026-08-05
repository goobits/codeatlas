use super::contexts::{is_conventional_test_module, is_conventional_tooling_module};

#[test]
fn conventional_test_detection_excludes_test_helpers() {
    assert!(is_conventional_test_module("src/example.test.ts"));
    assert!(is_conventional_test_module("tests/example.spec.js"));
    assert!(is_conventional_test_module("e2e/example.e2e.ts"));
    assert!(is_conventional_test_module("src/Example.test.svelte"));
    assert!(!is_conventional_test_module("src/__tests__/support.ts"));
    assert!(!is_conventional_test_module("src/contest.ts"));
    assert!(!is_conventional_test_module("src/example.test.d.ts"));
}

#[test]
fn conventional_tooling_detection_is_limited_to_root_config_modules() {
    assert!(is_conventional_tooling_module("vitest.config.ts"));
    assert!(is_conventional_tooling_module("playwright.config.mjs"));
    assert!(is_conventional_tooling_module("gulpfile.js"));
    assert!(!is_conventional_tooling_module("src/runtime.config.ts"));
    assert!(!is_conventional_tooling_module("vitest.config.json"));
}

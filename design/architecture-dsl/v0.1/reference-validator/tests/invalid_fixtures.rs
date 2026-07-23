use codeatlas_architecture_dsl_reference_validator::{parse_restricted_yaml, ParseLimits};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FixtureManifest {
    cases: Vec<InvalidFixture>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct InvalidFixture {
    id: String,
    input: String,
    expected_diagnostic: String,
}

fn design_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("design root")
        .to_path_buf()
}

#[test]
fn every_restricted_yaml_fixture_fails_for_its_declared_reason() {
    let root = design_root();
    let manifest_bytes =
        fs::read(root.join("fixtures/invalid/expectations.yaml")).expect("manifest");
    let manifest_value =
        parse_restricted_yaml(&manifest_bytes, ParseLimits::default()).expect("fixture manifest");
    let manifest: FixtureManifest =
        serde_json::from_value(manifest_value.value).expect("decode manifest");
    assert_eq!(manifest.cases.len(), 10);

    for fixture in manifest.cases {
        let source = fs::read(root.join(&fixture.input)).expect("fixture input");
        let error = parse_restricted_yaml(&source, ParseLimits::default()).expect_err(&fixture.id);
        assert_eq!(
            error.diagnostic.code, fixture.expected_diagnostic,
            "{}",
            fixture.id
        );
    }
}

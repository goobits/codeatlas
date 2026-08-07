use super::{
    validate_test_subjects, AnalysisContextConfig, AnalysisProjectConfig, RustAnalysisConfig,
    TestSubjectConfig,
};
use crate::config::CodeAtlasConfig;
use codeatlas_domain::source_graph::{ContextRole, ContextScope};
use codeatlas_domain::TestSubject;

#[test]
fn config_reads_arbitrary_named_reachability_contexts() {
    let config = serde_json::from_str::<CodeAtlasConfig>(
        r#"{
                "projects": [{
                    "id": "web",
                    "root": "packages/web",
                    "languages": ["js", "ts"],
                    "contexts": {
                        "application": {
                            "role": "production",
                            "scope": "public_surface",
                            "entrypoints": ["src/index.ts"]
                        },
                        "unit-tests": {
                            "role": "test",
                            "entrypoints": ["src/**/*.test.ts"],
                            "subjects": [
                                { "project": "web" },
                                { "source": "src/brushes/**" }
                            ]
                        }
                    },
                    "assume_reachable": ["src/runtime/plugins/**/*.ts"]
                }]
            }"#,
    )
    .expect("reachability config");

    let project = &config.projects[0];
    assert_eq!(project.id.as_deref(), Some("web"));
    assert_eq!(project.contexts["unit-tests"].role, ContextRole::Test);
    assert_eq!(
        project.contexts["application"].scope,
        ContextScope::PublicSurface
    );
    assert_eq!(project.contexts["unit-tests"].scope, ContextScope::Runtime);
    assert_eq!(
        project.contexts["unit-tests"].subjects,
        [
            TestSubjectConfig::Project("web".to_string()),
            TestSubjectConfig::Source("src/brushes/**".to_string())
        ]
    );
    assert_eq!(project.assume_reachable, ["src/runtime/plugins/**/*.ts"]);

    let round_trip =
        serde_json::to_value(&config.projects).expect("serialize project configuration");
    let decoded: Vec<AnalysisProjectConfig> =
        serde_json::from_value(round_trip).expect("deserialize project configuration");
    assert_eq!(
        decoded[0].contexts["application"].role,
        ContextRole::Production
    );
}

#[test]
fn test_subjects_are_bounded_to_test_contexts_and_valid_globs() {
    let production = AnalysisContextConfig {
        role: ContextRole::Production,
        subjects: vec![TestSubjectConfig::Project("web".to_string())],
        ..AnalysisContextConfig::default()
    };
    assert!(validate_test_subjects("web", "application", &production).is_err());

    let invalid_source = AnalysisContextConfig {
        role: ContextRole::Test,
        subjects: vec![TestSubjectConfig::Source("src/[".to_string())],
        ..AnalysisContextConfig::default()
    };
    assert!(validate_test_subjects("web", "unit-tests", &invalid_source).is_err());
}

#[test]
fn validated_analysis_inputs_resolve_once_without_changing_serialized_evidence() {
    let context = AnalysisContextConfig {
        role: ContextRole::Test,
        scope: ContextScope::Runtime,
        entrypoints: vec!["src/**/*.test.ts".to_string()],
        subjects: vec![
            TestSubjectConfig::Project("web".to_string()),
            TestSubjectConfig::Source("src/brushes/**".to_string()),
        ],
    };
    let expected_context = serde_json::to_value(&context).expect("raw context evidence");
    let resolved_context = context.resolve();
    assert_eq!(
        serde_json::to_value(&resolved_context).expect("resolved context evidence"),
        expected_context
    );
    assert_eq!(
        resolved_context.subjects,
        [
            TestSubject::Project("web".to_string()),
            TestSubject::Source("src/brushes/**".to_string())
        ]
    );

    let rust = RustAnalysisConfig {
        all_features: true,
        features: vec!["server".to_string()],
    };
    let expected_rust = serde_json::to_value(&rust).expect("raw Rust evidence");
    assert_eq!(
        serde_json::to_value(rust.resolve()).expect("resolved Rust evidence"),
        expected_rust
    );
}

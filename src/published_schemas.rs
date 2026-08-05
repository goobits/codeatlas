use crate::architecture::{
    ArchitectureConformance, ArchitectureDiagnosticReport, ArchitectureLockfile,
    ArchitectureObservation, CompileResult, ProviderQueryReport, SourceConformanceReport,
    ARCHITECTURE_API_VERSION, ARCHITECTURE_SCHEMA_VERSION, SOURCE_CONFORMANCE_SCHEMA_VERSION,
};
use crate::commands::diff::{
    PublicApiBaseline, BASELINE_FORMAT as PUBLIC_API_BASELINE_FORMAT,
    BASELINE_SCHEMA_VERSION as PUBLIC_API_BASELINE_SCHEMA_VERSION,
};
use crate::context_slice::{ContextSliceReport, CONTEXT_SLICE_SCHEMA_VERSION};
use crate::dead_code::{DeadCodeReport, DEAD_CODE_SCHEMA_VERSION};
use crate::domain::{ScanReport, SCAN_SCHEMA_VERSION};
use crate::execution::artifact::is_namespaced_artifact_version;
use crate::execution::{
    ExecutionPlan, ExecutionReceipt, EXECUTION_PLAN_SCHEMA_VERSION,
    EXECUTION_RECEIPT_SCHEMA_VERSION,
};
use crate::fuzz::reproducer::Reproducer;
use crate::fuzz::FUZZ_REPRODUCER_SCHEMA_VERSION;
use crate::http::{
    HttpBaselineReport, HttpCheckReport, HttpDiffReport, HttpFuzzReport, HttpFuzzWorkload,
    HttpInventoryReport, HTTP_API_VERSION, HTTP_BASELINE_API_VERSION, HTTP_BASELINE_SCHEMA_VERSION,
    HTTP_FUZZ_API_VERSION, HTTP_FUZZ_SCHEMA_VERSION, HTTP_FUZZ_WORKLOAD_SCHEMA_VERSION,
    HTTP_SCHEMA_VERSION,
};
use crate::lexicon::{LexiconReport, LEXICON_SCHEMA_VERSION};
use crate::postgres::{
    PostgresBaselineReport, PostgresCheckReport, PostgresDiffReport, PostgresInventoryReport,
    PostgresTestReport, POSTGRES_API_VERSION, POSTGRES_BASELINE_API_VERSION,
    POSTGRES_BASELINE_SCHEMA_VERSION, POSTGRES_DIFF_API_VERSION, POSTGRES_DIFF_SCHEMA_VERSION,
    POSTGRES_SCHEMA_VERSION, POSTGRES_TEST_API_VERSION, POSTGRES_TEST_SCHEMA_VERSION,
};
use crate::testing::{
    TestingImpactReport, TestingInventoryReport, TestingWitnessReport,
    TESTING_IMPACT_SCHEMA_VERSION, TESTING_INVENTORY_SCHEMA_VERSION,
    TESTING_WITNESS_SCHEMA_VERSION,
};
use schemars::{JsonSchema, SchemaGenerator};
use serde_json::Value;

struct PublishedSchema {
    contract_id: &'static str,
    filename: &'static str,
    payload_version: PayloadVersion,
    owner: &'static str,
    generate: fn() -> Value,
}

#[derive(Clone, Copy)]
enum ContractVersion {
    Api(&'static str),
    Schema(u32),
    Namespaced(&'static str),
}

#[derive(Clone, Copy)]
enum SchemaFieldValue {
    Integer(u32),
    Namespaced(&'static str),
}

#[derive(Clone, Copy)]
struct PayloadVersion {
    contract: ContractVersion,
    schema: Option<SchemaFieldValue>,
    api: Option<&'static str>,
    format: Option<&'static str>,
    kind: Option<&'static str>,
    nested: bool,
}

impl PayloadVersion {
    const fn from_schema(schema: u32) -> Self {
        Self {
            contract: ContractVersion::Schema(schema),
            schema: Some(SchemaFieldValue::Integer(schema)),
            api: None,
            format: None,
            kind: None,
            nested: false,
        }
    }

    const fn from_schema_and_api(schema: u32, api: &'static str) -> Self {
        Self {
            contract: ContractVersion::Schema(schema),
            schema: Some(SchemaFieldValue::Integer(schema)),
            api: Some(api),
            format: None,
            kind: None,
            nested: false,
        }
    }

    const fn from_api_with_schema(api: &'static str, schema: u32) -> Self {
        Self {
            contract: ContractVersion::Api(api),
            schema: Some(SchemaFieldValue::Integer(schema)),
            api: Some(api),
            format: None,
            kind: None,
            nested: false,
        }
    }

    const fn from_schema_with_format(schema: u32, format: &'static str) -> Self {
        Self {
            contract: ContractVersion::Schema(schema),
            schema: Some(SchemaFieldValue::Integer(schema)),
            api: None,
            format: Some(format),
            kind: None,
            nested: false,
        }
    }

    const fn from_api_with_kind(api: &'static str, kind: &'static str) -> Self {
        Self {
            contract: ContractVersion::Api(api),
            schema: None,
            api: Some(api),
            format: None,
            kind: Some(kind),
            nested: false,
        }
    }

    const fn from_namespaced(schema_version: &'static str) -> Self {
        Self {
            contract: ContractVersion::Namespaced(schema_version),
            schema: Some(SchemaFieldValue::Namespaced(schema_version)),
            api: None,
            format: None,
            kind: None,
            nested: false,
        }
    }

    const fn with_nested_identity(mut self) -> Self {
        self.nested = true;
        self
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AnnotationValueShape {
    OpaqueString,
}

#[derive(Clone, Copy)]
struct AnnotationKeyRegistration {
    key: &'static str,
    value_shape: AnnotationValueShape,
    description: &'static str,
}

const CODEATLAS_ANNOTATION_KEYS: &[AnnotationKeyRegistration] = &[
    AnnotationKeyRegistration {
        key: "codeatlas.node_id",
        value_shape: AnnotationValueShape::OpaqueString,
        description: "opaque exact CodeAtlas graph node ID",
    },
    AnnotationKeyRegistration {
        key: "codeatlas.symbol",
        value_shape: AnnotationValueShape::OpaqueString,
        description: "declared identifier as written in source",
    },
];

fn constrain_property(
    schema: &mut Value,
    names: &[&str],
    constant: &Value,
    include_definitions: bool,
) -> usize {
    fn constrain_object(schema: &mut Value, names: &[&str], constant: &Value) -> usize {
        let Some(properties) = schema
            .as_object_mut()
            .and_then(|object| object.get_mut("properties"))
            .and_then(Value::as_object_mut)
        else {
            return 0;
        };
        let mut constrained = 0;
        for name in names {
            if let Some(property) = properties.get_mut(*name).and_then(Value::as_object_mut) {
                property.insert("const".to_string(), constant.clone());
                constrained += 1;
            }
        }
        constrained
    }

    let mut constrained = constrain_object(schema, names, constant);
    if include_definitions {
        constrained += schema
            .as_object_mut()
            .and_then(|object| object.get_mut("$defs"))
            .and_then(Value::as_object_mut)
            .map(|definitions| {
                definitions
                    .values_mut()
                    .map(|definition| constrain_object(definition, names, constant))
                    .sum::<usize>()
            })
            .unwrap_or_default();
    }
    constrained
}

fn generate_registered_schema(schema: &PublishedSchema) -> Value {
    let mut generated = (schema.generate)();
    generated
        .as_object_mut()
        .expect("generated schema root must be an object")
        .insert(
            "$id".to_string(),
            Value::String(schema.contract_id.to_string()),
        );

    let identity = schema.payload_version;
    for (names, constant, label) in [
        (
            &["schemaVersion", "schema_version"][..],
            identity.schema.map(|value| match value {
                SchemaFieldValue::Integer(value) => Value::from(value),
                SchemaFieldValue::Namespaced(value) => Value::String(value.to_string()),
            }),
            "schema version",
        ),
        (
            &["apiVersion", "api_version"][..],
            identity.api.map(|value| Value::String(value.to_string())),
            "API version",
        ),
        (
            &["format"][..],
            identity
                .format
                .map(|value| Value::String(value.to_string())),
            "format",
        ),
        (
            &["kind"][..],
            identity.kind.map(|value| Value::String(value.to_string())),
            "kind",
        ),
    ] {
        let Some(constant) = constant else {
            continue;
        };
        assert!(
            constrain_property(&mut generated, names, &constant, identity.nested) > 0,
            "registered {label} field is absent from {}",
            schema.contract_id
        );
    }
    generated
}

fn generate_schema<T: JsonSchema>() -> Value {
    serde_json::to_value(SchemaGenerator::default().into_root_schema_for::<T>())
        .expect("generated schema must serialize")
}

impl PublishedSchema {
    const fn new(
        contract_id: &'static str,
        filename: &'static str,
        payload_version: PayloadVersion,
        owner: &'static str,
        generate: fn() -> Value,
    ) -> Self {
        Self {
            contract_id,
            filename,
            payload_version,
            owner,
            generate,
        }
    }

    const fn new_artifact(
        schema_version: &'static str,
        filename: &'static str,
        owner: &'static str,
        generate: fn() -> Value,
    ) -> Self {
        Self::new(
            schema_version,
            filename,
            PayloadVersion::from_namespaced(schema_version),
            owner,
            generate,
        )
    }
}

fn is_valid_annotation_key(value: &str) -> bool {
    let Some(name) = value.strip_prefix("codeatlas.") else {
        return false;
    };
    !name.is_empty()
        && !name.starts_with('_')
        && !name.ends_with('_')
        && !name.contains("__")
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

const PUBLISHED_SCHEMAS: &[PublishedSchema] = &[
    PublishedSchema::new_artifact(
        EXECUTION_PLAN_SCHEMA_VERSION,
        "codeatlas-execution-plan-v1.schema.json",
        "execution",
        generate_schema::<ExecutionPlan>,
    ),
    PublishedSchema::new_artifact(
        EXECUTION_RECEIPT_SCHEMA_VERSION,
        "codeatlas-execution-receipt-v1.schema.json",
        "execution",
        generate_schema::<ExecutionReceipt>,
    ),
    PublishedSchema::new_artifact(
        FUZZ_REPRODUCER_SCHEMA_VERSION,
        "codeatlas-reproducer-v1.schema.json",
        "fuzz",
        generate_schema::<Reproducer>,
    ),
    PublishedSchema::new_artifact(
        HTTP_FUZZ_WORKLOAD_SCHEMA_VERSION,
        "codeatlas-http-fuzz-workload-v1.schema.json",
        "http",
        generate_schema::<HttpFuzzWorkload>,
    ),
    PublishedSchema::new(
        "codeatlas.scan/v3",
        "codeatlas-scan-v3.schema.json",
        PayloadVersion::from_schema(SCAN_SCHEMA_VERSION),
        "domain",
        generate_schema::<ScanReport>,
    ),
    PublishedSchema::new(
        "codeatlas.dead-code/v5",
        "codeatlas-dead-code-v5.schema.json",
        PayloadVersion::from_schema(DEAD_CODE_SCHEMA_VERSION),
        "dead_code",
        generate_schema::<DeadCodeReport>,
    ),
    PublishedSchema::new(
        "codeatlas.public-api-baseline/v1",
        "codeatlas-public-api-baseline-v1.schema.json",
        PayloadVersion::from_schema_with_format(
            PUBLIC_API_BASELINE_SCHEMA_VERSION,
            PUBLIC_API_BASELINE_FORMAT,
        ),
        "commands::diff",
        generate_schema::<PublicApiBaseline>,
    ),
    PublishedSchema::new(
        "codeatlas.context-slice/v4",
        "codeatlas-context-slice-v4.schema.json",
        PayloadVersion::from_schema(CONTEXT_SLICE_SCHEMA_VERSION),
        "context_slice",
        generate_schema::<ContextSliceReport>,
    ),
    PublishedSchema::new(
        "codeatlas.lexicon/v5",
        "codeatlas-lexicon-v5.schema.json",
        PayloadVersion::from_schema(LEXICON_SCHEMA_VERSION),
        "lexicon",
        generate_schema::<LexiconReport>,
    ),
    PublishedSchema::new(
        "codeatlas.testing-inventory/v1",
        "codeatlas-testing-inventory-v1.schema.json",
        PayloadVersion::from_schema(TESTING_INVENTORY_SCHEMA_VERSION),
        "testing",
        generate_schema::<TestingInventoryReport>,
    ),
    PublishedSchema::new(
        "codeatlas.testing-impact/v1",
        "codeatlas-testing-impact-v1.schema.json",
        PayloadVersion::from_schema(TESTING_IMPACT_SCHEMA_VERSION),
        "testing",
        generate_schema::<TestingImpactReport>,
    ),
    PublishedSchema::new(
        "codeatlas.testing-witness/v2",
        "codeatlas-testing-witness-v2.schema.json",
        PayloadVersion::from_schema(TESTING_WITNESS_SCHEMA_VERSION),
        "testing",
        generate_schema::<TestingWitnessReport>,
    ),
    PublishedSchema::new(
        "codeatlas.http-inventory/v2",
        "codeatlas-http-inventory-v2.schema.json",
        PayloadVersion::from_schema_and_api(HTTP_SCHEMA_VERSION, HTTP_API_VERSION),
        "http",
        generate_schema::<HttpInventoryReport>,
    ),
    PublishedSchema::new(
        "codeatlas.http-check/v2",
        "codeatlas-http-check-v2.schema.json",
        PayloadVersion::from_schema_and_api(HTTP_SCHEMA_VERSION, HTTP_API_VERSION),
        "http",
        generate_schema::<HttpCheckReport>,
    ),
    PublishedSchema::new(
        "codeatlas.http-baseline/v1",
        "codeatlas-http-baseline-v1.schema.json",
        PayloadVersion::from_schema_and_api(
            HTTP_BASELINE_SCHEMA_VERSION,
            HTTP_BASELINE_API_VERSION,
        ),
        "http",
        generate_schema::<HttpBaselineReport>,
    ),
    PublishedSchema::new(
        "codeatlas.http-diff/v2",
        "codeatlas-http-diff-v2.schema.json",
        PayloadVersion::from_schema_and_api(HTTP_SCHEMA_VERSION, HTTP_API_VERSION),
        "http",
        generate_schema::<HttpDiffReport>,
    ),
    PublishedSchema::new(
        "codeatlas.http-fuzz/v2",
        "codeatlas-http-fuzz-v2.schema.json",
        PayloadVersion::from_schema_and_api(HTTP_FUZZ_SCHEMA_VERSION, HTTP_FUZZ_API_VERSION),
        "http",
        generate_schema::<HttpFuzzReport>,
    ),
    PublishedSchema::new(
        "codeatlas.postgres-inventory/v2",
        "codeatlas-postgres-inventory-v2.schema.json",
        PayloadVersion::from_schema_and_api(POSTGRES_SCHEMA_VERSION, POSTGRES_API_VERSION),
        "postgres",
        generate_schema::<PostgresInventoryReport>,
    ),
    PublishedSchema::new(
        "codeatlas.postgres-check/v2",
        "codeatlas-postgres-check-v2.schema.json",
        PayloadVersion::from_schema_and_api(POSTGRES_SCHEMA_VERSION, POSTGRES_API_VERSION),
        "postgres",
        generate_schema::<PostgresCheckReport>,
    ),
    PublishedSchema::new(
        "codeatlas.postgres-test/v2",
        "codeatlas-postgres-test-v2.schema.json",
        PayloadVersion::from_schema_and_api(
            POSTGRES_TEST_SCHEMA_VERSION,
            POSTGRES_TEST_API_VERSION,
        ),
        "postgres",
        generate_schema::<PostgresTestReport>,
    ),
    PublishedSchema::new(
        "codeatlas.postgres-baseline/v2",
        "codeatlas-postgres-baseline-v2.schema.json",
        PayloadVersion::from_schema_and_api(
            POSTGRES_BASELINE_SCHEMA_VERSION,
            POSTGRES_BASELINE_API_VERSION,
        ),
        "postgres",
        generate_schema::<PostgresBaselineReport>,
    ),
    PublishedSchema::new(
        "codeatlas.postgres-diff/v2",
        "codeatlas-postgres-diff-v2.schema.json",
        PayloadVersion::from_schema_and_api(
            POSTGRES_DIFF_SCHEMA_VERSION,
            POSTGRES_DIFF_API_VERSION,
        ),
        "postgres",
        generate_schema::<PostgresDiffReport>,
    ),
    PublishedSchema::new(
        "atlas.codeatlas.dev/architecture-compilation/v0.1",
        "codeatlas-architecture-compilation-v0-1.schema.json",
        PayloadVersion::from_api_with_schema(ARCHITECTURE_API_VERSION, ARCHITECTURE_SCHEMA_VERSION)
            .with_nested_identity(),
        "architecture",
        generate_schema::<CompileResult>,
    ),
    PublishedSchema::new(
        "atlas.codeatlas.dev/architecture-lock/v0.1",
        "codeatlas-architecture-lock-v0-1.schema.json",
        PayloadVersion::from_api_with_schema(ARCHITECTURE_API_VERSION, ARCHITECTURE_SCHEMA_VERSION),
        "architecture",
        generate_schema::<ArchitectureLockfile>,
    ),
    PublishedSchema::new(
        "atlas.codeatlas.dev/architecture-observation/v0.1",
        "codeatlas-architecture-observation-v0-1.schema.json",
        PayloadVersion::from_api_with_kind(ARCHITECTURE_API_VERSION, "ArchitectureObservation"),
        "architecture",
        generate_schema::<ArchitectureObservation>,
    ),
    PublishedSchema::new(
        "atlas.codeatlas.dev/architecture-conformance/v0.1",
        "codeatlas-architecture-conformance-v0-1.schema.json",
        PayloadVersion::from_api_with_kind(ARCHITECTURE_API_VERSION, "ArchitectureConformance"),
        "architecture",
        generate_schema::<ArchitectureConformance>,
    ),
    PublishedSchema::new(
        "codeatlas.architecture-source-conformance/v1",
        "codeatlas-architecture-source-conformance-v1.schema.json",
        PayloadVersion::from_schema(SOURCE_CONFORMANCE_SCHEMA_VERSION),
        "architecture",
        generate_schema::<SourceConformanceReport>,
    ),
    PublishedSchema::new(
        "atlas.codeatlas.dev/architecture-provider-query/v0.1",
        "codeatlas-architecture-provider-query-v0-1.schema.json",
        PayloadVersion::from_api_with_schema(ARCHITECTURE_API_VERSION, ARCHITECTURE_SCHEMA_VERSION),
        "architecture",
        generate_schema::<ProviderQueryReport>,
    ),
    PublishedSchema::new(
        "atlas.codeatlas.dev/architecture-diagnostics/v0.1",
        "codeatlas-architecture-diagnostics-v0-1.schema.json",
        PayloadVersion::from_api_with_schema(ARCHITECTURE_API_VERSION, ARCHITECTURE_SCHEMA_VERSION),
        "architecture",
        generate_schema::<ArchitectureDiagnosticReport<'static>>,
    ),
];

#[cfg(test)]
mod tests {
    use super::{
        generate_registered_schema, generate_schema, is_namespaced_artifact_version,
        is_valid_annotation_key, AnnotationValueShape, ContractVersion, PublishedSchema,
        SchemaFieldValue, CODEATLAS_ANNOTATION_KEYS, PUBLISHED_SCHEMAS,
    };
    use schemars::JsonSchema;
    use serde::Serialize;
    use serde_json::Value;
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::{Path, PathBuf};

    fn repository_path(relative: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
    }

    fn render_schema(schema: &PublishedSchema) -> String {
        let value = generate_registered_schema(schema);
        let mut rendered = serde_json::to_string_pretty(&value).expect("render generated schema");
        rendered.push('\n');
        rendered
    }

    fn schema_for_contract(contract_id: &str) -> Value {
        let schema = PUBLISHED_SCHEMAS
            .iter()
            .find(|schema| schema.contract_id == contract_id)
            .unwrap_or_else(|| panic!("unregistered fixture contract {contract_id}"));
        generate_registered_schema(schema)
    }

    fn validate_fixture(contract_id: &str, relative: &str) {
        let source = fs::read_to_string(repository_path(relative))
            .unwrap_or_else(|error| panic!("read fixture {relative}: {error}"));
        let document = if relative.ends_with(".json") {
            serde_json::from_str::<Value>(&source)
                .unwrap_or_else(|error| panic!("parse JSON fixture {relative}: {error}"))
        } else {
            let yaml = serde_yaml::from_str::<serde_yaml::Value>(&source)
                .unwrap_or_else(|error| panic!("parse YAML fixture {relative}: {error}"));
            serde_json::to_value(yaml)
                .unwrap_or_else(|error| panic!("normalize YAML fixture {relative}: {error}"))
        };
        let schema = schema_for_contract(contract_id);
        let validator = jsonschema::validator_for(&schema)
            .unwrap_or_else(|error| panic!("compile schema {contract_id}: {error}"));
        let errors = validator
            .iter_errors(&document)
            .map(|error| error.to_string())
            .collect::<Vec<_>>();
        assert!(
            errors.is_empty(),
            "fixture {relative} violates {contract_id}: {errors:#?}"
        );
    }

    fn assert_registered_identity(schema: &PublishedSchema) {
        let contract_version = match schema.payload_version.contract {
            ContractVersion::Schema(version) => format!("v{version}"),
            ContractVersion::Api(api) => api
                .rsplit_once('/')
                .map(|(_, version)| version.to_string())
                .unwrap_or_else(|| panic!("API version has no path boundary: {api}")),
            ContractVersion::Namespaced(version) => {
                assert!(
                    is_namespaced_artifact_version(version),
                    "invalid namespaced artifact version {version}"
                );
                assert_eq!(schema.contract_id, version);
                let expected_filename = format!("{}.schema.json", version.replace(['.', '/'], "-"));
                assert_eq!(schema.filename, expected_filename);
                version
                    .rsplit_once('/')
                    .map(|(_, version)| version.to_string())
                    .expect("namespaced artifact version has a path boundary")
            }
        };
        assert!(
            schema
                .contract_id
                .ends_with(&format!("/{contract_version}")),
            "contract ID {} disagrees with its owning payload version",
            schema.contract_id
        );
        assert!(
            schema.filename.ends_with(&format!(
                "-{}.schema.json",
                contract_version.replace('.', "-")
            )),
            "schema filename {} disagrees with its owning payload version",
            schema.filename
        );
        if let Some(SchemaFieldValue::Integer(version)) = schema.payload_version.schema {
            assert!(version > 0, "schema version must be positive");
        }
        if let Some(api) = schema.payload_version.api {
            assert!(!api.trim().is_empty(), "API version must not be empty");
        }
        if let Some(format) = schema.payload_version.format {
            assert!(
                schema.contract_id.starts_with(&format!("{format}/")),
                "contract ID {} disagrees with payload format {format}",
                schema.contract_id
            );
        }
    }

    fn assert_namespaced_artifact_shape(schema: &PublishedSchema, generated: &Value) {
        let ContractVersion::Namespaced(version) = schema.payload_version.contract else {
            return;
        };
        let properties = generated
            .get("properties")
            .and_then(Value::as_object)
            .expect("artifact schema must have root properties");
        let schema_fields = ["schemaVersion", "schema_version"]
            .into_iter()
            .filter(|name| properties.contains_key(*name))
            .collect::<Vec<_>>();
        assert_eq!(
            schema_fields.len(),
            1,
            "{} must have exactly one schema-version field",
            schema.contract_id
        );
        let schema_field = schema_fields[0];
        assert_eq!(properties[schema_field]["type"], "string");
        assert_eq!(properties[schema_field]["const"], version);
        assert!(
            generated
                .get("required")
                .and_then(Value::as_array)
                .is_some_and(|required| required.iter().any(|name| name == schema_field)),
            "{} schema-version field must be required",
            schema.contract_id
        );
        assert!(
            ["apiVersion", "api_version"]
                .into_iter()
                .all(|name| !properties.contains_key(name)),
            "{} must not carry a parallel API version",
            schema.contract_id
        );
    }

    #[derive(JsonSchema, Serialize)]
    #[serde(rename_all = "camelCase")]
    struct ProspectiveArtifact {
        schema_version: String,
        id: String,
    }

    #[derive(JsonSchema, Serialize)]
    #[serde(rename_all = "camelCase")]
    struct InvalidDualVersionArtifact {
        schema_version: String,
        api_version: String,
    }

    #[test]
    fn prospective_artifacts_require_one_namespaced_schema_identity() {
        let schema = PublishedSchema::new_artifact(
            "codeatlas.execution-plan/v1",
            "codeatlas-execution-plan-v1.schema.json",
            "execution",
            generate_schema::<ProspectiveArtifact>,
        );
        assert_registered_identity(&schema);
        assert_namespaced_artifact_shape(&schema, &generate_registered_schema(&schema));

        for invalid in [
            "1",
            "execution-plan/v1",
            "codeatlas./v1",
            "codeatlas.Execution-plan/v1",
            "codeatlas.execution_plan/v1",
            "codeatlas.execution-plan/v0",
            "codeatlas.execution-plan/v01",
        ] {
            assert!(
                !is_namespaced_artifact_version(invalid),
                "accepted invalid artifact schema version {invalid}"
            );
        }

        let invalid = PublishedSchema::new_artifact(
            "codeatlas.invalid-dual-version/v1",
            "codeatlas-invalid-dual-version-v1.schema.json",
            "test",
            generate_schema::<InvalidDualVersionArtifact>,
        );
        assert!(
            std::panic::catch_unwind(|| assert_namespaced_artifact_shape(
                &invalid,
                &generate_registered_schema(&invalid)
            ))
            .is_err(),
            "accepted a namespaced artifact with a parallel API version"
        );
    }

    #[test]
    fn annotation_keys_are_registered_without_vendoring_the_external_contract() {
        let mut keys = BTreeSet::new();
        let lexicon = fs::read_to_string(repository_path("docs/concepts/lexicon.md"))
            .expect("read canonical lexicon");
        for registration in CODEATLAS_ANNOTATION_KEYS {
            assert!(is_valid_annotation_key(registration.key));
            assert!(keys.insert(registration.key), "duplicate annotation key");
            assert_eq!(registration.value_shape, AnnotationValueShape::OpaqueString);
            assert!(!registration.description.trim().is_empty());
            assert!(
                lexicon.contains(&format!("`{}`", registration.key)),
                "annotation key {} is not documented in the lexicon",
                registration.key
            );
        }
        for invalid in [
            "node_id",
            "codeatlas.",
            "codeatlas.NodeId",
            "codeatlas.node-id",
            "codeatlas._node_id",
            "codeatlas.node__id",
        ] {
            assert!(
                !is_valid_annotation_key(invalid),
                "accepted invalid annotation key {invalid}"
            );
        }
        assert_eq!(
            keys,
            BTreeSet::from(["codeatlas.node_id", "codeatlas.symbol"])
        );

        for schema in PUBLISHED_SCHEMAS {
            assert!(
                !schema.contract_id.contains("source-target"),
                "external source-target schema must not be published by CodeAtlas"
            );
        }
        for entry in fs::read_dir(repository_path("schemas")).expect("read schema directory") {
            let entry = entry.expect("read schema entry");
            if entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "json")
            {
                let schema = fs::read_to_string(entry.path()).expect("read published schema");
                assert!(
                    !schema.contains("source-target"),
                    "external source-target schema must not be vendored by CodeAtlas"
                );
            }
        }
    }

    #[test]
    fn published_schema_registry_is_complete_and_current() {
        let mut contract_ids = BTreeSet::new();
        let mut filenames = BTreeSet::new();
        for schema in PUBLISHED_SCHEMAS {
            assert!(
                contract_ids.insert(schema.contract_id),
                "duplicate schema contract ID {}",
                schema.contract_id
            );
            assert!(
                filenames.insert(schema.filename.to_string()),
                "duplicate schema filename {}",
                schema.filename
            );
            assert!(!schema.owner.is_empty());
            assert_registered_identity(schema);

            let generated = generate_registered_schema(schema);
            assert_namespaced_artifact_shape(schema, &generated);
            jsonschema::meta::validate(&generated).unwrap_or_else(|error| {
                panic!("invalid generated schema {}: {error}", schema.contract_id)
            });
            let path = repository_path(&format!("schemas/{}", schema.filename));
            let committed = fs::read_to_string(&path).unwrap_or_else(|error| {
                panic!("read committed schema {}: {error}", path.display())
            });
            assert_eq!(
                committed,
                render_schema(schema),
                "checked-in schema drifted: {}",
                schema.filename
            );
        }

        let directory = repository_path("schemas");
        let committed = fs::read_dir(&directory)
            .unwrap_or_else(|error| {
                panic!("read schema directory {}: {error}", directory.display())
            })
            .map(|entry| entry.expect("read schema entry"))
            .filter_map(|entry| {
                if !entry.file_type().expect("read schema entry type").is_file() {
                    return None;
                }
                let name = entry
                    .file_name()
                    .into_string()
                    .expect("UTF-8 schema filename");
                name.ends_with(".schema.json").then_some(name)
            })
            .collect::<BTreeSet<_>>();
        assert_eq!(committed, filenames, "schema directory and registry differ");

        validate_fixture(
            "atlas.codeatlas.dev/architecture-observation/v0.1",
            "spec/architecture/v0.1/examples/observation/architecture-observation.generated.yaml",
        );
        validate_fixture(
            "atlas.codeatlas.dev/architecture-conformance/v0.1",
            "spec/architecture/v0.1/examples/conformance/architecture-conformance.generated.json",
        );
    }

    #[test]
    #[ignore = "explicit schema update task"]
    fn update_published_schemas() {
        let directory = repository_path("schemas");
        fs::create_dir_all(&directory).expect("create schema directory");
        for schema in PUBLISHED_SCHEMAS {
            let path = directory.join(schema.filename);
            fs::write(&path, render_schema(schema))
                .unwrap_or_else(|error| panic!("write schema {}: {error}", path.display()));
        }
        published_schema_registry_is_complete_and_current();
    }
}

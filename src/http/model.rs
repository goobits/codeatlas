use serde::{Deserialize, Serialize};

pub(crate) const HTTP_API_VERSION: &str = "codeatlas.http/v2";
pub(crate) const HTTP_SCHEMA_VERSION: u32 = 2;
pub(crate) const HTTP_BASELINE_API_VERSION: &str = "codeatlas.http-baseline/v1";
pub(crate) const HTTP_BASELINE_SCHEMA_VERSION: u32 = 1;
pub(crate) const HTTP_FUZZ_API_VERSION: &str = "codeatlas.http-fuzz/v1";
pub(crate) const HTTP_FUZZ_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HttpInventoryReport {
    pub schema_version: u32,
    pub api_version: String,
    pub tool_version: String,
    pub contracts: Vec<HttpContractInventory>,
}

impl HttpInventoryReport {
    pub(crate) fn new(contracts: Vec<HttpContractInventory>) -> Self {
        Self {
            schema_version: HTTP_SCHEMA_VERSION,
            api_version: HTTP_API_VERSION.to_string(),
            tool_version: env!("CARGO_PKG_VERSION").to_string(),
            contracts,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HttpBaselineReport {
    pub schema_version: u32,
    pub api_version: String,
    pub contracts: Vec<HttpBaselineContract>,
}

impl HttpBaselineReport {
    pub(crate) fn from_inventory(inventory: &HttpInventoryReport) -> anyhow::Result<Self> {
        let contracts = inventory
            .contracts
            .iter()
            .map(|contract| {
                let openapi_version = contract.openapi_version.clone().ok_or_else(|| {
                    anyhow::anyhow!(
                        "HTTP contract {} has no OpenAPI schema; baselines require schema-backed contracts",
                        contract.id
                    )
                })?;
                Ok(HttpBaselineContract {
                    id: contract.id.clone(),
                    openapi_version,
                    operations: contract.operations.clone(),
                })
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        Ok(Self {
            schema_version: HTTP_BASELINE_SCHEMA_VERSION,
            api_version: HTTP_BASELINE_API_VERSION.to_string(),
            contracts,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HttpBaselineContract {
    pub id: String,
    pub openapi_version: String,
    pub operations: Vec<HttpOperation>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HttpContractInventory {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contract_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub openapi_version: Option<String>,
    pub schema_missing: bool,
    pub operations: Vec<HttpOperation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<HttpContractDiagnostic>,
    pub source: HttpSourceInventory,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HttpContractDiagnostic {
    pub severity: HttpFindingSeverity,
    pub code: String,
    pub operation: String,
    pub location: String,
    pub message: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HttpOperation {
    pub key: String,
    pub method: String,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub security: Vec<HttpSecurityRequirement>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parameters: Vec<HttpParameter>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_body: Option<HttpRequestBody>,
    pub responses: Vec<HttpResponse>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HttpSecurityRequirement {
    pub schemes: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HttpParameter {
    pub name: String,
    pub location: String,
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_digest: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HttpRequestBody {
    pub required: bool,
    pub content: Vec<HttpMediaType>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HttpResponse {
    pub status: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content: Vec<HttpMediaType>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HttpMediaType {
    pub media_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_digest: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HttpSourceInventory {
    pub completeness: HttpSourceCompleteness,
    pub reason: String,
    pub operations: Vec<HttpSourceOperation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skipped_files: Vec<HttpSkippedFile>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum HttpSourceCompleteness {
    Partial,
    Complete,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HttpSourceOperation {
    pub key: String,
    pub method: String,
    pub path: String,
    pub kind: HttpSourceOperationKind,
    pub schema_missing: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_pattern: Option<String>,
    pub detector: String,
    pub confidence: HttpConfidence,
    pub evidence: HttpSourceEvidence,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum HttpSourceOperationKind {
    Endpoint,
    Page,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum HttpConfidence {
    High,
    Medium,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HttpSourceEvidence {
    pub path: String,
    pub line: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HttpSkippedFile {
    pub path: String,
    pub reason: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HttpCheckReport {
    pub schema_version: u32,
    pub api_version: String,
    pub tool_version: String,
    pub inventory: HttpInventoryReport,
    pub findings: Vec<HttpFinding>,
}

impl HttpCheckReport {
    pub(crate) fn new(inventory: HttpInventoryReport, findings: Vec<HttpFinding>) -> Self {
        Self {
            schema_version: HTTP_SCHEMA_VERSION,
            api_version: HTTP_API_VERSION.to_string(),
            tool_version: env!("CARGO_PKG_VERSION").to_string(),
            inventory,
            findings,
        }
    }

    pub(crate) fn gate_count(&self) -> usize {
        self.findings
            .iter()
            .filter(|finding| finding.severity == HttpFindingSeverity::Error)
            .count()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HttpFinding {
    pub severity: HttpFindingSeverity,
    pub code: String,
    pub contract_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation: Option<String>,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<HttpSourceEvidence>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum HttpFindingSeverity {
    Info,
    Warning,
    Error,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HttpDiffReport {
    pub schema_version: u32,
    pub api_version: String,
    pub tool_version: String,
    pub contracts: Vec<HttpContractDiff>,
    pub breaking_changes: usize,
    pub additive_changes: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HttpContractDiff {
    pub id: String,
    pub changes: Vec<HttpOperationChange>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HttpOperationChange {
    pub kind: HttpChangeKind,
    pub operation: String,
    pub message: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum HttpChangeKind {
    Additive,
    Breaking,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HttpFuzzReport {
    pub schema_version: u32,
    pub api_version: String,
    pub tool_version: String,
    pub target_id: String,
    pub contract_id: String,
    #[serde(default)]
    pub contract_mode: HttpFuzzContractMode,
    pub profile: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stateful: Option<HttpFuzzStatefulSummary>,
    pub totals: HttpFuzzTotals,
    pub operations: Vec<HttpFuzzOperationSummary>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum HttpFuzzContractMode {
    #[default]
    #[serde(rename = "openapi")]
    OpenApi,
    SourceTransport,
}

impl HttpFuzzContractMode {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::OpenApi => "openapi",
            Self::SourceTransport => "source_transport",
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HttpFuzzStatefulSummary {
    pub scenarios: u64,
    pub successful_scenarios: u64,
    pub failed_scenarios: u64,
    pub skipped_scenarios: u64,
    pub links_total: u64,
    pub links_selected: u64,
    pub links_inferred: u64,
    pub links_covered: u64,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct HttpFuzzTotals {
    pub operations: u64,
    pub success_observed_operations: u64,
    pub operations_without_success: u64,
    pub client_error_only_operations: u64,
    pub mixed_without_success_operations: u64,
    pub authentication_rejection_only_operations: u64,
    pub no_positive_case_operations: u64,
    pub cases: u64,
    pub positive_cases: u64,
    pub positive_successes: u64,
    pub positive_auth_rejections: u64,
    pub positive_client_errors: u64,
    pub negative_cases: u64,
    pub negative_rejections: u64,
    pub server_errors: u64,
    pub check_failures: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HttpFuzzOperationSummary {
    pub operation: String,
    pub positive_coverage: HttpFuzzPositiveCoverage,
    pub cases: u64,
    pub positive_cases: u64,
    pub positive_successes: u64,
    pub positive_auth_rejections: u64,
    pub positive_client_errors: u64,
    pub negative_cases: u64,
    pub negative_rejections: u64,
    pub server_errors: u64,
    pub check_failures: u64,
    pub observed_statuses: std::collections::BTreeMap<String, u64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum HttpFuzzPositiveCoverage {
    SuccessObserved,
    AuthenticationRejectionOnly,
    ClientErrorOnly,
    NoPositiveCases,
    MixedWithoutSuccess,
}

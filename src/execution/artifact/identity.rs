use super::{has_file_metadata_changed, ManagedArtifact};
use crate::execution::model::{
    ArtifactLink, ArtifactPayload, CallCount, ExecutionLimits, ExecutionOutcome, ExecutionPlan,
    ExecutionPlanBody, ExecutionReceipt, ExecutionReceiptBody, PlanArtifactKind,
    ReceiptArtifactKind, ToolIdentity, EXECUTION_PLAN_SCHEMA_VERSION,
    EXECUTION_RECEIPT_SCHEMA_VERSION,
};
use anyhow::{Context, Result};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::Read;
use std::path::Path;

const PLAN_DOMAIN: &str = "atlas.codeatlas.dev/execution-plan/v1";
const RECEIPT_DOMAIN: &str = "atlas.codeatlas.dev/execution-receipt/v1";
const MAX_JCS_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

#[derive(Serialize)]
struct PlanIdentity<'a> {
    schema_version: &'static str,
    kind: PlanArtifactKind,
    #[serde(flatten)]
    body: &'a ExecutionPlanBody,
}

#[derive(Serialize)]
struct ReceiptIdentity<'a> {
    schema_version: &'static str,
    kind: ReceiptArtifactKind,
    #[serde(flatten)]
    body: &'a ExecutionReceiptBody,
}

impl ExecutionPlan {
    pub(crate) fn new(body: ExecutionPlanBody) -> Result<Self> {
        validate_execution_plan_body(&body)?;
        let digest = digest_value(
            PLAN_DOMAIN,
            &PlanIdentity {
                schema_version: EXECUTION_PLAN_SCHEMA_VERSION,
                kind: PlanArtifactKind::Plan,
                body: &body,
            },
        )?;
        Ok(Self {
            schema_version: EXECUTION_PLAN_SCHEMA_VERSION.to_string(),
            kind: PlanArtifactKind::Plan,
            id: artifact_id("plan", &digest)?,
            content_digest: digest,
            body,
        })
    }
}

impl ExecutionReceipt {
    pub(crate) fn new(body: ExecutionReceiptBody) -> Result<Self> {
        validate_execution_receipt_body(&body)?;
        let digest = digest_value(
            RECEIPT_DOMAIN,
            &ReceiptIdentity {
                schema_version: EXECUTION_RECEIPT_SCHEMA_VERSION,
                kind: ReceiptArtifactKind::Receipt,
                body: &body,
            },
        )?;
        Ok(Self {
            schema_version: EXECUTION_RECEIPT_SCHEMA_VERSION.to_string(),
            kind: ReceiptArtifactKind::Receipt,
            id: artifact_id("receipt", &digest)?,
            content_digest: digest,
            body,
        })
    }
}

impl ArtifactPayload {
    pub(crate) fn from_serializable(schema_version: &str, value: &impl Serialize) -> Result<Self> {
        validate_schema_version(schema_version)?;
        let body = serde_json::to_value(value).context("serialize execution artifact payload")?;
        let content_digest = digest_value(
            &format!("atlas.codeatlas.dev/artifact-payload/v1/{schema_version}"),
            &body,
        )?;
        Ok(Self {
            schema_version: schema_version.to_string(),
            content_digest,
            body,
        })
    }

    pub(crate) fn decode<T: DeserializeOwned>(&self, expected_schema: &str) -> Result<T> {
        if self.schema_version != expected_schema {
            anyhow::bail!(
                "Artifact payload schema {:?} does not match expected {:?}",
                self.schema_version,
                expected_schema
            );
        }
        self.verify_identity()?;
        serde_json::from_value(self.body.clone()).context("decode typed artifact payload")
    }

    pub(crate) fn verify_identity(&self) -> Result<()> {
        validate_schema_version(&self.schema_version)?;
        let expected = digest_value(
            &format!(
                "atlas.codeatlas.dev/artifact-payload/v1/{}",
                self.schema_version
            ),
            &self.body,
        )?;
        if self.content_digest != expected {
            anyhow::bail!("Artifact payload digest does not match its canonical body");
        }
        Ok(())
    }
}

impl ManagedArtifact for ExecutionPlan {
    const DIRECTORY: &'static str = "plans";
    const PREFIX: &'static str = "plan";
    const LABEL: &'static str = "execution plan";

    fn artifact_id(&self) -> &str {
        &self.id
    }

    fn verify_identity(&self) -> Result<()> {
        if self.schema_version != EXECUTION_PLAN_SCHEMA_VERSION {
            anyhow::bail!(
                "Unsupported execution plan schema {:?}; expected {:?}",
                self.schema_version,
                EXECUTION_PLAN_SCHEMA_VERSION
            );
        }
        validate_execution_plan_body(&self.body)?;
        let expected = digest_value(
            PLAN_DOMAIN,
            &PlanIdentity {
                schema_version: EXECUTION_PLAN_SCHEMA_VERSION,
                kind: PlanArtifactKind::Plan,
                body: &self.body,
            },
        )?;
        verify_artifact_identity("plan", &self.id, &self.content_digest, &expected)
    }
}

impl ManagedArtifact for ExecutionReceipt {
    const DIRECTORY: &'static str = "receipts";
    const PREFIX: &'static str = "receipt";
    const LABEL: &'static str = "execution receipt";

    fn artifact_id(&self) -> &str {
        &self.id
    }

    fn verify_identity(&self) -> Result<()> {
        if self.schema_version != EXECUTION_RECEIPT_SCHEMA_VERSION {
            anyhow::bail!(
                "Unsupported execution receipt schema {:?}; expected {:?}",
                self.schema_version,
                EXECUTION_RECEIPT_SCHEMA_VERSION
            );
        }
        validate_execution_receipt_body(&self.body)?;
        let expected = digest_value(
            RECEIPT_DOMAIN,
            &ReceiptIdentity {
                schema_version: EXECUTION_RECEIPT_SCHEMA_VERSION,
                kind: ReceiptArtifactKind::Receipt,
                body: &self.body,
            },
        )?;
        verify_artifact_identity("receipt", &self.id, &self.content_digest, &expected)
    }
}

pub(crate) fn digest_value(domain: &str, value: &impl Serialize) -> Result<String> {
    validate_domain(domain)?;
    let value = serde_json::to_value(value).context("serialize canonical artifact value")?;
    validate_jcs_value(&value)?;
    let bytes = serde_json_canonicalizer::to_vec(&value)
        .context("canonicalize artifact value with RFC 8785")?;
    Ok(digest_domain_bytes(domain, &bytes))
}

pub(crate) fn digest_bytes(domain: &str, bytes: &[u8]) -> Result<String> {
    validate_domain(domain)?;
    Ok(digest_domain_bytes(domain, bytes))
}

pub(crate) fn digest_file(domain: &str, path: &Path, max_bytes: u64) -> Result<(String, u64)> {
    validate_domain(domain)?;
    let mut file = File::open(path)
        .with_context(|| format!("Could not read digest input {}", path.display()))?;
    let metadata = file
        .metadata()
        .with_context(|| format!("Could not inspect digest input {}", path.display()))?;
    if !metadata.is_file() {
        anyhow::bail!("Digest input {} is not a regular file", path.display());
    }
    if metadata.len() > max_bytes {
        anyhow::bail!(
            "Digest input {} exceeds the {max_bytes} byte ceiling",
            path.display()
        );
    }
    let mut digest = Sha256::new();
    digest.update(domain.as_bytes());
    digest.update(b"\n");
    let mut bytes = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .with_context(|| format!("Could not read digest input {}", path.display()))?;
        if count == 0 {
            break;
        }
        bytes = bytes
            .checked_add(u64::try_from(count).context("digest chunk size does not fit u64")?)
            .context("digest input byte count overflow")?;
        if bytes > max_bytes {
            anyhow::bail!(
                "Digest input {} exceeds the {max_bytes} byte ceiling",
                path.display()
            );
        }
        digest.update(&buffer[..count]);
    }
    let final_metadata = file
        .metadata()
        .with_context(|| format!("Could not recheck digest input {}", path.display()))?;
    if bytes != metadata.len() || has_file_metadata_changed(&metadata, &final_metadata) {
        anyhow::bail!("Digest input {} changed while it was read", path.display());
    }
    Ok((format!("sha256:{:x}", digest.finalize()), bytes))
}

fn digest_domain_bytes(domain: &str, bytes: &[u8]) -> String {
    let mut digest = Sha256::new();
    digest.update(domain.as_bytes());
    digest.update(b"\n");
    digest.update(bytes);
    format!("sha256:{:x}", digest.finalize())
}

fn validate_domain(domain: &str) -> Result<()> {
    if !domain.starts_with("atlas.codeatlas.dev/")
        || domain.trim() != domain
        || domain.chars().any(char::is_control)
    {
        anyhow::bail!("Invalid CodeAtlas digest domain {domain:?}");
    }
    Ok(())
}

fn validate_jcs_value(value: &Value) -> Result<()> {
    match value {
        Value::Null | Value::Bool(_) | Value::String(_) => Ok(()),
        Value::Array(values) => {
            for value in values {
                validate_jcs_value(value)?;
            }
            Ok(())
        }
        Value::Object(values) => {
            for value in values.values() {
                validate_jcs_value(value)?;
            }
            Ok(())
        }
        Value::Number(number) => {
            if number
                .as_u64()
                .is_some_and(|value| value > MAX_JCS_SAFE_INTEGER)
                || number.as_i64().is_some_and(|value| {
                    value < -(MAX_JCS_SAFE_INTEGER as i64) || value > MAX_JCS_SAFE_INTEGER as i64
                })
            {
                anyhow::bail!(
                    "RFC 8785 identity contains an integer outside the exact IEEE-754 range"
                );
            }
            if number.as_f64().is_none_or(|value| !value.is_finite()) {
                anyhow::bail!("RFC 8785 identity contains a non-finite number");
            }
            Ok(())
        }
    }
}

fn artifact_id(prefix: &str, digest: &str) -> Result<String> {
    let hex = validate_digest(digest)?;
    Ok(format!("{prefix}_{hex}"))
}

fn verify_artifact_identity(
    prefix: &str,
    id: &str,
    content_digest: &str,
    expected_digest: &str,
) -> Result<()> {
    validate_artifact_id(prefix, id)?;
    validate_digest(content_digest)?;
    if content_digest != expected_digest {
        anyhow::bail!("Artifact content digest does not match its canonical identity body");
    }
    if id != artifact_id(prefix, expected_digest)? {
        anyhow::bail!("Artifact ID does not match its canonical content digest");
    }
    Ok(())
}

pub(crate) fn validate_artifact_id(prefix: &str, value: &str) -> Result<()> {
    let Some(hex) = value.strip_prefix(&format!("{prefix}_")) else {
        anyhow::bail!("Expected {prefix}_ artifact ID, found {value:?}");
    };
    validate_lower_hex(hex, "artifact ID")
}

pub(super) fn is_artifact_id(value: &str) -> bool {
    [
        "plan",
        "receipt",
        "observation",
        "baseline",
        "reproducer",
        "report",
    ]
    .iter()
    .any(|prefix| validate_artifact_id(prefix, value).is_ok())
}

pub(crate) fn validate_digest(value: &str) -> Result<&str> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        anyhow::bail!("Digest must start with sha256:");
    };
    validate_lower_hex(hex, "digest")?;
    Ok(hex)
}

fn validate_lower_hex(value: &str, label: &str) -> Result<()> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        anyhow::bail!("{label} must contain exactly 64 lowercase hexadecimal characters");
    }
    Ok(())
}

pub(crate) fn is_namespaced_artifact_version(value: &str) -> bool {
    let Some(remainder) = value.strip_prefix("codeatlas.") else {
        return false;
    };
    let Some((kind, version)) = remainder.rsplit_once("/v") else {
        return false;
    };
    if kind.is_empty()
        || kind.starts_with('-')
        || kind.ends_with('-')
        || kind.contains("--")
        || !kind
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return false;
    }
    version
        .parse::<u32>()
        .ok()
        .filter(|parsed| *parsed > 0 && parsed.to_string() == version)
        .is_some()
}

fn validate_schema_version(value: &str) -> Result<()> {
    if !is_namespaced_artifact_version(value) {
        anyhow::bail!("Invalid namespaced artifact payload schema {value:?}");
    }
    Ok(())
}

pub(crate) fn validate_execution_limits(limits: &ExecutionLimits) -> Result<()> {
    for (name, value) in [
        ("max_calls", limits.max_calls),
        ("calls_per_second", limits.calls_per_second),
        ("max_concurrency", limits.max_concurrency),
        ("run_timeout_ms", limits.run_timeout_ms),
        ("max_cpu_time_ms", limits.max_cpu_time_ms),
        ("max_rss_bytes", limits.max_rss_bytes),
        ("max_processes", limits.max_processes),
        ("max_open_files", limits.max_open_files),
        ("max_call_result_bytes", limits.max_call_result_bytes),
        ("max_output_bytes", limits.max_output_bytes),
        ("max_artifact_bytes", limits.max_artifact_bytes),
    ] {
        if value == 0 || value > MAX_JCS_SAFE_INTEGER {
            anyhow::bail!("Execution limit {name} must be within 1..={MAX_JCS_SAFE_INTEGER}");
        }
    }
    if limits.max_concurrency > limits.max_calls {
        anyhow::bail!("Execution max_concurrency may not exceed max_calls");
    }
    Ok(())
}

pub(crate) fn validate_artifact_link(link: &ArtifactLink) -> Result<()> {
    if !matches!(
        link.kind.as_str(),
        "plan" | "receipt" | "observation" | "baseline" | "reproducer" | "report"
    ) {
        anyhow::bail!("Unsupported artifact link kind {:?}", link.kind);
    }
    validate_artifact_id(&link.kind, &link.id)?;
    validate_digest(&link.content_digest)?;
    Ok(())
}

pub(crate) fn validate_artifact_links(links: &[ArtifactLink]) -> Result<()> {
    validate_sorted_unique("artifact links", links)?;
    if links
        .windows(2)
        .any(|pair| pair[0].kind == pair[1].kind && pair[0].id == pair[1].id)
    {
        anyhow::bail!("Artifact links may not assign two digests to one artifact ID");
    }
    for link in links {
        validate_artifact_link(link)?;
    }
    Ok(())
}

fn validate_execution_plan_body(body: &ExecutionPlanBody) -> Result<()> {
    validate_nonblank("execution operation", &body.operation)?;
    validate_tool_identity(&body.tool)?;
    validate_tool_identity(&body.engine)?;
    for digest in [
        &body.evidence.workspace,
        &body.evidence.config,
        &body.evidence.target,
        &body.evidence.contract,
        &body.evidence.tool,
        &body.evidence.engine,
        &body.evidence.policy,
    ] {
        validate_digest(digest)?;
    }
    if body.tool.digest != body.evidence.tool || body.engine.digest != body.evidence.engine {
        anyhow::bail!("Plan tool and engine identities must match their evidence digests");
    }
    validate_nonblank("target ID", &body.target.id)?;
    if body.target.class != body.authorization.class {
        anyhow::bail!("Planned target class must match the authorization decision");
    }
    body.workload.verify_identity()?;
    validate_execution_limits(&body.limits)?;
    validate_sorted_unique("effects", &body.effects)?;
    validate_sorted_unique("required capabilities", &body.required_capabilities)?;
    validate_sorted_unique("destinations", &body.destinations)?;
    validate_sorted_unique("managed commands", &body.managed_commands)?;
    if body
        .managed_commands
        .windows(2)
        .any(|pair| pair[0].owner == pair[1].owner)
    {
        anyhow::bail!("Execution plan managed command owners must be unique");
    }
    let expected_calls =
        validate_call_counts("Execution plan expected calls", &body.expected_calls)?;
    if expected_calls > body.limits.max_calls {
        anyhow::bail!("Execution plan expected calls may not exceed max_calls");
    }
    validate_sorted_unique("writable scratch roots", &body.writable_scratch_roots)?;
    if body
        .writable_scratch_roots
        .windows(2)
        .any(|pair| pair[0].logical_name == pair[1].logical_name)
    {
        anyhow::bail!("Execution plan writable scratch-root names must be unique");
    }
    for root in &body.writable_scratch_roots {
        validate_nonblank("writable scratch-root logical name", &root.logical_name)?;
        validate_nonblank("writable scratch-root owner", &root.owner)?;
    }
    validate_sorted_unique("secret references", &body.target.secret_references)?;
    validate_artifact_links(&body.links)?;
    for destination in &body.destinations {
        validate_nonblank("destination scheme", &destination.scheme)?;
        validate_nonblank("destination host", &destination.host)?;
        if destination.port == 0 {
            anyhow::bail!("Destination port must be greater than zero");
        }
    }
    for command in &body.managed_commands {
        validate_nonblank("managed command owner", &command.owner)?;
        validate_digest(&command.digest)?;
    }
    for secret in &body.target.secret_references {
        validate_nonblank("secret reference name", &secret.name)?;
        validate_nonblank("secret reference scope", &secret.scope)?;
    }
    for reason in &body.authorization.reasons {
        validate_nonblank("authorization reason", reason)?;
    }
    Ok(())
}

fn validate_execution_receipt_body(body: &ExecutionReceiptBody) -> Result<()> {
    validate_nonblank("execution operation", &body.operation)?;
    validate_tool_identity(&body.tool)?;
    validate_artifact_id("plan", &body.plan_id)?;
    validate_digest(&body.plan_content_digest)?;
    for reason in &body.reasons {
        validate_nonblank("receipt reason", reason)?;
    }
    let categorized = validate_call_counts("Execution receipt calls", &body.calls.by_category)?;
    if body.calls.consumed > body.calls.reserved || categorized != body.calls.consumed {
        anyhow::bail!("Receipt call totals are internally inconsistent");
    }
    if body.outcome == ExecutionOutcome::Passed
        && body
            .cleanup
            .iter()
            .any(|cleanup| !cleanup.released || !cleanup.verified)
    {
        anyhow::bail!("A passed receipt may not contain incomplete cleanup");
    }
    if let Some(result) = &body.result {
        result.verify_identity()?;
    }
    if let Some(backend) = &body.runtime.backend {
        validate_tool_identity(backend)?;
    }
    if let Some(digest) = &body.runtime.environment_digest {
        validate_digest(digest)?;
    }
    validate_sorted_unique("runtime capabilities", &body.runtime.capabilities)?;
    validate_artifact_links(&body.links)?;
    if !body.links.iter().any(|link| {
        link.kind == "plan"
            && link.id == body.plan_id
            && link.content_digest == body.plan_content_digest
    }) {
        anyhow::bail!("Execution receipt must link its exact parent plan");
    }
    Ok(())
}

pub(crate) fn validate_tool_identity(identity: &ToolIdentity) -> Result<()> {
    validate_nonblank("tool name", &identity.name)?;
    validate_nonblank("tool version", &identity.version)?;
    validate_digest(&identity.digest)?;
    Ok(())
}

fn validate_nonblank(label: &str, value: &str) -> Result<()> {
    if value.is_empty() || value.trim() != value || value.chars().any(char::is_control) {
        anyhow::bail!("{label} must be nonblank and contain no control characters");
    }
    Ok(())
}

fn validate_call_counts(label: &str, values: &[CallCount]) -> Result<u64> {
    validate_sorted_unique(label, values)?;
    if values
        .windows(2)
        .any(|pair| pair[0].category == pair[1].category)
    {
        anyhow::bail!("{label} must use each category at most once");
    }
    values.iter().try_fold(0_u64, |total, entry| {
        if entry.count == 0 {
            anyhow::bail!("{label} counts must be greater than zero");
        }
        total
            .checked_add(entry.count)
            .context("call count overflow")
    })
}

fn validate_sorted_unique<T: Ord>(label: &str, values: &[T]) -> Result<()> {
    if !values.windows(2).all(|pair| pair[0] < pair[1]) {
        anyhow::bail!("{label} must be sorted and unique");
    }
    Ok(())
}

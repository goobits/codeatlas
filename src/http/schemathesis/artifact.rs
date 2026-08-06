use super::super::model::{
    HttpFuzzReport, HttpFuzzReportArtifactKind, HttpFuzzReportBody, HTTP_FUZZ_REPORT_SCHEMA_VERSION,
};
use crate::execution::artifact::{
    digest_value, validate_artifact_id, validate_digest, ManagedArtifact,
};
use crate::execution::ExecutionPlan;
use anyhow::Result;
use serde::Serialize;

const HTTP_FUZZ_REPORT_DOMAIN: &str = "atlas.codeatlas.dev/http-fuzz-report/v1";

#[derive(Serialize)]
struct ReportIdentity<'a> {
    schema_version: &'static str,
    kind: HttpFuzzReportArtifactKind,
    plan_id: &'a str,
    plan_content_digest: &'a str,
    #[serde(flatten)]
    body: &'a HttpFuzzReportBody,
}

impl HttpFuzzReport {
    pub(crate) fn new(plan: &ExecutionPlan, body: HttpFuzzReportBody) -> Result<Self> {
        validate_body(&body)?;
        let content_digest = digest_value(
            HTTP_FUZZ_REPORT_DOMAIN,
            &ReportIdentity {
                schema_version: HTTP_FUZZ_REPORT_SCHEMA_VERSION,
                kind: HttpFuzzReportArtifactKind::Report,
                plan_id: &plan.id,
                plan_content_digest: &plan.content_digest,
                body: &body,
            },
        )?;
        let id = format!(
            "report_{}",
            validate_digest(&content_digest).expect("fresh CodeAtlas report digest is valid")
        );
        Ok(Self {
            schema_version: HTTP_FUZZ_REPORT_SCHEMA_VERSION.to_string(),
            kind: HttpFuzzReportArtifactKind::Report,
            id,
            content_digest,
            plan_id: plan.id.clone(),
            plan_content_digest: plan.content_digest.clone(),
            body,
        })
    }
}

impl ManagedArtifact for HttpFuzzReport {
    const DIRECTORY: &'static str = "reports";
    const PREFIX: &'static str = "report";
    const LABEL: &'static str = "HTTP fuzz report";

    fn artifact_id(&self) -> &str {
        &self.id
    }

    fn verify_identity(&self) -> Result<()> {
        if self.schema_version != HTTP_FUZZ_REPORT_SCHEMA_VERSION
            || self.kind != HttpFuzzReportArtifactKind::Report
        {
            anyhow::bail!("Unsupported HTTP fuzz report artifact identity");
        }
        validate_artifact_id(Self::PREFIX, &self.id)?;
        validate_digest(&self.plan_content_digest)?;
        validate_artifact_id("plan", &self.plan_id)?;
        validate_body(&self.body)?;
        let expected = digest_value(
            HTTP_FUZZ_REPORT_DOMAIN,
            &ReportIdentity {
                schema_version: HTTP_FUZZ_REPORT_SCHEMA_VERSION,
                kind: HttpFuzzReportArtifactKind::Report,
                plan_id: &self.plan_id,
                plan_content_digest: &self.plan_content_digest,
                body: &self.body,
            },
        )?;
        if self.content_digest != expected
            || self.id
                != format!(
                    "report_{}",
                    validate_digest(&expected).expect("fresh CodeAtlas report digest is valid")
                )
        {
            anyhow::bail!("HTTP fuzz report identity does not match its canonical body");
        }
        Ok(())
    }
}

fn validate_body(body: &HttpFuzzReportBody) -> Result<()> {
    for (label, value) in [
        ("tool version", body.tool_version.as_str()),
        ("target ID", body.target_id.as_str()),
        ("contract ID", body.contract_id.as_str()),
        ("profile", body.profile.as_str()),
    ] {
        if value.is_empty() || value.trim() != value || value.chars().any(char::is_control) {
            anyhow::bail!("HTTP fuzz report {label} is invalid");
        }
    }
    if !body
        .operations
        .windows(2)
        .all(|pair| pair[0].operation < pair[1].operation)
    {
        anyhow::bail!("HTTP fuzz report operations must be sorted and unique");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::HttpFuzzReport;
    use crate::execution::artifact::{sample_plan, ArtifactStore};
    use crate::http::model::{
        HttpFuzzContractMode, HttpFuzzReportBody, HttpFuzzTotals, HTTP_FUZZ_REPORT_SCHEMA_VERSION,
    };

    fn body() -> HttpFuzzReportBody {
        HttpFuzzReportBody {
            tool_version: "1.0.0".to_string(),
            target_id: "fixture".to_string(),
            contract_id: "api".to_string(),
            contract_mode: HttpFuzzContractMode::OpenApi,
            profile: "standard".to_string(),
            seed: Some("42".to_string()),
            stateful: None,
            totals: HttpFuzzTotals::default(),
            operations: Vec::new(),
        }
    }

    #[test]
    fn report_identity_is_plan_bound_and_store_addressable() {
        let plan = sample_plan();
        let report = HttpFuzzReport::new(&plan, body()).expect("report");
        assert_eq!(report.schema_version, HTTP_FUZZ_REPORT_SCHEMA_VERSION);
        assert!(report.id.starts_with("report_"));

        let root = std::env::temp_dir().join(format!(
            "codeatlas-http-report-store-{}",
            std::process::id()
        ));
        let workspace = root.join("workspace");
        std::fs::create_dir_all(&workspace).expect("workspace");
        let store =
            ArtifactStore::for_tests(root.join("artifacts"), &workspace, 64 * 1024).expect("store");
        store.persist(&report).expect("persist report");
        std::fs::remove_dir_all(root).expect("remove report fixture");
    }
}

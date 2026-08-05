use crate::execution::artifact::{
    digest_value, validate_artifact_id, validate_artifact_links, validate_digest,
    validate_execution_limits, validate_tool_identity, ManagedArtifact,
};
use crate::execution::{
    ArtifactLink, ArtifactPayload, EvidenceDigests, ExecutionLimits, ExecutionSubject, ToolIdentity,
};
use crate::fuzz::{validate_fuzz_execution_limits, FuzzLimits, FUZZ_REPRODUCER_SCHEMA_VERSION};
use anyhow::Result;
use serde::{Deserialize, Serialize};

const REPRODUCER_DOMAIN: &str = "atlas.codeatlas.dev/reproducer/v1";

#[derive(schemars::JsonSchema, Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReproducerArtifactKind {
    Reproducer,
}

#[derive(schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Reproducer {
    pub schema_version: String,
    pub kind: ReproducerArtifactKind,
    pub id: String,
    pub content_digest: String,
    #[serde(flatten)]
    pub body: ReproducerBody,
}

#[derive(schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReproducerBody {
    pub subject: ExecutionSubject,
    pub tool: ToolIdentity,
    pub parent_plan_id: String,
    pub parent_plan_content_digest: String,
    pub evidence: EvidenceDigests,
    pub workload: ArtifactPayload,
    pub execution_limits: ExecutionLimits,
    pub fuzz_limits: FuzzLimits,
    pub oracle_digest: String,
    pub result_digest: String,
    pub links: Vec<ArtifactLink>,
}

#[derive(Serialize)]
struct ReproducerIdentity<'a> {
    schema_version: &'static str,
    kind: ReproducerArtifactKind,
    #[serde(flatten)]
    body: &'a ReproducerBody,
}

impl Reproducer {
    pub(crate) fn new(body: ReproducerBody) -> Result<Self> {
        validate_reproducer_body(&body)?;
        let digest = digest_value(
            REPRODUCER_DOMAIN,
            &ReproducerIdentity {
                schema_version: FUZZ_REPRODUCER_SCHEMA_VERSION,
                kind: ReproducerArtifactKind::Reproducer,
                body: &body,
            },
        )?;
        let hex = digest
            .strip_prefix("sha256:")
            .expect("execution digest always has sha256 prefix");
        Ok(Self {
            schema_version: FUZZ_REPRODUCER_SCHEMA_VERSION.to_string(),
            kind: ReproducerArtifactKind::Reproducer,
            id: format!("reproducer_{hex}"),
            content_digest: digest,
            body,
        })
    }
}

impl ManagedArtifact for Reproducer {
    const DIRECTORY: &'static str = "reproducers";
    const PREFIX: &'static str = "reproducer";
    const LABEL: &'static str = "fuzz reproducer";

    fn artifact_id(&self) -> &str {
        &self.id
    }

    fn verify_identity(&self) -> Result<()> {
        if self.schema_version != FUZZ_REPRODUCER_SCHEMA_VERSION {
            anyhow::bail!(
                "Unsupported fuzz reproducer schema {:?}",
                self.schema_version
            );
        }
        let expected = Self::new(self.body.clone())?;
        if self.id != expected.id || self.content_digest != expected.content_digest {
            anyhow::bail!("Fuzz reproducer identity does not match its canonical body");
        }
        Ok(())
    }
}

fn validate_reproducer_body(body: &ReproducerBody) -> Result<()> {
    if body.tool.digest != body.evidence.tool {
        anyhow::bail!("Reproducer creation tool must match its tool evidence digest");
    }
    validate_tool_identity(&body.tool)?;
    validate_artifact_id("plan", &body.parent_plan_id)?;
    for digest in [
        &body.parent_plan_content_digest,
        &body.evidence.workspace,
        &body.evidence.config,
        &body.evidence.target,
        &body.evidence.contract,
        &body.evidence.tool,
        &body.evidence.engine,
        &body.evidence.policy,
        &body.oracle_digest,
        &body.result_digest,
    ] {
        validate_digest(digest)?;
    }
    body.workload.verify_identity()?;
    validate_execution_limits(&body.execution_limits)?;
    validate_fuzz_execution_limits(&body.fuzz_limits, &body.execution_limits)?;
    validate_artifact_links(&body.links)?;
    if !body.links.iter().any(|link| {
        link.kind == "plan"
            && link.id == body.parent_plan_id
            && link.content_digest == body.parent_plan_content_digest
    }) {
        anyhow::bail!("Reproducer must link its exact parent execution plan");
    }
    Ok(())
}

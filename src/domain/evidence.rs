use serde::{Deserialize, Serialize};

#[derive(
    schemars::JsonSchema,
    Debug,
    Clone,
    Copy,
    Serialize,
    Deserialize,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
)]
#[serde(rename_all = "snake_case")]
pub(crate) enum EvidenceClass {
    Direct,
    Inferred,
    BoundaryLimited,
}

#[derive(
    schemars::JsonSchema,
    Debug,
    Clone,
    Copy,
    Serialize,
    Deserialize,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SourceDisposition {
    Maintained,
    Generated,
    Fixture,
    Test,
    Tooling,
}

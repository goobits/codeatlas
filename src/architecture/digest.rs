use super::diagnostic::ArchitectureError;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::fmt;
use std::str::FromStr;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum DigestKind {
    SourceDocument,
    CanonicalModule,
    ImportClosure,
    ArchitectureClosure,
    GoverningGraph,
    PolicyClosure,
    ReviewGraph,
    ObservationContent,
    ObservationEnvelope,
    ConformanceResult,
}

impl DigestKind {
    const REGISTRY: [(Self, &'static str); 10] = [
        (Self::SourceDocument, "source-document"),
        (Self::CanonicalModule, "canonical-module"),
        (Self::ImportClosure, "import-closure"),
        (Self::ArchitectureClosure, "architecture-closure"),
        (Self::GoverningGraph, "governing-graph"),
        (Self::PolicyClosure, "policy-closure"),
        (Self::ReviewGraph, "review-graph"),
        (Self::ObservationContent, "observation-content"),
        (Self::ObservationEnvelope, "observation-envelope"),
        (Self::ConformanceResult, "conformance-result"),
    ];

    fn name(self) -> &'static str {
        Self::REGISTRY
            .iter()
            .find_map(|(kind, name)| (*kind == self).then_some(*name))
            .expect("every digest kind is registered")
    }
}

#[derive(
    schemars::JsonSchema, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
)]
#[serde(transparent)]
pub(crate) struct TypedDigest(String);

impl TypedDigest {
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TypedDigest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl FromStr for TypedDigest {
    type Err = ArchitectureError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let Some(hex) = value.strip_prefix("sha256:") else {
            return Err(ArchitectureError::new(
                "digest.invalid-format",
                "digest must start with sha256:",
            ));
        };
        if hex.len() != 64
            || !hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(ArchitectureError::new(
                "digest.invalid-format",
                "digest must contain 64 lowercase hexadecimal characters",
            ));
        }
        Ok(Self(value.to_owned()))
    }
}

pub(crate) fn digest_bytes(kind: DigestKind, bytes: &[u8]) -> TypedDigest {
    let mut hasher = Sha256::new();
    hasher.update(format!("atlas.codeatlas.dev/{}/v0.1\n", kind.name()).as_bytes());
    hasher.update(bytes);
    TypedDigest(format!("sha256:{:x}", hasher.finalize()))
}

pub(crate) fn digest_value(
    kind: DigestKind,
    value: &Value,
) -> Result<TypedDigest, ArchitectureError> {
    Ok(digest_bytes(kind, &canonical_json_bytes(value)?))
}

pub(crate) fn canonical_json_bytes(value: &Value) -> Result<Vec<u8>, ArchitectureError> {
    let normalized = normalize_value(value)?;
    serde_json::to_vec(&normalized).map_err(|error| {
        ArchitectureError::new(
            "canonicalization.serialization-failed",
            format!("cannot serialize canonical JSON: {error}"),
        )
    })
}

fn normalize_value(value: &Value) -> Result<Value, ArchitectureError> {
    match value {
        Value::Null | Value::Bool(_) | Value::String(_) => Ok(value.clone()),
        Value::Number(number) => {
            if number.as_i64().is_none() {
                return Err(ArchitectureError::new(
                    "canonicalization.non-integer-number",
                    "canonical values support signed 64-bit integers only",
                ));
            }
            Ok(value.clone())
        }
        Value::Array(values) => values
            .iter()
            .map(normalize_value)
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        Value::Object(values) => {
            let mut normalized = Map::new();
            let mut keys = values.keys().collect::<Vec<_>>();
            keys.sort_unstable();
            for key in keys {
                normalized.insert(key.clone(), normalize_value(&values[key])?);
            }
            Ok(Value::Object(normalized))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{canonical_json_bytes, digest_bytes, DigestKind, TypedDigest};
    use serde_json::json;
    use std::str::FromStr;

    #[test]
    fn every_digest_family_has_a_distinct_domain() {
        let digests = DigestKind::REGISTRY
            .map(|(kind, _)| digest_bytes(kind, b"same payload"))
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(digests.len(), DigestKind::REGISTRY.len());
    }

    #[test]
    fn canonical_objects_sort_keys_recursively() {
        let bytes =
            canonical_json_bytes(&json!({"z": 1, "a": {"d": 4, "b": 2}})).expect("canonical");
        assert_eq!(
            String::from_utf8(bytes).expect("UTF-8"),
            r#"{"a":{"b":2,"d":4},"z":1}"#
        );
    }

    #[test]
    fn typed_digests_require_lowercase_sha256() {
        assert!(TypedDigest::from_str(&format!("sha256:{}", "a".repeat(64))).is_ok());
        assert!(TypedDigest::from_str(&format!("sha256:{}", "A".repeat(64))).is_err());
    }
}

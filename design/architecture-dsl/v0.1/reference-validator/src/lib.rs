//! Private executable specification evidence for Atlas Architecture DSL v0.1.
//!
//! This crate is unpublished and is not a production Code Atlas API.

pub mod canonicalization;
pub mod diagnostics;
pub mod restricted_yaml;

pub use canonicalization::{
    canonical_json_bytes, digest_bytes, digest_value, DigestKind, TypedDigest,
};
pub use diagnostics::{Diagnostic, Severity, SourcePosition, SourceSpan, ValidationError};
pub use restricted_yaml::{parse_restricted_yaml, ParseLimits, ParsedDocument};

//! Private executable specification evidence for Atlas Architecture DSL v0.1.
//!
//! This crate is unpublished and is not a production Code Atlas API.

pub mod artifacts;
pub mod canonicalization;
pub mod conformance;
pub mod diagnostics;
pub mod graph;
pub mod import_paths;
pub mod policy;
pub mod restricted_yaml;
pub mod schema_validation;
pub mod semantic_validation;

pub use artifacts::{check_generated_artifacts, write_generated_artifacts};
pub use canonicalization::{
    canonical_json_bytes, digest_bytes, digest_value, DigestKind, TypedDigest,
};
pub use conformance::validate_conformance_with_observation;
pub use diagnostics::{Diagnostic, Severity, SourcePosition, SourceSpan, ValidationError};
pub use graph::{compile_modules, CompileMode, CompiledGraph};
pub use import_paths::resolve_local_import;
pub use policy::{evaluate_exception, ExceptionContext, ExceptionDisposition};
pub use restricted_yaml::{parse_restricted_yaml, ParseLimits, ParsedDocument};
pub use schema_validation::{validate_document_schema, DocumentKind};
pub use semantic_validation::{is_qualified_identifier, Vocabulary};

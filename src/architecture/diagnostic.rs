use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;

use super::{ARCHITECTURE_API_VERSION, ARCHITECTURE_SCHEMA_VERSION};

#[derive(schemars::JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ArchitectureDiagnosticReport<'a> {
    schema_version: u32,
    api_version: &'static str,
    diagnostics: &'a [Diagnostic],
}

impl<'a> ArchitectureDiagnosticReport<'a> {
    pub(crate) fn new(diagnostics: &'a [Diagnostic]) -> Self {
        Self {
            schema_version: ARCHITECTURE_SCHEMA_VERSION,
            api_version: ARCHITECTURE_API_VERSION,
            diagnostics,
        }
    }
}

#[derive(
    schemars::JsonSchema, Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Severity {
    Error,
    Warning,
    Advisory,
}

#[derive(schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct Diagnostic {
    pub code: String,
    pub severity: Severity,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub document_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_path: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub related_ids: Vec<String>,
}

impl Diagnostic {
    pub(crate) fn error(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            severity: Severity::Error,
            message: message.into(),
            document_id: None,
            source_path: None,
            related_ids: Vec::new(),
        }
    }

    pub(crate) fn at_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.source_path = Some(path.into());
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ArchitectureError {
    pub diagnostic: Box<Diagnostic>,
}

impl ArchitectureError {
    pub(crate) fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            diagnostic: Box::new(Diagnostic::error(code, message)),
        }
    }

    pub(crate) fn at_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.diagnostic.source_path = Some(path.into());
        self
    }
}

impl fmt::Display for ArchitectureError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}: {}",
            self.diagnostic.code, self.diagnostic.message
        )
    }
}

impl std::error::Error for ArchitectureError {}

pub(crate) fn sort_diagnostics(diagnostics: &mut [Diagnostic]) {
    diagnostics.sort_by(|left, right| {
        left.source_path
            .cmp(&right.source_path)
            .then_with(|| left.code.cmp(&right.code))
            .then_with(|| left.related_ids.cmp(&right.related_ids))
            .then_with(|| left.message.cmp(&right.message))
    });
}

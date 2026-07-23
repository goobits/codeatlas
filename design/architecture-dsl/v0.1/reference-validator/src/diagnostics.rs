use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warning,
    Advisory,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SourcePosition {
    pub line: usize,
    pub column: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SourceSpan {
    pub start: SourcePosition,
    pub end: SourcePosition,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub code: String,
    pub severity: Severity,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub document_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_path: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span: Option<SourceSpan>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub related_ids: Vec<String>,
}

impl Diagnostic {
    pub fn error(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            severity: Severity::Error,
            message: message.into(),
            document_id: None,
            source_path: None,
            span: None,
            related_ids: Vec::new(),
        }
    }

    pub fn at_path(mut self, source_path: impl Into<PathBuf>) -> Self {
        self.source_path = Some(source_path.into());
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidationError {
    pub diagnostic: Box<Diagnostic>,
}

impl ValidationError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            diagnostic: Box::new(Diagnostic::error(code, message)),
        }
    }

    pub fn at_path(mut self, source_path: impl Into<PathBuf>) -> Self {
        self.diagnostic.source_path = Some(source_path.into());
        self
    }
}

impl fmt::Display for ValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}: {}",
            self.diagnostic.code, self.diagnostic.message
        )
    }
}

impl std::error::Error for ValidationError {}

#[cfg(test)]
mod tests {
    use super::{Diagnostic, Severity, ValidationError};

    #[test]
    fn errors_have_stable_structured_codes() {
        let error = ValidationError::new("yaml.duplicate-key", "duplicate key")
            .at_path("architecture.atlas.yaml");

        assert_eq!(error.diagnostic.code, "yaml.duplicate-key");
        assert_eq!(error.diagnostic.severity, Severity::Error);
        assert_eq!(
            error.diagnostic.source_path.as_deref(),
            Some(std::path::Path::new("architecture.atlas.yaml"))
        );
    }

    #[test]
    fn diagnostics_serialize_without_empty_optional_fields() {
        let value = serde_json::to_value(Diagnostic::error("test.error", "failed"))
            .expect("serialize diagnostic");

        assert_eq!(value["severity"], "error");
        assert!(value.get("document_id").is_none());
        assert!(value.get("related_ids").is_none());
    }
}

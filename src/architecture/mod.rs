mod compiler;
mod diagnostic;
mod digest;
mod graph;
mod schema;
mod vocabulary;
mod yaml;

pub(crate) use compiler::{compile, CompileRequest, CompileResult};
pub(crate) use diagnostic::Diagnostic;
pub(crate) use graph::CompileMode;

pub(crate) const ARCHITECTURE_API_VERSION: &str = "atlas.codeatlas.dev/v0.1";
pub(crate) const ARCHITECTURE_SCHEMA_VERSION: u32 = 1;

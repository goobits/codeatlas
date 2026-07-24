mod compiler;
mod conformance;
mod diagnostic;
mod digest;
mod documents;
mod graph;
mod model;
mod observation;
mod policy;
mod schema;
mod vocabulary;
mod yaml;

pub(crate) use compiler::{compile, CompileRequest, CompileResult};
pub(crate) use conformance::{
    conform, source_inputs as conformance_source_inputs, ConformanceRequest,
};
pub(crate) use diagnostic::Diagnostic;
pub(crate) use graph::CompileMode;
pub(crate) use observation::{observe, source_input_paths, ObserveRequest};

pub(crate) const ARCHITECTURE_API_VERSION: &str = "atlas.codeatlas.dev/v0.1";
pub(crate) const ARCHITECTURE_SCHEMA_VERSION: u32 = 1;

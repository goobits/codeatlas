mod baseline;
mod compiler;
mod conformance;
mod diagnostic;
mod digest;
mod documents;
mod graph;
mod model;
mod observation;
mod policy;
mod provider_query;
mod schema;
mod source_conformance;
mod vocabulary;
mod yaml;

pub(crate) use baseline::load as load_compilation;
#[cfg(test)]
pub(crate) use compiler::ArchitectureLockfile;
pub(crate) use compiler::{compile, CompileRequest, CompileResult};
#[cfg(test)]
pub(crate) use conformance::ArchitectureConformance;
pub(crate) use conformance::{
    conform, source_inputs as conformance_source_inputs, ConformanceRequest,
};
pub(crate) use diagnostic::{ArchitectureDiagnosticReport, Diagnostic};
pub(crate) use graph::CompileMode;
#[cfg(test)]
pub(crate) use observation::ArchitectureObservation;
pub(crate) use observation::{observe, source_input_paths, ObserveRequest};
pub(crate) use provider_query::{query_approved_providers, ProviderQueryReport};
#[cfg(test)]
pub(crate) use source_conformance::SOURCE_CONFORMANCE_SCHEMA_VERSION;
pub(crate) use source_conformance::{conform_source_dependencies, SourceConformanceReport};

pub(crate) const ARCHITECTURE_API_VERSION: &str = "atlas.codeatlas.dev/v0.1";
pub(crate) const ARCHITECTURE_SCHEMA_VERSION: u32 = 1;

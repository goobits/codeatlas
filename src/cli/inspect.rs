use crate::commands;
use crate::commands::architecture::providers::ArchitectureProviderApprovalScope;
use clap::{Args, Subcommand, ValueEnum};
use std::path::{Path, PathBuf};

use super::scope::RepositoryScopeArgs;

#[derive(Args)]
pub(super) struct GraphInspectionArgs {
    /// Exact subject target; repeat to inspect multiple roots
    #[arg(required = true)]
    target: Vec<String>,
    /// Incoming and outgoing graph traversal depth
    #[arg(long, default_value_t = 2)]
    depth: usize,
    /// Maximum nodes in the returned page
    #[arg(long, default_value_t = 128)]
    max_nodes: usize,
    /// Traverse incoming edges, outgoing edges, or both
    #[arg(long, value_enum, default_value_t = InspectDirection::Both)]
    direction: InspectDirection,
    /// Resume a prior page using its exact continuation cursor
    #[arg(long)]
    cursor: Option<String>,
    /// Write the JSON report instead of stdout
    #[arg(short, long)]
    out: Option<PathBuf>,
}

impl GraphInspectionArgs {
    fn into_request(self) -> (crate::inspection::InspectionRequest, Option<PathBuf>) {
        (
            crate::inspection::InspectionRequest {
                targets: self.target,
                depth: self.depth,
                max_nodes: self.max_nodes,
                direction: self.direction.into(),
                continuation: self.cursor,
            },
            self.out,
        )
    }
}

#[derive(Subcommand)]
pub(super) enum InspectSubject {
    /// Produce a bounded source context slice
    Code {
        #[command(flatten)]
        inspection: GraphInspectionArgs,
    },
    /// Produce a bounded HTTP contract and consumer graph
    Http {
        #[command(flatten)]
        inspection: GraphInspectionArgs,
        #[command(flatten)]
        repository: RepositoryScopeArgs,
    },
    /// Produce a bounded PostgreSQL contract and query graph
    Postgres {
        #[command(flatten)]
        inspection: GraphInspectionArgs,
        #[command(flatten)]
        repository: RepositoryScopeArgs,
    },
    /// Query approved providers for one architecture capability
    Architecture {
        /// Capability selector in the form capability:<id>
        selector: String,
        /// Root ArchitectureModule documents
        #[arg(required = true)]
        modules: Vec<PathBuf>,
        /// Filesystem boundary for modules and local imports
        #[arg(long, default_value = ".")]
        source_root: PathBuf,
        /// Provider approval scope to match
        #[arg(long, value_enum, default_value_t = ArchitectureProviderApprovalScope::Organization)]
        approval_scope: ArchitectureProviderApprovalScope,
        /// Write the query report instead of stdout
        #[arg(short, long)]
        out: Option<PathBuf>,
    },
}

impl InspectSubject {
    pub(super) fn run(self, root: &Path, config: Option<&Path>) -> i32 {
        match self {
            Self::Code { inspection } => {
                let (request, out) = inspection.into_request();
                commands::context_slice::run(root, request, out.as_deref(), config)
            }
            Self::Http {
                inspection,
                repository,
            } => {
                let (request, out) = inspection.into_request();
                commands::http::run_inspect(
                    root,
                    repository.workspace,
                    request,
                    out.as_deref(),
                    config,
                )
            }
            Self::Postgres {
                inspection,
                repository,
            } => {
                let (request, out) = inspection.into_request();
                commands::postgres::run_inspect(
                    root,
                    repository.workspace,
                    request,
                    out.as_deref(),
                    config,
                )
            }
            Self::Architecture {
                selector,
                modules,
                source_root,
                approval_scope,
                out,
            } => {
                let Some(capability) = selector.strip_prefix("capability:") else {
                    eprintln!("Error: architecture selectors must use capability:<id>");
                    return 2;
                };
                commands::architecture::providers::run(
                    &modules,
                    &source_root,
                    capability,
                    approval_scope,
                    out.as_deref(),
                )
            }
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
pub(super) enum InspectDirection {
    Incoming,
    Outgoing,
    Both,
}

impl From<InspectDirection> for crate::inspection::InspectionDirection {
    fn from(value: InspectDirection) -> Self {
        match value {
            InspectDirection::Incoming => Self::Incoming,
            InspectDirection::Outgoing => Self::Outgoing,
            InspectDirection::Both => Self::Both,
        }
    }
}

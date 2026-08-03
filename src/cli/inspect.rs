use crate::commands;
use crate::commands::architecture::providers::ArchitectureProviderApprovalScope;
use clap::{Subcommand, ValueEnum};
use std::path::{Path, PathBuf};

#[derive(Subcommand)]
pub(super) enum InspectSubject {
    /// Produce a bounded source context slice
    Code {
        /// Exact node ID, source path, or path#symbol target
        #[arg(required = true)]
        target: Vec<String>,
        /// Incoming and outgoing graph traversal depth
        #[arg(long, default_value_t = 2)]
        depth: usize,
        /// Maximum source nodes in the returned slice
        #[arg(long, default_value_t = 128)]
        max_nodes: usize,
        /// Traverse callers, callees, or both
        #[arg(long, value_enum, default_value_t = InspectDirection::Both)]
        direction: InspectDirection,
        /// Resume a prior page using its exact continuation cursor
        #[arg(long)]
        cursor: Option<String>,
        /// Write the JSON report instead of stdout
        #[arg(short, long)]
        out: Option<PathBuf>,
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
            Self::Code {
                target,
                depth,
                max_nodes,
                direction,
                cursor,
                out,
            } => commands::context_slice::run(
                root,
                crate::context_slice::ContextSliceRequest {
                    targets: target,
                    depth,
                    max_nodes,
                    direction: direction.into(),
                    continuation: cursor,
                },
                out.as_deref(),
                config,
            ),
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

impl From<InspectDirection> for crate::context_slice::ContextDirection {
    fn from(value: InspectDirection) -> Self {
        match value {
            InspectDirection::Incoming => Self::Incoming,
            InspectDirection::Outgoing => Self::Outgoing,
            InspectDirection::Both => Self::Both,
        }
    }
}

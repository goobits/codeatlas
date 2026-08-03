use crate::commands;
use crate::commands::architecture::providers::ArchitectureProviderApprovalScope;
use clap::Subcommand;
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
                out,
            } => {
                commands::context_slice::run(root, target, depth, max_nodes, out.as_deref(), config)
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

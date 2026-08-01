use crate::commands;
use crate::commands::architecture::compile::ArchitectureCompileMode;
use crate::commands::architecture::providers::ArchitectureProviderApprovalScope;
use clap::Subcommand;
use std::path::PathBuf;

#[derive(Subcommand)]
pub(super) enum ArchitectureCommand {
    /// Compile accepted ArchitectureModule declarations
    Compile {
        /// Root ArchitectureModule documents
        #[arg(required = true)]
        modules: Vec<PathBuf>,
        /// Filesystem boundary for modules and local imports
        #[arg(long, default_value = ".")]
        source_root: PathBuf,
        /// Compile the governing or non-governing review graph
        #[arg(long, value_enum, default_value_t = ArchitectureCompileMode::Governing)]
        mode: ArchitectureCompileMode,
        /// Write the complete compilation result instead of stdout
        #[arg(short, long)]
        out: Option<PathBuf>,
        /// Also write the generated import lockfile
        #[arg(long)]
        lock_out: Option<PathBuf>,
    },

    /// Query accepted provider approvals for one capability
    Providers {
        /// Root ArchitectureModule documents
        #[arg(required = true)]
        modules: Vec<PathBuf>,
        /// Filesystem boundary for modules and local imports
        #[arg(long, default_value = ".")]
        source_root: PathBuf,
        /// Stable capability ID to match
        #[arg(long)]
        capability: String,
        /// Provider approval scope to match
        #[arg(long, value_enum, default_value_t = ArchitectureProviderApprovalScope::Organization)]
        approval_scope: ArchitectureProviderApprovalScope,
        /// Write the query report instead of stdout
        #[arg(short, long)]
        out: Option<PathBuf>,
    },

    /// Observe implementation evidence for accepted architecture bindings
    Observe {
        /// Root ArchitectureModule documents
        #[arg(required = true)]
        modules: Vec<PathBuf>,
        /// Filesystem boundary for modules and local imports
        #[arg(long, default_value = ".")]
        source_root: PathBuf,
        /// Repository to inspect
        #[arg(long, default_value = ".")]
        repository: PathBuf,
        /// Stable qualified repository identity
        #[arg(long)]
        repository_id: String,
        /// Stable qualified observation identity
        #[arg(long)]
        observation_id: String,
        /// Lowercase hexadecimal source commit
        #[arg(long)]
        source_commit: String,
        /// Explicit RFC 3339 UTC observation time
        #[arg(long)]
        observed_at: String,
        /// Write the generated observation instead of stdout
        #[arg(short, long)]
        out: Option<PathBuf>,
    },

    /// Compare a governing graph with an architecture observation
    Conform {
        /// Root ArchitectureModule documents
        #[arg(required = true)]
        modules: Vec<PathBuf>,
        /// Filesystem boundary for modules, policies, and local imports
        #[arg(long, default_value = ".")]
        source_root: PathBuf,
        /// ArchitecturePolicy roots to evaluate
        #[arg(long = "policy")]
        policies: Vec<PathBuf>,
        /// Generated ArchitectureObservation document
        #[arg(long)]
        observation: PathBuf,
        /// Stable qualified conformance identity
        #[arg(long)]
        conformance_id: String,
        /// Explicit RFC 3339 UTC evaluation time
        #[arg(long)]
        as_of: String,
        /// Write the generated conformance report instead of stdout
        #[arg(short, long)]
        out: Option<PathBuf>,
        /// Exit non-zero when conformance contains error findings
        #[arg(long)]
        check: bool,
    },

    /// Check observed workspace imports against package exports and declared architecture
    SourceCheck {
        /// Root ArchitectureModule documents
        #[arg(required = true)]
        modules: Vec<PathBuf>,
        /// Filesystem boundary for modules and local imports
        #[arg(long, default_value = ".")]
        source_root: PathBuf,
        /// Repository and workspace to inspect
        #[arg(long, default_value = ".")]
        repository: PathBuf,
        /// Write the deterministic source conformance report instead of stdout
        #[arg(short, long)]
        out: Option<PathBuf>,
        /// Exit non-zero for source conformance errors
        #[arg(long)]
        check: bool,
    },
}

impl ArchitectureCommand {
    pub(super) fn run(self, config_path: Option<&std::path::Path>) -> i32 {
        match self {
            Self::Compile {
                modules,
                source_root,
                mode,
                out,
                lock_out,
            } => commands::architecture::compile::run(
                &modules,
                &source_root,
                mode,
                out.as_deref(),
                lock_out.as_deref(),
            ),
            Self::Providers {
                modules,
                source_root,
                capability,
                approval_scope,
                out,
            } => commands::architecture::providers::run(
                &modules,
                &source_root,
                &capability,
                approval_scope,
                out.as_deref(),
            ),
            Self::Observe {
                modules,
                source_root,
                repository,
                repository_id,
                observation_id,
                source_commit,
                observed_at,
                out,
            } => commands::architecture::observe::run(&commands::architecture::observe::Options {
                modules: &modules,
                source_root: &source_root,
                repository_root: &repository,
                repository_id: &repository_id,
                observation_id: &observation_id,
                source_commit: &source_commit,
                observed_at: &observed_at,
                out: out.as_deref(),
            }),
            Self::Conform {
                modules,
                source_root,
                policies,
                observation,
                conformance_id,
                as_of,
                out,
                check,
            } => commands::architecture::conform::run(&commands::architecture::conform::Options {
                modules: &modules,
                source_root: &source_root,
                policies: &policies,
                observation: &observation,
                conformance_id: &conformance_id,
                as_of: &as_of,
                out: out.as_deref(),
                check,
            }),
            Self::SourceCheck {
                modules,
                source_root,
                repository,
                out,
                check,
            } => commands::architecture::source_check::run(
                &commands::architecture::source_check::Options {
                    modules: &modules,
                    source_root: &source_root,
                    repository_root: &repository,
                    config_path,
                    out: out.as_deref(),
                    check,
                },
            ),
        }
    }
}

use crate::commands;
use crate::commands::dead_code::DeadCodeFormat;
use crate::commands::testing::TestingFormat;
use clap::{Subcommand, ValueEnum};
use std::path::{Path, PathBuf};

use super::scope::RepositoryScopeArgs;

#[derive(Copy, Clone, Default, Eq, PartialEq, ValueEnum)]
pub(super) enum UsageScope {
    /// Classify the full maintained source graph
    #[default]
    All,
    /// Report public exports with no known repository consumers
    Public,
}

#[derive(Subcommand)]
pub(super) enum UsageSubject {
    /// Classify code reachability and known consumers
    Code {
        /// Full source reachability or public consumer analysis
        #[arg(long, value_enum, default_value_t = UsageScope::All)]
        scope: UsageScope,
        /// External source tree whose package imports count as consumers
        #[arg(long)]
        consumer_root: Option<PathBuf>,
        #[command(flatten)]
        repository: RepositoryScopeArgs,
        /// Output format for full source reachability
        #[arg(short, long, value_enum, default_value_t = DeadCodeFormat::Text)]
        format: DeadCodeFormat,
        /// Write the full source report to a file instead of stdout
        #[arg(short, long)]
        out: Option<PathBuf>,
        /// Render only findings eligible for the code check
        #[arg(long)]
        gates_only: bool,
    },
    /// Select tests affected by repository-relative changed paths
    Tests {
        /// Repository-relative changed path; repeat for a set; omit for Git changes
        #[arg(long)]
        changed: Vec<PathBuf>,
        #[command(flatten)]
        repository: RepositoryScopeArgs,
        /// Output format
        #[arg(short, long, value_enum, default_value_t = TestingFormat::Text)]
        format: TestingFormat,
        /// Write the report to a file instead of stdout
        #[arg(short, long)]
        out: Option<PathBuf>,
    },
}

impl UsageSubject {
    pub(super) fn run(self, root: &Path, config: Option<&Path>) -> i32 {
        match self {
            Self::Code {
                scope: UsageScope::All,
                consumer_root: None,
                repository,
                format,
                out,
                gates_only,
            } => commands::dead_code::run(
                root,
                format,
                out.as_deref(),
                false,
                gates_only,
                repository.workspace,
                config,
            ),
            Self::Code {
                scope: UsageScope::All,
                consumer_root: Some(_),
                ..
            } => invalid("--consumer-root requires --scope public"),
            Self::Code {
                scope: UsageScope::Public,
                consumer_root,
                repository: RepositoryScopeArgs { workspace: false },
                format: DeadCodeFormat::Text,
                out: None,
                gates_only: false,
            } => commands::run_usage_public(root, consumer_root.as_deref(), config),
            Self::Code {
                scope: UsageScope::Public,
                ..
            } => invalid(
                "--scope public supports --consumer-root only; use --scope all for workspace, JSON, output files, or gate filtering",
            ),
            Self::Tests {
                changed,
                repository,
                format,
                out,
            } => commands::testing::run_impact(
                root,
                &changed,
                repository.workspace,
                format,
                out.as_deref(),
                config,
            ),
        }
    }
}

fn invalid(message: &str) -> i32 {
    eprintln!("Error: {message}");
    2
}

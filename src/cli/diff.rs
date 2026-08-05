use crate::commands;
use clap::Subcommand;
use std::path::{Path, PathBuf};

use super::architecture::ArchitectureDiffArgs;
use super::postgres::PostgresLiveArgs;
use super::scope::RepositoryScopeArgs;

#[derive(Subcommand)]
pub(super) enum DiffSubject {
    /// Compare a public code API baseline with current source
    Code {
        /// Baseline JSON to compare
        #[arg(long)]
        against: PathBuf,
        #[command(flatten)]
        scope: RepositoryScopeArgs,
        /// Fail on additions as well as removals and changed contracts
        #[arg(long)]
        exact: bool,
    },
    /// Compare an HTTP baseline with current contracts
    Http {
        /// Baseline JSON to compare
        #[arg(long)]
        against: PathBuf,
        /// OpenAPI file override; repeat once per configured contract
        #[arg(long)]
        openapi: Vec<PathBuf>,
        /// Write the versioned JSON report instead of stdout
        #[arg(short, long)]
        out: Option<PathBuf>,
    },
    /// Compare a PostgreSQL baseline with a fresh isolated replay
    Postgres {
        /// Baseline JSON to compare
        #[arg(long)]
        against: PathBuf,
        #[command(flatten)]
        live: PostgresLiveArgs,
    },
    /// Compare an observation with an exact saved governing graph
    Architecture {
        #[command(flatten)]
        args: ArchitectureDiffArgs,
    },
}

impl DiffSubject {
    pub(super) fn run(self, root: &Path, config: Option<&Path>) -> i32 {
        match self {
            Self::Code {
                against,
                scope,
                exact,
            } => commands::diff::run(&against, root, scope.workspace, exact, config),
            Self::Http {
                against,
                openapi,
                out,
            } => commands::http::run_diff(&against, root, &openapi, out.as_deref(), config),
            Self::Postgres { against, live } => {
                commands::postgres::run_diff(&against, &live.options(root, config))
            }
            Self::Architecture { args } => args.run(root),
        }
    }
}

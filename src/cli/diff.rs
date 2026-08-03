use crate::commands;
use clap::Subcommand;
use std::path::{Path, PathBuf};

use super::postgres::PostgresLiveArgs;

#[derive(Subcommand)]
pub(super) enum DiffSubject {
    /// Compare a public code API baseline with current source
    Code {
        /// Baseline JSON to compare
        #[arg(long)]
        against: PathBuf,
        /// Discover public packages from the nearest pnpm workspace
        #[arg(long)]
        workspace: bool,
        /// Fail on additive changes as well as breaking changes
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
}

impl DiffSubject {
    pub(super) fn run(self, root: &Path, config: Option<&Path>) -> i32 {
        match self {
            Self::Code {
                against,
                workspace,
                exact,
            } => commands::diff::run(&against, root, workspace, exact, config),
            Self::Http {
                against,
                openapi,
                out,
            } => commands::http::run_diff(&against, root, &openapi, out.as_deref(), config),
            Self::Postgres { against, live } => {
                commands::postgres::run_diff(&against, &live.options(root, config))
            }
        }
    }
}

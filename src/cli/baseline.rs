use crate::commands;
use clap::Subcommand;
use std::path::{Path, PathBuf};

use super::postgres::PostgresLiveArgs;

#[derive(Subcommand)]
pub(super) enum BaselineSubject {
    /// Save a canonical public code API baseline
    Code {
        /// Discover public packages from the nearest pnpm workspace
        #[arg(long)]
        workspace: bool,
        /// Write the baseline to this file
        #[arg(short, long)]
        out: Option<PathBuf>,
    },
    /// Save a canonical OpenAPI behavioral baseline
    Http {
        /// OpenAPI file override; repeat once per configured contract
        #[arg(long)]
        openapi: Vec<PathBuf>,
        /// Write the baseline to this file
        #[arg(short, long)]
        out: Option<PathBuf>,
    },
    /// Save a clean live PostgreSQL schema and source baseline
    Postgres {
        #[command(flatten)]
        live: PostgresLiveArgs,
    },
}

impl BaselineSubject {
    pub(super) fn run(self, root: &Path, config: Option<&Path>) -> i32 {
        match self {
            Self::Code { workspace, out } => {
                commands::diff::run_baseline(root, workspace, out.as_deref(), config)
            }
            Self::Http { openapi, out } => {
                commands::http::run_baseline(root, &openapi, out.as_deref(), config)
            }
            Self::Postgres { live } => {
                commands::postgres::run_baseline(&live.options(root, config))
            }
        }
    }
}

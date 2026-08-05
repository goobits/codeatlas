use crate::commands;
use clap::Subcommand;
use std::path::{Path, PathBuf};

use super::architecture::ArchitectureBaselineArgs;
use super::postgres::PostgresLiveArgs;
use super::scope::RepositoryScopeArgs;

#[derive(Subcommand)]
pub(super) enum BaselineSubject {
    /// Save a canonical public code API baseline
    Code {
        #[command(flatten)]
        scope: RepositoryScopeArgs,
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
    /// Save a canonical governing graph or non-governing review graph
    Architecture {
        #[command(flatten)]
        args: ArchitectureBaselineArgs,
    },
}

impl BaselineSubject {
    pub(super) fn run(self, root: &Path, config: Option<&Path>) -> i32 {
        match self {
            Self::Code { scope, out } => {
                commands::diff::run_baseline(root, scope.workspace, out.as_deref(), config)
            }
            Self::Http { openapi, out } => {
                commands::http::run_baseline(root, &openapi, out.as_deref(), config)
            }
            Self::Postgres { live } => {
                commands::postgres::run_baseline(&live.options(root, config))
            }
            Self::Architecture { args } => args.run(),
        }
    }
}

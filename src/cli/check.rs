use crate::commands;
use crate::commands::dead_code::DeadCodeFormat;
use clap::Subcommand;
use std::path::{Path, PathBuf};

use super::architecture::ArchitectureCheck;

#[derive(Subcommand)]
pub(super) enum CheckSubject {
    /// Gate high-confidence code reachability and completeness findings
    Code {
        /// Discover package projects from the nearest pnpm workspace
        #[arg(long)]
        workspace: bool,
        /// Output format
        #[arg(short, long, value_enum, default_value_t = DeadCodeFormat::Text)]
        format: DeadCodeFormat,
        /// Write the report to a file instead of stdout
        #[arg(short, long)]
        out: Option<PathBuf>,
        /// Render only findings that can fail the check
        #[arg(long)]
        gates_only: bool,
    },
    /// Check source routes and OpenAPI contract quality
    Http {
        /// OpenAPI file override; repeat once per configured contract
        #[arg(long)]
        openapi: Vec<PathBuf>,
        /// Write the versioned JSON report instead of stdout
        #[arg(short, long)]
        out: Option<PathBuf>,
        /// Also compare against this HTTP baseline
        #[arg(long)]
        against: Option<PathBuf>,
    },
    /// Check PostgreSQL migrations without starting a database
    Postgres {
        /// Write the versioned JSON report instead of stdout
        #[arg(short, long)]
        out: Option<PathBuf>,
        /// Use an exact Squawk executable
        #[arg(long)]
        squawk: Option<PathBuf>,
    },
    /// Check declared architecture against source or an observation
    Architecture {
        #[command(subcommand)]
        check: ArchitectureCheck,
    },
}

impl CheckSubject {
    pub(super) fn run(self, root: &Path, config: Option<&Path>) -> i32 {
        match self {
            Self::Code {
                workspace,
                format,
                out,
                gates_only,
            } => commands::dead_code::run(
                root,
                format,
                out.as_deref(),
                true,
                gates_only,
                workspace,
                config,
            ),
            Self::Http {
                openapi,
                out,
                against,
            } => commands::http::run_check(
                root,
                &openapi,
                out.as_deref(),
                against.as_deref(),
                config,
            ),
            Self::Postgres { out, squawk } => {
                commands::postgres::run_check(root, out.as_deref(), squawk.as_deref(), config)
            }
            Self::Architecture { check } => check.run(root, config),
        }
    }
}

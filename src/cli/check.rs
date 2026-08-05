use crate::commands;
use crate::commands::dead_code::DeadCodeFormat;
use crate::commands::testing::TestingFormat;
use clap::Subcommand;
use std::path::{Path, PathBuf};

use super::architecture::ArchitectureCheckArgs;
use super::scope::RepositoryScopeArgs;

#[derive(Subcommand)]
pub(super) enum CheckSubject {
    /// Gate high-confidence code reachability and completeness findings
    Code {
        #[command(flatten)]
        scope: RepositoryScopeArgs,
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
    /// Check declared architecture and source conformance
    Architecture {
        #[command(flatten)]
        args: ArchitectureCheckArgs,
    },
    /// Check public APIs for test witness evidence
    Tests {
        #[command(flatten)]
        scope: RepositoryScopeArgs,
        /// Output format
        #[arg(short, long, value_enum, default_value_t = TestingFormat::Text)]
        format: TestingFormat,
        /// Write the report to a file instead of stdout
        #[arg(short, long)]
        out: Option<PathBuf>,
        /// Render only public APIs that fail the witness check
        #[arg(long)]
        gates_only: bool,
    },
}

impl CheckSubject {
    pub(super) fn run(self, root: &Path, config: Option<&Path>) -> i32 {
        match self {
            Self::Code {
                scope,
                format,
                out,
                gates_only,
            } => commands::dead_code::run(
                root,
                format,
                out.as_deref(),
                true,
                gates_only,
                scope.workspace,
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
            Self::Architecture { args } => args.run(root, config),
            Self::Tests {
                scope,
                format,
                out,
                gates_only,
            } => commands::testing::run_witnesses(
                root,
                scope.workspace,
                format,
                out.as_deref(),
                gates_only,
                config,
            ),
        }
    }
}

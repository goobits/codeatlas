use crate::commands;
use crate::commands::http::HttpFuzzProfile;
use clap::Subcommand;
use std::path::{Path, PathBuf};

#[derive(Subcommand)]
pub(super) enum HttpCommand {
    /// Inventory static HTTP/page routes and optional OpenAPI schemas
    Inventory {
        /// Repository path
        #[arg(default_value = ".")]
        path: PathBuf,
        /// OpenAPI file override; repeat once per configured contract
        #[arg(long)]
        openapi: Vec<PathBuf>,
        /// Write the versioned JSON report instead of stdout
        #[arg(short, long)]
        out: Option<PathBuf>,
    },
    /// Write a compact OpenAPI behavioral baseline without source evidence
    Baseline {
        /// Repository path
        #[arg(default_value = ".")]
        path: PathBuf,
        /// OpenAPI file override; repeat once per configured contract
        #[arg(long)]
        openapi: Vec<PathBuf>,
        /// Write the versioned JSON baseline instead of stdout
        #[arg(short, long)]
        out: Option<PathBuf>,
    },
    /// Check OpenAPI contracts against static source evidence
    Check {
        /// Repository path
        #[arg(default_value = ".")]
        path: PathBuf,
        /// OpenAPI file override; repeat once per configured contract
        #[arg(long)]
        openapi: Vec<PathBuf>,
        /// Write the versioned JSON report instead of stdout
        #[arg(short, long)]
        out: Option<PathBuf>,
        /// Also compare the checked inventory with this HTTP baseline
        #[arg(long)]
        baseline: Option<PathBuf>,
    },
    /// Compare a prior HTTP inventory with the current contracts
    Diff {
        /// Baseline from `codeatlas http baseline --out`
        baseline: PathBuf,
        /// Repository path
        #[arg(default_value = ".")]
        path: PathBuf,
        /// OpenAPI file override; repeat once per configured contract
        #[arg(long)]
        openapi: Vec<PathBuf>,
        /// Write the versioned JSON report instead of stdout
        #[arg(short, long)]
        out: Option<PathBuf>,
    },
    /// Fuzz a configured live HTTP target with a managed Schemathesis toolchain
    Fuzz {
        /// Repository path
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Configured HTTP fuzz runtime target ID (not a contract ID); optional when exactly one target exists
        #[arg(long)]
        target: Option<String>,
        /// Standard local depth or a substantially deeper run
        #[arg(long, value_enum, default_value_t = HttpFuzzProfile::Standard)]
        profile: HttpFuzzProfile,
        /// Override examples generated per operation
        #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
        max_examples: Option<u32>,
        /// Reuse an exact random seed from a prior run
        #[arg(long)]
        seed: Option<u128>,
        /// Test one exact operation, formatted as `METHOD /path`
        #[arg(long)]
        operation: Option<String>,
        /// Use an existing Schemathesis executable instead of the managed toolchain
        #[arg(long)]
        schemathesis: Option<PathBuf>,
    },
}

impl HttpCommand {
    pub(super) fn run(self, config_path: Option<&Path>) -> i32 {
        match self {
            Self::Inventory { path, openapi, out } => {
                commands::http::run_inventory(&path, &openapi, out.as_deref(), config_path)
            }
            Self::Baseline { path, openapi, out } => {
                commands::http::run_baseline(&path, &openapi, out.as_deref(), config_path)
            }
            Self::Check {
                path,
                openapi,
                out,
                baseline,
            } => commands::http::run_check(
                &path,
                &openapi,
                out.as_deref(),
                baseline.as_deref(),
                config_path,
            ),
            Self::Diff {
                baseline,
                path,
                openapi,
                out,
            } => commands::http::run_diff(&baseline, &path, &openapi, out.as_deref(), config_path),
            Self::Fuzz {
                path,
                target,
                profile,
                max_examples,
                seed,
                operation,
                schemathesis,
            } => commands::http::run_fuzz(&commands::http::FuzzOptions {
                path: &path,
                target: target.as_deref(),
                profile,
                max_examples,
                seed,
                operation: operation.as_deref(),
                schemathesis: schemathesis.as_deref(),
                config_path,
            }),
        }
    }
}

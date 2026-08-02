use crate::commands::testing::{self, TestingFormat};
use clap::Subcommand;
use std::path::{Path, PathBuf};

#[derive(Subcommand)]
pub(super) enum TestingCommand {
    /// Inventory test contexts, package scripts, runners, and declarations
    Inventory {
        /// Path to the repository or configured project set
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Discover package projects from the nearest pnpm workspace
        #[arg(long)]
        workspace: bool,
        /// Output format
        #[arg(short, long, value_enum, default_value_t = TestingFormat::Text)]
        format: TestingFormat,
        /// Write the report to a file instead of stdout
        #[arg(short, long)]
        out: Option<PathBuf>,
    },
    /// Select tests affected by repository-relative changed paths
    Impact {
        /// Path to the repository or configured project set
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Repository-relative changed path; repeat for a set
        #[arg(long, required = true)]
        changed: Vec<PathBuf>,
        /// Discover package projects from the nearest pnpm workspace
        #[arg(long)]
        workspace: bool,
        /// Output format
        #[arg(short, long, value_enum, default_value_t = TestingFormat::Text)]
        format: TestingFormat,
        /// Write the report to a file instead of stdout
        #[arg(short, long)]
        out: Option<PathBuf>,
    },
    /// Report observed and declared test witnesses for public API symbols
    Witnesses {
        /// Path to the repository or configured project set
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Discover package projects from the nearest pnpm workspace
        #[arg(long)]
        workspace: bool,
        /// Output format
        #[arg(short, long, value_enum, default_value_t = TestingFormat::Text)]
        format: TestingFormat,
        /// Write the report to a file instead of stdout
        #[arg(short, long)]
        out: Option<PathBuf>,
    },
}

impl TestingCommand {
    pub(super) fn run(self, config_path: Option<&Path>) -> i32 {
        match self {
            Self::Inventory {
                path,
                workspace,
                format,
                out,
            } => testing::run_inventory(&path, workspace, format, out.as_deref(), config_path),
            Self::Impact {
                path,
                changed,
                workspace,
                format,
                out,
            } => testing::run_impact(
                &path,
                &changed,
                workspace,
                format,
                out.as_deref(),
                config_path,
            ),
            Self::Witnesses {
                path,
                workspace,
                format,
                out,
            } => testing::run_witnesses(&path, workspace, format, out.as_deref(), config_path),
        }
    }
}

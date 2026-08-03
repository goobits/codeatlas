use crate::commands::testing::{self, TestingFormat};
use clap::Subcommand;
use std::path::{Path, PathBuf};

#[derive(Subcommand)]
pub(super) enum TestsCommand {
    /// Inventory test contexts, package scripts, runners, and declarations
    Inventory {
        #[arg(long)]
        workspace: bool,
        #[arg(short, long, value_enum, default_value_t = TestingFormat::Text)]
        format: TestingFormat,
        #[arg(short, long)]
        out: Option<PathBuf>,
    },
    /// Select tests affected by repository-relative changed paths
    Impact {
        #[arg(long, required = true)]
        changed: Vec<PathBuf>,
        #[arg(long)]
        workspace: bool,
        #[arg(short, long, value_enum, default_value_t = TestingFormat::Text)]
        format: TestingFormat,
        #[arg(short, long)]
        out: Option<PathBuf>,
    },
    /// Report observed and declared test witnesses for public APIs
    Witnesses {
        #[arg(long)]
        workspace: bool,
        #[arg(short, long, value_enum, default_value_t = TestingFormat::Text)]
        format: TestingFormat,
        #[arg(short, long)]
        out: Option<PathBuf>,
    },
}

impl TestsCommand {
    pub(super) fn run(self, root: &Path, config: Option<&Path>) -> i32 {
        match self {
            Self::Inventory {
                workspace,
                format,
                out,
            } => testing::run_inventory(root, workspace, format, out.as_deref(), config),
            Self::Impact {
                changed,
                workspace,
                format,
                out,
            } => testing::run_impact(root, &changed, workspace, format, out.as_deref(), config),
            Self::Witnesses {
                workspace,
                format,
                out,
            } => testing::run_witnesses(root, workspace, format, out.as_deref(), config),
        }
    }
}

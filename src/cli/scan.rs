use crate::commands;
use crate::commands::http::HttpInventoryFormat;
use crate::commands::testing::TestingFormat;
use crate::commands::{OutputFormat, ScanScope};
use clap::Subcommand;
use std::path::{Path, PathBuf};

use super::architecture::ArchitectureScanArgs;
use super::scope::RepositoryScopeArgs;

#[derive(Subcommand)]
pub(super) enum ScanSubject {
    /// Discover the code API or maintained source surface
    Code {
        /// Output format
        #[arg(short, long, value_enum, default_value_t = OutputFormat::Tree)]
        format: OutputFormat,
        /// Include private and internal symbols
        #[arg(long)]
        all: bool,
        /// Follow the public API or inspect every maintained source file
        #[arg(long, value_enum, default_value_t = ScanScope::Api)]
        scope: ScanScope,
        /// Output directory instead of stdout
        #[arg(short, long)]
        out: Option<PathBuf>,
    },
    /// Inventory static HTTP routes and optional OpenAPI contracts
    Http {
        /// OpenAPI file override; repeat once per configured contract
        #[arg(long)]
        openapi: Vec<PathBuf>,
        /// Output format
        #[arg(short, long, value_enum, default_value_t = HttpInventoryFormat::Json)]
        format: HttpInventoryFormat,
        /// Write the selected inventory to a file instead of stdout
        #[arg(short, long)]
        out: Option<PathBuf>,
    },
    /// Inventory PostgreSQL migrations and static queries
    Postgres {
        /// Write the versioned JSON report instead of stdout
        #[arg(short, long)]
        out: Option<PathBuf>,
    },
    /// Inventory test contexts, scripts, runners, and declarations
    Tests {
        #[command(flatten)]
        scope: RepositoryScopeArgs,
        /// Output format
        #[arg(short, long, value_enum, default_value_t = TestingFormat::Text)]
        format: TestingFormat,
        /// Write the report to a file instead of stdout
        #[arg(short, long)]
        out: Option<PathBuf>,
    },
    /// Gather current implementation evidence for declared architecture
    Architecture {
        #[command(flatten)]
        args: ArchitectureScanArgs,
    },
}

impl ScanSubject {
    pub(super) fn run(self, root: &Path, config: Option<&Path>) -> i32 {
        match self {
            Self::Code {
                format,
                all,
                scope,
                out,
            } => commands::run_scan(root, format, all, scope, out, config),
            Self::Http {
                openapi,
                format,
                out,
            } => commands::http::run_inventory(root, &openapi, format, out.as_deref(), config),
            Self::Postgres { out } => {
                commands::postgres::run_inventory(root, out.as_deref(), config)
            }
            Self::Tests { scope, format, out } => commands::testing::run_inventory(
                root,
                scope.workspace,
                format,
                out.as_deref(),
                config,
            ),
            Self::Architecture { args } => args.run(root),
        }
    }
}

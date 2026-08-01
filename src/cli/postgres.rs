use crate::commands;
use clap::Subcommand;
use std::path::{Path, PathBuf};

#[derive(Subcommand)]
pub(super) enum PostgresCommand {
    /// Discover PostgreSQL sources and generate an explicit config contract
    Init {
        /// Repository path
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Add the discovered contract to codeatlas.json
        #[arg(long)]
        write: bool,
    },
    /// Inventory PostgreSQL migrations and queries without connecting to a database
    Inventory {
        /// Repository path
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Write the versioned JSON report instead of stdout
        #[arg(short, long)]
        out: Option<PathBuf>,
    },
    /// Check PostgreSQL migrations with the managed Squawk toolchain
    Check {
        /// Repository path
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Write the versioned JSON report instead of stdout
        #[arg(short, long)]
        out: Option<PathBuf>,
        /// Use an exact Squawk executable instead of the managed npm toolchain
        #[arg(long)]
        squawk: Option<PathBuf>,
    },
}

impl PostgresCommand {
    pub(super) fn run(self, config_path: Option<&Path>) -> i32 {
        match self {
            Self::Init { path, write } => commands::postgres::run_init(&path, write, config_path),
            Self::Inventory { path, out } => {
                commands::postgres::run_inventory(&path, out.as_deref(), config_path)
            }
            Self::Check { path, out, squawk } => {
                commands::postgres::run_check(&path, out.as_deref(), squawk.as_deref(), config_path)
            }
        }
    }
}

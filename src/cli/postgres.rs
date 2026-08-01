use crate::commands;
use clap::{Args, Subcommand};
use std::path::{Path, PathBuf};

#[derive(Args)]
pub(super) struct PostgresLiveArgs {
    /// Repository path
    #[arg(default_value = ".")]
    path: PathBuf,
    /// Configured PostgreSQL target ID; optional with one target or contract
    #[arg(long)]
    target: Option<String>,
    /// Write the versioned JSON report instead of stdout
    #[arg(short, long)]
    out: Option<PathBuf>,
    /// Use an exact Squawk executable instead of the managed npm toolchain
    #[arg(long)]
    squawk: Option<PathBuf>,
    /// Use an exact psql executable instead of PATH resolution
    #[arg(long)]
    psql: Option<PathBuf>,
}

impl PostgresLiveArgs {
    fn options<'a>(
        &'a self,
        config_path: Option<&'a Path>,
    ) -> commands::postgres::PostgresLiveOptions<'a> {
        commands::postgres::PostgresLiveOptions {
            path: &self.path,
            target: self.target.as_deref(),
            out: self.out.as_deref(),
            squawk: self.squawk.as_deref(),
            psql: self.psql.as_deref(),
            config_path,
        }
    }
}

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
    /// Replay migrations in an isolated database and prepare static queries
    Test {
        #[command(flatten)]
        live: PostgresLiveArgs,
    },
    /// Capture a clean live schema and source contract for later comparison
    Baseline {
        #[command(flatten)]
        live: PostgresLiveArgs,
    },
    /// Compare a clean baseline with a fresh isolated replay
    Diff {
        /// Baseline from `codeatlas postgres baseline --out`
        baseline: PathBuf,
        #[command(flatten)]
        live: PostgresLiveArgs,
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
            Self::Test { live } => commands::postgres::run_test(&live.options(config_path)),
            Self::Baseline { live } => commands::postgres::run_baseline(&live.options(config_path)),
            Self::Diff { baseline, live } => {
                commands::postgres::run_diff(&baseline, &live.options(config_path))
            }
        }
    }
}

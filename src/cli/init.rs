use crate::commands;
use clap::Subcommand;
use std::path::Path;

#[derive(Subcommand)]
pub(super) enum InitSubject {
    /// Discover supported code languages and source entrypoints
    Code {
        /// Add the discovered code settings to codeatlas.json
        #[arg(long)]
        write: bool,
    },
    /// Discover local HTTP sources and propose a static contract
    Http {
        /// Add the discovered HTTP contract to codeatlas.json
        #[arg(long)]
        write: bool,
    },
    /// Discover PostgreSQL sources and propose an explicit config contract
    Postgres {
        /// Add the discovered contract to codeatlas.json
        #[arg(long)]
        write: bool,
    },
}

impl InitSubject {
    pub(super) fn run(self, root: &Path, config: Option<&Path>) -> i32 {
        match self {
            Self::Code { write } => commands::init::run_code(root, write, config),
            Self::Http { write } => commands::init::run_http(root, write, config),
            Self::Postgres { write } => commands::init::run_postgres(root, write, config),
        }
    }
}

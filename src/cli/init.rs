use crate::commands;
use clap::Subcommand;
use std::path::Path;

#[derive(Subcommand)]
pub(super) enum InitSubject {
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
            Self::Postgres { write } => commands::postgres::run_init(root, write, config),
        }
    }
}

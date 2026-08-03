use crate::commands;
use clap::Subcommand;
use std::path::Path;

use super::postgres::PostgresLiveArgs;

#[derive(Subcommand)]
pub(super) enum TestSubject {
    /// Replay migrations and prepare static queries in an isolated database
    Postgres {
        #[command(flatten)]
        live: PostgresLiveArgs,
    },
}

impl TestSubject {
    pub(super) fn run(self, root: &Path, config: Option<&Path>) -> i32 {
        match self {
            Self::Postgres { live } => commands::postgres::run_test(&live.options(root, config)),
        }
    }
}

use crate::commands;
use clap::Args;
use std::path::{Path, PathBuf};

#[derive(Args)]
pub(super) struct PostgresLiveArgs {
    /// Configured PostgreSQL target ID
    #[arg(long)]
    target: Option<String>,
    /// Write the versioned JSON report instead of stdout
    #[arg(short, long)]
    out: Option<PathBuf>,
    /// Use an exact Squawk executable
    #[arg(long)]
    squawk: Option<PathBuf>,
    /// Use an exact psql executable
    #[arg(long)]
    psql: Option<PathBuf>,
}

impl PostgresLiveArgs {
    pub(super) fn options<'a>(
        &'a self,
        root: &'a Path,
        config: Option<&'a Path>,
    ) -> commands::postgres::PostgresLiveOptions<'a> {
        commands::postgres::PostgresLiveOptions {
            path: root,
            target: self.target.as_deref(),
            out: self.out.as_deref(),
            squawk: self.squawk.as_deref(),
            psql: self.psql.as_deref(),
            config_path: config,
        }
    }
}

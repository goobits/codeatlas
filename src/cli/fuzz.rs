use crate::commands;
use crate::commands::http::HttpFuzzProfile;
use clap::Subcommand;
use std::path::{Path, PathBuf};

#[derive(Subcommand)]
pub(super) enum FuzzSubject {
    /// Fuzz a configured live HTTP target
    Http {
        #[arg(long)]
        target: Option<String>,
        #[arg(long, value_enum, default_value_t = HttpFuzzProfile::Standard)]
        profile: HttpFuzzProfile,
        #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
        max_examples: Option<u32>,
        #[arg(long)]
        seed: Option<u128>,
        #[arg(long)]
        operation: Option<String>,
        #[arg(long)]
        schemathesis: Option<PathBuf>,
    },
}

impl FuzzSubject {
    pub(super) fn run(self, root: &Path, config: Option<&Path>) -> i32 {
        match self {
            Self::Http {
                target,
                profile,
                max_examples,
                seed,
                operation,
                schemathesis,
            } => commands::http::run_fuzz(&commands::http::FuzzOptions {
                path: root,
                target: target.as_deref(),
                profile,
                max_examples,
                seed,
                operation: operation.as_deref(),
                schemathesis: schemathesis.as_deref(),
                config_path: config,
            }),
        }
    }
}

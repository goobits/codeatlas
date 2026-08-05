use crate::commands;
use crate::commands::lexicon::LexiconFormat;
use clap::Subcommand;
use std::path::{Path, PathBuf};

use super::scope::RepositoryScopeArgs;

#[derive(Subcommand)]
pub(super) enum LexiconSubject {
    /// Find naming collisions, declared terms, and sourced conceptual overlap
    Code {
        #[command(flatten)]
        scope: RepositoryScopeArgs,
        /// Output format
        #[arg(short, long, value_enum, default_value_t = LexiconFormat::Text)]
        format: LexiconFormat,
        /// Write the report to a file instead of stdout
        #[arg(short, long)]
        out: Option<PathBuf>,
    },
}

impl LexiconSubject {
    pub(super) fn run(self, root: &Path, config: Option<&Path>) -> i32 {
        match self {
            Self::Code { scope, format, out } => {
                commands::lexicon::run(root, scope.workspace, format, out.as_deref(), config)
            }
        }
    }
}

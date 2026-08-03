use crate::commands;
use crate::commands::lexicon::LexiconFormat;
use clap::Subcommand;
use std::path::{Path, PathBuf};

#[derive(Subcommand)]
pub(super) enum LexiconSubject {
    /// Find code naming collisions, aliases, and duplicate symbol families
    Code {
        /// Preserve package ownership while scanning the nearest pnpm workspace
        #[arg(long)]
        workspace: bool,
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
            Self::Code {
                workspace,
                format,
                out,
            } => commands::lexicon::run(root, workspace, format, out.as_deref(), config),
        }
    }
}

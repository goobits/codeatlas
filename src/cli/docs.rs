use crate::commands;
use crate::commands::docs::DocsFormat;
use clap::Subcommand;
use std::path::{Path, PathBuf};

#[derive(Subcommand)]
pub(super) enum DocsSubject {
    /// Generate deterministic public code API documentation
    Code {
        #[arg(short, long)]
        out: Option<PathBuf>,
        #[arg(short, long, value_enum, default_value_t = DocsFormat::Markdown)]
        format: DocsFormat,
        #[arg(long)]
        check: bool,
        #[arg(long)]
        title: Option<String>,
    },
}

impl DocsSubject {
    pub(super) fn run(self, root: &Path, config: Option<&Path>) -> i32 {
        match self {
            Self::Code {
                out,
                format,
                check,
                title,
            } => commands::docs::run(
                root,
                out.as_deref(),
                format,
                check,
                title.as_deref(),
                config,
            ),
        }
    }
}

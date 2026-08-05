use crate::commands;
use crate::commands::docs::DocsFormat;
use clap::Subcommand;
use std::path::{Path, PathBuf};

use super::scope::RepositoryScopeArgs;

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
    /// Generate deterministic HTTP contract documentation from local evidence
    Http {
        #[command(flatten)]
        repository: RepositoryScopeArgs,
        #[arg(short, long)]
        out: Option<PathBuf>,
        #[arg(short, long, value_enum, default_value_t = DocsFormat::Markdown)]
        format: DocsFormat,
        /// Verify one explicit output file without rewriting it
        #[arg(long)]
        check: bool,
    },
    /// Generate deterministic PostgreSQL documentation from static evidence
    Postgres {
        #[command(flatten)]
        repository: RepositoryScopeArgs,
        #[arg(short, long)]
        out: Option<PathBuf>,
        #[arg(short, long, value_enum, default_value_t = DocsFormat::Markdown)]
        format: DocsFormat,
        /// Verify one explicit output file without rewriting it
        #[arg(long)]
        check: bool,
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
            } => commands::docs::run_code(
                root,
                out.as_deref(),
                format,
                check,
                title.as_deref(),
                config,
            ),
            Self::Http {
                repository,
                out,
                format,
                check,
            } => commands::docs::run_http(
                root,
                repository.workspace,
                out.as_deref(),
                format,
                check,
                config,
            ),
            Self::Postgres {
                repository,
                out,
                format,
                check,
            } => commands::docs::run_postgres(
                root,
                repository.workspace,
                out.as_deref(),
                format,
                check,
                config,
            ),
        }
    }
}

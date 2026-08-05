use crate::commands;
use crate::commands::lexicon::LexiconFormat;
use clap::{Subcommand, ValueEnum};
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
    /// Relate naming evidence across selected repository subjects
    Repository {
        #[command(flatten)]
        scope: RepositoryScopeArgs,
        /// Evidence subjects to include
        #[arg(
            long,
            value_enum,
            value_delimiter = ',',
            default_value = "code,http,postgres"
        )]
        subjects: Vec<RepositoryLexiconSubjectArg>,
        /// Output format
        #[arg(short, long, value_enum, default_value_t = LexiconFormat::Text)]
        format: LexiconFormat,
        /// Write the report to a file instead of stdout
        #[arg(short, long)]
        out: Option<PathBuf>,
    },
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, ValueEnum)]
pub(super) enum RepositoryLexiconSubjectArg {
    Code,
    Http,
    Postgres,
}

impl From<RepositoryLexiconSubjectArg> for crate::lexicon::RepositoryLexiconSubject {
    fn from(subject: RepositoryLexiconSubjectArg) -> Self {
        match subject {
            RepositoryLexiconSubjectArg::Code => Self::Code,
            RepositoryLexiconSubjectArg::Http => Self::Http,
            RepositoryLexiconSubjectArg::Postgres => Self::Postgres,
        }
    }
}

impl LexiconSubject {
    pub(super) fn run(self, root: &Path, config: Option<&Path>) -> i32 {
        match self {
            Self::Code { scope, format, out } => {
                commands::lexicon::run(root, scope.workspace, format, out.as_deref(), config)
            }
            Self::Repository {
                scope,
                subjects,
                format,
                out,
            } => commands::lexicon::run_repository(
                root,
                scope.workspace,
                subjects.into_iter().map(Into::into).collect(),
                format,
                out.as_deref(),
                config,
            ),
        }
    }
}

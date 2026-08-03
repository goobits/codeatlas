use crate::commands;
use crate::commands::architecture::compile::ArchitectureCompileMode;
use clap::Subcommand;
use std::path::{Path, PathBuf};

#[derive(Subcommand)]
pub(super) enum ArchitectureCheck {
    /// Check workspace imports against exports and declared architecture
    Source {
        #[arg(required = true)]
        modules: Vec<PathBuf>,
        #[arg(long, default_value = ".")]
        source_root: PathBuf,
        #[arg(short, long)]
        out: Option<PathBuf>,
    },
    /// Compare a governing graph with an architecture observation
    Observation {
        #[arg(required = true)]
        modules: Vec<PathBuf>,
        #[arg(long, default_value = ".")]
        source_root: PathBuf,
        #[arg(long = "policy")]
        policies: Vec<PathBuf>,
        #[arg(long)]
        observation: PathBuf,
        #[arg(long)]
        conformance_id: String,
        #[arg(long)]
        as_of: String,
        #[arg(short, long)]
        out: Option<PathBuf>,
    },
}

impl ArchitectureCheck {
    pub(super) fn run(self, root: &Path, config: Option<&Path>) -> i32 {
        match self {
            Self::Source {
                modules,
                source_root,
                out,
            } => commands::architecture::source_check::run(
                &commands::architecture::source_check::Options {
                    modules: &modules,
                    source_root: &source_root,
                    repository_root: root,
                    config_path: config,
                    out: out.as_deref(),
                    check: true,
                },
            ),
            Self::Observation {
                modules,
                source_root,
                policies,
                observation,
                conformance_id,
                as_of,
                out,
            } => commands::architecture::conform::run(&commands::architecture::conform::Options {
                modules: &modules,
                source_root: &source_root,
                policies: &policies,
                observation: &observation,
                conformance_id: &conformance_id,
                as_of: &as_of,
                out: out.as_deref(),
                check: true,
            }),
        }
    }
}

#[derive(Subcommand)]
pub(super) enum CompileSubject {
    /// Compile accepted ArchitectureModule declarations
    Architecture {
        #[arg(required = true)]
        modules: Vec<PathBuf>,
        #[arg(long, default_value = ".")]
        source_root: PathBuf,
        #[arg(long, value_enum, default_value_t = ArchitectureCompileMode::Governing)]
        mode: ArchitectureCompileMode,
        #[arg(short, long)]
        out: Option<PathBuf>,
        #[arg(long)]
        lock_out: Option<PathBuf>,
    },
}

impl CompileSubject {
    pub(super) fn run(self) -> i32 {
        match self {
            Self::Architecture {
                modules,
                source_root,
                mode,
                out,
                lock_out,
            } => commands::architecture::compile::run(
                &modules,
                &source_root,
                mode,
                out.as_deref(),
                lock_out.as_deref(),
            ),
        }
    }
}

#[derive(Subcommand)]
pub(super) enum ObserveSubject {
    /// Observe implementation evidence for accepted architecture bindings
    Architecture {
        #[arg(required = true)]
        modules: Vec<PathBuf>,
        #[arg(long, default_value = ".")]
        source_root: PathBuf,
        #[arg(long)]
        repository_id: String,
        #[arg(long)]
        observation_id: String,
        #[arg(long)]
        source_commit: String,
        #[arg(long)]
        observed_at: String,
        #[arg(short, long)]
        out: Option<PathBuf>,
    },
}

impl ObserveSubject {
    pub(super) fn run(self, root: &Path) -> i32 {
        match self {
            Self::Architecture {
                modules,
                source_root,
                repository_id,
                observation_id,
                source_commit,
                observed_at,
                out,
            } => commands::architecture::observe::run(&commands::architecture::observe::Options {
                modules: &modules,
                source_root: &source_root,
                repository_root: root,
                repository_id: &repository_id,
                observation_id: &observation_id,
                source_commit: &source_commit,
                observed_at: &observed_at,
                out: out.as_deref(),
            }),
        }
    }
}

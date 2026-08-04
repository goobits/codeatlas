use crate::commands;
use crate::commands::architecture::compile::ArchitectureCompileMode;
use clap::Args;
use std::path::{Path, PathBuf};

#[derive(Args)]
pub(super) struct ArchitectureScanArgs {
    /// Root ArchitectureModule documents
    #[arg(required = true)]
    modules: Vec<PathBuf>,
    /// Filesystem boundary for modules and local imports
    #[arg(long, default_value = ".")]
    source_root: PathBuf,
    /// Stable repository identity
    #[arg(long)]
    repository_id: String,
    /// Stable observation identity
    #[arg(long)]
    observation_id: String,
    /// Exact source revision being observed
    #[arg(long)]
    source_commit: String,
    /// RFC 3339 UTC observation time
    #[arg(long)]
    observed_at: String,
    /// Write the observation instead of stdout
    #[arg(short, long)]
    out: Option<PathBuf>,
}

impl ArchitectureScanArgs {
    pub(super) fn run(self, root: &Path) -> i32 {
        commands::architecture::observe::run(&commands::architecture::observe::Options {
            modules: &self.modules,
            source_root: &self.source_root,
            repository_root: root,
            repository_id: &self.repository_id,
            observation_id: &self.observation_id,
            source_commit: &self.source_commit,
            observed_at: &self.observed_at,
            out: self.out.as_deref(),
        })
    }
}

#[derive(Args)]
pub(super) struct ArchitectureCheckArgs {
    /// Root ArchitectureModule documents
    #[arg(required = true, value_parser = parse_architecture_module_path)]
    modules: Vec<PathBuf>,
    /// Filesystem boundary for modules and local imports
    #[arg(long, default_value = ".")]
    source_root: PathBuf,
    /// Write the source-conformance report instead of stdout
    #[arg(short, long)]
    out: Option<PathBuf>,
}

fn parse_architecture_module_path(value: &str) -> Result<PathBuf, String> {
    if matches!(value, "source" | "observation") {
        Err(format!(
            "{value:?} is a removed architecture check group; pass module paths directly"
        ))
    } else {
        Ok(PathBuf::from(value))
    }
}

impl ArchitectureCheckArgs {
    pub(super) fn run(self, root: &Path, config: Option<&Path>) -> i32 {
        commands::architecture::source_check::run(&commands::architecture::source_check::Options {
            modules: &self.modules,
            source_root: &self.source_root,
            repository_root: root,
            config_path: config,
            out: self.out.as_deref(),
            check: true,
        })
    }
}

#[derive(Args)]
pub(super) struct ArchitectureBaselineArgs {
    /// Root ArchitectureModule documents
    #[arg(required = true)]
    modules: Vec<PathBuf>,
    /// Filesystem boundary for modules and local imports
    #[arg(long, default_value = ".")]
    source_root: PathBuf,
    /// Governing evidence or a non-governing review artifact
    #[arg(long, value_enum, default_value_t = ArchitectureCompileMode::Governing)]
    mode: ArchitectureCompileMode,
    /// Write the compilation baseline instead of stdout
    #[arg(short, long)]
    out: Option<PathBuf>,
    /// Also write the canonical architecture lockfile
    #[arg(long)]
    lock_out: Option<PathBuf>,
}

impl ArchitectureBaselineArgs {
    pub(super) fn run(self) -> i32 {
        commands::architecture::compile::run(
            &self.modules,
            &self.source_root,
            self.mode,
            self.out.as_deref(),
            self.lock_out.as_deref(),
        )
    }
}

#[derive(Args)]
pub(super) struct ArchitectureDiffArgs {
    /// Saved architecture compilation baseline
    #[arg(long)]
    against: PathBuf,
    /// Architecture observation to compare
    #[arg(long)]
    observation: PathBuf,
    /// ArchitecturePolicy document; repeat for an accepted policy closure
    #[arg(long = "policy")]
    policies: Vec<PathBuf>,
    /// Stable conformance-report identity
    #[arg(long)]
    conformance_id: String,
    /// RFC 3339 UTC policy evaluation time
    #[arg(long)]
    as_of: String,
    /// Write the conformance report instead of stdout
    #[arg(short, long)]
    out: Option<PathBuf>,
}

impl ArchitectureDiffArgs {
    pub(super) fn run(self, root: &Path) -> i32 {
        commands::architecture::conform::run(&commands::architecture::conform::Options {
            baseline: &self.against,
            policy_allowed_root: root,
            policies: &self.policies,
            observation: &self.observation,
            conformance_id: &self.conformance_id,
            as_of: &self.as_of,
            out: self.out.as_deref(),
            check: true,
        })
    }
}

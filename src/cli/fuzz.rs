use super::execution::FuzzLimitArgs;
use crate::commands;
use clap::{Subcommand, ValueEnum};
use std::path::Path;

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, ValueEnum)]
pub(crate) enum HttpFuzzProfile {
    #[default]
    Standard,
    Stateful,
    Thorough,
}

impl HttpFuzzProfile {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::Stateful => "stateful",
            Self::Thorough => "thorough",
        }
    }

    pub(crate) fn profile_max_cases(self) -> u64 {
        match self {
            Self::Standard => 75,
            Self::Stateful => 25,
            Self::Thorough => 750,
        }
    }

    pub(crate) fn includes_stateful_workflows(self) -> bool {
        matches!(self, Self::Stateful)
    }
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, ValueEnum)]
pub(crate) enum CodeFuzzProfile {
    #[default]
    Standard,
    Thorough,
}

impl CodeFuzzProfile {
    pub(crate) fn profile_max_cases(self) -> u64 {
        match self {
            Self::Standard => 50,
            Self::Thorough => 500,
        }
    }
}

#[derive(Subcommand)]
pub(super) enum FuzzSubject {
    /// Plan or execute bounded public-callable fuzzing
    Code {
        /// Checked-in code target; planning is the default
        #[arg(long, conflicts_with_all = ["replay", "plan"])]
        target: Option<String>,
        /// Exact path#symbol inside the selected target
        #[arg(long, conflicts_with_all = ["replay", "plan"])]
        symbol: Option<String>,
        /// Derive a new zero-call plan from a saved reproducer
        #[arg(long, conflicts_with_all = ["target", "symbol", "plan", "execute"])]
        replay: Option<String>,
        /// Execute one exact reviewed plan ID or file
        #[arg(long, conflicts_with_all = ["target", "symbol", "replay"], requires = "execute")]
        plan: Option<String>,
        /// Execute instead of stopping after plan persistence
        #[arg(long)]
        execute: bool,
        #[arg(long, value_enum, default_value_t)]
        profile: CodeFuzzProfile,
        #[arg(long)]
        seed: Option<u128>,
        #[command(flatten)]
        limits: FuzzLimitArgs,
    },
    /// Plan or execute bounded HTTP contract fuzzing
    Http {
        /// Configured HTTP target; planning is the default
        #[arg(long, conflicts_with_all = ["replay", "plan"])]
        target: Option<String>,
        /// Derive a new zero-call plan from a saved reproducer
        #[arg(long, conflicts_with_all = ["target", "plan", "execute"])]
        replay: Option<String>,
        /// Execute one exact reviewed plan ID or file
        #[arg(long, conflicts_with_all = ["target", "replay"], requires = "execute")]
        plan: Option<String>,
        /// Execute instead of stopping after plan persistence
        #[arg(long)]
        execute: bool,
        #[arg(long, value_enum, default_value_t)]
        profile: HttpFuzzProfile,
        #[arg(long)]
        seed: Option<u128>,
        #[arg(long)]
        operation: Option<String>,
        /// Absolute Schemathesis executable path inside the configured workload image
        #[arg(long)]
        schemathesis: Option<String>,
        #[command(flatten)]
        limits: FuzzLimitArgs,
    },
}

impl FuzzSubject {
    pub(super) fn run(self, root: &Path, config: Option<&Path>) -> i32 {
        match self {
            Self::Code {
                target,
                symbol,
                replay,
                plan,
                execute,
                profile,
                seed,
                limits,
            } => commands::fuzz::run_code(&commands::fuzz::CodeOptions {
                path: root,
                target: target.as_deref(),
                symbol: symbol.as_deref(),
                replay: replay.as_deref(),
                plan: plan.as_deref(),
                execute,
                profile,
                seed,
                limits: &limits,
                config_path: config,
            }),
            Self::Http {
                target,
                replay,
                plan,
                execute,
                profile,
                seed,
                operation,
                schemathesis,
                limits,
            } => commands::fuzz::run_http(&commands::fuzz::HttpOptions {
                path: root,
                target: target.as_deref(),
                replay: replay.as_deref(),
                plan: plan.as_deref(),
                execute,
                profile,
                seed,
                operation: operation.as_deref(),
                schemathesis: schemathesis.as_deref(),
                limits: &limits,
                config_path: config,
            }),
        }
    }
}

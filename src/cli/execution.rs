use clap::Args;

#[derive(Args, Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct ExecutionLimitArgs {
    #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
    pub max_calls: Option<u64>,
    #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
    pub calls_per_second: Option<u64>,
    #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
    pub max_concurrency: Option<u64>,
    #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
    pub run_timeout_ms: Option<u64>,
    #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
    pub max_cpu_time_ms: Option<u64>,
    #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
    pub max_rss_bytes: Option<u64>,
    #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
    pub max_processes: Option<u64>,
    #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
    pub max_open_files: Option<u64>,
    #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
    pub max_call_result_bytes: Option<u64>,
    #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
    pub max_output_bytes: Option<u64>,
    #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
    pub max_artifact_bytes: Option<u64>,
}

impl ExecutionLimitArgs {
    pub(crate) fn to_overrides(&self) -> crate::execution::ExecutionLimitOverrides {
        crate::execution::ExecutionLimitOverrides {
            max_calls: self.max_calls,
            calls_per_second: self.calls_per_second,
            max_concurrency: self.max_concurrency,
            run_timeout_ms: self.run_timeout_ms,
            max_cpu_time_ms: self.max_cpu_time_ms,
            max_rss_bytes: self.max_rss_bytes,
            max_processes: self.max_processes,
            max_open_files: self.max_open_files,
            max_call_result_bytes: self.max_call_result_bytes,
            max_output_bytes: self.max_output_bytes,
            max_artifact_bytes: self.max_artifact_bytes,
        }
    }
}

#[derive(Args, Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct FuzzLimitArgs {
    #[command(flatten)]
    pub execution: ExecutionLimitArgs,
    #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
    pub max_cases: Option<u64>,
    #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
    pub max_shrinks: Option<u64>,
    #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
    pub max_failures: Option<u64>,
    #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
    pub case_timeout_ms: Option<u64>,
}

impl FuzzLimitArgs {
    pub(crate) fn is_empty(&self) -> bool {
        self == &Self::default()
    }

    pub(crate) fn to_overrides(&self) -> crate::fuzz::FuzzLimitOverrides {
        crate::fuzz::FuzzLimitOverrides {
            max_cases: self.max_cases,
            max_shrinks: self.max_shrinks,
            max_failures: self.max_failures,
            case_timeout_ms: self.case_timeout_ms,
        }
    }
}

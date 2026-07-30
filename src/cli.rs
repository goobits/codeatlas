use crate::commands;
use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "codeatlas")]
#[command(
    about = "Map public APIs, analyze source reachability, and compare architecture evidence."
)]
#[command(version)]
pub(crate) struct Cli {
    #[command(subcommand)]
    command: Command,

    /// Path to codeatlas.json
    #[arg(long, global = true)]
    pub(crate) config: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Command {
    /// Show public API surface (default command)
    Scan {
        /// Path to scan
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Output format
        #[arg(short, long, value_enum, default_value_t = OutputFormat::Tree)]
        format: OutputFormat,
        /// Include private/internal symbols
        #[arg(long)]
        all: bool,
        /// Output directory instead of stdout
        #[arg(short, long)]
        out: Option<PathBuf>,
    },

    /// Report public exports with no detected repository consumers
    Audit {
        /// Path to scan
        #[arg(default_value = ".")]
        path: PathBuf,
    },

    /// Analyze source reachability and report dead-code candidates
    DeadCode {
        /// Path to the repository or configured project set
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Output format
        #[arg(short, long, value_enum, default_value_t = DeadCodeFormat::Text)]
        format: DeadCodeFormat,
        /// Write the report to a file instead of stdout
        #[arg(short, long)]
        out: Option<PathBuf>,
        /// Exit non-zero for high-confidence gating findings
        #[arg(long)]
        check: bool,
    },

    /// Produce a bounded source context slice for exact files or symbols
    Context {
        /// Path to the repository or configured project set
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Exact node ID, source path, or path#symbol target
        #[arg(long, required = true)]
        target: Vec<String>,
        /// Incoming and outgoing graph traversal depth
        #[arg(long, default_value_t = 2)]
        depth: usize,
        /// Maximum source nodes in the returned slice
        #[arg(long, default_value_t = 128)]
        max_nodes: usize,
        /// Write the JSON report instead of stdout
        #[arg(short, long)]
        out: Option<PathBuf>,
    },

    /// Compile and compare declared architecture
    Architecture {
        #[command(subcommand)]
        command: ArchitectureCommand,
    },

    /// Inventory and compare HTTP contracts
    Http {
        #[command(subcommand)]
        command: HttpCommand,
    },

    /// CI mode: exit non-zero if issues found
    Ci {
        /// Path to scan
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Fail if any unused public exports exist
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
        fail_unused: bool,
        /// Output JSON baseline to this file
        #[arg(long)]
        baseline: Option<PathBuf>,
    },

    /// Generate Mermaid diagram
    Map {
        /// Path to scan
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Output file (default: stdout)
        #[arg(short, long)]
        out: Option<PathBuf>,
    },

    /// Generate deterministic API documentation
    Docs {
        /// Path to scan
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Documentation output file
        #[arg(short, long)]
        out: Option<PathBuf>,
        /// Documentation output format
        #[arg(short, long, value_enum, default_value_t = DocsFormat::Markdown)]
        format: DocsFormat,
        /// Fail when the output file differs instead of writing it
        #[arg(long)]
        check: bool,
        /// Override the generated page title
        #[arg(long)]
        title: Option<String>,
    },

    /// Compare current scan against a baseline JSON file
    Diff {
        /// Path to baseline JSON file from previous `codeatlas ci --baseline`
        baseline: PathBuf,
        /// Path to scan (default: current directory)
        #[arg(default_value = ".")]
        path: PathBuf,
    },
}

#[derive(Subcommand)]
enum ArchitectureCommand {
    /// Compile accepted ArchitectureModule declarations
    Compile {
        /// Root ArchitectureModule documents
        #[arg(required = true)]
        modules: Vec<PathBuf>,
        /// Filesystem boundary for modules and local imports
        #[arg(long, default_value = ".")]
        source_root: PathBuf,
        /// Compile the governing or non-governing review graph
        #[arg(long, value_enum, default_value_t = ArchitectureCompileMode::Governing)]
        mode: ArchitectureCompileMode,
        /// Write the complete compilation result instead of stdout
        #[arg(short, long)]
        out: Option<PathBuf>,
        /// Also write the generated import lockfile
        #[arg(long)]
        lock_out: Option<PathBuf>,
    },

    /// Query accepted provider approvals for one capability
    Providers {
        /// Root ArchitectureModule documents
        #[arg(required = true)]
        modules: Vec<PathBuf>,
        /// Filesystem boundary for modules and local imports
        #[arg(long, default_value = ".")]
        source_root: PathBuf,
        /// Stable capability ID to match
        #[arg(long)]
        capability: String,
        /// Provider approval scope to match
        #[arg(long, value_enum, default_value_t = ArchitectureProviderApprovalScope::Organization)]
        approval_scope: ArchitectureProviderApprovalScope,
        /// Write the query report instead of stdout
        #[arg(short, long)]
        out: Option<PathBuf>,
    },

    /// Observe implementation evidence for accepted architecture bindings
    Observe {
        /// Root ArchitectureModule documents
        #[arg(required = true)]
        modules: Vec<PathBuf>,
        /// Filesystem boundary for modules and local imports
        #[arg(long, default_value = ".")]
        source_root: PathBuf,
        /// Repository to inspect
        #[arg(long, default_value = ".")]
        repository: PathBuf,
        /// Stable qualified repository identity
        #[arg(long)]
        repository_id: String,
        /// Stable qualified observation identity
        #[arg(long)]
        observation_id: String,
        /// Lowercase hexadecimal source commit
        #[arg(long)]
        source_commit: String,
        /// Explicit RFC 3339 UTC observation time
        #[arg(long)]
        observed_at: String,
        /// Write the generated observation instead of stdout
        #[arg(short, long)]
        out: Option<PathBuf>,
    },

    /// Compare a governing graph with an architecture observation
    Conform {
        /// Root ArchitectureModule documents
        #[arg(required = true)]
        modules: Vec<PathBuf>,
        /// Filesystem boundary for modules, policies, and local imports
        #[arg(long, default_value = ".")]
        source_root: PathBuf,
        /// ArchitecturePolicy roots to evaluate
        #[arg(long = "policy")]
        policies: Vec<PathBuf>,
        /// Generated ArchitectureObservation document
        #[arg(long)]
        observation: PathBuf,
        /// Stable qualified conformance identity
        #[arg(long)]
        conformance_id: String,
        /// Explicit RFC 3339 UTC evaluation time
        #[arg(long)]
        as_of: String,
        /// Write the generated conformance report instead of stdout
        #[arg(short, long)]
        out: Option<PathBuf>,
        /// Exit non-zero when conformance contains error findings
        #[arg(long)]
        check: bool,
    },
}

#[derive(Subcommand)]
enum HttpCommand {
    /// Normalize OpenAPI contracts and static source evidence
    Inventory {
        /// Repository path
        #[arg(default_value = ".")]
        path: PathBuf,
        /// OpenAPI file override; repeat once per configured contract
        #[arg(long)]
        openapi: Vec<PathBuf>,
        /// Write the versioned JSON report instead of stdout
        #[arg(short, long)]
        out: Option<PathBuf>,
    },
    /// Write a compact behavioral baseline without source evidence
    Baseline {
        /// Repository path
        #[arg(default_value = ".")]
        path: PathBuf,
        /// OpenAPI file override; repeat once per configured contract
        #[arg(long)]
        openapi: Vec<PathBuf>,
        /// Write the versioned JSON baseline instead of stdout
        #[arg(short, long)]
        out: Option<PathBuf>,
    },
    /// Check OpenAPI contracts against static source evidence
    Check {
        /// Repository path
        #[arg(default_value = ".")]
        path: PathBuf,
        /// OpenAPI file override; repeat once per configured contract
        #[arg(long)]
        openapi: Vec<PathBuf>,
        /// Write the versioned JSON report instead of stdout
        #[arg(short, long)]
        out: Option<PathBuf>,
        /// Also compare the checked inventory with this HTTP baseline
        #[arg(long)]
        baseline: Option<PathBuf>,
    },
    /// Compare a prior HTTP inventory with the current contracts
    Diff {
        /// Baseline from `codeatlas http baseline --out`
        baseline: PathBuf,
        /// Repository path
        #[arg(default_value = ".")]
        path: PathBuf,
        /// OpenAPI file override; repeat once per configured contract
        #[arg(long)]
        openapi: Vec<PathBuf>,
        /// Write the versioned JSON report instead of stdout
        #[arg(short, long)]
        out: Option<PathBuf>,
    },
    /// Fuzz a configured live HTTP target with a managed Schemathesis toolchain
    Fuzz {
        /// Repository path
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Configured HTTP fuzz target ID; optional when exactly one target exists
        #[arg(long)]
        target: Option<String>,
        /// Standard local depth or a substantially deeper run
        #[arg(long, value_enum, default_value_t = HttpFuzzProfile::Standard)]
        profile: HttpFuzzProfile,
        /// Override examples generated per operation
        #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
        max_examples: Option<u32>,
        /// Reuse an exact random seed from a prior run
        #[arg(long)]
        seed: Option<u128>,
        /// Test one exact operation, formatted as `METHOD /path`
        #[arg(long)]
        operation: Option<String>,
        /// Use an existing Schemathesis executable instead of the managed toolchain
        #[arg(long)]
        schemathesis: Option<PathBuf>,
    },
}

#[derive(Copy, Clone, PartialEq, Eq, ValueEnum)]
pub(crate) enum HttpFuzzProfile {
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

    pub(crate) fn max_examples(self) -> u32 {
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

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
pub(crate) enum OutputFormat {
    /// ASCII tree view (default)
    Tree,
    /// Mermaid diagram
    Mermaid,
    /// JSON for tooling
    Json,
}

#[derive(Copy, Clone, PartialEq, Eq, ValueEnum)]
pub(crate) enum DocsFormat {
    /// Markdown reference
    Markdown,
    /// Standalone searchable HTML reference
    Html,
}

#[derive(Copy, Clone, PartialEq, Eq, ValueEnum)]
pub(crate) enum DeadCodeFormat {
    /// Human-readable findings and project completeness
    Text,
    /// Stable schema-versioned JSON
    Json,
}

#[derive(Copy, Clone, PartialEq, Eq, ValueEnum)]
pub(crate) enum ArchitectureCompileMode {
    /// Compile active accepted declarations only
    Governing,
    /// Compile accepted, proposed, and unresolved declarations for review
    Review,
}

#[derive(Copy, Clone, PartialEq, Eq, ValueEnum)]
pub(crate) enum ArchitectureProviderApprovalScope {
    Personal,
    Project,
    Organization,
}

impl ArchitectureProviderApprovalScope {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Personal => "personal",
            Self::Project => "project",
            Self::Organization => "organization",
        }
    }
}

pub(crate) fn run() -> i32 {
    let cli = Cli::parse();
    let config_path = cli.config.clone();

    match cli.command {
        Command::Scan {
            path,
            format,
            all,
            out,
        } => commands::run_scan(&path, format, all, out, config_path.as_deref()),
        Command::Audit { path } => commands::run_audit(&path, config_path.as_deref()),
        Command::DeadCode {
            path,
            format,
            out,
            check,
        } => commands::dead_code::run(&path, format, out.as_deref(), check, config_path.as_deref()),
        Command::Context {
            path,
            target,
            depth,
            max_nodes,
            out,
        } => commands::context_slice::run(
            &path,
            target,
            depth,
            max_nodes,
            out.as_deref(),
            config_path.as_deref(),
        ),
        Command::Architecture { command } => match command {
            ArchitectureCommand::Compile {
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
            ArchitectureCommand::Providers {
                modules,
                source_root,
                capability,
                approval_scope,
                out,
            } => commands::architecture::providers::run(
                &modules,
                &source_root,
                &capability,
                approval_scope,
                out.as_deref(),
            ),
            ArchitectureCommand::Observe {
                modules,
                source_root,
                repository,
                repository_id,
                observation_id,
                source_commit,
                observed_at,
                out,
            } => commands::architecture::observe::run(&commands::architecture::observe::Options {
                modules: &modules,
                source_root: &source_root,
                repository_root: &repository,
                repository_id: &repository_id,
                observation_id: &observation_id,
                source_commit: &source_commit,
                observed_at: &observed_at,
                out: out.as_deref(),
            }),
            ArchitectureCommand::Conform {
                modules,
                source_root,
                policies,
                observation,
                conformance_id,
                as_of,
                out,
                check,
            } => commands::architecture::conform::run(&commands::architecture::conform::Options {
                modules: &modules,
                source_root: &source_root,
                policies: &policies,
                observation: &observation,
                conformance_id: &conformance_id,
                as_of: &as_of,
                out: out.as_deref(),
                check,
            }),
        },
        Command::Http { command } => match command {
            HttpCommand::Inventory { path, openapi, out } => commands::http::run_inventory(
                &path,
                &openapi,
                out.as_deref(),
                config_path.as_deref(),
            ),
            HttpCommand::Baseline { path, openapi, out } => commands::http::run_baseline(
                &path,
                &openapi,
                out.as_deref(),
                config_path.as_deref(),
            ),
            HttpCommand::Check {
                path,
                openapi,
                out,
                baseline,
            } => commands::http::run_check(
                &path,
                &openapi,
                out.as_deref(),
                baseline.as_deref(),
                config_path.as_deref(),
            ),
            HttpCommand::Diff {
                baseline,
                path,
                openapi,
                out,
            } => commands::http::run_diff(
                &baseline,
                &path,
                &openapi,
                out.as_deref(),
                config_path.as_deref(),
            ),
            HttpCommand::Fuzz {
                path,
                target,
                profile,
                max_examples,
                seed,
                operation,
                schemathesis,
            } => commands::http::run_fuzz(&commands::http::FuzzOptions {
                path: &path,
                target: target.as_deref(),
                profile,
                max_examples,
                seed,
                operation: operation.as_deref(),
                schemathesis: schemathesis.as_deref(),
                config_path: config_path.as_deref(),
            }),
        },
        Command::Ci {
            path,
            fail_unused,
            baseline,
        } => commands::run_ci(&path, fail_unused, baseline, config_path.as_deref()),
        Command::Map { path, out } => commands::run_map(&path, out, config_path.as_deref()),
        Command::Docs {
            path,
            out,
            format,
            check,
            title,
        } => commands::docs::run(
            &path,
            out.as_deref(),
            format,
            check,
            title.as_deref(),
            config_path.as_deref(),
        ),
        Command::Diff { baseline, path } => {
            commands::diff::run(&baseline, &path, config_path.as_deref())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Cli;
    use clap::Parser;

    #[test]
    fn requires_an_explicit_command() {
        assert!(Cli::try_parse_from(["codeatlas"]).is_err());
        assert!(Cli::try_parse_from(["codeatlas", "scan", "."]).is_ok());
        assert!(Cli::try_parse_from([
            "codeatlas",
            "http",
            "inventory",
            ".",
            "--openapi",
            "openapi.json"
        ])
        .is_ok());
    }

    #[test]
    fn rejects_the_removed_flag_based_interface() {
        assert!(Cli::try_parse_from(["codeatlas", ".", "--format", "json"]).is_err());
        assert!(Cli::try_parse_from(["codeatlas", ".", "--suggest"]).is_err());
    }

    #[test]
    fn parses_an_approved_provider_query() {
        assert!(Cli::try_parse_from([
            "codeatlas",
            "architecture",
            "providers",
            "architecture/root.atlas.yaml",
            "--capability",
            "example.capability.context",
            "--approval-scope",
            "organization",
        ])
        .is_ok());
    }

    #[test]
    fn requires_a_capability_for_provider_queries() {
        assert!(Cli::try_parse_from([
            "codeatlas",
            "architecture",
            "providers",
            "architecture/root.atlas.yaml",
        ])
        .is_err());
    }
}

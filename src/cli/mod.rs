use crate::commands;
use crate::commands::dead_code::DeadCodeFormat;
use crate::commands::docs::DocsFormat;
use crate::commands::OutputFormat;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

use architecture::ArchitectureCommand;
use http::HttpCommand;
use postgres::PostgresCommand;

mod architecture;
mod http;
mod postgres;

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
        /// External source tree whose maintained package imports count as consumers
        #[arg(long)]
        consumer_root: Option<PathBuf>,
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
        /// Render only findings that can fail the dead-code gate
        #[arg(long)]
        gates_only: bool,
        /// Discover package projects from the nearest pnpm workspace
        #[arg(long)]
        workspace: bool,
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

    /// Inventory and check PostgreSQL contracts
    Postgres {
        #[command(subcommand)]
        command: PostgresCommand,
    },

    /// CI mode: exit non-zero if issues found
    Ci {
        /// Path to scan
        #[arg(default_value = ".")]
        path: PathBuf,
        /// External source tree whose maintained package imports count as consumers
        #[arg(long)]
        consumer_root: Option<PathBuf>,
        /// Fail if any unused public exports exist
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
        fail_unused: bool,
        /// Output JSON baseline to this file
        #[arg(long)]
        baseline: Option<PathBuf>,
        /// Discover public packages from the nearest pnpm workspace
        #[arg(long)]
        workspace: bool,
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
        /// Discover public packages from the nearest pnpm workspace
        #[arg(long)]
        workspace: bool,
        /// Fail on additive changes as well as breaking changes
        #[arg(long)]
        exact: bool,
    },
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
        Command::Audit {
            path,
            consumer_root,
        } => commands::run_audit(&path, consumer_root.as_deref(), config_path.as_deref()),
        Command::DeadCode {
            path,
            format,
            out,
            check,
            gates_only,
            workspace,
        } => commands::dead_code::run(
            &path,
            format,
            out.as_deref(),
            check,
            gates_only,
            workspace,
            config_path.as_deref(),
        ),
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
        Command::Architecture { command } => command.run(config_path.as_deref()),
        Command::Http { command } => command.run(config_path.as_deref()),
        Command::Postgres { command } => command.run(config_path.as_deref()),
        Command::Ci {
            path,
            consumer_root,
            fail_unused,
            baseline,
            workspace,
        } => commands::run_ci(
            &path,
            consumer_root.as_deref(),
            fail_unused,
            baseline,
            workspace,
            config_path.as_deref(),
        ),
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
        Command::Diff {
            baseline,
            path,
            workspace,
            exact,
        } => commands::diff::run(&baseline, &path, workspace, exact, config_path.as_deref()),
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
        assert!(Cli::try_parse_from([
            "codeatlas",
            "dead-code",
            "packages",
            "--workspace",
            "--check",
            "--gates-only"
        ])
        .is_ok());
        assert!(Cli::try_parse_from([
            "codeatlas",
            "audit",
            "packages/example",
            "--consumer-root",
            ".",
        ])
        .is_ok());
        assert!(Cli::try_parse_from([
            "codeatlas",
            "ci",
            "packages/example",
            "--consumer-root",
            ".",
        ])
        .is_ok());
        assert!(Cli::try_parse_from(["codeatlas", "dead-code", "packages", "--workspace"]).is_ok());
        assert!(Cli::try_parse_from([
            "codeatlas",
            "ci",
            ".",
            "--workspace",
            "--baseline",
            "public-api.json",
            "--fail-unused",
            "false"
        ])
        .is_ok());
        assert!(Cli::try_parse_from([
            "codeatlas",
            "diff",
            "public-api.json",
            ".",
            "--workspace",
            "--exact"
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

    #[test]
    fn parses_vcs_neutral_source_conformance() {
        assert!(Cli::try_parse_from([
            "codeatlas",
            "architecture",
            "source-check",
            "architecture/root.atlas.yaml",
            "--repository",
            ".",
            "--check",
        ])
        .is_ok());
    }
}

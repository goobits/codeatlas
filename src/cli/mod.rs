use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod architecture;
mod baseline;
mod check;
mod diff;
mod docs;
pub(crate) mod execution;
pub(crate) mod fuzz;
mod init;
mod inspect;
mod lexicon;
mod postgres;
mod scan;
mod scope;
mod test;
mod usage;

#[derive(Parser)]
#[command(name = "codeatlas")]
#[command(about = "Inspect and test code, HTTP, PostgreSQL, and architecture contracts.")]
#[command(version)]
pub(crate) struct Cli {
    /// Repository root
    #[arg(long, global = true, default_value = ".")]
    root: PathBuf,

    /// Path to codeatlas.json
    #[arg(long, global = true)]
    config: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Discover and model current contract evidence
    Scan {
        #[command(subcommand)]
        subject: scan::ScanSubject,
    },
    /// Apply static rules and contract conformance checks
    Check {
        #[command(subcommand)]
        subject: check::CheckSubject,
    },
    /// Save canonical comparison evidence
    Baseline {
        #[command(subcommand)]
        subject: baseline::BaselineSubject,
    },
    /// Compare current evidence with a baseline
    Diff {
        #[command(subcommand)]
        subject: diff::DiffSubject,
    },
    /// Classify code reachability and known consumers
    Usage {
        #[command(subcommand)]
        subject: usage::UsageSubject,
    },
    /// Explain one exact code, HTTP, PostgreSQL, or architecture target
    Inspect {
        #[command(subcommand)]
        subject: inspect::InspectSubject,
    },
    /// Find deterministic naming and conceptual overlap
    Lexicon {
        #[command(subcommand)]
        subject: lexicon::LexiconSubject,
    },
    /// Generate or check API documentation
    Docs {
        #[command(subcommand)]
        subject: docs::DocsSubject,
    },
    /// Plan or execute bounded contract fuzzing
    Fuzz {
        #[command(subcommand)]
        subject: Box<fuzz::FuzzSubject>,
    },
    /// Exercise PostgreSQL contracts in an isolated database
    Test {
        #[command(subcommand)]
        subject: test::TestSubject,
    },
    /// Discover and optionally write strict subject configuration
    Init {
        #[command(subcommand)]
        subject: init::InitSubject,
    },
}

pub(crate) fn run() -> i32 {
    let cli = Cli::parse();
    let config = cli.config.as_deref();
    match cli.command {
        Command::Scan { subject } => subject.run(&cli.root, config),
        Command::Check { subject } => subject.run(&cli.root, config),
        Command::Baseline { subject } => subject.run(&cli.root, config),
        Command::Diff { subject } => subject.run(&cli.root, config),
        Command::Usage { subject } => subject.run(&cli.root, config),
        Command::Inspect { subject } => subject.run(&cli.root, config),
        Command::Lexicon { subject } => subject.run(&cli.root, config),
        Command::Docs { subject } => subject.run(&cli.root, config),
        Command::Fuzz { subject } => (*subject).run(&cli.root, config),
        Command::Test { subject } => subject.run(&cli.root, config),
        Command::Init { subject } => subject.run(&cli.root, config),
    }
}

#[cfg(test)]
mod tests {
    use super::Cli;
    use clap::Parser;

    #[test]
    fn parses_the_clean_command_surface() {
        for args in [
            vec!["codeatlas", "scan", "code"],
            vec!["codeatlas", "scan", "http", "--openapi", "openapi.json"],
            vec!["codeatlas", "scan", "http", "--format", "hqa-inventory"],
            vec!["codeatlas", "scan", "postgres"],
            vec!["codeatlas", "scan", "tests"],
            vec![
                "codeatlas",
                "scan",
                "architecture",
                "architecture/root.atlas.yaml",
                "--repository-id",
                "example.repository.source",
                "--observation-id",
                "example.observation.current",
                "--source-commit",
                "0123456",
                "--observed-at",
                "2026-08-04T00:00:00Z",
            ],
            vec!["codeatlas", "check", "code", "--workspace"],
            vec!["codeatlas", "check", "tests", "--gates-only"],
            vec![
                "codeatlas",
                "check",
                "architecture",
                "architecture/root.atlas.yaml",
            ],
            vec!["codeatlas", "baseline", "code", "--out", "api.json"],
            vec![
                "codeatlas",
                "baseline",
                "architecture",
                "architecture/root.atlas.yaml",
            ],
            vec!["codeatlas", "diff", "code", "--against", "api.json"],
            vec![
                "codeatlas",
                "diff",
                "architecture",
                "--against",
                "architecture.json",
                "--observation",
                "observation.json",
                "--conformance-id",
                "example.conformance.current",
                "--as-of",
                "2026-08-04T00:00:00Z",
            ],
            vec!["codeatlas", "usage", "code"],
            vec!["codeatlas", "usage", "http", "--format", "json"],
            vec!["codeatlas", "usage", "postgres", "--workspace"],
            vec!["codeatlas", "usage", "tests", "--changed", "src/lib.rs"],
            vec!["codeatlas", "inspect", "code", "src/lib.rs#run"],
            vec!["codeatlas", "inspect", "http", "GET /health", "--workspace"],
            vec![
                "codeatlas",
                "inspect",
                "postgres",
                "table:public.users",
                "--direction",
                "incoming",
            ],
            vec!["codeatlas", "lexicon", "code"],
            vec!["codeatlas", "docs", "code"],
            vec!["codeatlas", "docs", "http", "--workspace"],
            vec!["codeatlas", "docs", "postgres", "--format", "html"],
            vec!["codeatlas", "fuzz", "http"],
            vec![
                "codeatlas",
                "fuzz",
                "http",
                "--target",
                "local",
                "--max-cases",
                "5",
                "--max-calls",
                "8",
            ],
            vec!["codeatlas", "fuzz", "http", "--replay", "reproducer.json"],
            vec![
                "codeatlas",
                "fuzz",
                "http",
                "--plan",
                "plan.json",
                "--execute",
            ],
            vec!["codeatlas", "test", "postgres"],
            vec!["codeatlas", "init", "code"],
            vec!["codeatlas", "init", "http"],
            vec!["codeatlas", "init", "postgres"],
        ] {
            assert!(Cli::try_parse_from(args).is_ok());
        }
    }

    #[test]
    fn accepts_global_repository_options_without_positional_roots() {
        assert!(Cli::try_parse_from([
            "codeatlas",
            "--root",
            "packages/example",
            "--config",
            "codeatlas.json",
            "usage",
            "tests",
            "--workspace",
            "--changed",
            "src/index.ts",
        ])
        .is_ok());
        assert!(Cli::try_parse_from(["codeatlas", "scan", "code", "."]).is_err());
    }

    #[test]
    fn rejects_removed_commands_instead_of_preserving_aliases() {
        for command in [
            "audit",
            "dead-code",
            "context",
            "architecture",
            "http",
            "postgres",
            "testing",
            "tests",
            "compile",
            "observe",
            "ci",
            "map",
        ] {
            assert!(Cli::try_parse_from(["codeatlas", command]).is_err());
        }
        assert!(
            Cli::try_parse_from(["codeatlas", "fuzz", "http", "--max-examples", "10"]).is_err()
        );
        assert!(Cli::try_parse_from(["codeatlas", "fuzz", "http", "--plan", "plan.json"]).is_err());
        assert!(Cli::try_parse_from([
            "codeatlas",
            "fuzz",
            "http",
            "--target",
            "local",
            "--replay",
            "reproducer.json"
        ])
        .is_err());
        assert!(Cli::try_parse_from([
            "codeatlas",
            "inspect",
            "http",
            "GET /health",
            "--observation",
            "observation.json",
        ])
        .is_err());
        assert!(Cli::try_parse_from([
            "codeatlas",
            "inspect",
            "postgres",
            "users",
            "--observation",
            "observation.json",
        ])
        .is_err());
    }
}

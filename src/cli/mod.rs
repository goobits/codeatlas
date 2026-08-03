use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod architecture;
mod baseline;
mod check;
mod diff;
mod docs;
mod fuzz;
mod init;
mod inspect;
mod lexicon;
mod postgres;
mod scan;
mod test;
#[path = "tests.rs"]
mod test_commands;
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
    /// Explain one exact code or architecture target
    Inspect {
        #[command(subcommand)]
        subject: inspect::InspectSubject,
    },
    /// Find deterministic naming collisions and aliases
    Lexicon {
        #[command(subcommand)]
        subject: lexicon::LexiconSubject,
    },
    /// Inventory tests, select affected suites, or report witnesses
    Tests {
        #[command(subcommand)]
        command: test_commands::TestsCommand,
    },
    /// Generate or check API documentation
    Docs {
        #[command(subcommand)]
        subject: docs::DocsSubject,
    },
    /// Exercise HTTP contracts with generated requests
    Fuzz {
        #[command(subcommand)]
        subject: fuzz::FuzzSubject,
    },
    /// Exercise PostgreSQL contracts in an isolated database
    Test {
        #[command(subcommand)]
        subject: test::TestSubject,
    },
    /// Discover and optionally write PostgreSQL configuration
    Init {
        #[command(subcommand)]
        subject: init::InitSubject,
    },
    /// Validate and normalize architecture declarations
    Compile {
        #[command(subcommand)]
        subject: architecture::CompileSubject,
    },
    /// Generate reproducible architecture source evidence
    Observe {
        #[command(subcommand)]
        subject: architecture::ObserveSubject,
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
        Command::Tests { command } => command.run(&cli.root, config),
        Command::Docs { subject } => subject.run(&cli.root, config),
        Command::Fuzz { subject } => subject.run(&cli.root, config),
        Command::Test { subject } => subject.run(&cli.root, config),
        Command::Init { subject } => subject.run(&cli.root, config),
        Command::Compile { subject } => subject.run(),
        Command::Observe { subject } => subject.run(&cli.root),
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
            vec!["codeatlas", "scan", "postgres"],
            vec!["codeatlas", "check", "code", "--workspace"],
            vec!["codeatlas", "baseline", "code", "--out", "api.json"],
            vec!["codeatlas", "diff", "code", "--against", "api.json"],
            vec!["codeatlas", "usage", "code"],
            vec!["codeatlas", "inspect", "code", "src/lib.rs#run"],
            vec!["codeatlas", "lexicon", "code"],
            vec!["codeatlas", "tests", "inventory"],
            vec!["codeatlas", "docs", "code"],
            vec!["codeatlas", "fuzz", "http"],
            vec!["codeatlas", "test", "postgres"],
            vec!["codeatlas", "init", "postgres"],
            vec![
                "codeatlas",
                "compile",
                "architecture",
                "architecture/root.atlas.yaml",
            ],
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
            "tests",
            "impact",
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
            "ci",
            "map",
        ] {
            assert!(Cli::try_parse_from(["codeatlas", command]).is_err());
        }
    }
}

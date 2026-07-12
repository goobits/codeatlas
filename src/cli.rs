use crate::commands;
use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "codeatlas")]
#[command(
    about = "Map your codebase's public API surface. Find unused exports, visualize dependencies."
)]
#[command(version)]
pub(crate) struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// Path to the repo root (for legacy flag-based usage)
    #[arg(default_value = ".")]
    pub(crate) path: PathBuf,

    /// Path to codeatlas.json
    #[arg(long, global = true)]
    pub(crate) config: Option<PathBuf>,

    // Hidden flags preserve the original flag-based CLI while callers migrate to subcommands.
    #[arg(short, long, value_delimiter = ',', hide = true)]
    pub(crate) languages: Option<Vec<String>>,
    #[arg(short, long, value_enum, hide = true)]
    pub(crate) format: Option<OutputFormat>,
    #[arg(short, long, hide = true)]
    pub(crate) out: Option<PathBuf>,
    #[arg(long, hide = true)]
    pub(crate) include_types: bool,
    #[arg(long, hide = true)]
    pub(crate) include_private: bool,
    #[arg(long, value_delimiter = ',', hide = true)]
    pub(crate) entrypoints: Option<Vec<String>>,
    #[arg(long, hide = true)]
    pub(crate) suggest: bool,
    #[arg(long, hide = true)]
    pub(crate) imports: bool,
    #[arg(long, hide = true)]
    pub(crate) no_default_ignore: bool,
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

    /// Audit for issues: unused exports, overly-broad visibility
    Audit {
        /// Path to scan
        #[arg(default_value = ".")]
        path: PathBuf,
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

    /// Generate deterministic Markdown API documentation
    Docs {
        /// Path to scan
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Markdown output file
        #[arg(short, long)]
        out: Option<PathBuf>,
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

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
pub(crate) enum OutputFormat {
    /// ASCII tree view (default)
    Tree,
    /// Mermaid diagram
    Mermaid,
    /// JSON for tooling
    Json,
}

pub(crate) fn run() -> i32 {
    let cli = Cli::parse();
    let config_path = cli.config.clone();

    match cli.command {
        Some(Command::Scan {
            path,
            format,
            all,
            out,
        }) => commands::run_scan(&path, format, all, out, config_path.as_deref()),
        Some(Command::Audit { path }) => commands::run_audit(&path, config_path.as_deref()),
        Some(Command::Ci {
            path,
            fail_unused,
            baseline,
        }) => commands::run_ci(&path, fail_unused, baseline, config_path.as_deref()),
        Some(Command::Map { path, out }) => commands::run_map(&path, out, config_path.as_deref()),
        Some(Command::Docs {
            path,
            out,
            check,
            title,
        }) => commands::docs::run(
            &path,
            out.as_deref(),
            check,
            title.as_deref(),
            config_path.as_deref(),
        ),
        Some(Command::Diff { baseline, path }) => {
            commands::diff::run(&baseline, &path, config_path.as_deref())
        }
        None if uses_legacy_flags(&cli) => commands::run_legacy(&cli),
        None => commands::run_scan(
            &cli.path,
            OutputFormat::Tree,
            false,
            None,
            config_path.as_deref(),
        ),
    }
}

fn uses_legacy_flags(cli: &Cli) -> bool {
    cli.format.is_some() || cli.suggest || cli.imports || cli.languages.is_some()
}

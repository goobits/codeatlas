mod analysis;
mod domain;
mod languages;
mod outputs;
mod paths;

#[cfg(test)]
mod tests;

use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;
use domain::ScanConfig;

#[derive(Parser)]
#[command(name = "codeatlas")]
#[command(about = "Map your codebase's public API surface. Find unused exports, visualize dependencies.")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Path to the repo root (for legacy flag-based usage)
    #[arg(default_value = ".")]
    path: PathBuf,

    // Legacy flags (kept for backward compatibility)
    #[arg(short, long, value_delimiter = ',', hide = true)]
    languages: Option<Vec<String>>,
    #[arg(short, long, value_enum, hide = true)]
    format: Option<OutputFormat>,
    #[arg(short, long, hide = true)]
    out: Option<PathBuf>,
    #[arg(long, hide = true)]
    include_types: bool,
    #[arg(long, hide = true)]
    include_private: bool,
    #[arg(long, value_delimiter = ',', hide = true)]
    entrypoints: Option<Vec<String>>,
    #[arg(long, hide = true)]
    suggest: bool,
    #[arg(long, hide = true)]
    imports: bool,
    #[arg(long, hide = true)]
    no_default_ignore: bool,
}

#[derive(Subcommand)]
enum Commands {
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
        /// Output to file instead of stdout
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
        #[arg(long, default_value_t = true)]
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
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
enum OutputFormat {
    Tree,
    Mermaid,
    Compact,
    Json,
}

fn main() {
    let cli = Cli::parse();

    let exit_code = match cli.command {
        Some(Commands::Scan { path, format, all, out }) => {
            run_scan(&path, format, all, out)
        }
        Some(Commands::Audit { path }) => {
            run_audit(&path)
        }
        Some(Commands::Ci { path, fail_unused, baseline }) => {
            run_ci(&path, fail_unused, baseline)
        }
        Some(Commands::Map { path, out }) => {
            run_map(&path, out)
        }
        None => {
            // Legacy mode: check if any old flags were used
            if cli.format.is_some() || cli.suggest || cli.imports || cli.languages.is_some() {
                run_legacy(&cli)
            } else {
                // Default: run scan
                run_scan(&cli.path, OutputFormat::Tree, false, None)
            }
        }
    };

    std::process::exit(exit_code);
}

/// Scan command: show public API surface
fn run_scan(path: &PathBuf, format: OutputFormat, include_private: bool, out: Option<PathBuf>) -> i32 {
    let config = ScanConfig {
        include_types: true,
        include_private,
        entrypoints: None,
        suggest: false,
        imports: false,
        no_default_ignore: false,
    };

    let scanners = languages::get_scanners_auto(path);
    if scanners.is_empty() {
        eprintln!("No supported languages found in {}", path.display());
        eprintln!("Supported: TypeScript/JavaScript (.ts, .js), Python (.py), Rust (.rs)");
        return 1;
    }

    let report = languages::scan_all(path, &config, scanners);
    let output = render_format(&report, format);
    output_result(output, out, format);
    0
}

/// Audit command: find issues with actionable suggestions
fn run_audit(path: &PathBuf) -> i32 {
    let config = ScanConfig {
        include_types: true,
        include_private: false,
        entrypoints: None,
        suggest: true,
        imports: true,
        no_default_ignore: false,
    };

    let scanners = languages::get_scanners_auto(path);
    if scanners.is_empty() {
        eprintln!("No supported languages found in {}", path.display());
        return 1;
    }

    let mut report = languages::scan_all(path, &config, scanners);
    let importers = analysis::annotate_imports(&mut report, path, false);
    analysis::annotate_unused_public(&mut report, &importers, false);

    let output = outputs::audit::render(&report);
    println!("{}", output);

    if report.unused_public.is_empty() {
        0
    } else {
        // Return number of issues (capped at 125 for shell conventions)
        std::cmp::min(report.unused_public.len() as i32, 125)
    }
}

/// CI command: exit non-zero if issues found
fn run_ci(path: &PathBuf, fail_unused: bool, baseline: Option<PathBuf>) -> i32 {
    let config = ScanConfig {
        include_types: true,
        include_private: false,
        entrypoints: None,
        suggest: fail_unused,
        imports: fail_unused,
        no_default_ignore: false,
    };

    let scanners = languages::get_scanners_auto(path);
    if scanners.is_empty() {
        eprintln!("No supported languages found in {}", path.display());
        return 1;
    }

    let mut report = languages::scan_all(path, &config, scanners);

    if fail_unused {
        let importers = analysis::annotate_imports(&mut report, path, false);
        analysis::annotate_unused_public(&mut report, &importers, false);
    }

    // Output baseline if requested
    if let Some(baseline_path) = baseline {
        match outputs::json::render(&report) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&baseline_path, json) {
                    eprintln!("Error writing baseline: {}", e);
                    return 1;
                }
                eprintln!("Baseline written to {}", baseline_path.display());
            }
            Err(e) => {
                eprintln!("Error generating baseline: {}", e);
                return 1;
            }
        }
    }

    // Report results
    let issue_count = report.unused_public.len();
    let symbol_count = report.symbols.len();

    if issue_count == 0 {
        println!("✓ No issues found. {} public symbols.", symbol_count);
        0
    } else {
        println!("✗ {} unused public export(s) found.", issue_count);
        for unused in &report.unused_public {
            println!("  - {}", unused.id);
        }
        println!("\nRun 'codeatlas audit' for fix suggestions.");
        1
    }
}

/// Map command: generate Mermaid diagram
fn run_map(path: &PathBuf, out: Option<PathBuf>) -> i32 {
    let config = ScanConfig {
        include_types: true,
        include_private: false,
        entrypoints: None,
        suggest: false,
        imports: true,
        no_default_ignore: false,
    };

    let scanners = languages::get_scanners_auto(path);
    if scanners.is_empty() {
        eprintln!("No supported languages found in {}", path.display());
        return 1;
    }

    let mut report = languages::scan_all(path, &config, scanners);
    analysis::annotate_imports(&mut report, path, false);

    let output = outputs::mermaid::render(&report);

    if let Some(out_path) = out {
        if let Err(e) = std::fs::write(&out_path, &output) {
            eprintln!("Error writing file: {}", e);
            return 1;
        }
        println!("Mermaid diagram written to {}", out_path.display());
    } else {
        println!("{}", output);
    }
    0
}

/// Legacy mode: support old flag-based CLI
fn run_legacy(cli: &Cli) -> i32 {
    let config = ScanConfig {
        include_types: cli.include_types || cli.format.is_none(),
        include_private: cli.include_private,
        entrypoints: cli.entrypoints.clone(),
        suggest: cli.suggest,
        imports: cli.imports,
        no_default_ignore: cli.no_default_ignore,
    };

    let scanners = if cli.languages.is_some() {
        languages::get_scanners(cli.languages.clone())
    } else {
        languages::get_scanners_auto(&cli.path)
    };

    let mut report = languages::scan_all(&cli.path, &config, scanners);
    let mut importers = None;

    if config.imports {
        importers = Some(analysis::annotate_imports(
            &mut report,
            &cli.path,
            config.no_default_ignore,
        ));
    }
    if config.suggest {
        let importers = importers.unwrap_or_else(|| {
            analysis::build_importers(&report, &cli.path, config.no_default_ignore)
        });
        analysis::annotate_unused_public(&mut report, &importers, config.no_default_ignore);
    }

    let format = cli.format.unwrap_or(OutputFormat::Tree);
    let output = render_format(&report, format);
    output_result(output, cli.out.clone(), format);
    0
}

fn render_format(report: &domain::ScanReport, format: OutputFormat) -> String {
    match format {
        OutputFormat::Tree => outputs::text_tree::render(report),
        OutputFormat::Mermaid => outputs::mermaid::render(report),
        OutputFormat::Compact => outputs::compact::render(report),
        OutputFormat::Json => outputs::json::render(report).unwrap_or_else(|e| {
            format!("Error: {}", e)
        }),
    }
}

fn output_result(content: String, out: Option<PathBuf>, format: OutputFormat) {
    if let Some(out_dir) = out {
        if let Err(e) = std::fs::create_dir_all(&out_dir) {
            eprintln!("Error creating output directory: {}", e);
            std::process::exit(1);
        }

        let filename = match format {
            OutputFormat::Tree => "atlas.tree",
            OutputFormat::Mermaid => "atlas.mmd",
            OutputFormat::Compact => "atlas.txt",
            OutputFormat::Json => "atlas.json",
        };
        let out_path = out_dir.join(filename);

        if let Err(e) = std::fs::write(&out_path, content) {
            eprintln!("Error writing output file: {}", e);
            std::process::exit(1);
        }

        println!("Report written to {}", out_path.display());
    } else {
        println!("{}", content);
    }
}

mod domain;
mod languages;
mod outputs;

use clap::{Parser, ValueEnum};
use std::path::PathBuf;
use domain::ScanConfig;

#[derive(Parser)]
#[command(name = "codeatlas")]
#[command(about = "Generate a high-density 'Public Surface Map' of a codebase.")]
struct Cli {
    /// Path to the repo root
    #[arg(default_value = ".")]
    path: PathBuf,

    /// Comma-separated list of languages to scan (ts, py, rs). Default: all.
    #[arg(short, long, value_delimiter = ',')]
    languages: Option<Vec<String>>,

    /// Output format
    #[arg(short, long, value_enum, default_value_t = OutputFormat::Tree)]
    format: OutputFormat,

    /// Output directory. If not set, prints to stdout.
    #[arg(short, long)]
    out: Option<PathBuf>,

    /// Include type definitions/interfaces
    #[arg(long, default_value_t = true)]
    include_types: bool,

    /// Include private members
    #[arg(long, default_value_t = false)]
    include_private: bool,

    /// Entrypoints for Audit Mode
    #[arg(long, value_delimiter = ',')]
    entrypoints: Option<Vec<String>>,
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

    let config = ScanConfig {
        include_types: cli.include_types,
        include_private: cli.include_private,
        entrypoints: cli.entrypoints,
    };

    let scanners = languages::get_scanners(cli.languages);
    let report = languages::scan_all(&cli.path, &config, scanners);

    let output_content = match cli.format {
        OutputFormat::Tree => outputs::text_tree::render(&report),
        OutputFormat::Mermaid => outputs::mermaid::render(&report),
        OutputFormat::Compact => outputs::compact::render(&report),
        OutputFormat::Json => match outputs::json::render(&report) {
            Ok(content) => content,
            Err(e) => {
                eprintln!("Error serializing report: {}", e);
                std::process::exit(1);
            }
        },
    };

    if let Some(out_dir) = cli.out {
        if let Err(e) = std::fs::create_dir_all(&out_dir) {
             eprintln!("Error creating output directory: {}", e);
             std::process::exit(1);
        }
        
        let filename = match cli.format {
            OutputFormat::Tree => "atlas.tree",
            OutputFormat::Mermaid => "atlas.mmd",
            OutputFormat::Compact => "atlas.txt",
            OutputFormat::Json => "atlas.json",
        };
        let out_path = out_dir.join(filename);
        
        if let Err(e) = std::fs::write(&out_path, output_content) {
             eprintln!("Error writing output file: {}", e);
             std::process::exit(1);
        }
        
        println!("Report written to {}", out_path.display());
    } else {
        println!("{}", output_content);
    }
}

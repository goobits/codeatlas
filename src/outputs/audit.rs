use crate::domain::{Language, ScanReport};
use colored::*;

pub(crate) fn render(report: &ScanReport) -> String {
    let mut output = String::new();

    output.push_str(&format!(
        "\n{}\n",
        " CodeAtlas Public Usage ".on_blue().white().bold()
    ));
    output.push_str(&format!("{}\n\n", "=================".blue()));

    let issue_count = report.unused_public.len();

    if issue_count == 0 {
        output.push_str(&format!("{} No issues found!\n\n", "✓".green().bold()));
        output.push_str(&format!(
            "  {} public symbols across {} files.\n",
            report.symbols.len(),
            report.stats.files_scanned
        ));
        output.push_str("  Your public API surface looks clean.\n\n");
        return output;
    }

    output.push_str(&format!(
        "{} {} issue(s) found:\n\n",
        "⚠".yellow().bold(),
        issue_count
    ));

    for (i, unused) in report.unused_public.iter().enumerate() {
        let num = format!("{}.", i + 1).white().bold();

        // Parse the ID to extract info: "rs:src/api.rs:fn#old_handler"
        let parts: Vec<&str> = unused.id.splitn(2, ':').collect();
        let (lang_str, rest) = if parts.len() == 2 {
            (parts[0], parts[1])
        } else {
            ("", unused.id.as_str())
        };

        let language = match lang_str {
            "rs" => Language::Rust,
            "ts" => Language::TypeScript,
            "py" => Language::Python,
            _ => Language::Unknown,
        };

        output.push_str(&format!(
            "{} {}: {}\n",
            num,
            "UNUSED PUBLIC".red().bold(),
            unused.id.yellow()
        ));

        output.push_str(&format!(
            "   {} This is exported but nothing imports it.\n",
            "→".dimmed()
        ));

        // Language-specific fix suggestions
        let fix = get_fix_suggestion(&language, rest);
        output.push_str(&format!("   {} {}\n\n", "Fix:".green().bold(), fix));
    }

    // Summary
    output.push_str(&format!("{}\n", "-".repeat(50).dimmed()));
    output.push_str(&format!("\n{}\n", "Next steps:".white().bold()));
    output.push_str(&format!(
        "  {} Review each unused export above\n",
        "1.".dimmed()
    ));
    output.push_str(&format!(
        "  {} Either remove the export or make it internal\n",
        "2.".dimmed()
    ));
    output.push_str(&format!(
        "  {} Run {} to verify fixes\n\n",
        "3.".dimmed(),
        "codeatlas check code".cyan()
    ));

    output.push_str(&format!("{}\n", "Quick commands:".white().bold()));
    output.push_str(&format!(
        "  {}  See full public API\n",
        "codeatlas scan code".cyan()
    ));
    output.push_str(&format!(
        "  {}   Generate architecture diagram\n",
        "codeatlas scan code --format mermaid".cyan()
    ));

    output
}

fn get_fix_suggestion(language: &Language, id_rest: &str) -> String {
    // Extract symbol type from id like "src/api.rs:fn#old_handler"
    let is_function = id_rest.contains(":fn#");
    let is_struct = id_rest.contains(":struct#");
    let is_enum = id_rest.contains(":enum#");
    let is_trait = id_rest.contains(":trait#");

    match language {
        Language::Rust => {
            if is_function {
                "Change `pub fn` to `pub(crate) fn` or remove the function".to_string()
            } else if is_struct {
                "Change `pub struct` to `pub(crate) struct`".to_string()
            } else if is_enum {
                "Change `pub enum` to `pub(crate) enum`".to_string()
            } else if is_trait {
                "Change `pub trait` to `pub(crate) trait` or keep if intentionally public"
                    .to_string()
            } else {
                "Change `pub` to `pub(crate)` or remove the export".to_string()
            }
        }
        Language::TypeScript => "Remove the `export` keyword or delete if unused".to_string(),
        Language::Python => "Prefix with underscore (e.g., `_function_name`) or remove".to_string(),
        Language::Unknown => "Remove the public export or make it internal".to_string(),
    }
}

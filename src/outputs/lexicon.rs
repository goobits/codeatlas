use crate::lexicon::{LexiconReport, LexiconSymbol};

pub(crate) fn render_text(report: &LexiconReport) -> String {
    let mut output = String::new();
    output.push_str("CodeAtlas lexicon\n");
    output.push_str(&format!(
        "{} source files · {} symbols · {} package-exposed symbols\n\n",
        report.stats.source_files, report.stats.symbols_analyzed, report.stats.public_symbols
    ));

    output.push_str(&format!(
        "Name collisions ({})\n",
        report.name_collisions.len()
    ));
    if report.name_collisions.is_empty() {
        output.push_str("  none\n");
    }
    for collision in &report.name_collisions {
        output.push_str(&format!(
            "- {} ({} structural shapes)\n",
            collision.name,
            collision.shapes.len()
        ));
        for shape in &collision.shapes {
            for symbol in &shape.symbols {
                output.push_str(&format_symbol(symbol, "  "));
            }
        }
    }

    output.push_str(&format!(
        "\nShape aliases ({})\n",
        report.shape_aliases.len()
    ));
    if report.shape_aliases.is_empty() {
        output.push_str("  none\n");
    }
    for alias in &report.shape_aliases {
        output.push_str(&format!("- {}\n", alias.names.join(" / ")));
        for symbol in &alias.symbols {
            output.push_str(&format_symbol(symbol, "  "));
        }
    }

    output.push_str(&format!(
        "\nDuplicate families ({})\n",
        report.duplicate_families.len()
    ));
    if report.duplicate_families.is_empty() {
        output.push_str("  none\n");
    }
    for family in &report.duplicate_families {
        output.push_str(&format!("- {}: {}\n", family.name, family.signature));
        for symbol in &family.symbols {
            output.push_str(&format_symbol(symbol, "  "));
        }
    }

    output.push_str(&format!("\nRepeated terms ({})\n", report.terms.len()));
    if report.terms.is_empty() {
        output.push_str("  none\n");
    }
    for term in report.terms.iter().take(30) {
        let names = summarize_names(&term.names);
        output.push_str(&format!(
            "- {}: {} symbols ({} package-exposed), {}\n",
            term.term, term.symbol_count, term.public_symbol_count, names
        ));
    }
    if report.terms.len() > 30 {
        output.push_str(&format!(
            "  … {} more terms are available in JSON output\n",
            report.terms.len() - 30
        ));
    }
    output
}

fn summarize_names(names: &[String]) -> String {
    const DISPLAY_LIMIT: usize = 12;
    let mut summary = names
        .iter()
        .take(DISPLAY_LIMIT)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    if names.len() > DISPLAY_LIMIT {
        summary.push_str(&format!(" … +{} more", names.len() - DISPLAY_LIMIT));
    }
    summary
}

pub(crate) fn render_json(report: &LexiconReport) -> anyhow::Result<String> {
    let mut output = serde_json::to_string_pretty(report)?;
    output.push('\n');
    Ok(output)
}

fn format_symbol(symbol: &LexiconSymbol, indent: &str) -> String {
    let exposure = if symbol.export_paths.is_empty() {
        "implementation-only".to_string()
    } else {
        format!("exported as {}", symbol.export_paths.join(", "))
    };
    format!(
        "{indent}{}:{}, {} ({exposure})\n",
        symbol.file_path, symbol.name, symbol.signature
    )
}

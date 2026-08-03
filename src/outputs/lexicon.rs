use crate::domain::EvidenceClass;
use crate::lexicon::{CallableCandidateKind, LexiconReport, LexiconSymbol};

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
        "\nCallable candidates ({})\n",
        report.callable_candidates.len()
    ));
    if report.callable_candidates.is_empty() {
        output.push_str("  none\n");
    }
    for candidate in &report.callable_candidates {
        let scope = candidate
            .scope
            .as_deref()
            .map(|scope| format!("; {scope}"))
            .unwrap_or_default();
        let terms = if candidate.shared_terms.is_empty() {
            String::new()
        } else {
            format!("; terms {}", candidate.shared_terms.join(", "))
        };
        output.push_str(&format!(
            "- [{}; {}{scope}{terms}] {}: {}\n",
            resolve_evidence_name(candidate.evidence_class),
            resolve_candidate_kind_name(candidate.kind),
            candidate.names.join(" / "),
            candidate.contract_shape
        ));
        for symbol in &candidate.symbols {
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

fn resolve_evidence_name(evidence: EvidenceClass) -> &'static str {
    match evidence {
        EvidenceClass::Direct => "direct evidence",
        EvidenceClass::Inferred => "inferred",
        EvidenceClass::BoundaryLimited => "boundary limited",
    }
}

fn resolve_candidate_kind_name(kind: CallableCandidateKind) -> &'static str {
    match kind {
        CallableCandidateKind::ExactSignature => "exact signature",
        CallableCandidateKind::SharedContractShape => "shared typed contract",
    }
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

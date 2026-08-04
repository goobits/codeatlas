use crate::config::{LexiconProviderCoverage, LexiconProviderFormat, LexiconProviderTier};
use crate::domain::EvidenceClass;
use crate::lexicon::{
    CallableCandidateKind, ConceptCandidate, ConceptCandidateConfidence, ConceptCandidateRule,
    ConceptEvidenceRelation, ConceptSuppressionKind, LexiconReport, LexiconSymbol,
};

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
    render_conceptual_analysis(report, &mut output);
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

fn render_conceptual_analysis(report: &LexiconReport, output: &mut String) {
    output.push_str(&format!(
        "\nConcept evidence sources ({})\n",
        report.conceptual_analysis.sources.len()
    ));
    if report.conceptual_analysis.sources.is_empty() {
        output.push_str("  none (local deterministic analysis only)\n");
    }
    let grammar = &report.conceptual_analysis.identifier_grammar;
    output.push_str(&format!(
        "\nIdentifier grammar\n- {}@{}: {} built-in + {} configured abbreviations; {} built-in + {} configured morphology rules; {} strategy\n",
        grammar.source_id,
        grammar.version,
        grammar.builtin_abbreviations,
        grammar.configured_abbreviations,
        grammar.builtin_morphology,
        grammar.configured_morphology,
        grammar.candidate_strategy
    ));
    for source in &report.conceptual_analysis.sources {
        let tier = match source.tier {
            LexiconProviderTier::Domain => "domain",
            LexiconProviderTier::General => "general corroboration",
        };
        output.push_str(&format!(
            "- {}@{} ({tier}, {}, {})\n  {} · {} indexed / {} supported / {} records\n  {} · {} · {}\n",
            source.id,
            source.version,
            resolve_provider_format_label(source.format),
            resolve_provider_coverage_label(source.coverage),
            source.sha256,
            source.relations_indexed,
            source.relations_loaded,
            source.records_read,
            source.license,
            source.attribution,
            source.url
        ));
    }

    output.push_str(&format!(
        "\nConcept candidates ({})\n",
        report.conceptual_analysis.candidates.len()
    ));
    if report.conceptual_analysis.candidates.is_empty() {
        output.push_str("  none\n");
    }
    for candidate in report.conceptual_analysis.candidates.iter().take(30) {
        render_candidate(candidate, output);
    }
    if report.conceptual_analysis.candidates.len() > 30 {
        output.push_str(&format!(
            "  … {} more candidates are available in JSON output\n",
            report.conceptual_analysis.candidates.len() - 30
        ));
    }

    output.push_str(&format!(
        "\nSuppressed concept candidates ({})\n",
        report.conceptual_analysis.suppressed_candidates.len()
    ));
    if report.conceptual_analysis.suppressed_candidates.is_empty() {
        output.push_str("  none\n");
    }
    for candidate in report
        .conceptual_analysis
        .suppressed_candidates
        .iter()
        .take(30)
    {
        output.push_str(&format!(
            "- {} / {} [{}]\n  {}\n",
            candidate.terms[0],
            candidate.terms[1],
            resolve_suppression_label(candidate.suppression.kind),
            candidate.suppression.reason
        ));
    }
    if report.conceptual_analysis.suppressed_candidates.len() > 30 {
        output.push_str(&format!(
            "  … {} more suppressions are available in JSON output\n",
            report.conceptual_analysis.suppressed_candidates.len() - 30
        ));
    }
}

fn render_candidate(candidate: &ConceptCandidate, output: &mut String) {
    output.push_str(&format!(
        "- {} / {} [{} · {}]\n  {}\n",
        candidate.terms[0],
        candidate.terms[1],
        resolve_candidate_rule_label(candidate.rule),
        resolve_confidence_label(candidate.confidence),
        candidate.reason
    ));
    if !candidate.preferred_terms.is_empty() {
        output.push_str(&format!(
            "  preferred: {}\n",
            candidate.preferred_terms.join(", ")
        ));
    }
    let evidence = candidate
        .evidence
        .iter()
        .map(|evidence| {
            format!(
                "{}@{}:{}({:?} -> {:?})",
                evidence.source_id,
                evidence.source_version,
                resolve_evidence_relation_label(evidence.relation),
                evidence.subject,
                evidence.object
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    output.push_str(&format!("  evidence: {evidence}\n"));
    if let Some(suppression) = &candidate.suggested_suppression {
        output.push_str(&format!(
            "  dismiss permanently: {} with a reason\n",
            suppression.config_key
        ));
    }
}

fn resolve_candidate_rule_label(rule: ConceptCandidateRule) -> &'static str {
    match rule {
        ConceptCandidateRule::ExactAlias => "exact alias",
        ConceptCandidateRule::RetiredTerm => "retired term",
        ConceptCandidateRule::ProgrammingGrammarVariant => "programming grammar",
        ConceptCandidateRule::DomainPreferentialEquivalent => "domain preference",
        ConceptCandidateRule::DomainRelatedEquivalent => "domain relation",
    }
}

fn resolve_confidence_label(confidence: ConceptCandidateConfidence) -> &'static str {
    match confidence {
        ConceptCandidateConfidence::Authoritative => "authoritative",
        ConceptCandidateConfidence::StrongAdvisory => "strong advisory",
        ConceptCandidateConfidence::CorroboratedAdvisory => "corroborated advisory",
        ConceptCandidateConfidence::Advisory => "advisory",
    }
}

fn resolve_suppression_label(kind: ConceptSuppressionKind) -> &'static str {
    match kind {
        ConceptSuppressionKind::DistinctFrom => "distinct from",
        ConceptSuppressionKind::NeverSuggest => "never suggest",
    }
}

fn resolve_provider_format_label(format: LexiconProviderFormat) -> &'static str {
    match format {
        LexiconProviderFormat::CsoCsv => "cso_csv",
        LexiconProviderFormat::RelationsJsonV1 => "relations_json_v1",
    }
}

fn resolve_provider_coverage_label(coverage: LexiconProviderCoverage) -> &'static str {
    match coverage {
        LexiconProviderCoverage::Complete => "complete",
        LexiconProviderCoverage::Filtered => "filtered",
    }
}

fn resolve_evidence_relation_label(relation: ConceptEvidenceRelation) -> &'static str {
    match relation {
        ConceptEvidenceRelation::ExactAlias => "exact_alias",
        ConceptEvidenceRelation::RetiredTerm => "retired_term",
        ConceptEvidenceRelation::CanonicalGrammar => "canonical_grammar",
        ConceptEvidenceRelation::MorphologicalVariant => "morphological_variant",
        ConceptEvidenceRelation::AbbreviationExpansion => "abbreviation_expansion",
        ConceptEvidenceRelation::CompatibleSymbolKind => "compatible_symbol_kind",
        ConceptEvidenceRelation::SharedCallableContract => "shared_callable_contract",
        ConceptEvidenceRelation::SharedCallableShape => "shared_callable_shape",
        ConceptEvidenceRelation::SharedStructuralShape => "shared_structural_shape",
        ConceptEvidenceRelation::PreferentialEquivalent => "preferential_equivalent",
        ConceptEvidenceRelation::RelatedEquivalent => "related_equivalent",
        ConceptEvidenceRelation::Synonym => "synonym",
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
    let location = symbol.span.as_ref().map_or_else(
        || symbol.file_path.clone(),
        |span| {
            format!(
                "{}:{}:{}",
                symbol.file_path, span.start_line, span.start_col
            )
        },
    );
    format!(
        "{indent}{location}:{}, {} ({exposure})\n",
        symbol.name, symbol.signature
    )
}

use crate::lexicon::{
    RepositoryLexiconReport, RepositoryRelationshipBasis, RepositoryRelationshipClaim,
};

const TEXT_RELATIONSHIP_LIMIT: usize = 100;
const TEXT_RELATIONSHIP_EVIDENCE_LIMIT: usize = 20;
const MAX_JSON_OUTPUT_BYTES: usize = 64 * 1024 * 1024;

pub(crate) fn render_text(report: &RepositoryLexiconReport) -> String {
    let mut output = String::from("CodeAtlas repository lexicon\n");
    output.push_str(&format!(
        "{} term evidence records · {} cross-subject relationships\n\n",
        report.terms.len(),
        report.relationships.len()
    ));
    output.push_str("Subjects\n");
    for subject in &report.subjects {
        output.push_str(&format!(
            "- {}: {} terms ({})\n",
            subject.subject.label(),
            subject.evidence_count,
            if subject.completeness.complete {
                "complete"
            } else {
                "partial"
            }
        ));
        for reason in &subject.completeness.reasons {
            output.push_str(&format!("  - {reason}\n"));
        }
    }

    output.push_str(&format!(
        "\nRelated evidence ({})\n",
        report.relationships.len()
    ));
    if report.relationships.is_empty() {
        output.push_str("  none\n");
    }
    for relationship in report.relationships.iter().take(TEXT_RELATIONSHIP_LIMIT) {
        let basis = match relationship.basis {
            RepositoryRelationshipBasis::ExactNormalizedTerm => "exact_normalized_term",
            RepositoryRelationshipBasis::DeclaredConcept => "declared_concept",
            RepositoryRelationshipBasis::PinnedDomainRelation => "pinned_domain_relation",
        };
        let claim = match relationship.claim {
            RepositoryRelationshipClaim::RelatedEvidence => "related_evidence",
        };
        output.push_str(&format!(
            "- [{}; {}] {} across {}\n",
            basis,
            claim,
            relationship.terms.join(" / "),
            relationship
                .subjects
                .iter()
                .map(|subject| subject.label())
                .collect::<Vec<_>>()
                .join(", ")
        ));
        for evidence in relationship
            .evidence
            .iter()
            .take(TEXT_RELATIONSHIP_EVIDENCE_LIMIT)
        {
            output.push_str(&format!(
                "  {} {}: {}\n",
                evidence.subject.label(),
                evidence.term,
                evidence.target
            ));
        }
        let displayed = TEXT_RELATIONSHIP_EVIDENCE_LIMIT.min(relationship.evidence.len());
        let retained_hidden = relationship.evidence.len().saturating_sub(displayed);
        if retained_hidden > 0 {
            output.push_str(&format!(
                "  … {retained_hidden} more retained evidence targets in JSON\n"
            ));
        }
        if relationship.omitted_evidence > 0 {
            output.push_str(&format!(
                "  … {} additional evidence targets omitted by the report bound\n",
                relationship.omitted_evidence
            ));
        }
    }
    if report.relationships.len() > TEXT_RELATIONSHIP_LIMIT {
        output.push_str(&format!(
            "  … {} more relationships are available in JSON output\n",
            report.relationships.len() - TEXT_RELATIONSHIP_LIMIT
        ));
    }
    output
}

pub(crate) fn render_json(report: &RepositoryLexiconReport) -> anyhow::Result<String> {
    let mut output = serde_json::to_string_pretty(report)?;
    output.push('\n');
    if output.len() > MAX_JSON_OUTPUT_BYTES {
        anyhow::bail!(
            "Repository lexicon JSON is {} bytes; the output limit is {MAX_JSON_OUTPUT_BYTES} bytes",
            output.len()
        );
    }
    Ok(output)
}

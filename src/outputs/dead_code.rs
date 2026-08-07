use crate::dead_code::{DeadCodeFinding, DeadCodeFindingKind, DeadCodeReport};
use crate::domain::source_graph::{
    AnalysisCompleteness, BoundaryKind, ContextRole, FindingConfidence,
};
use crate::domain::{EvidenceClass, SourceDisposition};
use anyhow::Result;
use std::collections::BTreeMap;

pub(crate) fn render_json(report: &DeadCodeReport) -> Result<String> {
    let mut report = report.clone();
    report.canonicalize();
    Ok(format!("{}\n", serde_json::to_string_pretty(&report)?))
}

pub(crate) fn render_text(report: &DeadCodeReport) -> String {
    let mut output = String::from("CodeAtlas dead-code report\n\n");
    let completeness_counts = report.projects.iter().fold(
        BTreeMap::<AnalysisCompleteness, usize>::new(),
        |mut counts, project| {
            *counts.entry(project.completeness).or_default() += 1;
            counts
        },
    );
    output.push_str(&format!(
        "Projects: {} ({} complete, {} partial, {} unsupported)\n",
        report.projects.len(),
        completeness_counts
            .get(&AnalysisCompleteness::Complete)
            .copied()
            .unwrap_or_default(),
        completeness_counts
            .get(&AnalysisCompleteness::Partial)
            .copied()
            .unwrap_or_default(),
        completeness_counts
            .get(&AnalysisCompleteness::Unsupported)
            .copied()
            .unwrap_or_default(),
    ));

    let evidence_counts = report.findings.iter().fold(
        BTreeMap::<EvidenceClass, usize>::new(),
        |mut counts, finding| {
            *counts.entry(finding.evidence_class).or_default() += 1;
            counts
        },
    );
    output.push_str(&format!(
        "Findings: {} ({} direct, {} inferred, {} boundary-limited)\n",
        report.findings.len(),
        evidence_counts
            .get(&EvidenceClass::Direct)
            .copied()
            .unwrap_or_default(),
        evidence_counts
            .get(&EvidenceClass::Inferred)
            .copied()
            .unwrap_or_default(),
        evidence_counts
            .get(&EvidenceClass::BoundaryLimited)
            .copied()
            .unwrap_or_default(),
    ));
    output.push_str(&format!(
        "Gates: {} finding, {} completeness\n",
        report.gate_count(),
        report.completeness_gate_count()
    ));

    let incomplete = report
        .projects
        .iter()
        .filter(|project| project.completeness != AnalysisCompleteness::Complete)
        .collect::<Vec<_>>();
    if !incomplete.is_empty() {
        output.push_str("\nIncomplete projects:\n");
        for project in incomplete {
            output.push_str(&format!(
                "- {} ({}): {}{}\n",
                project.project,
                project.root,
                completeness_name(project.completeness),
                if project.require_complete {
                    " [required]"
                } else {
                    ""
                }
            ));
            for reason in &project.completeness_reasons {
                output.push_str(&format!(
                    "  - {} at {}: {}\n",
                    boundary_kind_name(reason.kind),
                    reason.evidence.path,
                    reason.message
                ));
            }
        }
    }

    let gates = report
        .findings
        .iter()
        .filter(|finding| finding.gates)
        .collect::<Vec<_>>();
    output.push_str("\nGating findings:\n");
    if gates.is_empty() {
        output.push_str("No gating findings.\n");
    } else {
        for finding in gates {
            render_finding(&mut output, finding);
        }
    }

    let advisories = report
        .findings
        .iter()
        .filter(|finding| !finding.gates)
        .fold(
            BTreeMap::<(EvidenceClass, SourceDisposition, DeadCodeFindingKind), usize>::new(),
            |mut counts, finding| {
                *counts
                    .entry((
                        finding.evidence_class,
                        finding.source_disposition,
                        finding.kind,
                    ))
                    .or_default() += 1;
                counts
            },
        );
    output.push_str("\nAdvisory triage:\n");
    if advisories.is_empty() {
        output.push_str("No advisories.\n");
    } else {
        for ((evidence_class, disposition, kind), count) in advisories {
            output.push_str(&format!(
                "- {} / {} / {}: {}\n",
                evidence_class_name(evidence_class),
                disposition_name(disposition),
                kind.as_str(),
                count
            ));
        }
        output.push_str("Use --format json for exact advisory evidence.\n");
    }

    output
}

fn render_finding(output: &mut String, finding: &DeadCodeFinding) {
    let symbol = finding
        .symbol
        .as_ref()
        .map(|symbol| format!("#{symbol}"))
        .unwrap_or_default();
    let contexts = if finding.contexts.is_empty() {
        "none".to_string()
    } else {
        finding.contexts.join(", ")
    };
    let roles = if finding.roles.is_empty() {
        "none".to_string()
    } else {
        finding
            .roles
            .iter()
            .map(|role| role_name(*role))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let roots = if finding.root_contexts.is_empty() {
        "none".to_string()
    } else {
        finding
            .root_contexts
            .iter()
            .map(|root| format!("{}:{}", root.context, root.root))
            .collect::<Vec<_>>()
            .join(", ")
    };
    output.push_str(&format!(
        "{} {}{} ({}, confidence: {}, evidence: {}, source: {}) [gate]\n  id: {}\n  node: {}\n  {}\n  contexts: {}; roots: {}; roles: {}\n",
        finding.kind.as_str(),
        finding.path,
        symbol,
        finding.project,
        confidence_name(finding.confidence),
        evidence_class_name(finding.evidence_class),
        disposition_name(finding.source_disposition),
        finding.id,
        finding
            .node_id
            .as_ref()
            .map(|node| node.0.as_str())
            .unwrap_or("none"),
        finding.message,
        contexts,
        roots,
        roles
    ));
}

fn completeness_name(value: AnalysisCompleteness) -> &'static str {
    match value {
        AnalysisCompleteness::Complete => "complete",
        AnalysisCompleteness::Partial => "partial",
        AnalysisCompleteness::Unsupported => "unsupported",
    }
}

fn confidence_name(value: FindingConfidence) -> &'static str {
    match value {
        FindingConfidence::High => "high",
        FindingConfidence::Medium => "medium",
        FindingConfidence::Low => "low",
    }
}

fn evidence_class_name(value: EvidenceClass) -> &'static str {
    match value {
        EvidenceClass::Direct => "direct",
        EvidenceClass::Inferred => "inferred",
        EvidenceClass::BoundaryLimited => "boundary_limited",
    }
}

fn disposition_name(value: SourceDisposition) -> &'static str {
    match value {
        SourceDisposition::Maintained => "maintained",
        SourceDisposition::Generated => "generated",
        SourceDisposition::Fixture => "fixture",
        SourceDisposition::Test => "test",
        SourceDisposition::Tooling => "tooling",
    }
}

fn boundary_kind_name(value: BoundaryKind) -> &'static str {
    match value {
        BoundaryKind::DynamicImport => "dynamic_import",
        BoundaryKind::Reflection => "reflection",
        BoundaryKind::MacroExpansion => "macro_expansion",
        BoundaryKind::ConditionalCompilation => "conditional_compilation",
        BoundaryKind::UnresolvedInternal => "unresolved_internal",
        BoundaryKind::UnsupportedDependency => "unsupported_dependency",
        BoundaryKind::UnsupportedSyntax => "unsupported_syntax",
    }
}

fn role_name(value: ContextRole) -> &'static str {
    match value {
        ContextRole::Production => "production",
        ContextRole::Test => "test",
        ContextRole::Tooling => "tooling",
    }
}

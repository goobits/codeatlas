use crate::dead_code::DeadCodeReport;
use crate::domain::source_graph::{AnalysisCompleteness, ContextRole, FindingConfidence};
use anyhow::Result;

pub(crate) fn render_json(report: &DeadCodeReport) -> Result<String> {
    let mut report = report.clone();
    report.canonicalize();
    Ok(format!("{}\n", serde_json::to_string_pretty(&report)?))
}

pub(crate) fn render_text(report: &DeadCodeReport) -> String {
    let mut output = String::from("CodeAtlas dead-code report\n\n");
    for project in &report.projects {
        output.push_str(&format!(
            "Project {} ({})\n  completeness: {}{}\n  files: {}\n  symbols: {}\n\n",
            project.project,
            project.root,
            completeness_name(project.completeness),
            if project.require_complete {
                " [required]"
            } else {
                ""
            },
            project.files,
            project.symbols
        ));
    }

    if report.findings.is_empty() {
        output.push_str("No findings.\n");
    }
    for finding in &report.findings {
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
        let gate = if finding.gates { " [gate]" } else { "" };
        output.push_str(&format!(
            "{} {}{} ({}, confidence: {}){}\n  id: {}\n  node: {}\n  {}\n  contexts: {}; roots: {}; roles: {}\n",
            finding.kind.as_str(),
            finding.path,
            symbol,
            finding.project,
            confidence_name(finding.confidence),
            gate,
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
    output.push_str(&format!(
        "\n{} finding(s), {} finding gate(s), {} completeness gate(s).\n",
        report.findings.len(),
        report.gate_count(),
        report.completeness_gate_count()
    ));
    output
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

fn role_name(value: ContextRole) -> &'static str {
    match value {
        ContextRole::Production => "production",
        ContextRole::Test => "test",
        ContextRole::Tooling => "tooling",
    }
}

use crate::testing::{
    ChangedPathResolution, DeclaredTestSubject, TestImpactEvidenceKind, TestWitnessStatus,
    TestingImpactReport, TestingInventoryReport, TestingWitnessReport,
};
use codeatlas_domain::source_graph::{AnalysisCompleteness, FindingConfidence};

const TEST_WITNESS_TEXT_DETAIL_LIMIT: usize = 200;

pub(crate) fn render_inventory(report: &TestingInventoryReport) -> String {
    let contexts = report
        .projects
        .iter()
        .map(|project| project.contexts.len())
        .sum::<usize>();
    let scripts = report
        .projects
        .iter()
        .map(|project| project.scripts.len())
        .sum::<usize>();
    let mut output = format!(
        "Testing inventory: {} project(s), {contexts} test context(s), {scripts} test script(s).\n",
        report.projects.len()
    );
    for project in &report.projects {
        output.push_str(&format!(
            "\n{} [{}] ({})\n",
            project.project,
            project.root,
            completeness_name(project.completeness)
        ));
        if project.contexts.is_empty() {
            output.push_str("  contexts: none\n");
        }
        for context in &project.contexts {
            output.push_str(&format!(
                "  context {}: {} root(s)\n",
                context.name,
                context.roots.len()
            ));
            for subject in &context.declared_subjects {
                let subject = match subject {
                    DeclaredTestSubject::Project { project, resolved } => {
                        format!("project:{project} (resolved: {resolved})")
                    }
                    DeclaredTestSubject::Source {
                        pattern,
                        matched_paths,
                    } => format!("source:{pattern} ({} match(es))", matched_paths.len()),
                };
                output.push_str(&format!("    subject {subject}\n"));
            }
        }
        if project.scripts.is_empty() {
            output.push_str("  scripts: none\n");
        }
        for script in &project.scripts {
            let runners = script
                .runners
                .iter()
                .map(|runner| runner.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            let mut traits = Vec::new();
            if script.no_op {
                traits.push("no-op");
            }
            if script.allows_empty {
                traits.push("allows empty");
            }
            let traits = if traits.is_empty() {
                String::new()
            } else {
                format!(", {}", traits.join(", "))
            };
            output.push_str(&format!(
                "  script {} [{}{}]: {}\n",
                script.name, runners, traits, script.command
            ));
        }
    }
    if !report.duplicate_scripts.is_empty() {
        output.push_str("\nDuplicate test commands:\n");
        for duplicate in &report.duplicate_scripts {
            let locations = duplicate
                .locations
                .iter()
                .map(|location| format!("{}:{}", location.project, location.script))
                .collect::<Vec<_>>()
                .join(", ");
            output.push_str(&format!("  {} -> {locations}\n", duplicate.command));
        }
    }
    output
}

pub(crate) fn render_impact(report: &TestingImpactReport) -> String {
    let mut output = format!(
        "Testing impact: {} changed path(s), {} affected project(s), selection {}.\n",
        report.changed.len(),
        report.projects.len(),
        if report.selection_complete {
            "complete"
        } else {
            "conservative"
        }
    );
    for changed in &report.changed {
        output.push_str(&format!(
            "  {} -> {}{}\n",
            changed.path,
            resolution_name(changed.resolution),
            changed
                .project
                .as_ref()
                .map_or(String::new(), |project| format!(" ({project})"))
        ));
    }
    for project in &report.projects {
        output.push_str(&format!(
            "\n{} [{}] ({})\n",
            project.project,
            project.root,
            confidence_name(project.confidence)
        ));
        if !project.scripts.is_empty() {
            output.push_str(&format!(
                "  scripts: {}\n",
                project
                    .scripts
                    .iter()
                    .map(|script| script.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        for context in &project.contexts {
            output.push_str(&format!("  context {}\n", context.name));
            for evidence in &context.evidence {
                let mut detail = evidence
                    .subject
                    .as_ref()
                    .map_or(String::new(), |subject| format!(" [{subject}]"));
                if let Some(root) = &evidence.witness_root {
                    detail.push_str(&format!(" via {root}"));
                }
                output.push_str(&format!(
                    "    {}: {}{}\n",
                    evidence_name(evidence.kind),
                    evidence.changed_path,
                    detail
                ));
            }
        }
    }
    output
}

pub(crate) fn render_witnesses(report: &TestingWitnessReport) -> String {
    let mut output = format!(
        "Testing witnesses: {} public symbol(s); {} witnessed, {} declared-only, {} unwitnessed, {} unknown.\n",
        report.summary.public_symbols,
        report.summary.witnessed,
        report.summary.declared_only,
        report.summary.unwitnessed,
        report.summary.unknown
    );
    let non_witnessed_count = report
        .public_api
        .iter()
        .filter(|witness| witness.status != TestWitnessStatus::Witnessed)
        .count();
    let shown_count = non_witnessed_count.min(TEST_WITNESS_TEXT_DETAIL_LIMIT);
    for witness in report
        .public_api
        .iter()
        .filter(|witness| witness.status != TestWitnessStatus::Witnessed)
        .take(TEST_WITNESS_TEXT_DETAIL_LIMIT)
    {
        output.push_str(&format!(
            "  {}#{} [{}; {}; {}]\n",
            witness.path,
            witness.symbol,
            witness_status_name(witness.status),
            confidence_name(witness.confidence),
            witness.project
        ));
        for observed in &witness.observed {
            output.push_str(&format!(
                "    observed {}:{} via {}\n",
                observed.test_project, observed.context, observed.root
            ));
        }
        for declared in &witness.declared {
            output.push_str(&format!(
                "    declared {}:{} [{}]\n",
                declared.test_project, declared.context, declared.subject
            ));
        }
    }
    if report.summary.witnessed > 0 || shown_count < non_witnessed_count {
        output.push_str(&format!(
            "\nText detail: {shown_count}/{non_witnessed_count} non-witnessed symbol(s); {} witnessed symbol detail(s) omitted.\nUse --format json for complete witness evidence.\n",
            report.summary.witnessed
        ));
    }
    if !report.detached_contexts.is_empty() {
        output.push_str(&format!(
            "\nDetached test contexts ({}):\n",
            report.detached_contexts.len()
        ));
        for context in &report.detached_contexts {
            output.push_str(&format!(
                "  {}:{} ({} root(s))\n",
                context.project,
                context.context,
                context.roots.len()
            ));
        }
    }
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
        FindingConfidence::High => "high confidence",
        FindingConfidence::Medium => "medium confidence",
        FindingConfidence::Low => "low confidence",
    }
}

fn resolution_name(value: ChangedPathResolution) -> &'static str {
    match value {
        ChangedPathResolution::ExactSource => "exact source",
        ChangedPathResolution::ProjectFallback => "project fallback",
        ChangedPathResolution::WorkspaceFallback => "workspace fallback",
    }
}

fn evidence_name(value: TestImpactEvidenceKind) -> &'static str {
    match value {
        TestImpactEvidenceKind::ObservedDependency => "observed dependency",
        TestImpactEvidenceKind::DeclaredProject => "declared project",
        TestImpactEvidenceKind::DeclaredSource => "declared source",
        TestImpactEvidenceKind::ProjectFallback => "project fallback",
        TestImpactEvidenceKind::WorkspaceFallback => "workspace fallback",
    }
}

fn witness_status_name(value: TestWitnessStatus) -> &'static str {
    match value {
        TestWitnessStatus::Witnessed => "witnessed",
        TestWitnessStatus::DeclaredOnly => "declared only",
        TestWitnessStatus::Unwitnessed => "unwitnessed",
        TestWitnessStatus::Unknown => "unknown",
    }
}

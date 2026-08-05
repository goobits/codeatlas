use crate::http::{HttpUsageClassification, HttpUsageReport};
use crate::postgres::{PostgresUsageClassification, PostgresUsageReport};

pub(crate) fn render_http(report: &HttpUsageReport) -> String {
    let mut output = String::from("CodeAtlas HTTP usage report\n\n");
    let contracts = report
        .members
        .iter()
        .map(|member| member.contracts.len())
        .sum::<usize>();
    let operations = report
        .members
        .iter()
        .flat_map(|member| &member.contracts)
        .flat_map(|contract| &contract.operations)
        .collect::<Vec<_>>();
    let known = operations
        .iter()
        .filter(|operation| operation.classification == HttpUsageClassification::KnownRepository)
        .count();
    let declared_external = operations
        .iter()
        .filter(|operation| operation.classification == HttpUsageClassification::DeclaredExternal)
        .count();
    let no_known = operations.len().saturating_sub(known + declared_external);
    output.push_str(&format!(
        "Members: {}\nContracts: {contracts}\nOperations: {} ({known} known repository consumer, {declared_external} declared external, {no_known} no known repository consumer)\n",
        report.members.len(),
        operations.len()
    ));
    for member in &report.members {
        for contract in &member.contracts {
            output.push_str(&format!("\n{} :: {}\n", member.project, contract.id));
            for operation in &contract.operations {
                output.push_str(&format!(
                    "- {}: {} ({} evidence item{})\n",
                    operation.key,
                    http_classification(operation.classification),
                    operation.consumers.len(),
                    if operation.consumers.len() == 1 {
                        ""
                    } else {
                        "s"
                    }
                ));
            }
            for reason in &contract.completeness.reasons {
                output.push_str(&format!("  completeness: {reason}\n"));
            }
        }
    }
    output
}

pub(crate) fn render_postgres(report: &PostgresUsageReport) -> String {
    let mut output = String::from("CodeAtlas PostgreSQL usage report\n\n");
    let contracts = report
        .members
        .iter()
        .map(|member| member.contracts.len())
        .sum::<usize>();
    let objects = report
        .members
        .iter()
        .flat_map(|member| &member.contracts)
        .flat_map(|contract| &contract.objects)
        .collect::<Vec<_>>();
    let known = objects
        .iter()
        .filter(|object| {
            object.classification == PostgresUsageClassification::KnownStaticQueryTouch
        })
        .count();
    output.push_str(&format!(
        "Members: {}\nContracts: {contracts}\nStatic schema objects: {} ({known} known static query touch, {} no known static query touch)\n",
        report.members.len(),
        objects.len(),
        objects.len().saturating_sub(known)
    ));
    for member in &report.members {
        for contract in &member.contracts {
            output.push_str(&format!("\n{} :: {}\n", member.project, contract.id));
            for object in &contract.objects {
                output.push_str(&format!(
                    "- {}: {}\n",
                    postgres_object(&object.object),
                    postgres_classification(object.classification)
                ));
            }
            for reason in &contract.completeness.reasons {
                output.push_str(&format!("  completeness: {reason}\n"));
            }
        }
    }
    output
}

fn http_classification(value: HttpUsageClassification) -> &'static str {
    match value {
        HttpUsageClassification::KnownRepository => "known repository consumer",
        HttpUsageClassification::DeclaredExternal => "declared external consumer",
        HttpUsageClassification::NoKnownRepository => "no known repository consumer",
    }
}

fn postgres_classification(value: PostgresUsageClassification) -> &'static str {
    match value {
        PostgresUsageClassification::KnownStaticQueryTouch => "known static query touch",
        PostgresUsageClassification::NoKnownStaticQueryTouch => "no known static query touch",
    }
}

fn postgres_object(value: &crate::postgres::PostgresUsageObjectIdentity) -> String {
    match value.kind {
        crate::postgres::PostgresObjectKind::Table => value.schema.as_ref().map_or_else(
            || value.name.clone(),
            |schema| format!("{schema}.{}", value.name),
        ),
        crate::postgres::PostgresObjectKind::Column => {
            let relation = value.relation.as_deref().unwrap_or("?");
            value.schema.as_ref().map_or_else(
                || format!("{relation}.{}", value.name),
                |schema| format!("{schema}.{relation}.{}", value.name),
            )
        }
    }
}

use crate::{validate_document_schema, Diagnostic};
use serde_json::Value;

pub fn validate_conformance_with_observation(
    conformance: &Value,
    observation: &Value,
) -> Vec<Diagnostic> {
    let mut diagnostics = validate_document_schema(conformance);
    diagnostics.extend(validate_document_schema(observation));
    if !diagnostics.is_empty() {
        return diagnostics;
    }
    if conformance["metadata"]["generated"].as_bool() != Some(true) {
        diagnostics.push(Diagnostic::error(
            "generated.metadata-required",
            "ArchitectureConformance metadata.generated must be true",
        ));
    }
    if observation["metadata"]["generated"].as_bool() != Some(true) {
        diagnostics.push(Diagnostic::error(
            "generated.metadata-required",
            "ArchitectureObservation metadata.generated must be true",
        ));
    }

    let facts = observation["facts"]
        .as_object()
        .expect("schema validated observation facts");
    let coverage = observation["coverage"]
        .as_object()
        .expect("schema validated observation coverage");

    for (result_id, result) in conformance["results"]
        .as_object()
        .expect("schema validated conformance results")
    {
        let state = result["state"].as_str().expect("schema validated state");
        let severity = result["severity"]
            .as_str()
            .expect("schema validated severity");
        let fact_ids = result["evidence"]["factIds"]
            .as_array()
            .expect("schema validated fact IDs");
        let coverage_ids = result["evidence"]["coverageIds"]
            .as_array()
            .expect("schema validated coverage IDs");

        let deterministic = fact_ids.iter().all(|id| {
            id.as_str()
                .and_then(|id| facts.get(id))
                .is_some_and(|fact| fact["mode"].as_str() == Some("deterministic"))
        });
        let complete_coverage = !coverage_ids.is_empty()
            && coverage_ids.iter().all(|id| {
                id.as_str()
                    .and_then(|id| coverage.get(id))
                    .is_some_and(|coverage| coverage["status"].as_str() == Some("complete"))
            });

        if severity == "error"
            && matches!(state, "absent" | "conflicting" | "unexpected")
            && (!complete_coverage
                || (matches!(state, "conflicting" | "unexpected") && !deterministic))
        {
            diagnostics.push(Diagnostic::error(
                "conformance.hard-gate-without-complete-deterministic-evidence",
                format!("{result_id} cannot be an error without complete deterministic evidence"),
            ));
        }
        if state == "matched"
            && fact_ids.iter().any(|id| {
                id.as_str()
                    .and_then(|id| facts.get(id))
                    .is_some_and(|fact| fact["mode"].as_str() == Some("inferred"))
            })
        {
            diagnostics.push(Diagnostic::error(
                "conformance.inferred-evidence-cannot-match",
                format!("{result_id} reports inferred evidence as matched"),
            ));
        }
    }

    diagnostics.sort_by(|left, right| {
        left.code
            .cmp(&right.code)
            .then_with(|| left.message.cmp(&right.message))
    });
    diagnostics
}

#[cfg(test)]
mod tests {
    use super::validate_conformance_with_observation;
    use crate::{parse_restricted_yaml, ParseLimits};

    fn generated_examples() -> (serde_json::Value, serde_json::Value) {
        let observation = parse_restricted_yaml(
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../../../spec/architecture/v0.1/examples/observation/architecture-observation.generated.yaml"
            )),
            ParseLimits::default(),
        )
        .expect("observation")
        .value;
        let conformance = parse_restricted_yaml(
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../../../spec/architecture/v0.1/examples/conformance/architecture-conformance.generated.yaml"
            )),
            ParseLimits::default(),
        )
        .expect("conformance")
        .value;
        (observation, conformance)
    }

    #[test]
    fn generated_conformance_preserves_uncertainty() {
        let (observation, conformance) = generated_examples();
        let diagnostics = validate_conformance_with_observation(&conformance, &observation);
        assert!(diagnostics.is_empty(), "{diagnostics:#?}");
    }

    #[test]
    fn inferred_observations_cannot_create_hard_matches() {
        let (observation, mut conformance) = generated_examples();
        conformance["results"]["codeatlas.conformance.tab-host-candidate"]["state"] =
            serde_json::json!("matched");
        let diagnostics = validate_conformance_with_observation(&conformance, &observation);
        assert!(diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.code == "conformance.inferred-evidence-cannot-match" }));
    }

    #[test]
    fn unsupported_coverage_cannot_prove_absence() {
        let (mut observation, mut conformance) = generated_examples();
        observation["coverage"]["codeatlas.coverage.npm-packages"]["status"] =
            serde_json::json!("unsupported");
        conformance["results"]["codeatlas.conformance.tabby-package"]["state"] =
            serde_json::json!("absent");
        conformance["results"]["codeatlas.conformance.tabby-package"]["severity"] =
            serde_json::json!("error");
        let diagnostics = validate_conformance_with_observation(&conformance, &observation);
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "conformance.hard-gate-without-complete-deterministic-evidence"
        }));
    }
}

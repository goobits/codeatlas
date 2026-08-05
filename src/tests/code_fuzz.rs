use crate::domain::source_graph::SourceLanguage;
use crate::domain::{FuzzDirectiveIssueKind, FuzzPolicyEvidence, Symbol};
use std::collections::BTreeSet;
use std::path::Path;

const DENIAL_REASON: &str = "publishes to the real artifact registry";

#[test]
fn directive_attachment_has_rust_python_javascript_typescript_and_sql_parity() {
    let root = fixture_root();
    let rust = crate::languages::rust::parser::parse_module_info(
        &root.join("directives.rs"),
        &root,
        include_str!("../../tests/fixtures/code_fuzz/directives.rs"),
    )
    .expect("Rust directives");
    let python = crate::languages::python::parser::parse_module_info(
        &root.join("directives.py"),
        &root,
        include_str!("../../tests/fixtures/code_fuzz/directives.py"),
    )
    .expect("Python directives");
    let javascript = crate::languages::typescript::parser::parse_source(
        include_str!("../../tests/fixtures/code_fuzz/directives.js"),
        "directives.js",
    )
    .expect("JavaScript directives");
    let typescript = crate::languages::typescript::parser::parse_source(
        include_str!("../../tests/fixtures/code_fuzz/directives.ts"),
        "directives.ts",
    )
    .expect("TypeScript directives");

    let project = crate::config::ProjectConfig::load(&root, Some(&root.join("codeatlas.json")))
        .expect("code fuzz fixture config");
    let postgres = crate::postgres::inventory(&project).expect("SQL directives");
    let sql = &postgres.contracts[0].queries;
    assert_eq!(
        postgres.contracts[0]
            .diagnostics
            .iter()
            .filter(|finding| finding.code == "fuzz-directive-invalid" && finding.gates)
            .count(),
        1,
        "the SQL convenience must fail closed on stale allow directives"
    );

    let cases = [
        ("rust", 2, policy(&rust.symbols, "publish")),
        ("python", 2, policy(&python.symbols, "publish")),
        ("javascript", 1, policy(&javascript.symbols, "publish")),
        ("typescript", 2, policy(&typescript.symbols, "publish")),
        (
            "sql",
            1,
            sql.iter()
                .find_map(|query| {
                    query
                        .fuzz_policy
                        .as_ref()
                        .filter(|policy| policy.denial.is_some())
                })
                .expect("SQL denial"),
        ),
    ];
    for (adapter, line, policy) in cases {
        assert_eq!(
            policy.denial.as_ref().map(|denial| denial.reason.as_str()),
            Some(DENIAL_REASON),
            "{adapter}"
        );
        assert_eq!(policy.denial.as_ref().map(|denial| denial.line), Some(line));
        assert!(policy.issues.is_empty(), "{adapter}");
    }

    let malformed = [
        ("rust", 5, policy(&rust.symbols, "stale_allow")),
        ("python", 7, policy(&python.symbols, "stale_allow")),
        ("javascript", 6, policy(&javascript.symbols, "staleAllow")),
        ("typescript", 5, policy(&typescript.symbols, "staleAllow")),
        (
            "sql",
            1,
            sql.iter()
                .find_map(|query| {
                    query
                        .fuzz_policy
                        .as_ref()
                        .filter(|policy| !policy.issues.is_empty())
                })
                .expect("SQL malformed directive"),
        ),
    ];
    for (adapter, line, policy) in malformed {
        assert!(policy.denial.is_none(), "{adapter}");
        assert_eq!(
            policy.issues[0].kind,
            FuzzDirectiveIssueKind::UnsupportedAction,
            "{adapter}"
        );
        assert_eq!(policy.issues[0].line, line, "{adapter}");
    }

    for query in sql {
        let Some(policy) = &query.fuzz_policy else {
            continue;
        };
        let evidence = serde_json::to_value(query).expect("query policy evidence");
        assert_eq!(evidence["eligibility"], "blocked");
        let expected = if policy.denial.is_some() {
            "blocked_by_policy"
        } else {
            "malformed_fuzz_directive"
        };
        assert!(evidence["eligibilityReasons"]
            .as_array()
            .expect("eligibility reasons")
            .iter()
            .any(|reason| reason["code"] == expected));
    }
}

#[test]
fn public_callable_inventory_has_no_silent_omissions_and_policy_only_subtracts() {
    let root = fixture_root();
    let project = crate::config::ProjectConfig::load(&root, Some(&root.join("codeatlas.json")))
        .expect("code fuzz fixture config");
    let projects = project.analysis_projects().expect("analysis projects");
    let graph = crate::languages::reachability::build_source_graph(&projects)
        .expect("fixture source graph");
    let inventory =
        crate::fuzz::code::build_inventory(&graph, &[], project.config.fuzz.limits.max_cases)
            .expect("fuzzability inventory");

    let discovered = inventory
        .contracts
        .iter()
        .map(|contract| {
            (
                contract.language,
                contract.path.clone(),
                contract.symbol.clone(),
            )
        })
        .collect::<BTreeSet<_>>();
    let expected = BTreeSet::from([
        (
            SourceLanguage::Rust,
            "directives.rs".to_string(),
            "ArtifactPublisher.publish".to_string(),
        ),
        (
            SourceLanguage::Rust,
            "directives.rs".to_string(),
            "ArtifactPublisher.stale_allow".to_string(),
        ),
        (
            SourceLanguage::Python,
            "directives.py".to_string(),
            "publish".to_string(),
        ),
        (
            SourceLanguage::Python,
            "directives.py".to_string(),
            "stale_allow".to_string(),
        ),
        (
            SourceLanguage::JavaScript,
            "directives.js".to_string(),
            "publish".to_string(),
        ),
        (
            SourceLanguage::JavaScript,
            "directives.js".to_string(),
            "staleAllow".to_string(),
        ),
        (
            SourceLanguage::TypeScript,
            "directives.ts".to_string(),
            "ArtifactPublisher.publish".to_string(),
        ),
        (
            SourceLanguage::TypeScript,
            "directives.ts".to_string(),
            "ArtifactPublisher.staleAllow".to_string(),
        ),
    ]);
    assert_eq!(discovered, expected, "public callable inventory drifted");
    assert!(inventory.contracts.iter().all(|contract| {
        !contract.callable.signatures.is_empty()
            && contract.signatures.len() == contract.callable.signatures.len()
            && contract
                .signatures
                .iter()
                .all(|signature| !signature.deterministic_cases.is_empty())
    }));
    let source_denials = inventory
        .contracts
        .iter()
        .filter(|contract| {
            contract
                .source_policy
                .as_ref()
                .is_some_and(|policy| policy.denial.is_some())
        })
        .collect::<Vec<_>>();
    assert_eq!(source_denials.len(), 4);
    assert!(source_denials.iter().all(|contract| {
        contract.fuzz_block_reasons.iter().any(|reason| {
            reason.kind == crate::fuzz::code::CodeFuzzBlockKind::BlockedByPolicy
                && reason.subject == "source_directive"
        })
    }));
    let malformed_policies = inventory
        .contracts
        .iter()
        .filter(|contract| {
            contract
                .source_policy
                .as_ref()
                .is_some_and(|policy| !policy.issues.is_empty())
        })
        .collect::<Vec<_>>();
    assert_eq!(malformed_policies.len(), 4);
    assert!(malformed_policies.iter().all(|contract| {
        contract.fuzz_block_reasons.iter().any(|reason| {
            reason.kind == crate::fuzz::code::CodeFuzzBlockKind::MalformedDirective
                && reason.subject == "source_directive"
        })
    }));

    let excluded = crate::fuzz::code::build_inventory(
        &graph,
        &["directives.rs#ArtifactPublisher.publish".to_string()],
        project.config.fuzz.limits.max_cases,
    )
    .expect("exact config exclusion");
    let rust_publish = excluded
        .contracts
        .iter()
        .find(|contract| {
            contract.language == SourceLanguage::Rust
                && contract.symbol == "ArtifactPublisher.publish"
        })
        .expect("Rust publish contract");
    assert!(rust_publish.fuzz_block_reasons.iter().any(|reason| {
        reason.kind == crate::fuzz::code::CodeFuzzBlockKind::BlockedByPolicy
            && reason.subject == "config"
    }));

    let usage = crate::dead_code::analyze(&graph).expect("usage evidence");
    assert!(usage.findings.iter().all(|finding| {
        finding.kind != crate::dead_code::DeadCodeFindingKind::MalformedFuzzDirective
    }));

    let report = crate::dead_code::analyze_check(&graph).expect("directive findings");
    let malformed = report
        .findings
        .iter()
        .filter(|finding| {
            finding.kind == crate::dead_code::DeadCodeFindingKind::MalformedFuzzDirective
        })
        .collect::<Vec<_>>();
    assert_eq!(malformed.len(), 4);
    assert!(malformed.iter().all(|finding| finding.gates));
}

fn fixture_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/code_fuzz")
}

fn policy<'a>(symbols: &'a [Symbol], name: &str) -> &'a FuzzPolicyEvidence {
    symbols
        .iter()
        .find_map(|symbol| find_symbol(symbol, name))
        .and_then(|symbol| symbol.fuzz_policy.as_ref())
        .unwrap_or_else(|| panic!("missing fuzz policy for {name}"))
}

fn find_symbol<'a>(symbol: &'a Symbol, name: &str) -> Option<&'a Symbol> {
    (symbol.name == name).then_some(symbol).or_else(|| {
        symbol
            .children
            .iter()
            .find_map(|child| find_symbol(child, name))
    })
}

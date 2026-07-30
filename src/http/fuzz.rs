use crate::config::{HttpFuzzPositiveCoverageConfig, ResolvedHttpFuzzTarget};
use crate::http::fuzz_report;
use crate::http::model::HttpFuzzTotals;
use crate::http::request_adapter;
use crate::http::runtime::OwnedHttpServer;
use crate::http::toolchain::{cache_base, ensure_schemathesis, SCHEMATHESIS_VERSION};
use anyhow::{Context, Result};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const SCHEMATHESIS_CONFIG_FILENAME: &str = "schemathesis.toml";
const STATEFUL_CONFIG: &str = "\
[phases.stateful]
link-calibration = false

[phases.stateful.inference]
algorithms = []
";
const CHECKS: &[&str] = &[
    "not_a_server_error",
    "status_code_conformance",
    "content_type_conformance",
    "response_headers_conformance",
    "response_schema_conformance",
    "codeatlas_negative_data_rejection",
    "missing_required_header",
    "unsupported_method",
    "codeatlas_auth_rejection",
];
const STATEFUL_CHECKS: &[&str] = &["use_after_free", "ensure_resource_availability"];

pub(crate) struct RunOptions<'a> {
    pub max_examples: u32,
    pub profile: &'a str,
    pub stateful: bool,
    pub seed: Option<u128>,
    pub operation: Option<&'a str>,
    pub schemathesis: Option<&'a Path>,
}

pub(crate) fn run(target: &ResolvedHttpFuzzTarget, options: &RunOptions<'_>) -> Result<i32> {
    if options.max_examples == 0 {
        anyhow::bail!("Schemathesis max examples must be greater than zero");
    }
    let schemathesis = ensure_schemathesis(options.schemathesis)?;
    let report_dir = prepare_report_dir(target, options.profile)?;
    let config_path = prepare_schemathesis_config(&report_dir, options.stateful)?;
    let seed = options.seed.unwrap_or_else(generate_seed);
    let operation = options.operation.map(parse_operation).transpose()?;
    let args = schemathesis_args(
        target,
        options,
        seed,
        operation.as_ref(),
        &config_path,
        &report_dir,
    );
    let hooks = request_adapter::prepare(target)?;
    request_adapter::validate(&schemathesis, &hooks)?;
    let _server = OwnedHttpServer::start(target)?;
    let mut command = Command::new(&schemathesis);
    command
        .args(&args)
        .current_dir(&target.project_root)
        .envs(&target.environment);
    command
        .env("SCHEMATHESIS_HOOKS", &hooks.hook_path)
        .env(request_adapter::CONFIG_ENVIRONMENT_VARIABLE, &hooks.config);
    let status = match command.status() {
        Ok(status) => status,
        Err(error) => {
            fuzz_report::discard_raw_evidence(&report_dir);
            return Err(error).with_context(|| {
                format!("Could not start Schemathesis at {}", schemathesis.display())
            });
        }
    };
    fuzz_report::sanitize_events(
        &report_dir,
        target
            .headers
            .iter()
            .map(|header| (header.name.as_str(), header.value.as_str())),
    )?;
    let mut code = status.code().unwrap_or(1);
    println!("Replay this run by adding `--seed {seed}` to the same CodeAtlas command.");
    let event_path = report_dir.join(fuzz_report::EVENTS_FILENAME);
    match fuzz_report::summarize(&event_path, &target.id, &target.contract, options.profile) {
        Ok(summary) => {
            let summary_path = fuzz_report::write(&report_dir, &summary)?;
            println!(
                "CodeAtlas HTTP fuzz summary: {}/{} operations observed a positive success; {} client-error-only, {} authentication-rejection-only, {} mixed-without-success, and {} without positive cases; {} negative rejections ({}).",
                summary.totals.success_observed_operations,
                summary.totals.operations,
                summary.totals.client_error_only_operations,
                summary.totals.authentication_rejection_only_operations,
                summary.totals.mixed_without_success_operations,
                summary.totals.no_positive_case_operations,
                summary.totals.negative_rejections,
                summary_path.display()
            );
            if !options.stateful && options.operation.is_none() {
                for failure in
                    positive_coverage_failures(&target.positive_coverage, &summary.totals)
                {
                    eprintln!("CodeAtlas positive coverage policy failed: {failure}");
                    code = 1;
                }
            }
            if options.stateful {
                if let Some(stateful) = &summary.stateful {
                    println!(
                        "CodeAtlas stateful coverage: {}/{} selected API links across {}/{} successful scenarios.",
                        stateful.links_covered,
                        stateful.links_selected,
                        stateful.successful_scenarios,
                        stateful.scenarios
                    );
                    if stateful.links_covered < stateful.links_selected {
                        eprintln!(
                            "CodeAtlas stateful coverage is incomplete: {}/{} selected API links were exercised.",
                            stateful.links_covered, stateful.links_selected
                        );
                        code = 1;
                    }
                    if stateful.links_selected == 0 {
                        eprintln!(
                            "CodeAtlas stateful profile requires explicit OpenAPI links, but none were selected."
                        );
                        code = 1;
                    }
                } else {
                    eprintln!(
                        "CodeAtlas could not find stateful coverage in the Schemathesis report."
                    );
                    code = 1;
                }
            }
            fuzz_report::write_junit(&report_dir, &summary, code != 0)?;
        }
        Err(error) if code != 0 => {
            eprintln!("CodeAtlas could not summarize the failed Schemathesis run: {error:#}");
        }
        Err(error) => return Err(error),
    }
    if code == 0 {
        println!(
            "CodeAtlas HTTP fuzz target {} ({}) passed with Schemathesis {}: {} examples/operation ({}).",
            target.id,
            target.contract,
            SCHEMATHESIS_VERSION,
            options.max_examples,
            options.profile
        );
    }
    Ok(code)
}

fn schemathesis_args(
    target: &ResolvedHttpFuzzTarget,
    options: &RunOptions<'_>,
    seed: u128,
    operation: Option<&OperationFilter>,
    config_path: &Path,
    report_dir: &Path,
) -> Vec<OsString> {
    let mut args = vec![
        "--no-color".into(),
        "--config-file".into(),
        config_path.as_os_str().to_owned(),
        "run".into(),
        target.openapi_url.clone().into(),
        "--url".into(),
        target.base_url.clone().into(),
    ];
    args.extend([
        "--checks".into(),
        checks(options.stateful).into(),
        "--mode".into(),
        "all".into(),
        "--phases".into(),
        phases(options.stateful).into(),
        "--max-examples".into(),
        options.max_examples.to_string().into(),
        "--seed".into(),
        seed.to_string().into(),
        "--workers".into(),
        "1".into(),
        "--generation-database".into(),
        ":memory:".into(),
        "--generation-unique-inputs".into(),
        "--generation-with-security-parameters".into(),
        "false".into(),
        "--request-timeout".into(),
        "5".into(),
        "--max-failures".into(),
        "5".into(),
        "--wait-for-schema".into(),
        "30".into(),
    ]);
    if let Some(operation) = operation {
        args.extend(["--include-name".into(), operation.name.clone().into()]);
    }
    if !target.suppress_health_checks.is_empty() {
        args.push("--suppress-health-check".into());
        args.push(
            target
                .suppress_health_checks
                .iter()
                .map(|check| check.as_str())
                .collect::<Vec<_>>()
                .join(",")
                .into(),
        );
    }
    if target.suppress_warnings {
        args.extend(["--warnings".into(), "off".into()]);
    }
    args.extend([
        "--report".into(),
        "junit,ndjson".into(),
        "--report-dir".into(),
        report_dir.as_os_str().to_owned(),
        "--report-junit-path".into(),
        report_dir
            .join(fuzz_report::JUNIT_FILENAME)
            .into_os_string(),
        "--report-ndjson-path".into(),
        report_dir
            .join(fuzz_report::EVENTS_FILENAME)
            .into_os_string(),
    ]);
    args
}

fn checks(stateful: bool) -> String {
    CHECKS
        .iter()
        .chain(stateful.then_some(STATEFUL_CHECKS).into_iter().flatten())
        .copied()
        .collect::<Vec<_>>()
        .join(",")
}

fn phases(stateful: bool) -> &'static str {
    if stateful {
        "examples,stateful"
    } else {
        "examples,coverage,fuzzing"
    }
}

fn positive_coverage_failures(
    policy: &HttpFuzzPositiveCoverageConfig,
    totals: &HttpFuzzTotals,
) -> Vec<String> {
    let mut failures = Vec::new();
    if let Some(maximum) = policy.max_operations_without_success {
        if totals.operations_without_success > maximum {
            failures.push(format!(
                "{} operations had no positive success; configured maximum is {maximum}",
                totals.operations_without_success
            ));
        }
    }
    if let Some(maximum) = policy.max_authentication_rejection_only_operations {
        if totals.authentication_rejection_only_operations > maximum {
            failures.push(format!(
                "{} operations reached only authentication rejection; configured maximum is {maximum}",
                totals.authentication_rejection_only_operations
            ));
        }
    }
    failures
}

struct OperationFilter {
    name: String,
}

fn parse_operation(value: &str) -> Result<OperationFilter> {
    let Some((method, path)) = value.trim().split_once(' ') else {
        anyhow::bail!("HTTP operation must use the format `METHOD /path`");
    };
    if method.is_empty()
        || !method
            .bytes()
            .all(|byte| byte.is_ascii_alphabetic() || byte == b'-')
    {
        anyhow::bail!("HTTP operation method must contain only letters or `-`");
    }
    let path = path.trim();
    if !path.starts_with('/') || path.chars().any(char::is_whitespace) {
        anyhow::bail!("HTTP operation path must be absolute and contain no whitespace");
    }
    Ok(OperationFilter {
        name: format!("{} {path}", method.to_ascii_uppercase()),
    })
}

fn generate_seed() -> u128 {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    timestamp ^ (u128::from(std::process::id()) << 96)
}

fn prepare_report_dir(target: &ResolvedHttpFuzzTarget, profile: &str) -> Result<PathBuf> {
    let root = target
        .report_root
        .clone()
        .unwrap_or_else(|| cache_base().join("codeatlas").join("reports").join("http"));
    let report_dir = root.join(&target.id).join(profile);
    std::fs::create_dir_all(&report_dir).with_context(|| {
        format!(
            "Could not create CodeAtlas HTTP fuzz report directory {}",
            report_dir.display()
        )
    })?;
    let report_dir = report_dir.canonicalize().with_context(|| {
        format!(
            "Could not resolve CodeAtlas HTTP fuzz report directory {}",
            report_dir.display()
        )
    })?;
    fuzz_report::set_private_dir(&report_dir)?;
    clear_owned_report_files(&report_dir)?;
    Ok(report_dir)
}

fn clear_owned_report_files(report_dir: &Path) -> Result<()> {
    let sanitized_events = format!(".{}.sanitized", fuzz_report::EVENTS_FILENAME);
    for name in [
        SCHEMATHESIS_CONFIG_FILENAME,
        fuzz_report::EVENTS_FILENAME,
        fuzz_report::JUNIT_FILENAME,
        fuzz_report::SUMMARY_FILENAME,
        &sanitized_events,
    ] {
        let path = report_dir.join(name);
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "Could not clear CodeAtlas HTTP fuzz report file {}",
                        path.display()
                    )
                });
            }
        }
    }
    Ok(())
}

fn prepare_schemathesis_config(report_dir: &Path, stateful: bool) -> Result<PathBuf> {
    let path = report_dir.join(SCHEMATHESIS_CONFIG_FILENAME);
    fuzz_report::write_private(&path, schemathesis_config(stateful).as_bytes()).with_context(
        || {
            format!(
                "Could not write CodeAtlas Schemathesis configuration {}",
                path.display()
            )
        },
    )?;
    Ok(path)
}

fn schemathesis_config(stateful: bool) -> &'static str {
    if stateful {
        STATEFUL_CONFIG
    } else {
        ""
    }
}

#[cfg(test)]
mod tests {
    use super::{
        checks, clear_owned_report_files, parse_operation, phases, positive_coverage_failures,
        schemathesis_args, schemathesis_config, RunOptions, CHECKS, SCHEMATHESIS_CONFIG_FILENAME,
        STATEFUL_CONFIG,
    };
    use crate::config::{
        HttpFuzzHealthCheck, HttpFuzzPositiveCoverageConfig, ResolvedHttpFuzzHeader,
        ResolvedHttpFuzzTarget,
    };
    use crate::http::model::HttpFuzzTotals;
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn report_cleanup_removes_only_codeatlas_owned_files() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos();
        let report_dir = std::env::temp_dir().join(format!("codeatlas-owned-reports-{nonce}"));
        fs::create_dir(&report_dir).expect("report directory");
        let owned = [
            SCHEMATHESIS_CONFIG_FILENAME,
            super::fuzz_report::EVENTS_FILENAME,
            super::fuzz_report::JUNIT_FILENAME,
            super::fuzz_report::SUMMARY_FILENAME,
            ".events.ndjson.sanitized",
        ];
        for name in owned {
            fs::write(report_dir.join(name), "stale").expect("owned report");
        }
        fs::write(report_dir.join("keep.txt"), "unrelated").expect("unrelated file");

        clear_owned_report_files(&report_dir).expect("report cleanup");

        assert!(report_dir.join("keep.txt").exists());
        for name in owned {
            assert!(!report_dir.join(name).exists());
        }
        fs::remove_dir_all(report_dir).expect("test cleanup");
    }

    #[test]
    fn schemathesis_arguments_centralize_the_http_fuzz_policy() {
        let target = ResolvedHttpFuzzTarget {
            id: "api".to_string(),
            contract: "public-api".to_string(),
            base_url: "http://127.0.0.1:3443".to_string(),
            openapi_url: "http://127.0.0.1:3443/openapi.json".to_string(),
            environment: BTreeMap::new(),
            headers: vec![ResolvedHttpFuzzHeader {
                name: "Authorization".to_string(),
                value: "Bearer invalid".to_string(),
            }],
            project_root: PathBuf::from("."),
            report_root: None,
            server: None,
            request_adapter: None,
            positive_coverage: HttpFuzzPositiveCoverageConfig::default(),
            suppress_health_checks: vec![HttpFuzzHealthCheck::FilterTooMuch],
            suppress_warnings: true,
        };
        let options = RunOptions {
            max_examples: 75,
            profile: "standard",
            stateful: false,
            seed: Some(42),
            operation: Some("POST /widgets/{id}"),
            schemathesis: None,
        };
        let operation = parse_operation(options.operation.expect("operation")).expect("filter");
        let args = schemathesis_args(
            &target,
            &options,
            options.seed.expect("seed"),
            Some(&operation),
            Path::new("reports/schemathesis.toml"),
            Path::new("reports"),
        )
        .into_iter()
        .map(|argument| argument.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
        let checks = CHECKS.join(",");

        assert!(args
            .windows(2)
            .any(|pair| pair[0] == "--checks" && pair[1] == checks));
        assert!(args.windows(2).any(|pair| pair == ["--max-examples", "75"]));
        assert!(args.windows(2).any(|pair| pair == ["--seed", "42"]));
        assert!(args
            .windows(2)
            .any(|pair| pair == ["--include-name", "POST /widgets/{id}"]));
        assert!(args
            .windows(2)
            .any(|pair| { pair == ["--suppress-health-check", "filter_too_much"] }));
        assert!(args.windows(2).any(|pair| pair == ["--warnings", "off"]));
        assert!(args
            .windows(2)
            .any(|pair| pair == ["--phases", "examples,coverage,fuzzing"]));
        assert!(args
            .windows(2)
            .any(|pair| pair == ["--config-file", "reports/schemathesis.toml"]));
        assert!(!args.iter().any(|argument| argument == "--header"));
        assert!(!args
            .iter()
            .any(|argument| argument.contains("Bearer invalid")));
    }

    #[test]
    fn stateful_policy_is_explicit_and_adds_resource_checks() {
        assert_eq!(phases(true), "examples,stateful");
        assert!(STATEFUL_CONFIG.contains("algorithms = []"));
        assert!(STATEFUL_CONFIG.contains("link-calibration = false"));
        let stateful_checks = checks(true);
        assert!(stateful_checks.contains("use_after_free"));
        assert!(stateful_checks.contains("ensure_resource_availability"));
        assert!(stateful_checks.contains("codeatlas_auth_rejection"));
        assert!(checks(false).contains("codeatlas_auth_rejection"));
        assert!(CHECKS.contains(&"codeatlas_negative_data_rejection"));
        assert!(!CHECKS.contains(&"negative_data_rejection"));
    }

    #[test]
    fn managed_schemathesis_config_ignores_ambient_repository_configuration() {
        assert_eq!(schemathesis_config(false), "");
        assert_eq!(schemathesis_config(true), STATEFUL_CONFIG);
    }

    #[test]
    fn operation_filters_require_an_exact_method_and_absolute_path() {
        let filter = parse_operation("post /widgets/{id}").expect("valid filter");
        assert_eq!(filter.name, "POST /widgets/{id}");
        assert!(parse_operation("POST").is_err());
        assert!(parse_operation("POST widgets").is_err());
        assert!(parse_operation("POST /widget path").is_err());
    }

    #[test]
    fn positive_coverage_policy_gates_regressions_without_hiding_auth_failures() {
        let policy = HttpFuzzPositiveCoverageConfig {
            max_operations_without_success: Some(2),
            max_authentication_rejection_only_operations: Some(0),
        };
        let passing = HttpFuzzTotals {
            operations: 5,
            success_observed_operations: 3,
            operations_without_success: 2,
            ..HttpFuzzTotals::default()
        };
        assert!(positive_coverage_failures(&policy, &passing).is_empty());

        let failing = HttpFuzzTotals {
            operations: 5,
            success_observed_operations: 2,
            operations_without_success: 3,
            authentication_rejection_only_operations: 1,
            ..HttpFuzzTotals::default()
        };
        let failures = positive_coverage_failures(&policy, &failing);
        assert_eq!(failures.len(), 2);
        assert!(failures[0].contains("3 operations"));
        assert!(failures[1].contains("authentication rejection"));
    }
}

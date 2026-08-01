mod report;
mod request_adapter;
mod toolchain;

use self::toolchain::{ensure_schemathesis, SCHEMATHESIS_VERSION};
use super::environment::cache_base;
use super::target::{
    parse_http_fuzz_operation, HttpFuzzOperation, ResolvedHttpFuzzTarget,
    ResolvedHttpOpenApiSource, REQUEST_HOOK_CONFIG_ENV, SCHEMATHESIS_HOOKS_ENV,
};
use super::{openapi, provider, transport_schema};
use crate::config::HttpFuzzPositiveCoverageConfig;
use crate::http::model::{
    HttpFuzzContractMode, HttpFuzzOperationSummary, HttpFuzzTotals, HttpSourceCompleteness,
    HttpSourceInventory, HttpSourceOperationKind,
};
use crate::http::runtime::OwnedHttpServer;
use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    OnceLock,
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const SCHEMATHESIS_CONFIG_FILENAME: &str = "schemathesis.toml";
const PROVIDED_OPENAPI_FILENAME: &str = "provided-openapi.yaml";
const STATEFUL_CONFIG: &str = "\
[phases.coverage]
unexpected-methods = [\"get\", \"put\", \"post\", \"delete\", \"options\", \"patch\", \"trace\"]

[phases.stateful]
link-calibration = false

[phases.stateful.inference]
algorithms = []
";
const STANDARD_CONFIG: &str = "\
[phases.coverage]
unexpected-methods = [\"get\", \"put\", \"post\", \"delete\", \"options\", \"patch\", \"trace\"]
";
const CHECKS: &[&str] = &[
    "codeatlas_no_internal_server_error",
    "status_code_conformance",
    "content_type_conformance",
    "response_headers_conformance",
    "response_schema_conformance",
    "codeatlas_negative_data_rejection",
    "missing_required_header",
    "unsupported_method",
    "codeatlas_auth_rejection",
];
const SOURCE_TRANSPORT_CHECKS: &[&str] = &["not_a_server_error", "unsupported_method"];
const STATEFUL_CHECKS: &[&str] = &["use_after_free", "ensure_resource_availability"];
static INTERRUPTED: AtomicBool = AtomicBool::new(false);
static INTERRUPT_HANDLER: OnceLock<std::result::Result<(), String>> = OnceLock::new();

pub(crate) enum Contract {
    OpenApi {
        source: ResolvedHttpOpenApiSource,
        display: String,
    },
    SourceTransport(HttpSourceInventory),
}

impl Contract {
    fn mode(&self) -> HttpFuzzContractMode {
        match self {
            Self::OpenApi { .. } => HttpFuzzContractMode::OpenApi,
            Self::SourceTransport(_) => HttpFuzzContractMode::SourceTransport,
        }
    }
}

pub(crate) struct RunOptions<'a> {
    pub max_examples: u32,
    pub profile: &'a str,
    pub stateful: bool,
    pub seed: Option<u128>,
    pub operation: Option<&'a str>,
    pub schemathesis: Option<&'a Path>,
}

pub(crate) fn run(
    target: &ResolvedHttpFuzzTarget,
    contract: &Contract,
    options: &RunOptions<'_>,
) -> Result<i32> {
    if options.max_examples == 0 {
        anyhow::bail!("Schemathesis max examples must be greater than zero");
    }
    if options.stateful && matches!(contract, Contract::SourceTransport(_)) {
        anyhow::bail!(
            "Stateful HTTP fuzzing requires an explicit OpenAPI contract with declared links"
        );
    }
    let operation = options
        .operation
        .map(parse_http_fuzz_operation)
        .transpose()?;
    let report_dir = prepare_report_dir(target, options.profile, operation.as_ref())?;
    let (schema, available_operations, expected_non_success_operations) = match contract {
        Contract::OpenApi { source, display } => {
            let (document, openapi) = provider::read_with_inventory(source, display)?;
            let expected_non_success_operations =
                collect_expected_non_success_operations(&document, display)?;
            let operations = openapi
                .operations
                .iter()
                .map(|operation| HttpFuzzOperation {
                    name: operation.key.clone(),
                    method: operation.method.clone(),
                    path: operation.path.clone(),
                })
                .collect();
            let path = report_dir.join(PROVIDED_OPENAPI_FILENAME);
            report::write_private(&path, &document).with_context(|| {
                format!(
                    "Could not write provided OpenAPI contract {}",
                    path.display()
                )
            })?;
            (path, operations, expected_non_success_operations)
        }
        Contract::SourceTransport(source) => {
            if source.completeness == HttpSourceCompleteness::Partial {
                eprintln!(
                    "CodeAtlas source transport inventory is partial: {}",
                    source.reason
                );
            }
            println!(
                "CodeAtlas generated a source transport contract for {}. It checks route transport safety without claiming domain request, response, query, or authentication schemas.",
                target.contract
            );
            let operations = source
                .operations
                .iter()
                .filter(|operation| operation.kind == HttpSourceOperationKind::Endpoint)
                .map(|operation| parse_http_fuzz_operation(&operation.key))
                .collect::<Result<Vec<_>>>()?;
            (
                transport_schema::write(&report_dir, target, source)?,
                operations,
                BTreeSet::new(),
            )
        }
    };
    let selected_operations = select_operations(
        target,
        contract.mode(),
        &available_operations,
        operation.as_ref(),
    )?;
    let schemathesis = ensure_schemathesis(options.schemathesis)?;
    let hooks = request_adapter::prepare(target, &available_operations)?;
    request_adapter::validate(&schemathesis, &hooks)?;
    let config_path = prepare_schemathesis_config(&report_dir, options.stateful, &hooks.hook_path)?;
    let seed = options.seed.unwrap_or_else(generate_seed);
    let args = schemathesis_args(
        target,
        contract.mode(),
        options,
        seed,
        &selected_operations,
        &SchemathesisFiles {
            schema: &schema,
            config: &config_path,
            report_dir: &report_dir,
        },
    );
    install_interrupt_handler()?;
    INTERRUPTED.store(false, Ordering::SeqCst);
    let _server = OwnedHttpServer::start(target)?;
    let mut command = Command::new(&schemathesis);
    command
        .args(&args)
        .current_dir(&report_dir)
        .envs(&target.environment);
    command
        .env_remove(SCHEMATHESIS_HOOKS_ENV)
        .env(REQUEST_HOOK_CONFIG_ENV, hooks.config_path());
    let status = match interruptible_status(&mut command) {
        Ok(status) => status,
        Err(error) => {
            report::discard_raw_evidence(&report_dir);
            return Err(error).with_context(|| {
                format!("Could not run Schemathesis at {}", schemathesis.display())
            });
        }
    };
    report::sanitize_events(
        &report_dir,
        target
            .headers
            .iter()
            .map(|header| (header.name.as_str(), header.value.as_str())),
    )?;
    let mut code = status.code().unwrap_or(1);
    println!("Replay this run by adding `--seed {seed}` to the same CodeAtlas command.");
    let event_path = report_dir.join(report::EVENTS_FILENAME);
    match report::summarize(
        &event_path,
        &target.id,
        &target.contract,
        contract.mode(),
        options.profile,
        &expected_non_success_operations,
    ) {
        Ok(summary) => {
            let summary_path = report::write(&report_dir, &summary)?;
            println!(
                "CodeAtlas HTTP fuzz summary: {}/{} operations observed a positive success; {} expected non-success, {} client-error-only, {} authentication-rejection-only, {} mixed-without-success, and {} without positive cases; {} negative rejections ({}).",
                summary.totals.success_observed_operations,
                summary.totals.operations,
                summary.totals.expected_non_success_operations,
                summary.totals.client_error_only_operations,
                summary.totals.authentication_rejection_only_operations,
                summary.totals.mixed_without_success_operations,
                summary.totals.no_positive_case_operations,
                summary.totals.negative_rejections,
                summary_path.display()
            );
            if !options.stateful && (options.operation.is_none() || !target.operations.is_empty()) {
                for failure in
                    selected_operation_failures(&selected_operations, &summary.operations)
                {
                    eprintln!("CodeAtlas operation selection failed: {failure}");
                    code = 1;
                }
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
            report::write_junit(&report_dir, &summary, code != 0)?;
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

fn collect_expected_non_success_operations(
    document: &[u8],
    label: &str,
) -> Result<BTreeSet<String>> {
    let source = std::str::from_utf8(document)
        .with_context(|| format!("OpenAPI contract at {label} is not UTF-8"))?;
    let parsed = openapi::parse(source, label)?;
    Ok(parsed
        .operations
        .into_iter()
        .filter(|operation| {
            !operation
                .responses
                .iter()
                .any(|response| openapi::response_status_can_succeed(&response.status))
        })
        .map(|operation| operation.key)
        .collect())
}

fn install_interrupt_handler() -> Result<()> {
    let result = INTERRUPT_HANDLER.get_or_init(|| {
        ctrlc::set_handler(|| {
            INTERRUPTED.store(true, Ordering::SeqCst);
        })
        .map_err(|error| format!("Could not install the CodeAtlas interrupt handler: {error}"))
    });
    if let Err(error) = result {
        anyhow::bail!("{error}");
    }
    Ok(())
}

fn interruptible_status(command: &mut Command) -> Result<ExitStatus> {
    let mut child = command.spawn()?;
    loop {
        if INTERRUPTED.swap(false, Ordering::SeqCst) {
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!("CodeAtlas HTTP fuzz run interrupted");
        }
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn schemathesis_args(
    target: &ResolvedHttpFuzzTarget,
    contract_mode: HttpFuzzContractMode,
    options: &RunOptions<'_>,
    seed: u128,
    operations: &[HttpFuzzOperation],
    files: &SchemathesisFiles<'_>,
) -> Vec<OsString> {
    let mut args = vec![
        "--no-color".into(),
        "--config-file".into(),
        files.config.as_os_str().to_owned(),
        "run".into(),
        files.schema.as_os_str().to_owned(),
        "--url".into(),
        target.base_url.as_str().into(),
    ];
    args.extend([
        "--checks".into(),
        checks(contract_mode, options.stateful).into(),
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
        "30".into(),
        "--max-failures".into(),
        "5".into(),
        "--wait-for-schema".into(),
        "30".into(),
    ]);
    for operation in operations {
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
        files.report_dir.as_os_str().to_owned(),
        "--report-junit-path".into(),
        files
            .report_dir
            .join(report::JUNIT_FILENAME)
            .into_os_string(),
        "--report-ndjson-path".into(),
        files
            .report_dir
            .join(report::EVENTS_FILENAME)
            .into_os_string(),
    ]);
    args
}

struct SchemathesisFiles<'a> {
    schema: &'a Path,
    config: &'a Path,
    report_dir: &'a Path,
}

fn checks(contract_mode: HttpFuzzContractMode, stateful: bool) -> String {
    let checks = match contract_mode {
        HttpFuzzContractMode::OpenApi => CHECKS,
        HttpFuzzContractMode::SourceTransport => SOURCE_TRANSPORT_CHECKS,
    };
    checks
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

fn select_operations(
    target: &ResolvedHttpFuzzTarget,
    contract_mode: HttpFuzzContractMode,
    available: &[HttpFuzzOperation],
    requested: Option<&HttpFuzzOperation>,
) -> Result<Vec<HttpFuzzOperation>> {
    let available_names = available
        .iter()
        .map(|operation| operation.name.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    if contract_mode == HttpFuzzContractMode::SourceTransport && target.operations.is_empty() {
        anyhow::bail!(
            "Source-transport fuzz target {} needs a non-empty target-owned `operations` allowlist",
            target.id
        );
    }
    for operation in &target.operations {
        if !available_names.contains(operation.name.as_str()) {
            anyhow::bail!(
                "HTTP fuzz target {} selects unknown operation {}",
                target.id,
                operation.name
            );
        }
    }
    if let Some(requested) = requested {
        if !available_names.contains(requested.name.as_str()) {
            anyhow::bail!("Unknown HTTP operation {}", requested.name);
        }
        if !target.operations.is_empty()
            && !target
                .operations
                .iter()
                .any(|operation| operation.name == requested.name)
        {
            anyhow::bail!(
                "--operation can only narrow HTTP fuzz target {}'s configured allowlist; {} is not allowed",
                target.id,
                requested.name
            );
        }
        return Ok(vec![requested.clone()]);
    }
    Ok(target.operations.clone())
}

fn selected_operation_failures(
    selected: &[HttpFuzzOperation],
    observed: &[HttpFuzzOperationSummary],
) -> Vec<String> {
    let observed = observed
        .iter()
        .map(|operation| operation.operation.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    selected
        .iter()
        .filter(|operation| !observed.contains(operation.name.as_str()))
        .map(|operation| format!("{} produced no retained fuzz evidence", operation.name))
        .collect()
}

fn generate_seed() -> u128 {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    timestamp ^ (u128::from(std::process::id()) << 96)
}

fn operation_report_component(operation: &HttpFuzzOperation) -> String {
    let mut slug = operation
        .name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>();
    while slug.contains("--") {
        slug = slug.replace("--", "-");
    }
    let slug = slug.trim_matches('-').chars().take(72).collect::<String>();
    let digest = format!("{:x}", Sha256::digest(operation.name.as_bytes()));
    format!("{slug}-{}", &digest[..12])
}

fn prepare_report_dir(
    target: &ResolvedHttpFuzzTarget,
    profile: &str,
    operation: Option<&HttpFuzzOperation>,
) -> Result<PathBuf> {
    let root = target
        .report_root
        .clone()
        .unwrap_or_else(|| cache_base().join("codeatlas").join("reports").join("http"));
    let mut report_dir = root.join(&target.id).join(profile);
    if let Some(operation) = operation {
        report_dir = report_dir.join(operation_report_component(operation));
    }
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
    report::set_private_dir(&report_dir)?;
    clear_owned_report_files(&report_dir)?;
    Ok(report_dir)
}

fn clear_owned_report_files(report_dir: &Path) -> Result<()> {
    let sanitized_events = format!(".{}.sanitized", report::EVENTS_FILENAME);
    for name in [
        SCHEMATHESIS_CONFIG_FILENAME,
        PROVIDED_OPENAPI_FILENAME,
        report::EVENTS_FILENAME,
        report::JUNIT_FILENAME,
        report::SUMMARY_FILENAME,
        transport_schema::FILENAME,
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

fn prepare_schemathesis_config(
    report_dir: &Path,
    stateful: bool,
    hook_path: &Path,
) -> Result<PathBuf> {
    let path = report_dir.join(SCHEMATHESIS_CONFIG_FILENAME);
    let contents = render_schemathesis_config(stateful, hook_path)?;
    report::write_private(&path, contents.as_bytes()).with_context(|| {
        format!(
            "Could not write CodeAtlas Schemathesis configuration {}",
            path.display()
        )
    })?;
    Ok(path)
}

fn render_schemathesis_config(stateful: bool, hook_path: &Path) -> Result<String> {
    let hook_path = serde_json::to_string(&hook_path.to_string_lossy())?;
    Ok(format!(
        "hooks = {hook_path}\n\n{}",
        schemathesis_config(stateful)
    ))
}

fn schemathesis_config(stateful: bool) -> &'static str {
    if stateful {
        STATEFUL_CONFIG
    } else {
        STANDARD_CONFIG
    }
}

#[cfg(test)]
mod tests {
    use super::{
        checks, clear_owned_report_files, collect_expected_non_success_operations,
        operation_report_component, phases, positive_coverage_failures, render_schemathesis_config,
        schemathesis_args, schemathesis_config, select_operations, selected_operation_failures,
        RunOptions, SchemathesisFiles, CHECKS, PROVIDED_OPENAPI_FILENAME,
        SCHEMATHESIS_CONFIG_FILENAME, SOURCE_TRANSPORT_CHECKS, STATEFUL_CONFIG,
    };
    use crate::config::{HttpFuzzHealthCheck, HttpFuzzPositiveCoverageConfig};
    use crate::http::model::{
        HttpFuzzContractMode, HttpFuzzOperationSummary, HttpFuzzPositiveCoverage, HttpFuzzTotals,
    };
    use crate::http::target::{
        parse_http_fuzz_operation, ResolvedHttpFuzzHeader, ResolvedHttpFuzzTarget,
    };
    use std::collections::{BTreeMap, BTreeSet};
    use std::fs;
    use std::path::Path;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn target_with_operations(operations: &[&str]) -> ResolvedHttpFuzzTarget {
        ResolvedHttpFuzzTarget {
            id: "api".to_string(),
            contract: "public-api".to_string(),
            base_url: url::Url::parse("http://127.0.0.1:3443").expect("base URL"),
            openapi_url: url::Url::parse("http://127.0.0.1:3443/openapi.json")
                .expect("OpenAPI URL"),
            environment: BTreeMap::new(),
            headers: Vec::new(),
            report_root: None,
            server: None,
            request_adapter: None,
            operations: operations
                .iter()
                .map(|operation| parse_http_fuzz_operation(operation).expect("operation"))
                .collect(),
            positive_coverage: HttpFuzzPositiveCoverageConfig::default(),
            suppress_health_checks: Vec::new(),
            suppress_warnings: false,
        }
    }

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
            PROVIDED_OPENAPI_FILENAME,
            super::report::EVENTS_FILENAME,
            super::report::JUNIT_FILENAME,
            super::report::SUMMARY_FILENAME,
            super::transport_schema::FILENAME,
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
        let mut target = target_with_operations(&["GET /health", "POST /widgets/{id}"]);
        target.headers.push(ResolvedHttpFuzzHeader {
            name: "Authorization".to_string(),
            value: "Bearer invalid".to_string(),
        });
        target.suppress_health_checks = vec![HttpFuzzHealthCheck::FilterTooMuch];
        target.suppress_warnings = true;
        let options = RunOptions {
            max_examples: 75,
            profile: "standard",
            stateful: false,
            seed: Some(42),
            operation: Some("POST /widgets/{id}"),
            schemathesis: None,
        };
        let operation =
            parse_http_fuzz_operation(options.operation.expect("operation")).expect("filter");
        let args = schemathesis_args(
            &target,
            HttpFuzzContractMode::OpenApi,
            &options,
            options.seed.expect("seed"),
            std::slice::from_ref(&operation),
            &SchemathesisFiles {
                schema: Path::new("reports/provided-openapi.yaml"),
                config: Path::new("reports/schemathesis.toml"),
                report_dir: Path::new("reports"),
            },
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
            .any(|pair| pair == ["--request-timeout", "30"]));
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
        let stateful_checks = checks(HttpFuzzContractMode::OpenApi, true);
        assert!(stateful_checks.contains("use_after_free"));
        assert!(stateful_checks.contains("ensure_resource_availability"));
        assert!(stateful_checks.contains("codeatlas_auth_rejection"));
        assert!(checks(HttpFuzzContractMode::OpenApi, false).contains("codeatlas_auth_rejection"));
        assert!(CHECKS.contains(&"codeatlas_negative_data_rejection"));
        assert!(CHECKS.contains(&"codeatlas_no_internal_server_error"));
        assert!(!CHECKS.contains(&"not_a_server_error"));
        assert!(!CHECKS.contains(&"negative_data_rejection"));
        assert_eq!(
            checks(HttpFuzzContractMode::SourceTransport, false),
            SOURCE_TRANSPORT_CHECKS.join(",")
        );
        assert!(SOURCE_TRANSPORT_CHECKS.contains(&"not_a_server_error"));
    }

    #[test]
    fn managed_schemathesis_config_ignores_ambient_repository_configuration() {
        assert!(schemathesis_config(false).contains("unexpected-methods"));
        assert!(!schemathesis_config(false).contains("\"head\""));
        assert!(schemathesis_config(false).contains("\"trace\""));
        assert_eq!(schemathesis_config(true), STATEFUL_CONFIG);
        let rendered = render_schemathesis_config(false, Path::new("cache/hooks.py"))
            .expect("rendered Schemathesis config");
        assert!(rendered.starts_with("hooks = \"cache/hooks.py\""));
    }

    #[test]
    fn operation_filters_require_an_exact_method_and_absolute_path() {
        let filter = parse_http_fuzz_operation("post /widgets/{id}").expect("valid filter");
        assert_eq!(filter.name, "POST /widgets/{id}");
        let component = operation_report_component(&filter);
        assert!(component.starts_with("post-widgets-id-"));
        assert_eq!(component.len(), "post-widgets-id-".len() + 12);
        assert_ne!(
            component,
            operation_report_component(
                &parse_http_fuzz_operation("GET /widgets/{id}").expect("filter")
            )
        );
        assert!(parse_http_fuzz_operation("POST").is_err());
        assert!(parse_http_fuzz_operation("POST widgets").is_err());
        assert!(parse_http_fuzz_operation("POST /widget path").is_err());
    }

    #[test]
    fn target_operations_are_validated_and_cli_selection_only_narrows() {
        let target = target_with_operations(&["GET /health"]);
        let available = [
            parse_http_fuzz_operation("GET /health").expect("GET operation"),
            parse_http_fuzz_operation("POST /health").expect("POST operation"),
        ];
        assert_eq!(
            select_operations(
                &target,
                HttpFuzzContractMode::SourceTransport,
                &available,
                None,
            )
            .expect("target allowlist"),
            target.operations
        );
        let disallowed = parse_http_fuzz_operation("POST /health").expect("POST operation");
        let error = select_operations(
            &target,
            HttpFuzzContractMode::SourceTransport,
            &available,
            Some(&disallowed),
        )
        .expect_err("CLI selection must not expand the target")
        .to_string();
        assert!(error.contains("can only narrow"), "{error}");

        let empty = target_with_operations(&[]);
        assert!(select_operations(
            &empty,
            HttpFuzzContractMode::SourceTransport,
            &available,
            None,
        )
        .is_err());
    }

    #[test]
    fn selected_operations_must_produce_retained_evidence() {
        let selected = [parse_http_fuzz_operation("GET /health").expect("operation")];
        let observed = [HttpFuzzOperationSummary {
            operation: "POST /widgets".to_string(),
            positive_coverage: HttpFuzzPositiveCoverage::SuccessObserved,
            cases: 1,
            positive_cases: 1,
            positive_successes: 1,
            positive_auth_rejections: 0,
            positive_client_errors: 0,
            negative_cases: 0,
            negative_rejections: 0,
            server_errors: 0,
            check_failures: 0,
            observed_statuses: BTreeMap::new(),
        }];
        let failures = selected_operation_failures(&selected, &observed);
        assert_eq!(failures.len(), 1);
        assert!(failures[0].contains("GET /health"));
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

    #[test]
    fn infers_declared_non_success_operations() {
        let document = br#"{
          "openapi": "3.1.0",
          "info": {"title": "fixture", "version": "1"},
          "paths": {
            "/hidden": {"get": {"responses": {"404": {"description": "hidden"}}}},
            "/ready": {"get": {"responses": {"204": {"description": "ready"}}}},
            "/redirect": {"get": {"responses": {"3XX": {"description": "redirect"}}}},
            "/fallback": {"get": {"responses": {"default": {"description": "fallback"}}}}
          }
        }"#;

        assert_eq!(
            collect_expected_non_success_operations(document, "fixture")
                .expect("OpenAPI should parse"),
            BTreeSet::from(["GET /hidden".to_string()])
        );
    }
}

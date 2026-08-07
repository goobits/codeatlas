use crate::config::{
    HttpFuzzCommandConfig, HttpFuzzEnvironmentClassConfig, HttpFuzzHealthCheck,
    HttpFuzzOperationScopeConfig, HttpFuzzOperationSelectionConfig, HttpFuzzPositiveCoverageConfig,
    HttpFuzzServerConfig, ProjectConfig,
};
use crate::execution::CALL_CATEGORY_HEADER;
use anyhow::{Context, Result};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use url::Url;

pub(super) const REQUEST_HOOK_CONFIG_ENV: &str = "CODEATLAS_HTTP_REQUEST_ADAPTER_CONFIG";
pub(super) const SCHEMATHESIS_HOOKS_ENV: &str = "SCHEMATHESIS_HOOKS";

#[derive(Debug, Clone)]
pub(crate) struct ResolvedHttpContract {
    pub id: String,
    pub openapi: Option<PathBuf>,
    pub openapi_display: Option<String>,
    pub external_operations: Vec<String>,
    pub source_roots: Vec<PathBuf>,
    pub source_complete: bool,
    pub source_include_paths: Vec<String>,
    pub source_exclude_paths: Vec<String>,
    pub source_include_operations: Vec<String>,
    pub source_exclude_operations: Vec<String>,
    pub repository_root: PathBuf,
}

#[derive(Debug, Clone)]
pub(crate) struct ResolvedHttpFuzzTarget {
    pub id: String,
    pub contract: String,
    pub workload_image: Option<String>,
    pub base_url: Url,
    pub environment: BTreeMap<String, String>,
    pub secret_environment: BTreeMap<String, String>,
    pub headers: Vec<ResolvedHttpFuzzHeader>,
    pub environment_class: HttpFuzzEnvironmentClassConfig,
    pub preauthorized: bool,
    pub server: Option<ResolvedHttpFuzzServer>,
    pub request_adapter: Option<ResolvedHttpFuzzCommand>,
    pub operation_selection: ResolvedHttpFuzzOperationSelection,
    pub expected_non_success_operations: Vec<HttpFuzzOperation>,
    pub positive_coverage: HttpFuzzPositiveCoverageConfig,
    pub suppress_health_checks: Vec<HttpFuzzHealthCheck>,
    pub suppress_warnings: bool,
}

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub(crate) struct HttpFuzzOperation {
    pub name: String,
    pub method: String,
    pub path: String,
}

#[derive(Debug, Clone)]
pub(crate) enum ResolvedHttpFuzzOperationSelection {
    Contract,
    Explicit(Vec<HttpFuzzOperation>),
}

#[derive(Debug, Clone)]
pub(crate) struct ResolvedHttpFuzzHeader {
    pub name: String,
    pub value: Option<String>,
    pub value_reference: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct ResolvedHttpFuzzCommand {
    pub command: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
}

#[derive(Debug, Clone)]
pub(crate) struct ResolvedHttpFuzzServer {
    pub command: ResolvedHttpFuzzCommand,
    pub prepare: Vec<ResolvedHttpFuzzCommand>,
    pub startup_timeout_seconds: u64,
}

impl ResolvedHttpFuzzTarget {
    pub(crate) fn resolve_secret_environment(&self) -> Result<BTreeMap<String, String>> {
        let mut environment = BTreeMap::new();
        for (name, reference) in &self.secret_environment {
            let value = std::env::var(reference).with_context(|| {
                format!(
                    "HTTP fuzz target {} needs secret environment reference {} for {}",
                    self.id, reference, name
                )
            })?;
            if value.is_empty() {
                anyhow::bail!(
                    "HTTP fuzz target {} secret environment reference {} is empty",
                    self.id,
                    reference
                );
            }
            environment.insert(name.clone(), value);
        }
        Ok(environment)
    }

    pub(crate) fn resolve_runtime_headers(&self) -> Result<Vec<(String, String)>> {
        self.headers
            .iter()
            .map(|header| {
                let value = match (&header.value, &header.value_reference) {
                    (Some(value), None) => value.clone(),
                    (None, Some(reference)) => {
                        let value = std::env::var(reference).with_context(|| {
                            format!(
                                "HTTP fuzz target {} needs header secret reference {}",
                                self.id, reference
                            )
                        })?;
                        if value.is_empty() {
                            anyhow::bail!(
                                "HTTP fuzz target {} has an empty secret header reference for {}",
                                self.id,
                                header.name
                            );
                        }
                        value
                    }
                    _ => anyhow::bail!(
                        "HTTP fuzz target {} has an invalid resolved header source",
                        self.id
                    ),
                };
                Ok((header.name.clone(), value))
            })
            .collect()
    }
}

impl ProjectConfig {
    pub(crate) fn http_contracts(
        &self,
        openapi_overrides: &[PathBuf],
    ) -> Result<Vec<ResolvedHttpContract>> {
        if self.config.http.contracts.is_empty() {
            if openapi_overrides.is_empty() {
                return Ok(vec![ResolvedHttpContract {
                    id: "source".to_string(),
                    openapi: None,
                    openapi_display: None,
                    external_operations: Vec::new(),
                    source_roots: vec![self.root.clone()],
                    source_complete: false,
                    source_include_paths: Vec::new(),
                    source_exclude_paths: Vec::new(),
                    source_include_operations: Vec::new(),
                    source_exclude_operations: Vec::new(),
                    repository_root: self.root.clone(),
                }]);
            }
            let current_dir = std::env::current_dir()?;
            let mut ids = BTreeSet::new();
            return openapi_overrides
                .iter()
                .enumerate()
                .map(|(index, path)| {
                    let unresolved = if path.is_absolute() {
                        path.clone()
                    } else {
                        current_dir.join(path)
                    };
                    let openapi = absolute_existing(path, &current_dir, "OpenAPI contract")?;
                    let base = openapi
                        .file_stem()
                        .and_then(|name| name.to_str())
                        .filter(|name| !name.is_empty())
                        .unwrap_or("http");
                    let mut id = base.to_string();
                    if !ids.insert(id.clone()) {
                        id = format!("{base}-{}", index + 1);
                        ids.insert(id.clone());
                    }
                    Ok(ResolvedHttpContract {
                        id,
                        openapi: Some(openapi),
                        openapi_display: Some(codeatlas_source::paths::normalize_relative_path(
                            &unresolved,
                            &self.root,
                        )),
                        external_operations: Vec::new(),
                        source_roots: vec![self.root.clone()],
                        source_complete: false,
                        source_include_paths: Vec::new(),
                        source_exclude_paths: Vec::new(),
                        source_include_operations: Vec::new(),
                        source_exclude_operations: Vec::new(),
                        repository_root: self.root.clone(),
                    })
                })
                .collect();
        }

        if !openapi_overrides.is_empty()
            && openapi_overrides.len() != self.config.http.contracts.len()
        {
            anyhow::bail!(
                "Received {} --openapi override(s) for {} configured HTTP contract(s); provide one per contract in config order.",
                openapi_overrides.len(),
                self.config.http.contracts.len()
            );
        }

        let current_dir = std::env::current_dir()?;
        let mut ids = BTreeSet::new();
        self.config
            .http
            .contracts
            .iter()
            .enumerate()
            .map(|(index, contract)| {
                if contract.id.trim().is_empty() {
                    anyhow::bail!("HTTP contract at index {index} needs a non-empty `id`");
                }
                if !ids.insert(contract.id.clone()) {
                    anyhow::bail!("Duplicate HTTP contract ID: {}", contract.id);
                }
                let resolved_openapi = if let Some(path) = openapi_overrides.get(index) {
                    let unresolved = if path.is_absolute() {
                        path.clone()
                    } else {
                        current_dir.join(path)
                    };
                    Some((
                        absolute_existing(path, &current_dir, "OpenAPI contract")?,
                        codeatlas_source::paths::normalize_relative_path(&unresolved, &self.root),
                    ))
                } else if let Some(path) = &contract.openapi {
                    let unresolved = if path.is_absolute() {
                        path.clone()
                    } else {
                        self.config_dir.join(path)
                    };
                    Some((
                        absolute_existing(path, &self.config_dir, "OpenAPI contract")?,
                        codeatlas_source::paths::normalize_relative_path(&unresolved, &self.root),
                    ))
                } else {
                    None
                };
                let source_roots = if contract.source_roots.is_empty() {
                    vec![self.root.clone()]
                } else {
                    contract
                        .source_roots
                        .iter()
                        .map(|root| absolute_existing(root, &self.config_dir, "HTTP source root"))
                        .collect::<Result<Vec<_>>>()?
                };
                Ok(ResolvedHttpContract {
                    id: contract.id.clone(),
                    openapi: resolved_openapi
                        .as_ref()
                        .map(|(openapi, _)| openapi.clone()),
                    openapi_display: resolved_openapi.map(|(_, display)| display),
                    external_operations: contract.external_operations.clone(),
                    source_roots,
                    source_complete: contract.source_complete,
                    source_include_paths: contract.source_include_paths.clone(),
                    source_exclude_paths: contract.source_exclude_paths.clone(),
                    source_include_operations: contract.source_include_operations.clone(),
                    source_exclude_operations: contract.source_exclude_operations.clone(),
                    repository_root: self.root.clone(),
                })
            })
            .collect()
    }

    pub(crate) fn http_fuzz_target(
        &self,
        requested_id: Option<&str>,
    ) -> Result<ResolvedHttpFuzzTarget> {
        let targets = &self.config.http.fuzz.targets;
        if targets.is_empty() {
            anyhow::bail!(
                "No HTTP fuzz targets configured. Add `http.fuzz.targets` to codeatlas.json."
            );
        }

        let contract_ids = self
            .config
            .http
            .contracts
            .iter()
            .map(|contract| contract.id.as_str())
            .collect::<BTreeSet<_>>();
        let mut target_ids = BTreeSet::new();
        for (index, target) in targets.iter().enumerate() {
            if !is_safe_id(&target.id) {
                anyhow::bail!(
                    "HTTP fuzz target at index {index} needs an ID containing only letters, numbers, `.`, `_`, or `-`"
                );
            }
            if !target_ids.insert(target.id.as_str()) {
                anyhow::bail!("Duplicate HTTP fuzz target ID: {}", target.id);
            }
            if !contract_ids.contains(target.contract.as_str()) {
                anyhow::bail!(
                    "HTTP fuzz target {} references unknown contract {}",
                    target.id,
                    target.contract
                );
            }
        }

        let available_targets = target_ids.iter().copied().collect::<Vec<_>>().join(", ");
        let target = match requested_id {
            Some(id) => match targets.iter().find(|target| target.id == id) {
                Some(target) => target,
                None => {
                    let contract_targets = targets
                        .iter()
                        .filter(|target| target.contract == id)
                        .map(|target| target.id.as_str())
                        .collect::<Vec<_>>();
                    if contract_targets.is_empty() {
                        anyhow::bail!(
                            "Unknown HTTP fuzz target {id:?}. Available targets: {available_targets}"
                        );
                    }
                    anyhow::bail!(
                        "Unknown HTTP fuzz target {id:?}: that is a contract ID, not a runtime target ID. Matching targets: {}",
                        contract_targets.join(", ")
                    );
                }
            },
            None if targets.len() == 1 => &targets[0],
            None => {
                anyhow::bail!(
                    "Multiple HTTP fuzz targets are configured; select one with --target. Available targets: {}",
                    available_targets
                )
            }
        };
        let base_url = parse_http_url(
            &target.base_url,
            &format!("HTTP fuzz target {} `base_url`", target.id),
            true,
        )?;
        validate_environment(
            &target.environment,
            &format!("HTTP fuzz target {}", target.id),
        )?;
        validate_secret_environment(
            &target.secret_environment,
            &target.environment,
            &format!("HTTP fuzz target {}", target.id),
        )?;
        let operation_selection = match &target.operations {
            HttpFuzzOperationSelectionConfig::Explicit(configured) => {
                let mut operation_names = BTreeSet::new();
                let operations = configured
                    .iter()
                    .map(|operation| {
                        let operation =
                            parse_http_fuzz_operation(operation).with_context(|| {
                                format!("Invalid operation in HTTP fuzz target {}", target.id)
                            })?;
                        if !operation_names.insert(operation.name.clone()) {
                            anyhow::bail!(
                                "HTTP fuzz target {} repeats operation {}",
                                target.id,
                                operation.name
                            );
                        }
                        Ok(operation)
                    })
                    .collect::<Result<Vec<_>>>()?;
                ResolvedHttpFuzzOperationSelection::Explicit(operations)
            }
            HttpFuzzOperationSelectionConfig::Scope(HttpFuzzOperationScopeConfig::Contract) => {
                ResolvedHttpFuzzOperationSelection::Contract
            }
        };
        let mut expected_non_success_names = BTreeSet::new();
        let expected_non_success_operations = target
            .expected_non_success_operations
            .iter()
            .map(|operation| {
                let operation = parse_http_fuzz_operation(operation).with_context(|| {
                    format!(
                        "Invalid expected non-success operation in HTTP fuzz target {}",
                        target.id
                    )
                })?;
                if !expected_non_success_names.insert(operation.name.clone()) {
                    anyhow::bail!(
                        "HTTP fuzz target {} repeats expected non-success operation {}",
                        target.id,
                        operation.name
                    );
                }
                Ok(operation)
            })
            .collect::<Result<Vec<_>>>()?;

        let mut headers = Vec::with_capacity(target.headers.len());
        for header in &target.headers {
            if header.name.eq_ignore_ascii_case(CALL_CATEGORY_HEADER) {
                anyhow::bail!(
                    "HTTP fuzz target {} cannot configure reserved internal header {}",
                    target.id,
                    header.name
                );
            }
            if !is_http_token(&header.name) {
                anyhow::bail!(
                    "HTTP fuzz target {} contains invalid header name {:?}",
                    target.id,
                    header.name
                );
            }
            let (value, value_reference) = match (&header.value, &header.value_env) {
                (Some(value), None) => (Some(value.clone()), None),
                (None, Some(name)) if is_environment_name(name) => (None, Some(name.clone())),
                _ => anyhow::bail!(
                    "HTTP fuzz target {} header {} needs exactly one of `value` or `value_env`",
                    target.id,
                    header.name
                ),
            };
            if value
                .as_deref()
                .is_some_and(|value| value.contains(['\r', '\n', '\0']))
            {
                anyhow::bail!(
                    "HTTP fuzz target {} header {} contains an invalid value",
                    target.id,
                    header.name
                );
            }
            headers.push(ResolvedHttpFuzzHeader {
                name: header.name.clone(),
                value,
                value_reference,
            });
        }

        let server = target
            .server
            .as_ref()
            .map(|server| self.resolve_http_fuzz_server(&target.id, server))
            .transpose()?;
        let request_adapter = target
            .request_adapter
            .as_ref()
            .map(|command| self.resolve_http_fuzz_command(&target.id, "request adapter", command))
            .transpose()?;
        Ok(ResolvedHttpFuzzTarget {
            id: target.id.clone(),
            contract: target.contract.clone(),
            workload_image: self.config.http.fuzz.image.clone(),
            base_url,
            environment: target.environment.clone(),
            secret_environment: target.secret_environment.clone(),
            headers,
            environment_class: target.environment_class,
            preauthorized: target.preauthorized,
            server,
            request_adapter,
            operation_selection,
            expected_non_success_operations,
            positive_coverage: target.positive_coverage.clone(),
            suppress_health_checks: target.suppress_health_checks.clone(),
            suppress_warnings: target.suppress_warnings,
        })
    }

    fn resolve_http_fuzz_command(
        &self,
        target_id: &str,
        label: &str,
        command: &HttpFuzzCommandConfig,
    ) -> Result<ResolvedHttpFuzzCommand> {
        if command.command.trim().is_empty() || command.command.contains('\0') {
            anyhow::bail!("HTTP fuzz target {target_id} {label} needs a valid `command`");
        }
        if command.args.iter().any(|argument| argument.contains('\0')) {
            anyhow::bail!("HTTP fuzz target {target_id} {label} contains an invalid argument");
        }
        let unresolved = command
            .cwd
            .as_ref()
            .map(|cwd| self.config_dir.join(cwd))
            .unwrap_or_else(|| self.root.clone());
        let cwd = unresolved.canonicalize().with_context(|| {
            format!(
                "HTTP fuzz target {target_id} {label} working directory does not exist: {}",
                unresolved.display()
            )
        })?;
        if !cwd.is_dir() {
            anyhow::bail!(
                "HTTP fuzz target {target_id} {label} working directory is not a directory: {}",
                cwd.display()
            );
        }
        if cwd.strip_prefix(&self.root).is_err() {
            anyhow::bail!(
                "HTTP fuzz target {target_id} {label} working directory must stay within the project root: {}",
                cwd.display()
            );
        }
        Ok(ResolvedHttpFuzzCommand {
            command: command.command.clone(),
            args: command.args.clone(),
            cwd,
        })
    }

    fn resolve_http_fuzz_server(
        &self,
        target_id: &str,
        server: &HttpFuzzServerConfig,
    ) -> Result<ResolvedHttpFuzzServer> {
        let command = self.resolve_http_fuzz_command(
            target_id,
            "server",
            &HttpFuzzCommandConfig {
                command: server.command.clone(),
                args: server.args.clone(),
                cwd: server.cwd.clone(),
            },
        )?;
        let prepare = server
            .prepare
            .iter()
            .enumerate()
            .map(|(index, command)| {
                self.resolve_http_fuzz_command(
                    target_id,
                    &format!("server prepare command {}", index + 1),
                    command,
                )
            })
            .collect::<Result<Vec<_>>>()?;
        let startup_timeout_seconds = server.startup_timeout_seconds.unwrap_or(30);
        if !(1..=600).contains(&startup_timeout_seconds) {
            anyhow::bail!(
                "HTTP fuzz target {target_id} server `startup_timeout_seconds` must be between 1 and 600"
            );
        }
        Ok(ResolvedHttpFuzzServer {
            command,
            prepare,
            startup_timeout_seconds,
        })
    }
}

pub(crate) fn parse_http_fuzz_operation(value: &str) -> Result<HttpFuzzOperation> {
    let Some((method, path)) = value.trim().split_once(' ') else {
        anyhow::bail!("HTTP operation must use the format `METHOD /path`");
    };
    let method = method.to_ascii_uppercase();
    if !matches!(
        method.as_str(),
        "GET" | "PUT" | "POST" | "DELETE" | "OPTIONS" | "HEAD" | "PATCH" | "TRACE"
    ) {
        anyhow::bail!(
            "HTTP operation method must be GET, PUT, POST, DELETE, OPTIONS, HEAD, PATCH, or TRACE"
        );
    }
    let path = path.trim();
    if !path.starts_with('/') || path.chars().any(char::is_whitespace) || path.contains(['?', '#'])
    {
        anyhow::bail!("HTTP operation path must be absolute, path-only, and contain no whitespace");
    }
    Ok(HttpFuzzOperation {
        name: format!("{method} {path}"),
        method,
        path: path.to_string(),
    })
}

fn is_safe_id(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn validate_environment(environment: &BTreeMap<String, String>, label: &str) -> Result<()> {
    for (name, value) in environment {
        if !is_environment_name(name)
            || value.contains('\0')
            || matches!(
                name.as_str(),
                REQUEST_HOOK_CONFIG_ENV | SCHEMATHESIS_HOOKS_ENV
            )
        {
            anyhow::bail!("{label} contains an invalid environment entry");
        }
    }
    Ok(())
}

fn validate_secret_environment(
    secret_environment: &BTreeMap<String, String>,
    literal_environment: &BTreeMap<String, String>,
    label: &str,
) -> Result<()> {
    for (name, reference) in secret_environment {
        if !is_environment_name(name)
            || !is_environment_name(reference)
            || literal_environment.contains_key(name)
            || matches!(
                name.as_str(),
                REQUEST_HOOK_CONFIG_ENV | SCHEMATHESIS_HOOKS_ENV
            )
        {
            anyhow::bail!("{label} contains an invalid secret environment entry");
        }
    }
    Ok(())
}

fn is_environment_name(value: &str) -> bool {
    let mut bytes = value.bytes();
    bytes
        .next()
        .is_some_and(|byte| byte == b'_' || byte.is_ascii_alphabetic())
        && bytes.all(|byte| byte == b'_' || byte.is_ascii_alphanumeric())
}

fn parse_http_url(value: &str, label: &str, base: bool) -> Result<Url> {
    let url = Url::parse(value).with_context(|| format!("{label} is not a valid URL"))?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        anyhow::bail!("{label} needs an absolute HTTP(S) URL");
    }
    if !url.username().is_empty() || url.password().is_some() {
        anyhow::bail!("{label} must not contain credentials");
    }
    if base && (url.query().is_some() || url.fragment().is_some()) {
        anyhow::bail!("{label} must not contain a query or fragment");
    }
    Ok(url)
}

fn is_http_token(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'!' | b'#'
                        | b'$'
                        | b'%'
                        | b'&'
                        | b'\''
                        | b'*'
                        | b'+'
                        | b'-'
                        | b'.'
                        | b'^'
                        | b'_'
                        | b'`'
                        | b'|'
                        | b'~'
                )
        })
}

fn absolute_existing(path: &Path, base: &Path, label: &str) -> Result<PathBuf> {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    };
    path.canonicalize()
        .with_context(|| format!("{label} does not exist: {}", path.display()))
}

#[cfg(test)]
mod tests;

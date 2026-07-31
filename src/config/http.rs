use super::ProjectConfig;
use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct HttpConfig {
    pub contracts: Vec<HttpContractConfig>,
    pub fuzz: HttpFuzzConfig,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct HttpContractConfig {
    pub id: String,
    pub openapi: Option<HttpOpenApiSourceConfig>,
    pub source_roots: Vec<PathBuf>,
    pub source_complete: bool,
    pub source_include_paths: Vec<String>,
    pub source_exclude_paths: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub(crate) enum HttpOpenApiSourceConfig {
    File(PathBuf),
    Provider(HttpOpenApiProviderConfig),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum HttpOpenApiProviderConfig {
    File {
        path: PathBuf,
    },
    Command {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        cwd: Option<PathBuf>,
        #[serde(default)]
        environment: BTreeMap<String, String>,
    },
    Url {
        url: String,
    },
    Target {
        target: String,
    },
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct HttpFuzzConfig {
    pub targets: Vec<HttpFuzzTargetConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct HttpFuzzTargetConfig {
    pub id: String,
    pub contract: String,
    pub base_url: String,
    pub openapi_path: String,
    pub environment: BTreeMap<String, String>,
    pub headers: Vec<HttpFuzzHeaderConfig>,
    pub report_dir: Option<PathBuf>,
    pub server: Option<HttpFuzzCommandConfig>,
    pub request_adapter: Option<HttpFuzzCommandConfig>,
    pub positive_coverage: HttpFuzzPositiveCoverageConfig,
    pub suppress_health_checks: Vec<HttpFuzzHealthCheck>,
    pub suppress_warnings: bool,
}

impl Default for HttpFuzzTargetConfig {
    fn default() -> Self {
        Self {
            id: String::new(),
            contract: String::new(),
            base_url: String::new(),
            openapi_path: "/openapi.json".to_string(),
            environment: BTreeMap::new(),
            headers: Vec::new(),
            report_dir: None,
            server: None,
            request_adapter: None,
            positive_coverage: HttpFuzzPositiveCoverageConfig::default(),
            suppress_health_checks: Vec::new(),
            suppress_warnings: false,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct HttpFuzzPositiveCoverageConfig {
    pub max_operations_without_success: Option<u64>,
    pub max_authentication_rejection_only_operations: Option<u64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct HttpFuzzHeaderConfig {
    pub name: String,
    pub value: Option<String>,
    pub value_env: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct HttpFuzzCommandConfig {
    pub command: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum HttpFuzzHealthCheck {
    DataTooLarge,
    FilterTooMuch,
    LargeBaseExample,
    TooSlow,
}

impl HttpFuzzHealthCheck {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::DataTooLarge => "data_too_large",
            Self::FilterTooMuch => "filter_too_much",
            Self::LargeBaseExample => "large_base_example",
            Self::TooSlow => "too_slow",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ResolvedHttpContract {
    pub id: String,
    pub openapi: Option<ResolvedHttpOpenApiSource>,
    pub openapi_display: Option<String>,
    pub source_roots: Vec<PathBuf>,
    pub source_complete: bool,
    pub source_include_paths: Vec<String>,
    pub source_exclude_paths: Vec<String>,
    pub repository_root: PathBuf,
}

#[derive(Debug, Clone)]
pub(crate) enum ResolvedHttpOpenApiSource {
    File(PathBuf),
    Command {
        command: String,
        args: Vec<String>,
        cwd: PathBuf,
        environment: BTreeMap<String, String>,
    },
    Url {
        url: String,
    },
    Target(Box<ResolvedHttpFuzzTarget>),
}

#[derive(Debug, Clone)]
pub(crate) struct ResolvedHttpFuzzTarget {
    pub id: String,
    pub contract: String,
    pub base_url: String,
    pub openapi_url: String,
    pub environment: BTreeMap<String, String>,
    pub headers: Vec<ResolvedHttpFuzzHeader>,
    pub project_root: PathBuf,
    pub report_root: Option<PathBuf>,
    pub server: Option<ResolvedHttpFuzzCommand>,
    pub request_adapter: Option<ResolvedHttpFuzzCommand>,
    pub positive_coverage: HttpFuzzPositiveCoverageConfig,
    pub suppress_health_checks: Vec<HttpFuzzHealthCheck>,
    pub suppress_warnings: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct ResolvedHttpFuzzHeader {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone)]
pub(crate) struct ResolvedHttpFuzzCommand {
    pub command: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
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
                    source_roots: vec![self.root.clone()],
                    source_complete: false,
                    source_include_paths: Vec::new(),
                    source_exclude_paths: Vec::new(),
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
                        openapi: Some(ResolvedHttpOpenApiSource::File(openapi)),
                        openapi_display: Some(crate::paths::normalize_relative_path(
                            &unresolved,
                            &self.root,
                        )),
                        source_roots: vec![self.root.clone()],
                        source_complete: false,
                        source_include_paths: Vec::new(),
                        source_exclude_paths: Vec::new(),
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
                        ResolvedHttpOpenApiSource::File(absolute_existing(
                            path,
                            &current_dir,
                            "OpenAPI contract",
                        )?),
                        crate::paths::normalize_relative_path(&unresolved, &self.root),
                    ))
                } else {
                    contract
                        .openapi
                        .as_ref()
                        .map(|source| self.resolve_http_openapi_source(source))
                        .transpose()?
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
                    source_roots,
                    source_complete: contract.source_complete,
                    source_include_paths: contract.source_include_paths.clone(),
                    source_exclude_paths: contract.source_exclude_paths.clone(),
                    repository_root: self.root.clone(),
                })
            })
            .collect()
    }

    fn resolve_http_openapi_source(
        &self,
        source: &HttpOpenApiSourceConfig,
    ) -> Result<(ResolvedHttpOpenApiSource, String)> {
        match source {
            HttpOpenApiSourceConfig::File(path)
            | HttpOpenApiSourceConfig::Provider(HttpOpenApiProviderConfig::File { path }) => {
                let unresolved = if path.is_absolute() {
                    path.clone()
                } else {
                    self.config_dir.join(path)
                };
                Ok((
                    ResolvedHttpOpenApiSource::File(absolute_existing(
                        path,
                        &self.config_dir,
                        "OpenAPI contract",
                    )?),
                    crate::paths::normalize_relative_path(&unresolved, &self.root),
                ))
            }
            HttpOpenApiSourceConfig::Provider(HttpOpenApiProviderConfig::Command {
                command,
                args,
                cwd,
                environment,
            }) => {
                if command.trim().is_empty()
                    || command.contains('\0')
                    || args.iter().any(|argument| argument.contains('\0'))
                {
                    anyhow::bail!("OpenAPI command provider needs a valid command and arguments");
                }
                validate_environment(environment, "OpenAPI command provider")?;
                let unresolved = cwd
                    .as_ref()
                    .map(|cwd| self.config_dir.join(cwd))
                    .unwrap_or_else(|| self.root.clone());
                let cwd = unresolved.canonicalize().with_context(|| {
                    format!(
                        "OpenAPI command provider working directory does not exist: {}",
                        unresolved.display()
                    )
                })?;
                if !cwd.is_dir() {
                    anyhow::bail!(
                        "OpenAPI command provider working directory is not a directory: {}",
                        cwd.display()
                    );
                }
                Ok((
                    ResolvedHttpOpenApiSource::Command {
                        command: command.clone(),
                        args: args.clone(),
                        cwd,
                        environment: environment.clone(),
                    },
                    format!("command:{command}"),
                ))
            }
            HttpOpenApiSourceConfig::Provider(HttpOpenApiProviderConfig::Url { url }) => {
                validate_http_url(url, "OpenAPI URL provider")?;
                Ok((
                    ResolvedHttpOpenApiSource::Url { url: url.clone() },
                    url.clone(),
                ))
            }
            HttpOpenApiSourceConfig::Provider(HttpOpenApiProviderConfig::Target { target }) => {
                let resolved = self.http_fuzz_target(Some(target))?;
                Ok((
                    ResolvedHttpOpenApiSource::Target(Box::new(resolved)),
                    format!("target:{target}"),
                ))
            }
        }
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

        let target = match requested_id {
            Some(id) => targets
                .iter()
                .find(|target| target.id == id)
                .with_context(|| format!("Unknown HTTP fuzz target {id:?}"))?,
            None if targets.len() == 1 => &targets[0],
            None => {
                anyhow::bail!(
                    "Multiple HTTP fuzz targets are configured; select one with --target. Available targets: {}",
                    target_ids.into_iter().collect::<Vec<_>>().join(", ")
                )
            }
        };
        let base_url = target.base_url.trim_end_matches('/').to_string();
        validate_http_url(
            &base_url,
            &format!("HTTP fuzz target {} `base_url`", target.id),
        )?;
        if !target.openapi_path.starts_with('/')
            || target.openapi_path.chars().any(char::is_whitespace)
        {
            anyhow::bail!(
                "HTTP fuzz target {} needs an absolute, whitespace-free `openapi_path`",
                target.id
            );
        }
        validate_environment(
            &target.environment,
            &format!("HTTP fuzz target {}", target.id),
        )?;

        let mut headers = Vec::with_capacity(target.headers.len());
        for header in &target.headers {
            if !is_http_token(&header.name) {
                anyhow::bail!(
                    "HTTP fuzz target {} contains invalid header name {:?}",
                    target.id,
                    header.name
                );
            }
            let value = match (&header.value, &header.value_env) {
                (Some(value), None) => value.clone(),
                (None, Some(name)) if !name.is_empty() => target
                    .environment
                    .get(name)
                    .cloned()
                    .or_else(|| std::env::var(name).ok())
                    .with_context(|| {
                        format!(
                            "HTTP fuzz target {} header {} needs environment variable {}",
                            target.id, header.name, name
                        )
                    })?,
                _ => anyhow::bail!(
                    "HTTP fuzz target {} header {} needs exactly one of `value` or `value_env`",
                    target.id,
                    header.name
                ),
            };
            if value.contains(['\r', '\n', '\0']) {
                anyhow::bail!(
                    "HTTP fuzz target {} header {} contains an invalid value",
                    target.id,
                    header.name
                );
            }
            headers.push(ResolvedHttpFuzzHeader {
                name: header.name.clone(),
                value,
            });
        }

        let server = target
            .server
            .as_ref()
            .map(|command| self.resolve_http_fuzz_command(&target.id, "server", command))
            .transpose()?;
        let request_adapter = target
            .request_adapter
            .as_ref()
            .map(|command| self.resolve_http_fuzz_command(&target.id, "request adapter", command))
            .transpose()?;
        let report_root = target.report_dir.as_ref().map(|path| {
            if path.is_absolute() {
                path.clone()
            } else {
                self.config_dir.join(path)
            }
        });

        Ok(ResolvedHttpFuzzTarget {
            id: target.id.clone(),
            contract: target.contract.clone(),
            base_url,
            openapi_url: format!(
                "{}{}",
                target.base_url.trim_end_matches('/'),
                target.openapi_path
            ),
            environment: target.environment.clone(),
            headers,
            project_root: self.root.clone(),
            report_root,
            server,
            request_adapter,
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
        Ok(ResolvedHttpFuzzCommand {
            command: command.command.clone(),
            args: command.args.clone(),
            cwd,
        })
    }
}

fn is_safe_id(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn validate_environment(environment: &BTreeMap<String, String>, label: &str) -> Result<()> {
    for (name, value) in environment {
        if name.is_empty() || name.contains(['=', '\0']) || value.contains('\0') {
            anyhow::bail!("{label} contains an invalid environment entry");
        }
    }
    Ok(())
}

fn validate_http_url(url: &str, label: &str) -> Result<()> {
    if !(url.starts_with("http://") || url.starts_with("https://"))
        || url.chars().any(char::is_whitespace)
    {
        anyhow::bail!("{label} needs an absolute HTTP(S) URL");
    }
    Ok(())
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
mod tests {
    use crate::config::CodeAtlasConfig;

    #[test]
    fn config_reads_portable_http_fuzz_targets() {
        let config = serde_json::from_str::<CodeAtlasConfig>(
            r#"{
                "http": {
                    "contracts": [{
                        "id": "public-api",
                        "openapi": "openapi.json"
                    }],
                    "fuzz": {
                        "targets": [{
                            "id": "public-local",
                            "contract": "public-api",
                            "base_url": "http://127.0.0.1:3443",
                            "environment": {
                                "LOCAL_API_TOKEN": "test-token"
                            },
                            "headers": [{
                                "name": "Authorization",
                                "value_env": "LOCAL_API_TOKEN"
                            }],
                            "server": {
                                "command": "node",
                                "args": ["src/test-server.js"]
                            },
                            "request_adapter": {
                                "command": "node",
                                "args": ["src/sign-fuzz-request.js"]
                            },
                            "positive_coverage": {
                                "max_operations_without_success": 3,
                                "max_authentication_rejection_only_operations": 0
                            },
                            "suppress_health_checks": ["filter_too_much"],
                            "suppress_warnings": true
                        }]
                    }
                }
            }"#,
        )
        .expect("HTTP fuzz config");

        let target = &config.http.fuzz.targets[0];
        assert_eq!(target.id, "public-local");
        assert_eq!(target.contract, "public-api");
        assert_eq!(target.openapi_path, "/openapi.json");
        assert_eq!(
            target.headers[0].value_env.as_deref(),
            Some("LOCAL_API_TOKEN")
        );
        assert_eq!(
            target.server.as_ref().map(|server| server.command.as_str()),
            Some("node")
        );
        assert_eq!(
            target
                .request_adapter
                .as_ref()
                .and_then(|adapter| adapter.args.first())
                .map(String::as_str),
            Some("src/sign-fuzz-request.js")
        );
        assert_eq!(
            target.positive_coverage.max_operations_without_success,
            Some(3)
        );
        assert_eq!(
            target
                .positive_coverage
                .max_authentication_rejection_only_operations,
            Some(0)
        );
        assert!(target.suppress_warnings);
    }
}

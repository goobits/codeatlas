use crate::config::{
    HttpFuzzCommandConfig, HttpFuzzHealthCheck, HttpFuzzPositiveCoverageConfig,
    HttpFuzzServerConfig, HttpOpenApiProviderConfig, HttpOpenApiSourceConfig, ProjectConfig,
};
use anyhow::{Context, Result};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use url::Url;

pub(super) const REQUEST_HOOK_CONFIG_ENV: &str = "CODEATLAS_HTTP_REQUEST_ADAPTER_CONFIG";
pub(super) const SCHEMATHESIS_HOOKS_ENV: &str = "SCHEMATHESIS_HOOKS";

#[derive(Debug, Clone)]
pub(crate) struct ResolvedHttpContract {
    pub id: String,
    pub openapi: Option<ResolvedHttpOpenApiSource>,
    pub openapi_display: Option<String>,
    pub source_roots: Vec<PathBuf>,
    pub source_complete: bool,
    pub source_include_paths: Vec<String>,
    pub source_exclude_paths: Vec<String>,
    pub source_include_operations: Vec<String>,
    pub source_exclude_operations: Vec<String>,
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
        url: Url,
    },
    Target(Box<ResolvedHttpFuzzTarget>),
}

#[derive(Debug, Clone)]
pub(crate) struct ResolvedHttpFuzzTarget {
    pub id: String,
    pub contract: String,
    pub base_url: Url,
    pub openapi_url: Url,
    pub environment: BTreeMap<String, String>,
    pub headers: Vec<ResolvedHttpFuzzHeader>,
    pub report_root: Option<PathBuf>,
    pub server: Option<ResolvedHttpFuzzServer>,
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

#[derive(Debug, Clone)]
pub(crate) struct ResolvedHttpFuzzServer {
    pub command: ResolvedHttpFuzzCommand,
    pub prepare: Vec<ResolvedHttpFuzzCommand>,
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
                        openapi: Some(ResolvedHttpOpenApiSource::File(openapi)),
                        openapi_display: Some(crate::paths::normalize_relative_path(
                            &unresolved,
                            &self.root,
                        )),
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
                    source_include_operations: contract.source_include_operations.clone(),
                    source_exclude_operations: contract.source_exclude_operations.clone(),
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
                let url = parse_http_url(url, "OpenAPI URL provider", false)?;
                Ok((
                    ResolvedHttpOpenApiSource::Url { url: url.clone() },
                    url.to_string(),
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
        if !target.openapi_path.starts_with('/')
            || target.openapi_path.chars().any(char::is_whitespace)
            || target.openapi_path.contains(['?', '#'])
        {
            anyhow::bail!(
                "HTTP fuzz target {} needs an absolute path-only, whitespace-free `openapi_path`",
                target.id
            );
        }
        let openapi_url = join_base_path(&base_url, &target.openapi_path)?;
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
            .map(|server| self.resolve_http_fuzz_server(&target.id, server))
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
            openapi_url,
            environment: target.environment.clone(),
            headers,
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
        Ok(ResolvedHttpFuzzServer { command, prepare })
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
        if name.is_empty()
            || name.contains(['=', '\0'])
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

fn join_base_path(base: &Url, path: &str) -> Result<Url> {
    let mut directory = base.clone();
    let mut base_path = directory.path().trim_end_matches('/').to_string();
    base_path.push('/');
    directory.set_path(&base_path);
    directory
        .join(path.trim_start_matches('/'))
        .with_context(|| format!("Could not join HTTP target URL {base} with {path:?}"))
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
    use crate::config::{CodeAtlasConfig, ProjectConfig};
    use serde_json::{json, Value};

    fn project_config(config: CodeAtlasConfig) -> ProjectConfig {
        let root = std::env::current_dir().expect("current directory");
        ProjectConfig {
            root: root.clone(),
            config,
            config_dir: root,
        }
    }

    fn fuzz_target_error(target: Value) -> String {
        let config = serde_json::from_value::<CodeAtlasConfig>(json!({
            "http": {
                "contracts": [{ "id": "public-api" }],
                "fuzz": { "targets": [target] }
            }
        }))
        .expect("HTTP fuzz config");
        project_config(config)
            .http_fuzz_target(None)
            .expect_err("unsafe fuzz target should be rejected")
            .to_string()
    }

    #[test]
    fn fuzz_target_error_distinguishes_contract_ids_from_runtime_target_ids() {
        let config = serde_json::from_str::<CodeAtlasConfig>(
            r#"{
				"http": {
					"contracts": [{ "id": "public-api" }],
					"fuzz": {
						"targets": [{
							"id": "public-local",
							"contract": "public-api",
							"base_url": "http://127.0.0.1:3443"
						}]
					}
				}
			}"#,
        )
        .expect("HTTP fuzz config");

        let error = project_config(config)
            .http_fuzz_target(Some("public-api"))
            .expect_err("a contract ID should not resolve as a runtime target")
            .to_string();

        assert!(error.contains("contract ID, not a runtime target ID"));
        assert!(error.contains("Matching targets: public-local"));
    }

    #[test]
    fn fuzz_target_uses_structural_urls_and_rejects_ambiguous_bases() {
        let config = serde_json::from_str::<CodeAtlasConfig>(
            r#"{
                "http": {
                    "contracts": [{ "id": "public-api" }],
                    "fuzz": { "targets": [{
                        "id": "public-local",
                        "contract": "public-api",
                        "base_url": "http://127.0.0.1:3443/api/",
                        "openapi_path": "/schema/openapi.json"
                    }] }
                }
            }"#,
        )
        .expect("HTTP fuzz config");
        let target = project_config(config)
            .http_fuzz_target(None)
            .expect("resolved fuzz target");
        assert_eq!(target.base_url.as_str(), "http://127.0.0.1:3443/api/");
        assert_eq!(
            target.openapi_url.as_str(),
            "http://127.0.0.1:3443/api/schema/openapi.json"
        );

        for base_url in [
            "http://user:secret@127.0.0.1:3443",
            "http://127.0.0.1:3443?token=secret",
            "http://127.0.0.1:3443#fragment",
        ] {
            let source = format!(
                r#"{{
                    "http": {{
                        "contracts": [{{ "id": "public-api" }}],
                        "fuzz": {{ "targets": [{{
                            "id": "public-local",
                            "contract": "public-api",
                            "base_url": {base_url:?}
                        }}] }}
                    }}
                }}"#
            );
            let config = serde_json::from_str::<CodeAtlasConfig>(&source).expect("HTTP config");
            assert!(project_config(config).http_fuzz_target(None).is_err());
        }
    }

    #[test]
    fn fuzz_target_rejects_unsafe_runtime_configuration() {
        let cases = [
            (
                "unsafe ID",
                json!({
                    "id": "../public-local",
                    "contract": "public-api",
                    "base_url": "http://127.0.0.1:3443"
                }),
                "needs an ID",
            ),
            (
                "unknown contract",
                json!({
                    "id": "public-local",
                    "contract": "missing",
                    "base_url": "http://127.0.0.1:3443"
                }),
                "unknown contract",
            ),
            (
                "credentialed URL",
                json!({
                    "id": "public-local",
                    "contract": "public-api",
                    "base_url": "http://user:secret@127.0.0.1:3443"
                }),
                "must not contain credentials",
            ),
            (
                "relative OpenAPI path",
                json!({
                    "id": "public-local",
                    "contract": "public-api",
                    "base_url": "http://127.0.0.1:3443",
                    "openapi_path": "openapi.json"
                }),
                "absolute path-only",
            ),
            (
                "reserved hook environment",
                json!({
                    "id": "public-local",
                    "contract": "public-api",
                    "base_url": "http://127.0.0.1:3443",
                    "environment": {
                        "CODEATLAS_HTTP_REQUEST_ADAPTER_CONFIG": "untrusted"
                    }
                }),
                "invalid environment entry",
            ),
            (
                "ambient Schemathesis hook",
                json!({
                    "id": "public-local",
                    "contract": "public-api",
                    "base_url": "http://127.0.0.1:3443",
                    "environment": { "SCHEMATHESIS_HOOKS": "untrusted.py" }
                }),
                "invalid environment entry",
            ),
            (
                "invalid header name",
                json!({
                    "id": "public-local",
                    "contract": "public-api",
                    "base_url": "http://127.0.0.1:3443",
                    "headers": [{ "name": "Bad Header", "value": "value" }]
                }),
                "invalid header name",
            ),
            (
                "header injection",
                json!({
                    "id": "public-local",
                    "contract": "public-api",
                    "base_url": "http://127.0.0.1:3443",
                    "headers": [{ "name": "Authorization", "value": "safe\r\ninjected" }]
                }),
                "invalid value",
            ),
            (
                "ambiguous header source",
                json!({
                    "id": "public-local",
                    "contract": "public-api",
                    "base_url": "http://127.0.0.1:3443",
                    "headers": [{
                        "name": "Authorization",
                        "value": "literal",
                        "value_env": "TOKEN"
                    }]
                }),
                "exactly one",
            ),
            (
                "empty request adapter",
                json!({
                    "id": "public-local",
                    "contract": "public-api",
                    "base_url": "http://127.0.0.1:3443",
                    "request_adapter": { "command": "" }
                }),
                "needs a valid `command`",
            ),
        ];

        for (label, target, expected) in cases {
            let error = fuzz_target_error(target);
            assert!(
                error.contains(expected),
                "{label} produced unexpected error: {error}"
            );
        }
    }
}

use crate::config::{ResolvedHttpFuzzCommand, ResolvedHttpFuzzTarget};
use anyhow::{Context, Result};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Command;

pub(super) const API_VERSION: &str = "codeatlas.http-request-adapter/v1";
pub(super) const CONFIG_ENVIRONMENT_VARIABLE: &str = "CODEATLAS_HTTP_REQUEST_ADAPTER";

const HOOK_SOURCE: &str = include_str!("schemathesis_hooks.py");

pub(super) struct PreparedRequestHooks {
    pub(super) hook_path: PathBuf,
    pub(super) config: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RequestHooksConfig<'a> {
    api_version: &'static str,
    headers: Vec<HeaderConfig<'a>>,
    adapter: Option<AdapterConfig<'a>>,
}

#[derive(Serialize)]
struct HeaderConfig<'a> {
    name: &'a str,
    value: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AdapterConfig<'a> {
    command: &'a str,
    args: &'a [String],
    cwd: String,
}

impl<'a> From<&'a ResolvedHttpFuzzCommand> for AdapterConfig<'a> {
    fn from(adapter: &'a ResolvedHttpFuzzCommand) -> Self {
        Self {
            command: &adapter.command,
            args: &adapter.args,
            cwd: adapter.cwd.to_string_lossy().into_owned(),
        }
    }
}

pub(super) fn prepare(target: &ResolvedHttpFuzzTarget) -> Result<PreparedRequestHooks> {
    let hook_root = super::toolchain::cache_base()
        .join("codeatlas")
        .join("hooks")
        .join(API_VERSION.replace(['/', '.'], "-"));
    std::fs::create_dir_all(&hook_root).with_context(|| {
        format!(
            "Could not create CodeAtlas hook cache {}",
            hook_root.display()
        )
    })?;
    let hook_path = hook_root.join("schemathesis_hooks.py");
    if std::fs::read_to_string(&hook_path).ok().as_deref() != Some(HOOK_SOURCE) {
        std::fs::write(&hook_path, HOOK_SOURCE).with_context(|| {
            format!(
                "Could not write CodeAtlas Schemathesis hook {}",
                hook_path.display()
            )
        })?;
    }
    let config = serde_json::to_string(&RequestHooksConfig {
        api_version: API_VERSION,
        headers: target
            .headers
            .iter()
            .map(|header| HeaderConfig {
                name: &header.name,
                value: &header.value,
            })
            .collect(),
        adapter: target.request_adapter.as_ref().map(AdapterConfig::from),
    })?;
    Ok(PreparedRequestHooks { hook_path, config })
}

pub(super) fn validate(schemathesis: &Path, hooks: &PreparedRequestHooks) -> Result<()> {
    let smoke_config = serde_json::to_string(&RequestHooksConfig {
        api_version: API_VERSION,
        headers: Vec::new(),
        adapter: None,
    })?;
    let output = Command::new(schemathesis)
        .args(["run", "--help"])
        .env("SCHEMATHESIS_HOOKS", &hooks.hook_path)
        .env(CONFIG_ENVIRONMENT_VARIABLE, smoke_config)
        .output()
        .with_context(|| {
            format!(
                "Could not validate CodeAtlas hooks with {}",
                schemathesis.display()
            )
        })?;
    if !output.status.success() {
        let combined = [&output.stdout[..], &output.stderr[..]].concat();
        let diagnostic = String::from_utf8_lossy(&combined)
            .trim()
            .chars()
            .take(2_000)
            .collect::<String>();
        anyhow::bail!(
            "CodeAtlas hooks are incompatible with {}: {}",
            schemathesis.display(),
            diagnostic
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{AdapterConfig, HeaderConfig, RequestHooksConfig, API_VERSION};

    #[test]
    fn hook_configuration_is_versioned_and_engine_neutral() {
        let args = vec!["adapter.js".to_string()];
        let config = serde_json::to_value(RequestHooksConfig {
            api_version: API_VERSION,
            headers: vec![HeaderConfig {
                name: "Authorization",
                value: "Bearer test-token",
            }],
            adapter: Some(AdapterConfig {
                command: "node",
                args: &args,
                cwd: "/workspace".to_string(),
            }),
        })
        .expect("request adapter configuration");

        assert_eq!(config["apiVersion"], API_VERSION);
        assert_eq!(config["headers"][0]["name"], "Authorization");
        assert_eq!(config["headers"][0]["value"], "Bearer test-token");
        assert_eq!(config["adapter"]["command"], "node");
        assert_eq!(config["adapter"]["args"][0], "adapter.js");
    }
}

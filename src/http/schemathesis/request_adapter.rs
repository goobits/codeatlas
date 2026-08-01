use crate::http::private_fs;
use crate::http::target::{
    HttpFuzzOperation, ResolvedHttpFuzzCommand, ResolvedHttpFuzzTarget, REQUEST_HOOK_CONFIG_ENV,
};
use anyhow::{Context, Result};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

pub(super) const API_VERSION: &str = "codeatlas.http-request-adapter/v2";
const HOOK_SOURCE: &str = include_str!("hooks.py");
static CONFIG_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub(super) struct PreparedRequestHooks {
    pub(super) hook_path: PathBuf,
    config: PrivateConfig,
}

impl PreparedRequestHooks {
    pub(super) fn config_path(&self) -> &Path {
        &self.config.path
    }
}

struct PrivateConfig {
    path: PathBuf,
}

impl PrivateConfig {
    fn create(root: &Path, contents: &[u8]) -> Result<Self> {
        let sequence = CONFIG_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = root.join(format!("config-{}-{sequence}.json", std::process::id()));
        let mut file = private_fs::create(&path)?;
        if let Err(error) = file.write_all(contents) {
            drop(file);
            let _ = std::fs::remove_file(&path);
            return Err(error).with_context(|| {
                format!("Could not write private hook config {}", path.display())
            });
        }
        Ok(Self { path })
    }
}

impl Drop for PrivateConfig {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RequestHooksConfig<'a> {
    api_version: &'static str,
    headers: Vec<HeaderConfig<'a>>,
    adapter: Option<AdapterConfig<'a>>,
    methods_by_path: BTreeMap<String, Vec<String>>,
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

pub(super) fn prepare(
    target: &ResolvedHttpFuzzTarget,
    operations: &[HttpFuzzOperation],
) -> Result<PreparedRequestHooks> {
    let hook_root = crate::http::environment::cache_base()
        .join("codeatlas")
        .join("hooks")
        .join(API_VERSION.replace(['/', '.'], "-"));
    std::fs::create_dir_all(&hook_root).with_context(|| {
        format!(
            "Could not create CodeAtlas hook cache {}",
            hook_root.display()
        )
    })?;
    private_fs::secure_dir(&hook_root)?;
    let hook_path = hook_root.join("schemathesis_hooks.py");
    if std::fs::read_to_string(&hook_path).ok().as_deref() != Some(HOOK_SOURCE) {
        std::fs::write(&hook_path, HOOK_SOURCE).with_context(|| {
            format!(
                "Could not write CodeAtlas Schemathesis hook {}",
                hook_path.display()
            )
        })?;
    }
    let config = serde_json::to_vec(&RequestHooksConfig {
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
        methods_by_path: methods_by_path(operations),
    })?;
    let config = PrivateConfig::create(&hook_root, &config)?;
    Ok(PreparedRequestHooks { hook_path, config })
}

pub(super) fn validate(schemathesis: &Path, hooks: &PreparedRequestHooks) -> Result<()> {
    let smoke_config = serde_json::to_vec(&RequestHooksConfig {
        api_version: API_VERSION,
        headers: Vec::new(),
        adapter: None,
        methods_by_path: BTreeMap::new(),
    })?;
    let hook_root = hooks
        .hook_path
        .parent()
        .context("CodeAtlas hook path has no parent directory")?;
    let smoke_config = PrivateConfig::create(hook_root, &smoke_config)?;
    let output = Command::new(schemathesis)
        .args(["run", "--help"])
        .env("SCHEMATHESIS_HOOKS", &hooks.hook_path)
        .env(REQUEST_HOOK_CONFIG_ENV, &smoke_config.path)
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

fn methods_by_path(operations: &[HttpFuzzOperation]) -> BTreeMap<String, Vec<String>> {
    let mut methods = BTreeMap::<String, BTreeSet<String>>::new();
    for operation in operations {
        methods
            .entry(operation.path.clone())
            .or_default()
            .insert(operation.method.clone());
    }
    methods
        .into_iter()
        .map(|(path, methods)| (path, methods.into_iter().collect()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        methods_by_path, AdapterConfig, HeaderConfig, PrivateConfig, RequestHooksConfig,
        API_VERSION,
    };
    use crate::http::target::parse_http_fuzz_operation;

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

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
            methods_by_path: methods_by_path(&[
                parse_http_fuzz_operation("GET /widgets/{id}").expect("GET operation"),
                parse_http_fuzz_operation("POST /widgets/{id}").expect("POST operation"),
            ]),
        })
        .expect("request adapter configuration");

        assert_eq!(config["apiVersion"], API_VERSION);
        assert_eq!(config["headers"][0]["name"], "Authorization");
        assert_eq!(config["headers"][0]["value"], "Bearer test-token");
        assert_eq!(config["adapter"]["command"], "node");
        assert_eq!(config["adapter"]["args"][0], "adapter.js");
        assert_eq!(
            config["methodsByPath"]["/widgets/{id}"],
            serde_json::json!(["GET", "POST"])
        );
    }

    #[test]
    fn hook_configuration_is_private_and_removed_with_its_owner() {
        let root =
            std::env::temp_dir().join(format!("codeatlas-hook-config-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir(&root).expect("temporary hook directory");
        let config =
            PrivateConfig::create(&root, br#"{"token":"secret"}"#).expect("private hook config");
        let path = config.path.clone();
        #[cfg(unix)]
        assert_eq!(
            path.metadata()
                .expect("config metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        drop(config);
        assert!(!path.exists());
        std::fs::remove_dir(root).expect("temporary hook cleanup");
    }
}

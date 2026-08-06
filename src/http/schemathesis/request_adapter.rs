use crate::execution::WorkloadCommand;
use crate::execution::CALL_CATEGORY_HEADER;
use crate::http::target::HttpFuzzOperation;
use anyhow::{Context, Result};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

pub(super) const API_VERSION: &str = "codeatlas.http-request-adapter/v3";
pub(super) const HOOK_SOURCE: &str = include_str!("hooks.py");

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RequestHooksConfig<'a> {
    api_version: &'static str,
    call_category_header: &'static str,
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
    cwd: &'a str,
}

pub(super) fn render_config(
    operations: &[HttpFuzzOperation],
    headers: &[(String, String)],
    adapter: Option<&WorkloadCommand>,
) -> Result<Vec<u8>> {
    serde_json::to_vec(&RequestHooksConfig {
        api_version: API_VERSION,
        call_category_header: CALL_CATEGORY_HEADER,
        headers: headers
            .iter()
            .map(|(name, value)| HeaderConfig { name, value })
            .collect(),
        adapter: adapter.map(adapter_config).transpose()?,
        methods_by_path: methods_by_path(operations),
    })
    .context("serialize HTTP request-hook configuration")
}

fn adapter_config(adapter: &WorkloadCommand) -> Result<AdapterConfig<'_>> {
    if adapter.executable.is_empty() {
        anyhow::bail!("HTTP request-adapter executable is blank");
    }
    Ok(AdapterConfig {
        command: &adapter.executable,
        args: &adapter.arguments,
        cwd: &adapter.working_directory,
    })
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
    use super::{render_config, API_VERSION, CALL_CATEGORY_HEADER};
    use crate::execution::WorkloadCommand;
    use crate::http::target::parse_http_fuzz_operation;
    use std::collections::BTreeMap;

    #[test]
    fn hook_configuration_is_versioned_and_engine_neutral() {
        let operations = [
            parse_http_fuzz_operation("GET /widgets/{id}").expect("GET operation"),
            parse_http_fuzz_operation("POST /widgets/{id}").expect("POST operation"),
        ];
        let adapter = WorkloadCommand {
            owner: "http_request_adapter".to_string(),
            executable: "/usr/bin/node".to_string(),
            arguments: vec!["adapter.js".to_string()],
            working_directory: "/codeatlas/workspace".to_string(),
            environment: BTreeMap::new(),
            secret_environment_file: None,
        };
        let config: serde_json::Value = serde_json::from_slice(
            &render_config(
                &operations,
                &[("Authorization".to_string(), "Bearer test-token".to_string())],
                Some(&adapter),
            )
            .expect("request adapter configuration"),
        )
        .expect("request adapter JSON");

        assert_eq!(config["apiVersion"], API_VERSION);
        assert_eq!(config["callCategoryHeader"], CALL_CATEGORY_HEADER);
        assert_eq!(config["headers"][0]["name"], "Authorization");
        assert_eq!(config["headers"][0]["value"], "Bearer test-token");
        assert_eq!(config["adapter"]["command"], "/usr/bin/node");
        assert_eq!(config["adapter"]["args"][0], "adapter.js");
        assert_eq!(config["adapter"]["cwd"], "/codeatlas/workspace");
        assert_eq!(
            config["methodsByPath"]["/widgets/{id}"],
            serde_json::json!(["GET", "POST"])
        );
    }
}

use super::openapi::{self, LoadedOpenApi};
use super::runtime::OwnedHttpServer;
use super::toolchain::configured_python;
use crate::config::{ResolvedHttpFuzzHeader, ResolvedHttpOpenApiSource};
use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::process::Command;

const MAX_OPENAPI_BYTES: usize = 16 * 1024 * 1024;
const FETCH_SCRIPT: &str = r#"
import sys
import time
import urllib.request

url = sys.argv[1]
headers = {}
for raw in sys.argv[2:]:
    name, separator, value = raw.partition(":")
    if not separator:
        raise SystemExit(f"Invalid HTTP header argument: {raw!r}")
    headers[name] = value.lstrip()

deadline = time.monotonic() + 30
last_error = None
while time.monotonic() < deadline:
    try:
        request = urllib.request.Request(url, headers=headers)
        with urllib.request.urlopen(request, timeout=3) as response:
            data = response.read(16777217)
            if len(data) > 16777216:
                raise RuntimeError("OpenAPI document exceeds 16 MiB")
            sys.stdout.buffer.write(data)
            raise SystemExit(0)
    except Exception as error:
        last_error = error
        time.sleep(0.1)
raise SystemExit(f"Could not fetch {url}: {last_error}")
"#;

pub(super) fn load(source: &ResolvedHttpOpenApiSource, label: &str) -> Result<LoadedOpenApi> {
    match source {
        ResolvedHttpOpenApiSource::File(path) => openapi::load(path),
        ResolvedHttpOpenApiSource::Command {
            command,
            args,
            cwd,
            environment,
        } => {
            let output = Command::new(command)
                .args(args)
                .current_dir(cwd)
                .envs(environment)
                .output()
                .with_context(|| {
                    format!("Could not run OpenAPI provider command {command:?} for {label}")
                })?;
            if !output.status.success() {
                anyhow::bail!(
                    "OpenAPI provider command {command:?} failed for {label}: {}",
                    concise_stderr(&output.stderr)
                );
            }
            parse_output(&output.stdout, label)
        }
        ResolvedHttpOpenApiSource::Url { url } => {
            let output = fetch_url(url, &BTreeMap::new(), &[])?;
            parse_output(&output, label)
        }
        ResolvedHttpOpenApiSource::Target(target) => {
            let _server = OwnedHttpServer::start(target)?;
            let output = fetch_url(&target.openapi_url, &target.environment, &target.headers)?;
            parse_output(&output, label)
        }
    }
}

fn fetch_url(
    url: &str,
    environment: &BTreeMap<String, String>,
    headers: &[ResolvedHttpFuzzHeader],
) -> Result<Vec<u8>> {
    let python = configured_python();
    let output = Command::new(&python)
        .args(["-c", FETCH_SCRIPT, url])
        .args(
            headers
                .iter()
                .map(|header| format!("{}: {}", header.name, header.value)),
        )
        .envs(environment)
        .output()
        .with_context(|| {
            format!(
                "Could not start OpenAPI URL provider with {}",
                python.display()
            )
        })?;
    if !output.status.success() {
        anyhow::bail!(
            "OpenAPI URL provider failed for {url}: {}",
            concise_stderr(&output.stderr)
        );
    }
    if output.stdout.len() > MAX_OPENAPI_BYTES {
        anyhow::bail!("OpenAPI document from {url} exceeds 16 MiB");
    }
    Ok(output.stdout)
}

fn parse_output(output: &[u8], label: &str) -> Result<LoadedOpenApi> {
    if output.len() > MAX_OPENAPI_BYTES {
        anyhow::bail!("OpenAPI document from {label} exceeds 16 MiB");
    }
    let source = std::str::from_utf8(output)
        .with_context(|| format!("OpenAPI provider output for {label} is not UTF-8"))?;
    openapi::parse(source, label)
}

fn concise_stderr(stderr: &[u8]) -> String {
    String::from_utf8_lossy(stderr)
        .trim()
        .chars()
        .take(1_000)
        .collect()
}

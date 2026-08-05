use super::environment::configured_python;
use super::openapi::{self, LoadedOpenApi};
use super::runtime::OwnedHttpServer;
use super::target::ResolvedHttpOpenApiSource;
use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Write};
use std::process::{Child, Command, ExitStatus, Stdio};
use url::Url;

const MAX_OPENAPI_BYTES: usize = 16 * 1024 * 1024;
const MAX_DIAGNOSTIC_BYTES: usize = 64 * 1024;
const FETCH_SCRIPT: &str = r#"
import json
import sys
import time
import urllib.request

config = json.load(sys.stdin)
url = config["url"]
headers = config["headers"]

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
raise SystemExit(f"Could not fetch configured OpenAPI URL: {last_error}")
"#;

pub(super) fn load(source: &ResolvedHttpOpenApiSource, label: &str) -> Result<LoadedOpenApi> {
    read_with_inventory(source, label).map(|(_, openapi)| openapi)
}

pub(super) fn read_with_inventory(
    source: &ResolvedHttpOpenApiSource,
    label: &str,
) -> Result<(Vec<u8>, LoadedOpenApi)> {
    let output = read(source, label)?;
    let openapi = parse_output(&output, label)?;
    Ok((output, openapi))
}

pub(super) fn read(source: &ResolvedHttpOpenApiSource, label: &str) -> Result<Vec<u8>> {
    match source {
        ResolvedHttpOpenApiSource::File(path) => {
            let mut file = File::open(path)
                .with_context(|| format!("Could not read OpenAPI contract {}", path.display()))?;
            read_limited(&mut file, &path.display().to_string())
        }
        ResolvedHttpOpenApiSource::Command {
            command,
            args,
            cwd,
            environment,
        } => {
            let mut provider = Command::new(command);
            provider.args(args).current_dir(cwd).envs(environment);
            let output = run_provider(
                &mut provider,
                None,
                &format!("OpenAPI provider command {command:?} for {label}"),
            )?;
            if !output.status.success() {
                anyhow::bail!(
                    "OpenAPI provider command {command:?} failed for {label}: {}",
                    concise_diagnostic(&output.stderr)
                );
            }
            Ok(output.stdout)
        }
        ResolvedHttpOpenApiSource::Url { url } => fetch_url(url, &BTreeMap::new(), &[]),
        ResolvedHttpOpenApiSource::Target(target) => {
            let _server = OwnedHttpServer::start(target)?;
            let environment = target.resolve_runtime_environment()?;
            let headers = target.resolve_runtime_headers()?;
            fetch_url(&target.openapi_url, &environment, &headers)
        }
    }
}

fn fetch_url(
    url: &Url,
    environment: &BTreeMap<String, String>,
    headers: &[(String, String)],
) -> Result<Vec<u8>> {
    let python = configured_python();
    let header_values = headers
        .iter()
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect::<BTreeMap<_, _>>();
    let input = serde_json::to_vec(&serde_json::json!({
        "url": url.as_str(),
        "headers": header_values,
    }))?;
    let mut provider = Command::new(&python);
    provider.args(["-c", FETCH_SCRIPT]).envs(environment);
    let output =
        run_provider(&mut provider, Some(&input), "OpenAPI URL provider").with_context(|| {
            format!(
                "Could not run OpenAPI URL provider with {}",
                python.display()
            )
        })?;
    if !output.status.success() {
        anyhow::bail!(
            "OpenAPI URL provider failed: {}",
            concise_diagnostic(&output.stderr)
        );
    }
    Ok(output.stdout)
}

fn parse_output(output: &[u8], label: &str) -> Result<LoadedOpenApi> {
    enforce_size(output, label)?;
    let source = std::str::from_utf8(output)
        .with_context(|| format!("OpenAPI provider output for {label} is not UTF-8"))?;
    openapi::parse(source, label)
}

fn enforce_size(output: &[u8], label: &str) -> Result<()> {
    if output.len() > MAX_OPENAPI_BYTES {
        anyhow::bail!("OpenAPI document from {label} exceeds 16 MiB");
    }
    Ok(())
}

fn read_limited(reader: &mut impl Read, label: &str) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        let count = reader
            .read(&mut buffer)
            .with_context(|| format!("Could not read OpenAPI document from {label}"))?;
        if count == 0 {
            return Ok(output);
        }
        if output.len().saturating_add(count) > MAX_OPENAPI_BYTES {
            anyhow::bail!("OpenAPI document from {label} exceeds 16 MiB");
        }
        output.extend_from_slice(&buffer[..count]);
    }
}

struct ProviderOutput {
    status: ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

fn run_provider(
    command: &mut Command,
    input: Option<&[u8]>,
    label: &str,
) -> Result<ProviderOutput> {
    command
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .with_context(|| format!("Could not start {label}"))?;
    let stderr = child
        .stderr
        .take()
        .context("Provider stderr was not piped")?;
    let stderr_reader = std::thread::spawn(move || read_diagnostic(stderr));
    if let Some(input) = input {
        let write_result = child
            .stdin
            .take()
            .context("Provider stdin was not piped")?
            .write_all(input);
        if let Err(error) = write_result {
            terminate(&mut child);
            let _ = stderr_reader.join();
            return Err(error).with_context(|| format!("Could not configure {label}"));
        }
    }
    let mut stdout = child
        .stdout
        .take()
        .context("Provider stdout was not piped")?;
    let output = match read_limited(&mut stdout, label) {
        Ok(output) => output,
        Err(error) => {
            terminate(&mut child);
            let _ = stderr_reader.join();
            return Err(error);
        }
    };
    let status = child
        .wait()
        .with_context(|| format!("Could not wait for {label}"))?;
    let stderr = stderr_reader
        .join()
        .map_err(|_| anyhow::anyhow!("{label} diagnostic reader panicked"))?;
    Ok(ProviderOutput {
        status,
        stdout: output,
        stderr,
    })
}

fn read_diagnostic(mut reader: impl Read) -> Vec<u8> {
    let mut diagnostic = Vec::new();
    let mut buffer = [0_u8; 8 * 1024];
    loop {
        let Ok(count) = reader.read(&mut buffer) else {
            return diagnostic;
        };
        if count == 0 {
            return diagnostic;
        }
        let remaining = MAX_DIAGNOSTIC_BYTES.saturating_sub(diagnostic.len());
        diagnostic.extend_from_slice(&buffer[..count.min(remaining)]);
    }
}

fn terminate(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn concise_diagnostic(stderr: &[u8]) -> String {
    String::from_utf8_lossy(stderr)
        .trim()
        .chars()
        .take(1_000)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        read, read_diagnostic, read_limited, run_provider, MAX_DIAGNOSTIC_BYTES, MAX_OPENAPI_BYTES,
    };
    use crate::http::environment::configured_python;
    use crate::http::target::ResolvedHttpOpenApiSource;
    use std::collections::BTreeMap;
    use std::io::Cursor;
    use std::process::Command;
    use std::time::{Duration, Instant};

    #[test]
    fn provider_streams_enforce_payload_and_diagnostic_bounds() {
        let mut accepted = Cursor::new(vec![b'a'; MAX_OPENAPI_BYTES]);
        assert_eq!(
            read_limited(&mut accepted, "boundary document")
                .expect("boundary-sized document")
                .len(),
            MAX_OPENAPI_BYTES
        );

        let mut oversized = Cursor::new(vec![b'a'; MAX_OPENAPI_BYTES + 1]);
        assert!(read_limited(&mut oversized, "oversized document")
            .expect_err("oversized provider output")
            .to_string()
            .contains("exceeds 16 MiB"));

        assert_eq!(
            read_diagnostic(Cursor::new(vec![b'e'; MAX_DIAGNOSTIC_BYTES + 1])).len(),
            MAX_DIAGNOSTIC_BYTES
        );
    }

    #[test]
    fn failed_child_provider_reports_its_diagnostic() {
        let python = configured_python();
        let source = ResolvedHttpOpenApiSource::Command {
            command: python.to_string_lossy().into_owned(),
            args: vec![
                "-c".to_string(),
                "import sys; sys.stderr.write('provider exploded\\n'); raise SystemExit(7)"
                    .to_string(),
            ],
            cwd: std::env::current_dir().expect("current directory"),
            environment: BTreeMap::new(),
        };

        let error = read(&source, "failing fixture")
            .expect_err("failed provider")
            .to_string();
        assert!(error.contains("provider exploded"), "{error}");
    }

    #[test]
    fn oversized_child_provider_is_terminated_before_it_can_linger() {
        let python = configured_python();
        let mut provider = Command::new(python);
        provider.args([
            "-c",
            "import sys,time; sys.stdout.buffer.write(b'x' * 16777217); sys.stdout.flush(); time.sleep(30)",
        ]);

        let started = Instant::now();
        let error = match run_provider(&mut provider, None, "oversized child provider") {
            Ok(_) => panic!("oversized child output should fail"),
            Err(error) => error.to_string(),
        };

        assert!(error.contains("exceeds 16 MiB"), "{error}");
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "oversized provider was not terminated promptly"
        );
    }
}

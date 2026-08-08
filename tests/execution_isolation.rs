mod support;

use self::support::TestDirectory;
use codeatlas_isolation_conformance::WORKSPACE_MOUNT;
use serde_json::{json, Value};
use std::fs::{self, OpenOptions};
use std::io::{Error, ErrorKind, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
#[cfg(unix)]
use std::os::unix::net::UnixListener;

const PROBE_IMAGE: &str =
    "fixture/codeatlas-probe@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const HEADER_SECRET: &str = "fixture-header-secret-value";
const RUNTIME_SECRET: &str = "fixture-runtime-secret-value";
static NEXT_SOCKET_ID: AtomicU64 = AtomicU64::new(0);

#[cfg(unix)]
fn write_live_receipt(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if !path.is_absolute() {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "live receipt path must be absolute",
        ));
    }
    let parent = path
        .parent()
        .ok_or_else(|| Error::new(ErrorKind::InvalidInput, "live receipt path has no parent"))?
        .canonicalize()?;
    let checkout = Path::new(env!("CARGO_MANIFEST_DIR")).canonicalize()?;
    if parent.starts_with(&checkout) || checkout.starts_with(&parent) {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "live receipt path must be disjoint from the CodeAtlas checkout",
        ));
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    if file.metadata()?.permissions().mode() & 0o777 != 0o600 {
        return Err(Error::new(
            ErrorKind::PermissionDenied,
            "live receipt file is not owner-only",
        ));
    }
    Ok(())
}

fn run_codeatlas(root: &Path, state: &Path, args: &[&str]) -> Output {
    codeatlas_command(root, state)
        .args(args)
        .output()
        .expect("CodeAtlas isolation command should start")
}

fn codeatlas_command(root: &Path, state: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_codeatlas"));
    command
        .arg("--root")
        .arg(root)
        .env("CODEATLAS_STATE_DIR", state)
        .env("CODEATLAS_CACHE_DIR", state.join("cache"))
        .env("CODEATLAS_TEST_AMBIENT_SECRET", "must-not-enter-sandbox")
        .env("CODEATLAS_TEST_HEADER_SECRET", HEADER_SECRET)
        .env("CODEATLAS_TEST_RUNTIME_SECRET", RUNTIME_SECRET);
    command
}

fn assert_no_target_call(listener: &TcpListener) {
    listener
        .set_nonblocking(true)
        .expect("target listener should become nonblocking");
    match listener.accept() {
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
        Ok((_stream, address)) => panic!("isolation probe contacted target at {address}"),
        Err(error) => panic!("could not inspect target listener: {error}"),
    }
}

#[cfg(unix)]
struct IsolationFixture {
    _directory: TestDirectory,
    workspace: PathBuf,
    state: PathBuf,
    runtime: PathBuf,
    runtime_socket: UnixListener,
    runtime_socket_path: PathBuf,
    target: TcpListener,
}

#[cfg(unix)]
impl IsolationFixture {
    fn create(prefix: &str) -> Self {
        let directory = TestDirectory::create(prefix);
        let workspace = directory.path().join("workspace");
        let state = directory.path().join("state");
        fs::create_dir_all(workspace.join("src")).expect("workspace source");
        fs::create_dir_all(&state).expect("external state root");
        fs::write(workspace.join("src/server.rs"), "fn serve() {}\n").expect("source fixture");
        fs::write(
            workspace.join("openapi.json"),
            serde_json::to_vec_pretty(&json!({
                "openapi": "3.0.3",
                "info": {"title": "fixture", "version": "1"},
                "paths": {
                    "/health": {
                        "get": {"responses": {"200": {"description": "ok"}}}
                    }
                }
            }))
            .expect("OpenAPI JSON"),
        )
        .expect("OpenAPI fixture");
        fs::write(
            workspace.join("server.py"),
            r#"from http.server import BaseHTTPRequestHandler, HTTPServer
import sys


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def do_GET(self):
        body = b'{"status":"ok"}'
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, _format, *_arguments):
        pass


HTTPServer(("127.0.0.1", int(sys.argv[1])), Handler).serve_forever()
"#,
        )
        .expect("managed HTTP server fixture");
        let runtime = directory.path().join("fake-runtime.py");
        fs::copy(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/execution_isolation/fake_runtime.py"),
            &runtime,
        )
        .expect("copy fake runtime outside checkout");
        fs::set_permissions(&runtime, fs::Permissions::from_mode(0o700))
            .expect("fake runtime permissions");
        let socket_path = PathBuf::from(format!(
            "/tmp/codeatlas-oci-{}-{}.sock",
            std::process::id(),
            NEXT_SOCKET_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_file(&socket_path);
        let runtime_socket = UnixListener::bind(&socket_path).expect("runtime socket fixture");
        let target = TcpListener::bind("127.0.0.1:0").expect("target listener");
        let address = target.local_addr().expect("target address");
        fs::write(
            workspace.join("codeatlas.json"),
            serde_json::to_vec_pretty(&json!({
                "root": ".",
                "package_exports": false,
                "execution": {
                    "limits": {
                        "max_calls": 5,
                        "calls_per_second": 5,
                        "max_concurrency": 1,
                        "run_timeout_ms": 10000,
                        "max_cpu_time_ms": 2000,
                        "max_rss_bytes": 67108864,
                        "max_processes": 2,
                        "max_open_files": 32,
                        "max_output_bytes": 65536
                    },
                    "isolation": {
                        "backend": "container",
                        "filesystem": "scratch_only",
                        "network": "proxy_only",
                        "processes": "planned_only",
                        "container": {
                            "executable": runtime,
                            "socket": socket_path,
                            "probe_image": PROBE_IMAGE
                        }
                    }
                },
                "fuzz": {"limits": {"max_cases": 1, "max_failures": 1}},
                "http": {
                    "contracts": [{"id": "fixture", "openapi": "openapi.json"}],
                    "fuzz": {"targets": [{
                        "id": "local",
                        "contract": "fixture",
                        "base_url": format!("http://{address}"),
                        "operations": "contract"
                    }]}
                }
            }))
            .expect("CodeAtlas config JSON"),
        )
        .expect("CodeAtlas config fixture");
        Self {
            _directory: directory,
            workspace,
            state,
            runtime,
            runtime_socket,
            runtime_socket_path: socket_path,
            target,
        }
    }

    fn set_mode(&self, mode: &str) {
        fs::write(self.runtime.with_extension("mode"), mode).expect("fake runtime mode");
    }

    fn enable_workload(&self, preauthorized: bool) {
        let workspace = &self.workspace;
        self.update_config(|config| {
            config["http"]["fuzz"]["image"] = Value::String(PROBE_IMAGE.to_string());
            let target = &mut config["http"]["fuzz"]["targets"][0];
            target["preauthorized"] = json!(preauthorized);
            target["server"] = json!({
                "command": "/usr/bin/python3",
                "args": ["-c", "raise SystemExit('fixture runtime executes the managed target')"],
                "cwd": workspace
            });
        });
    }

    fn enable_live_workload(&self, image: &str) {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/http");
        for filename in ["openapi.yaml", "server.py", "request_adapter.py"] {
            fs::copy(fixture.join(filename), self.workspace.join(filename))
                .unwrap_or_else(|error| panic!("copy live HTTP {filename} fixture: {error}"));
        }
        let workspace = &self.workspace;
        let port = self
            .target
            .local_addr()
            .expect("target fixture address")
            .port();
        self.update_config(|config| {
            config["http"]["contracts"][0]["openapi"] = Value::String("openapi.yaml".to_string());
            config["http"]["fuzz"]["image"] = Value::String(image.to_string());
            let target = &mut config["http"]["fuzz"]["targets"][0];
            target["preauthorized"] = Value::Bool(false);
            target["headers"] = json!([{
                "name": "X-CodeAtlas-Static",
                "value": "fixture-static-token"
            }]);
            target["server"] = json!({
                "command": "/usr/local/bin/python3",
                "args": [
                    "server.py",
                    port.to_string(),
                    "openapi.yaml",
                    "fixture-runtime-token"
                ],
                "cwd": workspace,
                "startup_timeout_seconds": 15
            });
            target["request_adapter"] = json!({
                "command": "/usr/local/bin/python3",
                "args": ["request_adapter.py", "/codeatlas/scratch/adapter.ndjson"],
                "cwd": workspace
            });
        });
    }

    fn enable_live_python_code_workload(&self, image: &str) {
        fs::write(
            self.workspace.join("safe.py"),
            r#"import os


def fails_at_or_above_two(value: int) -> int:
    if os.environ.get("CODEATLAS_FUZZ") != "1":
        raise RuntimeError("planned fuzz marker missing")
    # A monotone adaptive predicate guarantees a native shrink path to 2.
    if value >= 2:
        raise ValueError("deterministic native-engine fixture")
    return value
"#,
        )
        .expect("live Python code-fuzz fixture");
        self.update_config(|config| {
            config["projects"] = json!([{
                "id": "python-live",
                "root": ".",
                "languages": ["py"],
                "contexts": {
                    "public-api": {
                        "role": "production",
                        "scope": "public_surface",
                        "entrypoints": ["safe.py"]
                    }
                }
            }]);
            config["fuzz"]["code"] = json!({
                "targets": [{
                    "id": "python-live",
                    "project": "python-live",
                    "language": "python",
                    "image": image,
                    "preauthorized": true
                }]
            });
        });
    }

    fn enable_live_rust_code_workload(&self, image: &str) {
        fs::write(
            self.workspace.join("Cargo.toml"),
            "[package]\nname = \"codeatlas-rust-live\"\nversion = \"0.0.0\"\nedition = \"2021\"\n",
        )
        .expect("live Rust manifest fixture");
        fs::write(
            self.workspace.join("src/lib.rs"),
            r#"pub fn fails_in_shrinkable_window(value: i8) -> i8 {
    assert_eq!(std::env::var("CODEATLAS_FUZZ").as_deref(), Ok("1"));
    if (2..=64).contains(&value) {
        panic!("deterministic native-engine fixture");
    }
    value
}
"#,
        )
        .expect("live Rust code-fuzz fixture");
        self.update_config(|config| {
            config["projects"] = json!([{
                "id": "rust-live",
                "root": ".",
                "languages": ["rs"],
                "contexts": {
                    "public-api": {
                        "role": "production",
                        "scope": "public_surface",
                        "entrypoints": ["src/lib.rs"]
                    }
                }
            }]);
            config["fuzz"]["code"] = json!({
                "targets": [{
                    "id": "rust-live",
                    "project": "rust-live",
                    "language": "rust",
                    "image": image,
                    "preauthorized": true
                }]
            });
        });
    }

    fn enable_secrets(&self) {
        self.update_config(|config| {
            let target = &mut config["http"]["fuzz"]["targets"][0];
            target["secret_environment"] = json!({
                "FIXTURE_RUNTIME_SECRET": "CODEATLAS_TEST_RUNTIME_SECRET"
            });
            target["headers"] = json!([{
                "name": "Authorization",
                "value_env": "CODEATLAS_TEST_HEADER_SECRET"
            }]);
        });
    }

    fn enable_request_adapter(&self) {
        fs::write(
            self.workspace.join("adapter.py"),
            "raise SystemExit('fake runtime must not execute the host adapter')\n",
        )
        .expect("request-adapter fixture");
        let workspace = &self.workspace;
        self.update_config(|config| {
            config["http"]["fuzz"]["targets"][0]["request_adapter"] = json!({
                "command": "/usr/bin/python3",
                "args": ["adapter.py"],
                "cwd": workspace
            });
        });
    }

    fn set_environment_class(&self, class: &str) {
        self.update_config(|config| {
            let target = &mut config["http"]["fuzz"]["targets"][0];
            target["environment_class"] = Value::String(class.to_string());
        });
    }

    fn set_base_url(&self, base_url: &str) {
        self.update_config(|config| {
            config["http"]["fuzz"]["targets"][0]["base_url"] = Value::String(base_url.to_string());
        });
    }

    fn update_config(&self, update: impl FnOnce(&mut Value)) {
        let path = self.workspace.join("codeatlas.json");
        let mut config: Value =
            serde_json::from_slice(&fs::read(&path).expect("HTTP target config fixture"))
                .expect("HTTP target config JSON");
        update(&mut config);
        fs::write(
            path,
            serde_json::to_vec_pretty(&config).expect("HTTP target config bytes"),
        )
        .expect("write HTTP target config");
    }

    fn plan(&self) -> Value {
        self.plan_with(&["--seed", "42"])
    }

    fn plan_with(&self, extra: &[&str]) -> Value {
        let mut arguments = vec!["fuzz", "http", "--target", "local"];
        arguments.extend_from_slice(extra);
        let output = run_codeatlas(&self.workspace, &self.state, &arguments);
        assert!(
            output.status.success(),
            "planning failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_no_target_call(&self.target);
        serde_json::from_slice(&output.stdout).expect("execution plan JSON")
    }

    fn plan_code(&self, target: &str, symbol: &str, extra: &[&str]) -> Value {
        let mut arguments = vec!["fuzz", "code", "--target", target, "--symbol", symbol];
        arguments.extend_from_slice(extra);
        let output = run_codeatlas(&self.workspace, &self.state, &arguments);
        assert!(
            output.status.success(),
            "code planning failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).expect("code execution plan JSON")
    }

    fn plan_code_replay(&self, reproducer: &str) -> Value {
        let output = run_codeatlas(
            &self.workspace,
            &self.state,
            &["fuzz", "code", "--replay", reproducer],
        );
        assert!(
            output.status.success(),
            "code replay planning failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).expect("code replay plan JSON")
    }

    fn execute(&self, plan: &Value) -> Value {
        self.execute_with_status(plan, 2)
    }

    fn execute_with_status(&self, plan: &Value, expected_status: i32) -> Value {
        self.execute_with_status_and_export(plan, expected_status, true)
    }

    fn execute_with_status_and_export(
        &self,
        plan: &Value,
        expected_status: i32,
        export: bool,
    ) -> Value {
        let plan_id = plan["id"].as_str().expect("plan ID");
        let output = run_codeatlas(
            &self.workspace,
            &self.state,
            &["fuzz", "http", "--plan", plan_id, "--execute"],
        );
        assert_eq!(
            output.status.code(),
            Some(expected_status),
            "execution stdout:\n{}\nexecution stderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_no_target_call(&self.target);
        if export {
            if let Some(path) = std::env::var_os("CODEATLAS_TEST_OCI_RECEIPT_OUT") {
                write_live_receipt(Path::new(&path), &output.stdout)
                    .expect("persist private external live receipt");
            }
        }
        serde_json::from_slice(&output.stdout).expect("execution receipt JSON")
    }

    fn execute_code(&self, plan: &Value, expected_status: i32) -> Value {
        let plan_id = plan["id"].as_str().expect("code plan ID");
        let output = run_codeatlas(
            &self.workspace,
            &self.state,
            &["fuzz", "code", "--plan", plan_id, "--execute"],
        );
        assert_eq!(
            output.status.code(),
            Some(expected_status),
            "code execution stdout:\n{}\ncode execution stderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).expect("code execution receipt JSON")
    }

    fn execute_and_interrupt(&self, plan: &Value, marker: &Path) -> Value {
        let plan_id = plan["id"].as_str().expect("plan ID");
        let mut child = codeatlas_command(&self.workspace, &self.state)
            .args(["fuzz", "http", "--plan", plan_id, "--execute"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("interruptible CodeAtlas execution");
        let marker_deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !marker.is_file() {
            if let Some(status) = child.try_wait().expect("inspect interruptible execution") {
                let output = child
                    .wait_with_output()
                    .expect("collect early interruptible execution");
                panic!(
                    "execution exited before its interrupt marker ({status}): {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            if std::time::Instant::now() >= marker_deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("execution did not expose its interrupt marker");
            }
            std::thread::yield_now();
        }
        let interrupt = Command::new("/bin/kill")
            .args(["-INT", &child.id().to_string()])
            .status()
            .expect("send SIGINT to CodeAtlas");
        assert!(interrupt.success());
        let output = child
            .wait_with_output()
            .expect("collect interrupted CodeAtlas execution");
        assert_eq!(
            output.status.code(),
            Some(2),
            "interrupted stdout:\n{}\ninterrupted stderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).expect("cancelled execution receipt JSON")
    }

    fn assert_runtime_absent(&self) {
        let state: Value = serde_json::from_slice(
            &fs::read(self.runtime.with_extension("state.json"))
                .expect("fake runtime state after cleanup"),
        )
        .expect("fake runtime state JSON");
        assert_eq!(state["exists"], false);
        assert_eq!(state["sentinel_verified"], true);
    }

    fn linked_artifact(&self, link: &Value, directory: &str) -> Value {
        let id = link["id"].as_str().expect("linked artifact ID");
        let path = self
            .state
            .join("codeatlas/execution/v1")
            .join(directory)
            .join(format!("{id}.json"));
        let artifact: Value = serde_json::from_slice(&fs::read(&path).unwrap_or_else(|error| {
            panic!("could not read linked artifact {}: {error}", path.display())
        }))
        .expect("linked artifact JSON");
        assert_eq!(artifact["id"], link["id"]);
        assert_eq!(artifact["content_digest"], link["content_digest"]);
        artifact
    }

    fn report(&self, receipt: &Value) -> Value {
        let link = receipt["links"]
            .as_array()
            .expect("receipt artifact links")
            .iter()
            .find(|link| link["kind"] == "report")
            .expect("fuzz report link");
        self.linked_artifact(link, "reports")
    }

    fn assert_state_excludes(&self, values: &[&str]) {
        for entry in walkdir::WalkDir::new(&self.state).follow_links(false) {
            let entry = entry.expect("external state entry");
            if !entry.file_type().is_file() {
                continue;
            }
            let bytes = fs::read(entry.path()).expect("external state bytes");
            for value in values {
                assert!(
                    !bytes
                        .windows(value.len())
                        .any(|window| window == value.as_bytes()),
                    "secret value survived in {}",
                    entry.path().display()
                );
            }
        }
    }
}

#[cfg(unix)]
impl Drop for IsolationFixture {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.runtime_socket_path);
    }
}

#[cfg(unix)]
#[test]
fn live_receipt_export_is_private_external_and_immutable() {
    let directory = TestDirectory::create("codeatlas-live-receipt");
    let receipt = directory.path().join("receipt.json");
    write_live_receipt(&receipt, b"{\"outcome\":\"blocked\"}\n")
        .expect("write external receipt evidence");
    assert_eq!(
        fs::read(&receipt).expect("read receipt evidence"),
        b"{\"outcome\":\"blocked\"}\n"
    );
    assert_eq!(
        fs::metadata(&receipt)
            .expect("receipt metadata")
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert_eq!(
        write_live_receipt(&receipt, b"replacement")
            .expect_err("receipt evidence must not be overwritten")
            .kind(),
        ErrorKind::AlreadyExists
    );
    let checkout_receipt = Path::new(env!("CARGO_MANIFEST_DIR")).join("receipt.json");
    assert_eq!(
        write_live_receipt(&checkout_receipt, b"forbidden")
            .expect_err("receipt evidence must stay outside the checkout")
            .kind(),
        ErrorKind::InvalidInput
    );
}

#[cfg(unix)]
#[test]
fn missing_workload_image_blocks_before_the_runtime_probe() {
    let fixture = IsolationFixture::create("codeatlas-workload-image-block");
    let plan = fixture.plan();
    let receipt = fixture.execute(&plan);

    assert_eq!(receipt["outcome"], "blocked");
    assert!(receipt["reasons"]
        .as_array()
        .expect("blocked reasons")
        .iter()
        .any(|reason| reason
            .as_str()
            .is_some_and(|reason| reason.contains("no managed image"))));
    assert!(!fixture.runtime.with_extension("log").exists());
    assert_no_target_call(&fixture.target);
}

#[cfg(unix)]
#[test]
fn managed_http_planning_accepts_one_explicit_seed() {
    let fixture = IsolationFixture::create("codeatlas-explicit-http-seed");
    let plan = fixture.plan_with(&["--seed", "43"]);

    assert_eq!(plan["workload"]["body"]["seed"], "43");
}

#[cfg(unix)]
#[test]
fn missing_secret_blocks_inside_the_kernel_before_the_runtime_probe() {
    let fixture = IsolationFixture::create("codeatlas-workload-secret-block");
    fixture.enable_workload(false);
    fixture.enable_secrets();
    let plan = fixture.plan();
    let plan_id = plan["id"].as_str().expect("plan ID");
    let output = codeatlas_command(&fixture.workspace, &fixture.state)
        .env_remove("CODEATLAS_TEST_RUNTIME_SECRET")
        .args(["fuzz", "http", "--plan", plan_id, "--execute"])
        .output()
        .expect("missing-secret execution should start");
    assert_eq!(
        output.status.code(),
        Some(2),
        "missing-secret stdout:\n{}\nmissing-secret stderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let receipt: Value =
        serde_json::from_slice(&output.stdout).expect("missing-secret execution receipt JSON");
    assert_eq!(receipt["outcome"], "blocked");
    assert!(receipt["reasons"]
        .as_array()
        .expect("missing-secret reasons")
        .iter()
        .any(|reason| reason.as_str().is_some_and(|reason| {
            reason.contains("needs secret environment reference CODEATLAS_TEST_RUNTIME_SECRET")
        })));
    assert!(!fixture.runtime.with_extension("log").exists());
    assert_no_target_call(&fixture.target);
}

#[cfg(unix)]
#[test]
fn production_is_blocked_and_uncontained_targets_cannot_use_single_shot() {
    let production = IsolationFixture::create("codeatlas-production-target-block");
    production.enable_workload(false);
    production.set_environment_class("production");
    let plan = production.plan();
    assert_eq!(plan["authorization"]["disposition"], "blocked");
    let receipt = production.execute(&plan);
    assert_eq!(receipt["outcome"], "blocked");
    assert_eq!(receipt["cleanup"], json!([]));
    assert!(!production.runtime.with_extension("log").exists());
    assert_no_target_call(&production.target);

    let remote = IsolationFixture::create("codeatlas-remote-single-shot-block");
    remote.enable_workload(true);
    remote.set_environment_class("disposable");
    remote.set_base_url("https://remote.example.invalid:443");
    let output = run_codeatlas(
        &remote.workspace,
        &remote.state,
        &[
            "fuzz",
            "http",
            "--target",
            "local",
            "--seed",
            "42",
            "--execute",
        ],
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("requires reviewed authorization"));
    assert!(!remote.runtime.with_extension("log").exists());
    assert_no_target_call(&remote.target);

    let local_external = IsolationFixture::create("codeatlas-local-external-single-shot-block");
    local_external.update_config(|config| {
        config["http"]["fuzz"]["image"] = Value::String(PROBE_IMAGE.to_string());
        let target = &mut config["http"]["fuzz"]["targets"][0];
        target["environment_class"] = Value::String("disposable".to_string());
        target["preauthorized"] = Value::Bool(true);
    });
    let output = run_codeatlas(
        &local_external.workspace,
        &local_external.state,
        &[
            "fuzz",
            "http",
            "--target",
            "local",
            "--seed",
            "42",
            "--execute",
        ],
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("requires reviewed authorization"));
    assert!(!local_external.runtime.with_extension("log").exists());
    assert_no_target_call(&local_external.target);
}

#[cfg(unix)]
#[test]
fn target_observed_container_contract_grants_only_proven_capabilities() {
    let fixture = IsolationFixture::create("codeatlas-isolation-conformance");
    fixture.enable_workload(false);
    fixture.enable_secrets();
    fixture.enable_request_adapter();
    let plan = fixture.plan();
    let receipt = fixture.execute_with_status(&plan, 0);
    assert_eq!(receipt["outcome"], "passed");
    let report = fixture.report(&receipt);
    assert_eq!(report["schema_version"], "codeatlas.http-fuzz-report/v1");
    assert_eq!(report["plan_id"], plan["id"]);
    assert_eq!(report["plan_content_digest"], plan["content_digest"]);
    assert_eq!(
        fs::read_to_string(fixture.runtime.with_extension("target-calls"))
            .expect("target-side call count"),
        "1"
    );
    let capabilities = receipt["runtime"]["capabilities"]
        .as_array()
        .expect("runtime capabilities");
    for capability in [
        "cleanup_verification",
        "network_allowlist",
        "process_allowlist",
        "read_only_checkout",
        "read_only_runtime",
        "resource_limits",
        "scratch_filesystem",
    ] {
        assert!(
            capabilities.iter().any(|value| value == capability),
            "missing capability {capability}"
        );
    }
    assert_eq!(receipt["runtime"]["rootless"], true);
    assert!(receipt["runtime"]["nested"].is_boolean());
    assert!(receipt["runtime"]["environment_digest"]
        .as_str()
        .is_some_and(|digest| digest.starts_with("sha256:") && digest.len() == 71));
    assert!(receipt["resources"]["output_bytes"]
        .as_u64()
        .is_some_and(|bytes| bytes > 0 && bytes <= 65536));
    assert_eq!(receipt["resources"]["cpu_time_ms"], 1);
    assert_eq!(receipt["resources"]["peak_rss_bytes"], 4096);
    assert_eq!(receipt["cleanup"].as_array().map(Vec::len), Some(4));
    assert!(receipt["cleanup"]
        .as_array()
        .expect("cleanup evidence")
        .iter()
        .all(|cleanup| cleanup["released"] == true && cleanup["verified"] == true));
    fixture.assert_runtime_absent();

    let invocations = fs::read_to_string(fixture.runtime.with_extension("log"))
        .expect("fake runtime invocation log")
        .lines()
        .map(|line| serde_json::from_str::<Vec<String>>(line).expect("runtime invocation JSON"))
        .collect::<Vec<_>>();
    let create = invocations
        .iter()
        .find(|arguments| {
            arguments
                .windows(2)
                .any(|pair| pair == ["container", "create"])
        })
        .expect("container create invocation");
    let container_index = create
        .iter()
        .position(|argument| argument == "container")
        .expect("container command boundary");
    let child_visible = &create[container_index..];
    assert!(child_visible
        .iter()
        .any(|argument| argument == "--read-only"));
    assert!(child_visible
        .iter()
        .any(|argument| argument == "--cap-drop"));
    assert!(!child_visible.iter().any(|argument| {
        argument.contains("runtime.sock")
            || argument.contains("CODEATLAS_TEST_AMBIENT_SECRET")
            || argument.contains("must-not-enter-sandbox")
    }));
    let workspace_destination = format!("dst={WORKSPACE_MOUNT}");
    let workspace_mount = child_visible
        .windows(2)
        .find(|pair| pair[0] == "--mount" && pair[1].contains(&workspace_destination))
        .map(|pair| &pair[1])
        .expect("disposable workspace mount");
    let workspace_path = fixture.workspace.to_string_lossy();
    let state_path = fixture.state.to_string_lossy();
    assert!(!workspace_mount.contains(workspace_path.as_ref()));
    assert!(workspace_mount.contains(state_path.as_ref()));
    assert!(fixture.runtime_socket.local_addr().is_ok());
    fixture.assert_state_excludes(&[HEADER_SECRET, RUNTIME_SECRET]);
}

#[cfg(unix)]
#[test]
fn preauthorized_single_shot_uses_the_same_kernel_runner() {
    let fixture = IsolationFixture::create("codeatlas-http-single-shot");
    fixture.enable_workload(true);
    let output = run_codeatlas(
        &fixture.workspace,
        &fixture.state,
        &[
            "fuzz",
            "http",
            "--target",
            "local",
            "--seed",
            "42",
            "--execute",
        ],
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "single-shot stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let receipt: Value =
        serde_json::from_slice(&output.stdout).expect("single-shot execution receipt JSON");
    assert_eq!(receipt["outcome"], "passed");
    assert_eq!(receipt["authorization_mode"], "preauthorized_isolated");
    assert!(receipt["links"]
        .as_array()
        .expect("single-shot artifact links")
        .iter()
        .any(|link| link["kind"] == "plan"));
    assert_eq!(
        fs::read_to_string(fixture.runtime.with_extension("target-calls"))
            .expect("single-shot target-side calls"),
        "1"
    );
    fixture.assert_runtime_absent();
    assert_no_target_call(&fixture.target);
}

#[cfg(unix)]
#[test]
fn call_budget_exhaustion_is_partial_and_never_reaches_the_target() {
    let fixture = IsolationFixture::create("codeatlas-http-call-budget");
    fixture.enable_workload(false);
    fixture.set_mode("workload-budget");
    let plan = fixture.plan();
    let receipt = fixture.execute(&plan);

    assert_eq!(receipt["outcome"], "partial");
    assert_eq!(receipt["calls"]["consumed"], 5);
    assert_eq!(
        fs::read_to_string(fixture.runtime.with_extension("target-calls"))
            .expect("budget target-side calls"),
        "5"
    );
    fixture.assert_runtime_absent();
    assert_no_target_call(&fixture.target);
}

#[cfg(unix)]
#[test]
fn incomplete_workload_cleanup_is_explicit_and_can_never_pass() {
    let fixture = IsolationFixture::create("codeatlas-workload-cleanup-incomplete");
    fixture.enable_workload(false);
    fixture.set_mode("workload-cleanup-total-fail");
    let plan = fixture.plan();
    let receipt = fixture.execute(&plan);

    assert_eq!(receipt["outcome"], "partial");
    assert!(receipt["reasons"]
        .as_array()
        .expect("incomplete cleanup reasons")
        .iter()
        .any(|reason| reason
            .as_str()
            .is_some_and(|reason| reason.contains("cleanup was incomplete"))));
    assert!(receipt["cleanup"]
        .as_array()
        .expect("incomplete cleanup evidence")
        .iter()
        .any(|cleanup| cleanup["resource"]
            .as_str()
            .is_some_and(|resource| resource.starts_with("oci_container:"))
            && cleanup["verified"] == false));
    assert_eq!(
        fs::read_to_string(fixture.runtime.with_extension("target-calls"))
            .expect("cleanup-failure target-side calls"),
        "1"
    );
    assert_no_target_call(&fixture.target);
}

#[cfg(unix)]
#[test]
fn failed_target_observation_blocks_and_cleanup_still_verifies() {
    let fixture = IsolationFixture::create("codeatlas-isolation-negative");
    fixture.enable_workload(false);
    fixture.set_mode("network-leak");
    let plan = fixture.plan();
    let receipt = fixture.execute(&plan);
    assert!(receipt["runtime"]["capabilities"]
        .as_array()
        .expect("runtime capabilities")
        .iter()
        .all(|capability| capability != "network_allowlist"));
    assert!(
        receipt["reasons"]
            .as_array()
            .expect("receipt reasons")
            .iter()
            .any(|reason| reason
                .as_str()
                .is_some_and(|reason| reason.contains("external network denial"))),
        "unexpected receipt reasons: {}",
        receipt["reasons"]
    );
    assert!(receipt["cleanup"]
        .as_array()
        .expect("cleanup evidence")
        .iter()
        .all(|cleanup| cleanup["verified"] == true));
    fixture.assert_runtime_absent();
}

#[cfg(unix)]
#[test]
fn execution_and_primary_cleanup_failures_leave_no_container() {
    for mode in ["start-fail", "output-exhausted", "cleanup-primary-fail"] {
        let fixture = IsolationFixture::create("codeatlas-isolation-cleanup");
        fixture.enable_workload(false);
        fixture.set_mode(mode);
        let plan = fixture.plan();
        let receipt = fixture.execute(&plan);
        assert!(receipt["reasons"]
            .as_array()
            .expect("receipt reasons")
            .iter()
            .any(|reason| reason
                .as_str()
                .is_some_and(|reason| reason.contains("isolation conformance failed"))));
        assert!(receipt["cleanup"]
            .as_array()
            .expect("cleanup evidence")
            .iter()
            .all(|cleanup| cleanup["verified"] == true));
        fixture.assert_runtime_absent();
    }
}

#[cfg(unix)]
#[test]
fn interrupt_cancels_the_probe_then_runs_verified_cleanup() {
    let fixture = IsolationFixture::create("codeatlas-isolation-interrupt");
    fixture.enable_workload(false);
    fixture.set_mode("hang");
    let plan = fixture.plan();
    let started = fixture.runtime.with_extension("started");
    let receipt = fixture.execute_and_interrupt(&plan, &started);
    assert_eq!(receipt["outcome"], "cancelled");
    assert!(receipt["reasons"]
        .as_array()
        .expect("cancelled reasons")
        .iter()
        .any(|reason| reason
            .as_str()
            .is_some_and(|reason| reason.contains("cancelled"))));
    assert!(receipt["cleanup"]
        .as_array()
        .expect("cancelled cleanup evidence")
        .iter()
        .all(|cleanup| cleanup["verified"] == true));
    fixture.assert_runtime_absent();
    assert_no_target_call(&fixture.target);
}

#[cfg(unix)]
#[test]
fn interrupt_cancels_the_workload_then_runs_verified_cleanup() {
    let fixture = IsolationFixture::create("codeatlas-workload-interrupt");
    fixture.enable_workload(false);
    fixture.set_mode("workload-hang");
    let plan = fixture.plan();
    let started = fixture.runtime.with_extension("workload-started");
    let receipt = fixture.execute_and_interrupt(&plan, &started);
    assert_eq!(receipt["outcome"], "cancelled");
    assert!(receipt["reasons"]
        .as_array()
        .expect("cancelled workload reasons")
        .iter()
        .any(|reason| reason.as_str().is_some_and(|reason| reason
            .contains("Container workload was cancelled before producing its result"))));
    assert!(receipt["cleanup"]
        .as_array()
        .expect("cancelled workload cleanup evidence")
        .iter()
        .all(|cleanup| cleanup["verified"] == true));
    fixture.assert_runtime_absent();
    assert_no_target_call(&fixture.target);
}

#[cfg(unix)]
#[test]
#[ignore = "requires digest-pinned probe/workload images and a usable local OCI socket"]
fn live_oci_backend_executes_managed_http_and_code_workloads() {
    let runtime = std::env::var_os("CODEATLAS_TEST_OCI_RUNTIME")
        .expect("CODEATLAS_TEST_OCI_RUNTIME is required for the live isolation gate");
    let socket = std::env::var_os("CODEATLAS_TEST_OCI_SOCKET")
        .expect("CODEATLAS_TEST_OCI_SOCKET is required for the live isolation gate");
    let image = std::env::var("CODEATLAS_TEST_OCI_PROBE_IMAGE")
        .expect("CODEATLAS_TEST_OCI_PROBE_IMAGE is required for the live isolation gate");
    let http_image = std::env::var("CODEATLAS_TEST_OCI_HTTP_IMAGE")
        .expect("CODEATLAS_TEST_OCI_HTTP_IMAGE is required for the live isolation gate");
    let python_code_image = std::env::var("CODEATLAS_TEST_OCI_PYTHON_CODE_IMAGE")
        .expect("CODEATLAS_TEST_OCI_PYTHON_CODE_IMAGE is required for the live isolation gate");
    let rust_code_image = std::env::var("CODEATLAS_TEST_OCI_RUST_CODE_IMAGE")
        .expect("CODEATLAS_TEST_OCI_RUST_CODE_IMAGE is required for the live isolation gate");
    assert!(
        image.contains("@sha256:"),
        "live probe image must be digest-pinned"
    );
    assert!(
        http_image.contains("@sha256:"),
        "live HTTP workload image must be digest-pinned"
    );
    assert!(
        python_code_image.contains("@sha256:"),
        "live Python code-fuzz workload image must be digest-pinned"
    );
    assert!(
        rust_code_image.contains("@sha256:"),
        "live Rust code-fuzz workload image must be digest-pinned"
    );
    let fixture = IsolationFixture::create("codeatlas-isolation-live");
    fixture.enable_live_workload(&http_image);
    let config_path = fixture.workspace.join("codeatlas.json");
    let mut config: Value =
        serde_json::from_slice(&fs::read(&config_path).expect("live config fixture"))
            .expect("live config JSON");
    config["execution"]["isolation"]["container"]["executable"] =
        Value::String(PathBuf::from(runtime).to_string_lossy().into_owned());
    config["execution"]["isolation"]["container"]["socket"] =
        Value::String(PathBuf::from(socket).to_string_lossy().into_owned());
    config["execution"]["isolation"]["container"]["probe_image"] = Value::String(image);
    config["execution"]["limits"]["run_timeout_ms"] = json!(120_000);
    config["execution"]["limits"]["max_cpu_time_ms"] = json!(60_000);
    config["execution"]["limits"]["max_rss_bytes"] = json!(536_870_912_u64);
    config["execution"]["limits"]["max_processes"] = json!(32);
    config["execution"]["limits"]["max_open_files"] = json!(128);
    config["execution"]["limits"]["max_calls"] = json!(256);
    config["execution"]["limits"]["calls_per_second"] = json!(50);
    config["execution"]["limits"]["max_output_bytes"] = json!(1_048_576);
    config["fuzz"]["limits"]["max_cases"] = json!(32);
    fs::write(
        &config_path,
        serde_json::to_vec_pretty(&config).expect("live config JSON bytes"),
    )
    .expect("write live config fixture");
    let assert_live_receipt = |receipt: &Value| {
        assert_eq!(
            receipt["outcome"], "passed",
            "unexpected live receipt: {receipt}"
        );
        let capabilities = receipt["runtime"]["capabilities"]
            .as_array()
            .expect("live capabilities");
        for capability in [
            "cleanup_verification",
            "network_allowlist",
            "process_allowlist",
            "read_only_checkout",
            "read_only_runtime",
            "resource_limits",
            "scratch_filesystem",
            "tls_interception",
        ] {
            assert!(
                capabilities.iter().any(|value| value == capability),
                "missing live capability {capability}: {receipt}"
            );
        }
        assert!(receipt["calls"]["consumed"]
            .as_u64()
            .is_some_and(|calls| calls > 0 && calls <= 256));
        assert!(receipt["cleanup"]
            .as_array()
            .expect("live cleanup evidence")
            .iter()
            .all(|cleanup| cleanup["released"] == true && cleanup["verified"] == true));
    };

    let stateful_plan =
        fixture.plan_with(&["--profile", "stateful", "--max-cases", "4", "--seed", "42"]);
    let stateful_receipt = fixture.execute_with_status_and_export(&stateful_plan, 0, false);
    assert_live_receipt(&stateful_receipt);
    let stateful_report = fixture.report(&stateful_receipt);
    assert_eq!(stateful_report["stateful"]["linksSelected"], 2);
    assert_eq!(stateful_report["stateful"]["linksCovered"], 2);
    assert!(stateful_report["stateful"]["scenarios"]
        .as_u64()
        .is_some_and(|scenarios| scenarios > 0));

    let plan = fixture.plan_with(&["--max-cases", "12", "--seed", "43"]);
    let receipt = fixture.execute_with_status(&plan, 0);
    assert_live_receipt(&receipt);
    let report = fixture.report(&receipt);
    assert_eq!(report["totals"]["operations"], 3);
    assert!(report["totals"]["positiveSuccesses"]
        .as_u64()
        .is_some_and(|cases| cases > 0));
    assert!(report["totals"]["negativeRejections"]
        .as_u64()
        .is_some_and(|cases| cases > 0));
    assert_eq!(report["totals"]["serverErrors"], 0);
    assert_eq!(report["totals"]["checkFailures"], 0);
    assert!(report["operations"].as_array().is_some_and(|operations| {
        operations
            .iter()
            .any(|operation| operation["operation"] == "POST /widgets/{id}")
    }));

    fixture.enable_live_python_code_workload(&python_code_image);
    let source_before = fs::read(fixture.workspace.join("safe.py")).expect("live code source");
    let code_plan = fixture.plan_code(
        "python-live",
        "safe.py#fails_at_or_above_two",
        &[
            "--max-cases",
            "32",
            "--max-shrinks",
            "32",
            "--max-failures",
            "1",
            "--max-calls",
            "66",
            "--seed",
            "44",
        ],
    );
    assert_eq!(code_plan["workload"]["body"]["fuzz_marker"], true);
    assert_eq!(
        code_plan["workload"]["body"]["adapter_version"],
        "codeatlas.python-hypothesis/v1"
    );
    let code_receipt = fixture.execute_code(&code_plan, 1);
    assert_eq!(code_receipt["outcome"], "failed");
    assert!(code_receipt["calls"]["consumed"]
        .as_u64()
        .is_some_and(|calls| calls > 0 && calls <= 66));
    let categories = code_receipt["calls"]["by_category"]
        .as_array()
        .expect("code call categories");
    for category in ["readiness", "generated_case", "reduction", "retry"] {
        assert!(
            categories.iter().any(|calls| {
                calls["category"] == category
                    && calls["count"].as_u64().is_some_and(|count| count > 0)
            }),
            "missing code call category {category}: {code_receipt}"
        );
    }
    let code_report = fixture.report(&code_receipt);
    assert_eq!(code_report["alternate_behavior"], true);
    assert!(code_report["deterministic_cases"]
        .as_u64()
        .is_some_and(|cases| cases > 0));
    assert!(code_report["adaptive_cases"]
        .as_u64()
        .is_some_and(|cases| cases > 0));
    assert_eq!(code_report["failures"].as_array().map(Vec::len), Some(1));
    assert_eq!(code_report["failures"][0]["kind"], "panic_or_crash");
    assert_eq!(code_report["failures"][0]["minimized"], true);
    let reproducer_link = &code_report["failures"][0]["reproducer"];
    let reproducer = fixture.linked_artifact(reproducer_link, "reproducers");
    assert_eq!(
        reproducer["workload"]["body"]["replay_input"],
        json!([{"kind": "integer", "value": "2"}])
    );
    let replay_plan =
        fixture.plan_code_replay(reproducer_link["id"].as_str().expect("code reproducer ID"));
    assert_eq!(
        replay_plan["workload"]["body"]["replay_input"],
        reproducer["workload"]["body"]["replay_input"]
    );
    let replay_receipt = fixture.execute_code(&replay_plan, 1);
    assert_eq!(replay_receipt["outcome"], "failed");
    assert_eq!(replay_receipt["calls"]["consumed"], 2);
    assert_eq!(
        fs::read(fixture.workspace.join("safe.py")).expect("live code source after fuzzing"),
        source_before
    );

    fixture.enable_live_rust_code_workload(&rust_code_image);
    let rust_source_before =
        fs::read(fixture.workspace.join("src/lib.rs")).expect("live Rust source");
    let rust_plan = fixture.plan_code(
        "rust-live",
        "src/lib.rs#fails_in_shrinkable_window",
        &[
            "--max-cases",
            "32",
            "--max-shrinks",
            "32",
            "--max-failures",
            "1",
            "--max-calls",
            "66",
            "--seed",
            "45",
        ],
    );
    assert_eq!(rust_plan["workload"]["body"]["engine"], "proptest");
    assert_eq!(
        rust_plan["workload"]["body"]["adapter_version"],
        "codeatlas.rust-proptest/v1"
    );
    let rust_receipt = fixture.execute_code(&rust_plan, 1);
    assert_eq!(rust_receipt["outcome"], "failed");
    let rust_categories = rust_receipt["calls"]["by_category"]
        .as_array()
        .expect("Rust call categories");
    for category in ["readiness", "generated_case", "reduction", "retry"] {
        assert!(
            rust_categories.iter().any(|calls| {
                calls["category"] == category
                    && calls["count"].as_u64().is_some_and(|count| count > 0)
            }),
            "missing Rust call category {category}: {rust_receipt}"
        );
    }
    let rust_report = fixture.report(&rust_receipt);
    assert_eq!(rust_report["alternate_behavior"], true);
    assert_eq!(rust_report["failures"].as_array().map(Vec::len), Some(1));
    assert_eq!(rust_report["failures"][0]["kind"], "panic_or_crash");
    assert_eq!(rust_report["failures"][0]["minimized"], true);
    let rust_reproducer_link = &rust_report["failures"][0]["reproducer"];
    let rust_reproducer = fixture.linked_artifact(rust_reproducer_link, "reproducers");
    assert_eq!(
        rust_reproducer["workload"]["body"]["replay_input"],
        json!([{"kind": "integer", "value": "2"}])
    );
    let rust_replay_plan = fixture.plan_code_replay(
        rust_reproducer_link["id"]
            .as_str()
            .expect("Rust reproducer ID"),
    );
    let rust_replay_receipt = fixture.execute_code(&rust_replay_plan, 1);
    assert_eq!(rust_replay_receipt["outcome"], "failed");
    assert_eq!(rust_replay_receipt["calls"]["consumed"], 2);
    assert_eq!(
        fs::read(fixture.workspace.join("src/lib.rs")).expect("live Rust source after fuzzing"),
        rust_source_before
    );
    assert_no_target_call(&fixture.target);
}

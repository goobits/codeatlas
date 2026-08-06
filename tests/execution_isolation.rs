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
        .env("CODEATLAS_TEST_AMBIENT_SECRET", "must-not-enter-sandbox");
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
                        "network": "deny",
                        "processes": "deny",
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
                        "base_url": format!("http://{address}")
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

    fn plan(&self) -> Value {
        let output = run_codeatlas(
            &self.workspace,
            &self.state,
            &["fuzz", "http", "--target", "local", "--seed", "42"],
        );
        assert!(
            output.status.success(),
            "planning failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_no_target_call(&self.target);
        serde_json::from_slice(&output.stdout).expect("execution plan JSON")
    }

    fn execute(&self, plan: &Value) -> Value {
        let plan_id = plan["id"].as_str().expect("plan ID");
        let output = run_codeatlas(
            &self.workspace,
            &self.state,
            &["fuzz", "http", "--plan", plan_id, "--execute"],
        );
        assert_eq!(
            output.status.code(),
            Some(2),
            "execution stderr:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_no_target_call(&self.target);
        if let Some(path) = std::env::var_os("CODEATLAS_TEST_OCI_RECEIPT_OUT") {
            write_live_receipt(Path::new(&path), &output.stdout)
                .expect("persist private external live receipt");
        }
        serde_json::from_slice(&output.stdout).expect("execution receipt JSON")
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
fn target_observed_container_contract_grants_only_proven_capabilities() {
    let fixture = IsolationFixture::create("codeatlas-isolation-conformance");
    let plan = fixture.plan();
    let receipt = fixture.execute(&plan);
    assert_eq!(receipt["outcome"], "blocked");
    assert!(
        receipt["reasons"]
            .as_array()
            .expect("receipt reasons")
            .iter()
            .any(|reason| reason
                .as_str()
                .is_some_and(|reason| reason.contains("remains disconnected until Phase 5"))),
        "unexpected receipt reasons: {}",
        receipt["reasons"]
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
    assert_eq!(receipt["cleanup"].as_array().map(Vec::len), Some(2));
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
}

#[cfg(unix)]
#[test]
fn failed_target_observation_blocks_and_cleanup_still_verifies() {
    let fixture = IsolationFixture::create("codeatlas-isolation-negative");
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
    fixture.set_mode("hang");
    let plan = fixture.plan();
    let plan_id = plan["id"].as_str().expect("plan ID");
    let mut child = codeatlas_command(&fixture.workspace, &fixture.state)
        .args(["fuzz", "http", "--plan", plan_id, "--execute"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("interruptible CodeAtlas execution");
    let started = fixture.runtime.with_extension("started");
    let marker_deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !started.is_file() {
        if let Some(status) = child.try_wait().expect("inspect interruptible execution") {
            let output = child
                .wait_with_output()
                .expect("collect early interruptible execution");
            panic!(
                "execution exited before the probe started ({status}): {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        if std::time::Instant::now() >= marker_deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("isolation probe did not expose its start marker");
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
        "interrupted execution stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let receipt: Value =
        serde_json::from_slice(&output.stdout).expect("cancelled execution receipt JSON");
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
#[ignore = "requires an operator-provided digest-pinned probe image and usable local OCI socket"]
fn live_oci_backend_passes_target_observed_conformance() {
    let runtime = std::env::var_os("CODEATLAS_TEST_OCI_RUNTIME")
        .expect("CODEATLAS_TEST_OCI_RUNTIME is required for the live isolation gate");
    let socket = std::env::var_os("CODEATLAS_TEST_OCI_SOCKET")
        .expect("CODEATLAS_TEST_OCI_SOCKET is required for the live isolation gate");
    let image = std::env::var("CODEATLAS_TEST_OCI_PROBE_IMAGE")
        .expect("CODEATLAS_TEST_OCI_PROBE_IMAGE is required for the live isolation gate");
    assert!(
        image.contains("@sha256:"),
        "live probe image must be digest-pinned"
    );
    let fixture = IsolationFixture::create("codeatlas-isolation-live");
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
    fs::write(
        &config_path,
        serde_json::to_vec_pretty(&config).expect("live config JSON bytes"),
    )
    .expect("write live config fixture");
    let plan = fixture.plan();
    let receipt = fixture.execute(&plan);
    assert_eq!(
        receipt["runtime"]["capabilities"].as_array().map(Vec::len),
        Some(7),
        "unexpected live isolation receipt: {receipt}"
    );
    assert!(receipt["cleanup"]
        .as_array()
        .expect("live cleanup evidence")
        .iter()
        .all(|cleanup| cleanup["released"] == true && cleanup["verified"] == true));
}

use proptest::strategy::{BoxedStrategy, Strategy, ValueTree};
use proptest::test_runner::{Config, RngSeed, TestRunner};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::net::UnixStream;
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

const RESULT_SCHEMA: &str = "codeatlas.code-fuzz-harness-result/v1";
const PERMIT_SCHEMA: &str = "codeatlas.execution-call-permit/v1";
const TARGET_ID: &str = __TARGET_ID__;
const CALLABLE_ID: &str = __CALLABLE_ID__;
const SEED_TEXT: &str = __SEED_TEXT__;
const SEED_U64: u64 = __SEED_U64__;
const MAX_CASES: u64 = __MAX_CASES__;
const MAX_SHRINKS: u64 = __MAX_SHRINKS__;
const MAX_FAILURES: u64 = __MAX_FAILURES__;
const CASE_TIMEOUT_MS: u64 = __CASE_TIMEOUT_MS__;
const ALTERNATE_BEHAVIOR: bool = __ALTERNATE_BEHAVIOR__;
const MAX_FRAME_BYTES: usize = 512;

type Input = __INPUT_TYPE__;

#[derive(Clone, Serialize)]
struct Failure {
    kind: String,
    input: Vec<Value>,
    detail: Value,
    minimized: bool,
}

#[derive(Serialize)]
struct HarnessResult<'a> {
    schema_version: &'static str,
    plan_id: &'a str,
    target_id: &'static str,
    callable_id: &'static str,
    seed: &'static str,
    deterministic_cases: u64,
    adaptive_cases: u64,
    alternate_behavior: bool,
    failures: Vec<Failure>,
}

#[derive(Default)]
struct RunState {
    deterministic_cases: u64,
    adaptive_cases: u64,
    retries: u64,
    failures: Vec<Failure>,
    incomplete: bool,
}

enum Evaluation {
    Passed,
    Failed(Failure, bool),
    Denied,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PermitResponse {
    schema_version: String,
    status: String,
    sequence: u64,
    #[serde(default)]
    reason: Option<String>,
}

struct Permit {
    stream: UnixStream,
    reader: BufReader<UnixStream>,
    sequence: u64,
}

impl Permit {
    fn finish(mut self, disposition: &str) -> Result<(), String> {
        write_frame(
            &mut self.stream,
            &json!({
                "schema_version": PERMIT_SCHEMA,
                "disposition": disposition,
            }),
        )?;
        let acknowledgement = read_frame(&mut self.reader)?;
        if acknowledgement.schema_version != PERMIT_SCHEMA
            || acknowledgement.status != "recorded"
            || acknowledgement.sequence != self.sequence
            || acknowledgement.reason.is_some()
        {
            return Err("call-permit completion acknowledgement is invalid".to_string());
        }
        Ok(())
    }
}

fn strategy() -> BoxedStrategy<Input> {
    (__STRATEGY__).boxed()
}

fn deterministic_inputs() -> Vec<Input> {
    __DETERMINISTIC_INPUTS__
}

fn replay_input() -> Option<Input> {
    __REPLAY_INPUT__
}

fn encode_input(input: Input) -> Vec<Value> {
    let __ENCODE_DESTRUCTURE__ = input;
    __ENCODED_INPUTS__
}

fn encode_float(value: f64) -> Value {
    let value = if value.is_nan() {
        "nan".to_string()
    } else if value == f64::INFINITY {
        "infinity".to_string()
    } else if value == f64::NEG_INFINITY {
        "-infinity".to_string()
    } else if value == 0.0 && value.is_sign_negative() {
        "-0".to_string()
    } else {
        value.to_string()
    };
    json!({"kind":"float","value":value})
}

fn invoke_target(input: Input) {
    let __INVOKE_DESTRUCTURE__ = input;
    __INVOKE_TARGET__
}

fn result_root() -> Result<PathBuf, String> {
    let scratch = std::env::var("CODEATLAS_SCRATCH")
        .map_err(|_| "kernel scratch environment is unavailable".to_string())?;
    let root = PathBuf::from(scratch).join("control");
    let metadata = fs::symlink_metadata(&root)
        .map_err(|error| format!("kernel result directory is unavailable: {error}"))?;
    if !metadata.file_type().is_dir() {
        return Err("kernel result directory is not a directory".to_string());
    }
    Ok(root)
}

fn write_private_json(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let temporary = path.with_extension("json.tmp");
    if temporary.exists() {
        fs::remove_file(&temporary)
            .map_err(|error| format!("remove stale private result: {error}"))?;
    }
    let bytes = serde_json::to_vec(value).map_err(|error| format!("serialize result: {error}"))?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temporary)
        .map_err(|error| format!("create private result: {error}"))?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|error| format!("persist private result: {error}"))?;
    fs::rename(&temporary, path).map_err(|error| format!("publish private result: {error}"))
}

fn current_path() -> Result<PathBuf, String> {
    Ok(result_root()?.join("rust-current-input.json"))
}

fn write_current(input: &[Value]) -> Result<(), String> {
    write_private_json(&current_path()?, &json!({"input":input}))
}

fn remove_current() -> Result<(), String> {
    let path = current_path()?;
    if path.exists() {
        fs::remove_file(path).map_err(|error| format!("remove current Rust fuzz input: {error}"))?;
    }
    Ok(())
}

fn write_result(state: RunState, plan_id: &str) -> Result<i32, String> {
    let failed = state.incomplete || !state.failures.is_empty();
    write_private_json(
        &result_root()?.join("code-result.json"),
        &HarnessResult {
            schema_version: RESULT_SCHEMA,
            plan_id,
            target_id: TARGET_ID,
            callable_id: CALLABLE_ID,
            seed: SEED_TEXT,
            deterministic_cases: state.deterministic_cases,
            adaptive_cases: state.adaptive_cases,
            alternate_behavior: ALTERNATE_BEHAVIOR,
            failures: state.failures,
        },
    )?;
    Ok(i32::from(failed))
}

fn read_frame(reader: &mut BufReader<UnixStream>) -> Result<PermitResponse, String> {
    let mut bytes = Vec::new();
    reader
        .take((MAX_FRAME_BYTES + 1) as u64)
        .read_until(b'\n', &mut bytes)
        .map_err(|error| format!("read call-permit frame: {error}"))?;
    if bytes.is_empty()
        || bytes.len() > MAX_FRAME_BYTES
        || bytes.pop() != Some(b'\n')
        || bytes.contains(&b'\n')
        || bytes.contains(&b'\r')
    {
        return Err("call-permit response is not one bounded line".to_string());
    }
    serde_json::from_slice(&bytes).map_err(|error| format!("parse call-permit response: {error}"))
}

fn write_frame(stream: &mut UnixStream, value: &Value) -> Result<(), String> {
    let mut bytes = serde_json::to_vec(value)
        .map_err(|error| format!("serialize call-permit frame: {error}"))?;
    if bytes.len() >= MAX_FRAME_BYTES {
        return Err("call-permit request exceeds its byte ceiling".to_string());
    }
    bytes.push(b'\n');
    stream
        .write_all(&bytes)
        .and_then(|_| stream.flush())
        .map_err(|error| format!("write call-permit frame: {error}"))
}

fn request_permit(category: &str) -> Result<Option<Permit>, String> {
    let address = std::env::var("CODEATLAS_CALL_PERMIT_SOCKET")
        .map_err(|_| "call-permit socket is unavailable".to_string())?;
    let mut stream = UnixStream::connect(address)
        .map_err(|error| format!("connect call-permit socket: {error}"))?;
    let reader_stream = stream
        .try_clone()
        .map_err(|error| format!("clone call-permit socket: {error}"))?;
    let mut reader = BufReader::new(reader_stream);
    write_frame(
        &mut stream,
        &json!({"schema_version":PERMIT_SCHEMA,"category":category}),
    )?;
    let response = read_frame(&mut reader)?;
    if response.schema_version != PERMIT_SCHEMA {
        return Err("call-permit response has the wrong schema".to_string());
    }
    match response.status.as_str() {
        "granted" if response.sequence > 0 && response.reason.is_none() => Ok(Some(Permit {
            stream,
            reader,
            sequence: response.sequence,
        })),
        "denied" if response.sequence == 0 && response.reason.is_some() => Ok(None),
        _ => Err("call-permit response is invalid".to_string()),
    }
}

fn evaluate(input: Input, category: &str) -> Result<Evaluation, String> {
    let encoded = encode_input(input);
    write_current(&encoded)?;
    let Some(permit) = request_permit(category)? else {
        remove_current()?;
        return Ok(Evaluation::Denied);
    };
    let (sender, receiver) = mpsc::sync_channel(1);
    let spawned = thread::Builder::new()
        .name("codeatlas-rust-fuzz-case".to_string())
        .spawn(move || {
            let passed = panic::catch_unwind(AssertUnwindSafe(|| invoke_target(input))).is_ok();
            let _ = sender.send(passed);
        });
    let evaluation = match spawned {
        Err(error) => Evaluation::Failed(
            Failure {
                kind: "resource_limit".to_string(),
                input: encoded,
                detail: json!({"thread_spawn":error.kind().to_string()}),
                minimized: false,
            },
            false,
        ),
        Ok(_thread) => match receiver.recv_timeout(Duration::from_millis(CASE_TIMEOUT_MS)) {
            Ok(true) => Evaluation::Passed,
            Ok(false) | Err(mpsc::RecvTimeoutError::Disconnected) => Evaluation::Failed(
                Failure {
                    kind: "panic_or_crash".to_string(),
                    input: encoded,
                    detail: json!({"panic":true}),
                    minimized: false,
                },
                false,
            ),
            Err(mpsc::RecvTimeoutError::Timeout) => Evaluation::Failed(
                Failure {
                    kind: "timeout".to_string(),
                    input: encoded,
                    detail: json!({"timeout":true}),
                    minimized: false,
                },
                true,
            ),
        },
    };
    let disposition = if matches!(evaluation, Evaluation::Passed) {
        "completed"
    } else {
        "failed"
    };
    permit.finish(disposition)?;
    remove_current()?;
    Ok(evaluation)
}

fn record_fatal(state: &mut RunState, failure: Failure) {
    if state.failures.len() < MAX_FAILURES as usize {
        state.failures.push(failure);
    }
    state.incomplete = true;
}

fn run_deterministic(state: &mut RunState) -> Result<bool, String> {
    for input in deterministic_inputs() {
        if state.failures.len() >= MAX_FAILURES as usize || state.deterministic_cases >= MAX_CASES {
            break;
        }
        let observed = evaluate(input, "generated_case")?;
        state.deterministic_cases += 1;
        match observed {
            Evaluation::Passed => {}
            Evaluation::Denied => {
                state.incomplete = true;
                return Ok(false);
            }
            Evaluation::Failed(failure, true) => {
                record_fatal(state, failure);
                return Ok(false);
            }
            Evaluation::Failed(failure, false) => {
                if state.retries >= MAX_FAILURES {
                    state.incomplete = true;
                    return Ok(false);
                }
                state.retries += 1;
                match evaluate(input, "retry")? {
                    Evaluation::Failed(confirmation, fatal)
                        if confirmation.kind == failure.kind =>
                    {
                        state.failures.push(confirmation);
                        if fatal {
                            state.incomplete = true;
                            return Ok(false);
                        }
                    }
                    Evaluation::Denied => {
                        state.incomplete = true;
                        return Ok(false);
                    }
                    Evaluation::Failed(confirmation, true) => {
                        record_fatal(state, confirmation);
                        return Ok(false);
                    }
                    _ => {}
                }
            }
        }
    }
    Ok(true)
}

fn run_adaptive(state: &mut RunState) -> Result<(), String> {
    if state.failures.len() >= MAX_FAILURES as usize {
        return Ok(());
    }
    let mut runner = TestRunner::new(Config {
        failure_persistence: None,
        rng_seed: RngSeed::Fixed(SEED_U64),
        ..Config::default()
    });
    let strategy = strategy();
    while state
        .deterministic_cases
        .saturating_add(state.adaptive_cases)
        < MAX_CASES
    {
        let mut tree = strategy
            .new_tree(&mut runner)
            .map_err(|error| format!("generate proptest value tree: {error}"))?;
        let input = tree.current();
        let observed = evaluate(input, "generated_case")?;
        state.adaptive_cases += 1;
        let Evaluation::Failed(mut best, fatal) = observed else {
            if matches!(observed, Evaluation::Denied) {
                state.incomplete = true;
                return Ok(());
            }
            continue;
        };
        if fatal {
            record_fatal(state, best);
            return Ok(());
        }
        let oracle = best.kind.clone();
        let mut best_input = input;
        for _ in 0..MAX_SHRINKS {
            if !tree.simplify() {
                break;
            }
            let candidate = tree.current();
            match evaluate(candidate, "reduction")? {
                Evaluation::Failed(failure, false) if failure.kind == oracle => {
                    best = failure;
                    best_input = candidate;
                }
                Evaluation::Failed(failure, true) => {
                    record_fatal(state, failure);
                    return Ok(());
                }
                Evaluation::Denied => {
                    state.failures.push(best);
                    state.incomplete = true;
                    return Ok(());
                }
                _ => {
                    let _ = tree.complicate();
                }
            }
        }
        if state.retries >= MAX_FAILURES {
            state.failures.push(best);
            state.incomplete = true;
            return Ok(());
        }
        state.retries += 1;
        match evaluate(best_input, "retry")? {
            Evaluation::Failed(mut confirmation, fatal) if confirmation.kind == oracle => {
                confirmation.minimized = true;
                state.failures.push(confirmation);
                if fatal {
                    state.incomplete = true;
                }
            }
            Evaluation::Denied => state.incomplete = true,
            Evaluation::Failed(failure, true) => record_fatal(state, failure),
            _ => {}
        }
        return Ok(());
    }
    Ok(())
}

fn run() -> Result<i32, String> {
    if std::env::var("CODEATLAS_FUZZ").as_deref() != Ok("1") {
        return Err("planned fuzz marker is unavailable".to_string());
    }
    let plan_id = std::env::var("CODEATLAS_PLAN_ID")
        .map_err(|_| "planned execution identity is unavailable".to_string())?;
    let mut state = RunState::default();
    if let Some(input) = replay_input() {
        state.adaptive_cases = 1;
        match evaluate(input, "generated_case")? {
            Evaluation::Failed(failure, fatal) => {
                state.failures.push(failure);
                state.incomplete = fatal;
            }
            Evaluation::Denied => state.incomplete = true,
            Evaluation::Passed => {}
        }
    } else if run_deterministic(&mut state)? {
        run_adaptive(&mut state)?;
    }
    write_result(state, &plan_id)
}

fn main() {
    match run() {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("Rust fuzz harness failed: {error}");
            std::process::exit(2);
        }
    }
}

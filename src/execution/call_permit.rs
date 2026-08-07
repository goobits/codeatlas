use super::budget::{CallDisposition, CallPermitError};
use super::lease::ExecutionLease;
use super::model::{CallCategory, ExecutionLimits};
use super::permit_protocol::CALL_PERMIT_PROTOCOL_SCHEMA_VERSION;
use super::scheduler::ExecutionContext;
use super::unix_socket::{bind_private_unix_listener, remove_private_unix_socket};
use anyhow::{Context, Result};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{watch, Semaphore};
use tokio::task::{JoinHandle, JoinSet};

const MAX_FRAME_BYTES: u64 = 512;
const PERMIT_RESERVED_FILE_DESCRIPTORS: u64 = 2;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PermitRequest {
    schema_version: String,
    category: CallCategory,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PermitCompletion {
    schema_version: String,
    disposition: PermitDisposition,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum PermitDisposition {
    Completed,
    Failed,
    Rejected,
    Cancelled,
}

impl From<PermitDisposition> for CallDisposition {
    fn from(value: PermitDisposition) -> Self {
        match value {
            PermitDisposition::Completed => Self::Completed,
            PermitDisposition::Failed => Self::Failed,
            PermitDisposition::Rejected => Self::Rejected,
            PermitDisposition::Cancelled => Self::Cancelled,
        }
    }
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct PermitResponse<'a> {
    schema_version: &'static str,
    status: &'a str,
    sequence: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<&'a str>,
}

pub(crate) struct CallPermitBroker {
    shutdown: watch::Sender<bool>,
    task: Option<JoinHandle<Result<()>>>,
    socket_path: Option<PathBuf>,
}

impl CallPermitBroker {
    pub(crate) fn start(
        context: &ExecutionContext,
        limits: &ExecutionLimits,
        socket_path: &Path,
    ) -> Result<Self> {
        let connection_limit = resolve_connection_limit(limits)?;
        let listener = bind_private_unix_listener(socket_path, "call-permit broker")?;
        let (shutdown, receiver) = watch::channel(false);
        let task = tokio::spawn(serve_permits(
            listener,
            context.clone(),
            receiver,
            connection_limit,
        ));
        Ok(Self {
            shutdown,
            task: Some(task),
            socket_path: Some(socket_path.to_path_buf()),
        })
    }

    pub(crate) fn cleanup_lease(&self) -> ExecutionLease {
        let shutdown = self.shutdown.clone();
        let socket_path = self.socket_path.clone();
        ExecutionLease::new("execution_kernel", "call_permit_broker", move || {
            shutdown.send_replace(true);
            remove_private_unix_socket(socket_path.as_deref(), "call-permit broker")
        })
    }

    pub(crate) async fn shutdown(mut self) -> Result<()> {
        self.shutdown.send_replace(true);
        if let Some(task) = self.task.take() {
            task.await.context("Call-permit broker task panicked")??;
        }
        self.remove_socket()?;
        Ok(())
    }

    fn remove_socket(&mut self) -> Result<()> {
        let path = self.socket_path.take();
        remove_private_unix_socket(path.as_deref(), "call-permit broker").map(|_| ())
    }
}

impl Drop for CallPermitBroker {
    fn drop(&mut self) {
        self.shutdown.send_replace(true);
        let _ = self.remove_socket();
    }
}

fn resolve_connection_limit(limits: &ExecutionLimits) -> Result<usize> {
    let available = limits
        .max_open_files
        .checked_sub(PERMIT_RESERVED_FILE_DESCRIPTORS)
        .context("max_open_files leaves no call-permit broker file reserve")?;
    let limit = available.min(limits.max_concurrency);
    if limit == 0 {
        anyhow::bail!("Call-permit broker needs positive connection capacity");
    }
    usize::try_from(limit)
        .map_err(|_| anyhow::anyhow!("Call-permit connection limit does not fit this host"))
}

async fn serve_permits(
    listener: UnixListener,
    context: ExecutionContext,
    mut shutdown: watch::Receiver<bool>,
    connection_limit: usize,
) -> Result<()> {
    let connections = std::sync::Arc::new(Semaphore::new(connection_limit));
    let mut tasks = JoinSet::new();
    loop {
        tokio::select! {
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    break;
                }
            }
            _ = context.budget().wait_for_cancellation() => break,
            accepted = listener.accept() => {
                let (stream, _) = accepted.context("Could not accept a call-permit connection")?;
                let permit = std::sync::Arc::clone(&connections)
                    .try_acquire_owned()
                    .map_err(|_| anyhow::anyhow!("Call-permit connection capacity was exceeded"))?;
                let connection_context = context.clone();
                let connection_shutdown = shutdown.clone();
                tasks.spawn(async move {
                    let _connection = permit;
                    handle_connection(stream, connection_context, connection_shutdown).await
                });
            }
            joined = tasks.join_next(), if !tasks.is_empty() => {
                joined
                    .expect("non-empty call-permit task set")
                    .context("Call-permit connection task panicked")??;
            }
        }
    }
    tasks.abort_all();
    while let Some(joined) = tasks.join_next().await {
        if let Err(error) = joined {
            if !error.is_cancelled() {
                return Err(error).context("Call-permit connection task panicked");
            }
        }
    }
    Ok(())
}

async fn handle_connection(
    stream: UnixStream,
    context: ExecutionContext,
    mut shutdown: watch::Receiver<bool>,
) -> Result<()> {
    let mut stream = BufReader::new(stream);
    let request: PermitRequest = tokio::select! {
        result = read_frame(&mut stream, "request") => result?,
        _ = context.budget().wait_for_cancellation() => anyhow::bail!("Call-permit request was cancelled"),
        changed = shutdown.changed() => {
            let _ = changed;
            anyhow::bail!("Call-permit broker stopped before request admission")
        }
    };
    validate_schema(&request.schema_version)?;
    let permit = match context.budget().reserve_call(request.category).await {
        Ok(permit) => permit,
        Err(error) => {
            let reason = permit_error_code(&error);
            write_frame(
                stream.get_mut(),
                &PermitResponse {
                    schema_version: CALL_PERMIT_PROTOCOL_SCHEMA_VERSION,
                    status: "denied",
                    sequence: 0,
                    reason: Some(reason),
                },
            )
            .await?;
            return Ok(());
        }
    };
    let sequence = permit.sequence();
    write_frame(
        stream.get_mut(),
        &PermitResponse {
            schema_version: CALL_PERMIT_PROTOCOL_SCHEMA_VERSION,
            status: "granted",
            sequence,
            reason: None,
        },
    )
    .await?;
    let completion: PermitCompletion = tokio::select! {
        result = tokio::time::timeout_at(permit.deadline(), read_frame(&mut stream, "completion")) => {
            result.context("Call-permit completion exceeded its deadline")??
        }
        _ = context.budget().wait_for_cancellation() => anyhow::bail!("Call-permit completion was cancelled"),
        changed = shutdown.changed() => {
            let _ = changed;
            anyhow::bail!("Call-permit broker stopped before completion")
        }
    };
    validate_schema(&completion.schema_version)?;
    permit.finish(completion.disposition.into());
    write_frame(
        stream.get_mut(),
        &PermitResponse {
            schema_version: CALL_PERMIT_PROTOCOL_SCHEMA_VERSION,
            status: "recorded",
            sequence,
            reason: None,
        },
    )
    .await
}

fn validate_schema(schema_version: &str) -> Result<()> {
    if schema_version != CALL_PERMIT_PROTOCOL_SCHEMA_VERSION {
        anyhow::bail!("Unsupported call-permit protocol schema");
    }
    Ok(())
}

fn permit_error_code(error: &CallPermitError) -> &'static str {
    match error {
        CallPermitError::CallsExhausted => "calls_exhausted",
        CallPermitError::CleanupExhausted => "cleanup_exhausted",
        CallPermitError::Cancelled => "cancelled",
        CallPermitError::DeadlineExhausted => "deadline_exhausted",
        CallPermitError::SchedulerClosed => "scheduler_closed",
    }
}

async fn read_frame<T: DeserializeOwned>(
    stream: &mut BufReader<UnixStream>,
    label: &str,
) -> Result<T> {
    let mut bytes = Vec::new();
    let count = (&mut *stream)
        .take(MAX_FRAME_BYTES + 1)
        .read_until(b'\n', &mut bytes)
        .await
        .with_context(|| format!("Could not read call-permit {label}"))?;
    if count == 0 || u64::try_from(count).unwrap_or(u64::MAX) > MAX_FRAME_BYTES {
        anyhow::bail!("Call-permit {label} is empty or exceeds its byte ceiling");
    }
    if bytes.pop() != Some(b'\n') || bytes.contains(&b'\n') || bytes.contains(&b'\r') {
        anyhow::bail!("Call-permit {label} is not one canonical line");
    }
    serde_json::from_slice(&bytes)
        .with_context(|| format!("Call-permit {label} is not strict JSON"))
}

async fn write_frame(stream: &mut UnixStream, value: &impl Serialize) -> Result<()> {
    let mut bytes =
        serde_json::to_vec(value).context("Could not serialize call-permit response")?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) >= MAX_FRAME_BYTES {
        anyhow::bail!("Call-permit response exceeds its byte ceiling");
    }
    bytes.push(b'\n');
    stream
        .write_all(&bytes)
        .await
        .context("Could not write call-permit response")?;
    stream
        .flush()
        .await
        .context("Could not flush call-permit response")
}

#[cfg(test)]
mod tests {
    use super::{CallPermitBroker, CALL_PERMIT_PROTOCOL_SCHEMA_VERSION};
    use crate::execution::budget::BudgetTermination;
    use crate::execution::model::{sample_execution_limits, CallCategory};
    use crate::execution::scheduler::ExecutionScheduler;
    use anyhow::Result;
    use serde_json::{json, Value};
    use std::path::Path;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixStream;

    async fn request_permit(
        socket: &Path,
        category: &str,
    ) -> Result<(BufReader<UnixStream>, Value)> {
        let stream = UnixStream::connect(socket).await?;
        let mut stream = BufReader::new(stream);
        write_value(
            stream.get_mut(),
            &json!({
                "schema_version": CALL_PERMIT_PROTOCOL_SCHEMA_VERSION,
                "category": category,
            }),
        )
        .await?;
        let response = read_value(&mut stream).await?;
        Ok((stream, response))
    }

    async fn complete_permit(stream: &mut BufReader<UnixStream>) -> Result<Value> {
        write_value(
            stream.get_mut(),
            &json!({
                "schema_version": CALL_PERMIT_PROTOCOL_SCHEMA_VERSION,
                "disposition": "completed",
            }),
        )
        .await?;
        read_value(stream).await
    }

    async fn write_value(stream: &mut UnixStream, value: &Value) -> Result<()> {
        let mut bytes = serde_json::to_vec(value)?;
        bytes.push(b'\n');
        stream.write_all(&bytes).await?;
        Ok(())
    }

    async fn read_value(stream: &mut BufReader<UnixStream>) -> Result<Value> {
        let mut response = String::new();
        stream.read_line(&mut response).await?;
        Ok(serde_json::from_str(&response)?)
    }

    #[test]
    fn broker_grants_before_target_action_and_records_exact_completion() {
        let mut limits = sample_execution_limits();
        limits.calls_per_second = 1_000;
        limits.max_open_files = 8;
        let scheduler = ExecutionScheduler::new(&limits, 0).expect("permit scheduler");
        let root =
            std::env::temp_dir().join(format!("codeatlas-call-permit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir(&root).expect("permit fixture root");
        let socket = root.join("permit.sock");
        scheduler
            .run(|context| {
                let limits = limits.clone();
                let socket = socket.clone();
                async move {
                    let broker = CallPermitBroker::start(&context, &limits, &socket)?;
                    let (mut stream, grant) = request_permit(&socket, "generated_case").await?;
                    assert_eq!(grant["status"], "granted");
                    assert_eq!(grant["sequence"], 1);
                    let acknowledgement = complete_permit(&mut stream).await?;
                    assert_eq!(acknowledgement["status"], "recorded");
                    broker.shutdown().await?;
                    Ok(())
                }
            })
            .expect("permit exchange");
        let snapshot = scheduler.context().budget().snapshot();
        assert_eq!(snapshot.usage.consumed, 1);
        assert_eq!(snapshot.records[0].category, CallCategory::GeneratedCase);
        std::fs::remove_dir_all(root).expect("remove permit fixture");
    }

    #[test]
    fn broker_reports_budget_denial_without_granting_or_crashing() {
        let mut limits = sample_execution_limits();
        limits.max_calls = 1;
        limits.calls_per_second = 1_000;
        limits.max_open_files = 8;
        let scheduler = ExecutionScheduler::new(&limits, 0).expect("permit scheduler");
        let root = std::env::temp_dir().join(format!(
            "codeatlas-call-permit-denial-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir(&root).expect("permit fixture root");
        let socket = root.join("permit.sock");
        scheduler
            .run(|context| {
                let limits = limits.clone();
                let socket = socket.clone();
                async move {
                    let broker = CallPermitBroker::start(&context, &limits, &socket)?;
                    let (mut first, grant) = request_permit(&socket, "generated_case").await?;
                    assert_eq!(grant["status"], "granted");
                    assert_eq!(complete_permit(&mut first).await?["status"], "recorded");

                    let (_second, denial) = request_permit(&socket, "reduction").await?;
                    assert_eq!(denial["status"], "denied");
                    assert_eq!(denial["sequence"], 0);
                    assert_eq!(denial["reason"], "calls_exhausted");
                    broker.shutdown().await?;
                    Ok(())
                }
            })
            .expect("bounded permit denial");
        let snapshot = scheduler.context().budget().snapshot();
        assert_eq!(snapshot.usage.consumed, 1);
        assert_eq!(snapshot.records.len(), 1);
        assert_eq!(
            snapshot.termination,
            Some(BudgetTermination::CallsExhausted)
        );
        std::fs::remove_dir_all(root).expect("remove permit fixture");
    }
}

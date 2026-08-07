use super::budget::{CallBudget, CallDisposition};
use super::lease::ExecutionLease;
use super::model::{CallCategory, ExecutionLimits};
use super::scheduler::ExecutionContext;
use anyhow::{Context, Result};
use bytes::Bytes;
use http_body_util::{BodyExt, Full, Limited};
use hyper::body::Incoming;
#[cfg(unix)]
use hyper::client::conn::http1;
use hyper::header::{HeaderName, HeaderValue, CONNECTION, CONTENT_LENGTH, HOST, LOCATION, TE};
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode, Uri};
use hyper_rustls::{HttpsConnector, HttpsConnectorBuilder};
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::client::legacy::Client;
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto::Builder as ServerBuilder;
use rcgen::{BasicConstraints, CertificateParams, CertifiedIssuer, DnType, IsCa, KeyPair, SanType};
use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::ServerConfig;
use std::collections::HashSet;
use std::convert::Infallible;
#[cfg(target_os = "linux")]
use std::fs::File;
use std::io;
use std::net::{IpAddr, Ipv4Addr};
#[cfg(target_os = "linux")]
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::task::{Context as TaskContext, Poll};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
#[cfg(test)]
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{watch, Semaphore};
use tokio::task::{JoinHandle, JoinSet};
use tokio_rustls::TlsAcceptor;
use url::Url;

#[cfg(unix)]
use tokio::net::{UnixListener, UnixStream};

pub(crate) const CALL_CATEGORY_HEADER: &str = "x-codeatlas-call-category";

const PROXY_RESERVED_FILE_DESCRIPTORS: u64 = 4;
const PROXY_MAX_DOWNSTREAM_CONNECTIONS: u64 = 64;

type ProxyBody = Full<Bytes>;
type UpstreamConnector = HttpsConnector<HttpConnector>;
type UpstreamClient = Client<UpstreamConnector, ProxyBody>;

#[derive(Clone, Debug)]
pub(crate) enum ProxyUpstream {
    Network,
    #[cfg(unix)]
    ManagedServerSocket {
        host_path: PathBuf,
    },
}

#[derive(Clone, Debug)]
pub(crate) struct ProxyEndpoint {
    #[cfg(test)]
    pub base_url: Url,
    pub ca_pem: String,
    #[cfg(test)]
    pub ca_der: Vec<u8>,
}

pub(crate) struct EnforcingProxy {
    endpoint: ProxyEndpoint,
    shutdown: watch::Sender<bool>,
    task: Option<JoinHandle<Result<()>>>,
    socket_path: Option<PathBuf>,
    managed_server_peer_pid: Option<Arc<AtomicU32>>,
}

impl EnforcingProxy {
    #[cfg(test)]
    pub(crate) async fn start(
        context: &ExecutionContext,
        upstream: Url,
        limits: &ExecutionLimits,
        call_timeout_ms: u64,
    ) -> Result<Self> {
        validate_proxy_start(&upstream, limits, call_timeout_ms)?;
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .context("Could not bind the enforcing HTTP proxy")?;
        let address = listener
            .local_addr()
            .context("Could not inspect the enforcing proxy address")?;
        Self::start_with_listener(
            context,
            upstream,
            ProxyUpstream::Network,
            limits,
            call_timeout_ms,
            ProxyBinding {
                listener: ProxyListener::Tcp(listener),
                endpoint_port: address.port(),
                socket_path: None,
            },
        )
        .await
    }

    #[cfg(unix)]
    pub(crate) async fn start_unix(
        context: &ExecutionContext,
        upstream: Url,
        upstream_transport: ProxyUpstream,
        limits: &ExecutionLimits,
        call_timeout_ms: u64,
        socket_path: &Path,
        container_port: u16,
    ) -> Result<Self> {
        validate_proxy_start(&upstream, limits, call_timeout_ms)?;
        if container_port == 0 {
            anyhow::bail!("Unix enforcing proxy needs a container port");
        }
        validate_unix_socket_parent(socket_path)?;
        if std::fs::symlink_metadata(socket_path).is_ok() {
            anyhow::bail!(
                "Unix enforcing proxy socket already exists: {}",
                socket_path.display()
            );
        }
        let socket_address = UnixSocketAddress::new(socket_path)?;
        let listener = UnixListener::bind(socket_address.path()).with_context(|| {
            format!(
                "Could not bind Unix enforcing proxy {}",
                socket_path.display()
            )
        })?;
        if let Err(error) = crate::execution::private_fs::secure_file(socket_path) {
            drop(listener);
            let _ = std::fs::remove_file(socket_path);
            return Err(error);
        }
        Self::start_with_listener(
            context,
            upstream,
            upstream_transport,
            limits,
            call_timeout_ms,
            ProxyBinding {
                listener: ProxyListener::Unix(listener),
                endpoint_port: container_port,
                socket_path: Some(socket_path.to_path_buf()),
            },
        )
        .await
    }

    async fn start_with_listener(
        context: &ExecutionContext,
        upstream: Url,
        upstream_transport: ProxyUpstream,
        limits: &ExecutionLimits,
        call_timeout_ms: u64,
        binding: ProxyBinding,
    ) -> Result<Self> {
        let tls = create_proxy_tls_identity()?;
        let mut base_url = Url::parse(&format!("https://127.0.0.1:{}/", binding.endpoint_port))?;
        base_url.set_path(upstream.path());
        let endpoint = ProxyEndpoint {
            #[cfg(test)]
            base_url: base_url.clone(),
            ca_pem: tls.ca_pem,
            #[cfg(test)]
            ca_der: tls.ca_der,
        };
        let (client, managed_server_peer_pid) = ProxyUpstreamClient::new(upstream_transport)?;
        let state = Arc::new(ProxyState {
            upstream,
            endpoint: base_url,
            client,
            budget: Arc::clone(context.budget()),
            max_body_bytes: usize::try_from(limits.max_call_result_bytes)
                .map_err(|_| anyhow::anyhow!("max_call_result_bytes does not fit this host"))?,
            call_timeout: Duration::from_millis(call_timeout_ms),
            connection_timeout: Duration::from_millis(limits.run_timeout_ms),
            max_streams: u32::try_from(limits.max_concurrency)
                .map_err(|_| anyhow::anyhow!("max_concurrency exceeds HTTP/2 support"))?,
        });
        let acceptor = TlsAcceptor::from(tls.server);
        let max_connections = resolve_proxy_connection_limit(limits)?;
        let (shutdown, receiver) = watch::channel(false);
        let task = tokio::spawn(serve_proxy(
            binding.listener,
            acceptor,
            state,
            Arc::new(Semaphore::new(max_connections)),
            receiver,
        ));
        Ok(Self {
            endpoint,
            shutdown,
            task: Some(task),
            socket_path: binding.socket_path,
            managed_server_peer_pid,
        })
    }

    pub(crate) fn endpoint(&self) -> &ProxyEndpoint {
        &self.endpoint
    }

    pub(crate) fn cleanup_lease(&self) -> ExecutionLease {
        let shutdown = self.shutdown.clone();
        let socket_path = self.socket_path.clone();
        ExecutionLease::new("execution_kernel", "http_proxy", move || {
            shutdown.send_replace(true);
            remove_socket_path(socket_path.as_deref())
        })
    }

    pub(crate) fn corroborate_managed_server_peer(&self, pid: u32) -> Result<()> {
        if pid == 0 {
            anyhow::bail!("Managed-server bridge peer PID must be positive");
        }
        let expected = self
            .managed_server_peer_pid
            .as_ref()
            .context("Enforcing proxy has no managed-server bridge")?;
        expected
            .compare_exchange(0, pid, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| anyhow::anyhow!("Managed-server bridge peer PID is already fixed"))?;
        Ok(())
    }

    pub(crate) async fn shutdown(mut self) -> Result<()> {
        self.shutdown.send_replace(true);
        if let Some(task) = self.task.take() {
            task.await.context("Enforcing HTTP proxy task panicked")??;
        }
        self.remove_socket()?;
        Ok(())
    }

    fn remove_socket(&mut self) -> Result<()> {
        let path = self.socket_path.take();
        remove_socket_path(path.as_deref()).map(|_| ())
    }
}

fn remove_socket_path(path: Option<&Path>) -> Result<bool> {
    let Some(path) = path else {
        return Ok(true);
    };
    match std::fs::remove_file(path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error).with_context(|| {
                format!("Could not remove Unix enforcing proxy {}", path.display())
            });
        }
    }
    Ok(!path.exists())
}

enum ProxyListener {
    #[cfg(test)]
    Tcp(TcpListener),
    #[cfg(unix)]
    Unix(UnixListener),
}

struct ProxyBinding {
    listener: ProxyListener,
    endpoint_port: u16,
    socket_path: Option<PathBuf>,
}

impl ProxyListener {
    async fn accept(&self) -> io::Result<ProxyStream> {
        match self {
            #[cfg(test)]
            Self::Tcp(listener) => listener
                .accept()
                .await
                .map(|(stream, _)| ProxyStream::Tcp(stream)),
            #[cfg(unix)]
            Self::Unix(listener) => listener
                .accept()
                .await
                .map(|(stream, _)| ProxyStream::Unix(stream)),
        }
    }
}

enum ProxyStream {
    #[cfg(test)]
    Tcp(TcpStream),
    #[cfg(unix)]
    Unix(UnixStream),
}

impl AsyncRead for ProxyStream {
    fn poll_read(
        self: Pin<&mut Self>,
        context: &mut TaskContext<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match self.get_mut() {
            #[cfg(test)]
            Self::Tcp(stream) => Pin::new(stream).poll_read(context, buffer),
            #[cfg(unix)]
            Self::Unix(stream) => Pin::new(stream).poll_read(context, buffer),
        }
    }
}

impl AsyncWrite for ProxyStream {
    fn poll_write(
        self: Pin<&mut Self>,
        context: &mut TaskContext<'_>,
        buffer: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.get_mut() {
            #[cfg(test)]
            Self::Tcp(stream) => Pin::new(stream).poll_write(context, buffer),
            #[cfg(unix)]
            Self::Unix(stream) => Pin::new(stream).poll_write(context, buffer),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, context: &mut TaskContext<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            #[cfg(test)]
            Self::Tcp(stream) => Pin::new(stream).poll_flush(context),
            #[cfg(unix)]
            Self::Unix(stream) => Pin::new(stream).poll_flush(context),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, context: &mut TaskContext<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            #[cfg(test)]
            Self::Tcp(stream) => Pin::new(stream).poll_shutdown(context),
            #[cfg(unix)]
            Self::Unix(stream) => Pin::new(stream).poll_shutdown(context),
        }
    }
}

impl Drop for EnforcingProxy {
    fn drop(&mut self) {
        self.shutdown.send_replace(true);
        let _ = self.remove_socket();
    }
}

struct ProxyTlsIdentity {
    server: Arc<ServerConfig>,
    ca_pem: String,
    #[cfg(test)]
    ca_der: Vec<u8>,
}

struct ProxyState {
    upstream: Url,
    endpoint: Url,
    client: ProxyUpstreamClient,
    budget: Arc<CallBudget>,
    max_body_bytes: usize,
    call_timeout: Duration,
    connection_timeout: Duration,
    max_streams: u32,
}

enum ProxyUpstreamClient {
    Network(Box<UpstreamClient>),
    #[cfg(unix)]
    ManagedServerSocket {
        host_path: PathBuf,
        expected_peer_pid: Arc<AtomicU32>,
    },
}

impl ProxyUpstreamClient {
    fn new(transport: ProxyUpstream) -> Result<(Self, Option<Arc<AtomicU32>>)> {
        match transport {
            ProxyUpstream::Network => create_upstream_client()
                .map(Box::new)
                .map(Self::Network)
                .map(|client| (client, None)),
            #[cfg(unix)]
            ProxyUpstream::ManagedServerSocket { host_path } => {
                validate_unix_socket_parent(&host_path)?;
                let expected_peer_pid = Arc::new(AtomicU32::new(0));
                Ok((
                    Self::ManagedServerSocket {
                        host_path,
                        expected_peer_pid: Arc::clone(&expected_peer_pid),
                    },
                    Some(expected_peer_pid),
                ))
            }
        }
    }

    async fn request(
        &self,
        request: Request<ProxyBody>,
    ) -> std::result::Result<Response<Incoming>, ForwardFailure> {
        match self {
            Self::Network(client) => client
                .request(request)
                .await
                .map_err(|_| ForwardFailure::Failed),
            #[cfg(unix)]
            Self::ManagedServerSocket {
                host_path,
                expected_peer_pid,
            } => {
                use std::os::unix::fs::FileTypeExt;

                let expected_peer_pid = expected_peer_pid.load(Ordering::Acquire);
                if expected_peer_pid == 0 {
                    return Err(ForwardFailure::Rejected);
                }
                let metadata =
                    std::fs::symlink_metadata(host_path).map_err(|_| ForwardFailure::Failed)?;
                if !metadata.file_type().is_socket() {
                    return Err(ForwardFailure::Rejected);
                }
                let socket_address =
                    UnixSocketAddress::new(host_path).map_err(|_| ForwardFailure::Failed)?;
                let stream = UnixStream::connect(socket_address.path())
                    .await
                    .map_err(|_| ForwardFailure::Failed)?;
                let peer_pid = stream
                    .peer_cred()
                    .ok()
                    .and_then(|credential| credential.pid())
                    .and_then(|pid| u32::try_from(pid).ok());
                if peer_pid != Some(expected_peer_pid) {
                    return Err(ForwardFailure::Rejected);
                }
                let (mut sender, connection) =
                    http1::handshake::<_, ProxyBody>(TokioIo::new(stream))
                        .await
                        .map_err(|_| ForwardFailure::Failed)?;
                tokio::spawn(async move {
                    let _ = connection.await;
                });
                sender
                    .send_request(request)
                    .await
                    .map_err(|_| ForwardFailure::Failed)
            }
        }
    }
}

#[derive(Debug)]
enum ForwardFailure {
    Rejected,
    Failed,
}

impl ForwardFailure {
    fn into_disposition(self) -> CallDisposition {
        match self {
            Self::Rejected => CallDisposition::Rejected,
            Self::Failed => CallDisposition::Failed,
        }
    }
}

async fn serve_proxy(
    listener: ProxyListener,
    acceptor: TlsAcceptor,
    state: Arc<ProxyState>,
    connection_limit: Arc<Semaphore>,
    mut shutdown: watch::Receiver<bool>,
) -> Result<()> {
    let mut connections = JoinSet::new();
    loop {
        while connections.try_join_next().is_some() {}
        let connection_permit = tokio::select! {
            changed = shutdown.changed() => {
                let _ = changed;
                break;
            }
            permit = Arc::clone(&connection_limit).acquire_owned() => {
                permit.context("Enforcing proxy connection scheduler closed")?
            }
        };
        let stream = tokio::select! {
            changed = shutdown.changed() => {
                let _ = changed;
                break;
            }
            accepted = listener.accept() => accepted.context("Enforcing proxy accept failed")?,
        };
        let acceptor = acceptor.clone();
        let state = Arc::clone(&state);
        connections.spawn(async move {
            let _connection_permit = connection_permit;
            let stream =
                match tokio::time::timeout(state.connection_timeout, acceptor.accept(stream)).await
                {
                    Ok(Ok(stream)) => stream,
                    Ok(Err(_)) | Err(_) => return,
                };
            let connection_timeout = state.connection_timeout;
            let max_streams = state.max_streams;
            let service_state = Arc::clone(&state);
            let service =
                service_fn(move |request| handle_request(Arc::clone(&service_state), request));
            let mut builder = ServerBuilder::new(TokioExecutor::new());
            builder.http1().max_headers(128).max_buf_size(64 * 1024);
            builder
                .http2()
                .max_concurrent_streams(max_streams)
                .max_header_list_size(64 * 1024);
            let _ = tokio::time::timeout(
                connection_timeout,
                builder.serve_connection(TokioIo::new(stream), service),
            )
            .await;
        });
    }
    connections.abort_all();
    while connections.join_next().await.is_some() {}
    Ok(())
}

fn validate_proxy_start(
    upstream: &Url,
    limits: &ExecutionLimits,
    call_timeout_ms: u64,
) -> Result<()> {
    validate_upstream(upstream)?;
    if call_timeout_ms == 0 || call_timeout_ms > limits.run_timeout_ms {
        anyhow::bail!("Proxy call timeout must be within the execution timeout");
    }
    Ok(())
}

fn resolve_proxy_connection_limit(limits: &ExecutionLimits) -> Result<usize> {
    let available = limits
        .max_open_files
        .checked_sub(PROXY_RESERVED_FILE_DESCRIPTORS)
        .context("max_open_files leaves no enforcing-proxy file reserve")?;
    let connections = (available / 2).min(PROXY_MAX_DOWNSTREAM_CONNECTIONS);
    if connections < limits.max_concurrency {
        anyhow::bail!("max_concurrency exceeds enforcing-proxy connection capacity");
    }
    usize::try_from(connections)
        .map_err(|_| anyhow::anyhow!("enforcing-proxy connection limit does not fit this host"))
}

#[cfg(unix)]
fn validate_unix_socket_parent(path: &Path) -> Result<()> {
    if !path.is_absolute() {
        anyhow::bail!("Unix enforcing proxy socket must be absolute");
    }
    let parent = path
        .parent()
        .context("Unix enforcing proxy socket has no parent")?;
    let metadata = std::fs::symlink_metadata(parent).with_context(|| {
        format!(
            "Could not inspect Unix enforcing proxy directory {}",
            parent.display()
        )
    })?;
    if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
        anyhow::bail!("Unix enforcing proxy parent must be a real directory");
    }
    Ok(())
}

#[cfg(unix)]
struct UnixSocketAddress {
    path: PathBuf,
    #[cfg(target_os = "linux")]
    _parent: File,
}

#[cfg(unix)]
impl UnixSocketAddress {
    fn new(path: &Path) -> Result<Self> {
        validate_unix_socket_parent(path)?;
        #[cfg(target_os = "linux")]
        {
            let parent_path = path.parent().context("Unix socket path has no parent")?;
            let file_name = path
                .file_name()
                .context("Unix socket path has no file name")?;
            let parent = File::open(parent_path).with_context(|| {
                format!(
                    "Could not open Unix socket directory {}",
                    parent_path.display()
                )
            })?;
            let path =
                PathBuf::from(format!("/proc/self/fd/{}", parent.as_raw_fd())).join(file_name);
            Ok(Self {
                path,
                _parent: parent,
            })
        }
        #[cfg(not(target_os = "linux"))]
        Ok(Self {
            path: path.to_path_buf(),
        })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

async fn handle_request(
    state: Arc<ProxyState>,
    request: Request<Incoming>,
) -> std::result::Result<Response<ProxyBody>, Infallible> {
    let category = request_category(&request);
    let permit = match state.budget.reserve_call(category).await {
        Ok(permit) => permit,
        Err(_) => return Ok(error_response(StatusCode::TOO_MANY_REQUESTS)),
    };
    let call_deadline = tokio::time::Instant::now()
        .checked_add(state.call_timeout)
        .unwrap_or_else(|| permit.deadline());
    let deadline = permit.deadline().min(call_deadline);
    let result = tokio::select! {
        result = tokio::time::timeout_at(deadline, forward_request(&state, request)) => result,
        () = state.budget.wait_for_cancellation() => {
            permit.finish(CallDisposition::Cancelled);
            return Ok(error_response(StatusCode::GATEWAY_TIMEOUT));
        }
    };
    match result {
        Ok(Ok(response)) => {
            permit.finish(CallDisposition::Completed);
            Ok(response)
        }
        Ok(Err(error)) => {
            let disposition = error.into_disposition();
            permit.finish(disposition);
            Ok(error_response(StatusCode::BAD_GATEWAY))
        }
        Err(_) => {
            state.budget.cancel();
            permit.finish(CallDisposition::Cancelled);
            Ok(error_response(StatusCode::GATEWAY_TIMEOUT))
        }
    }
}

async fn forward_request(
    state: &ProxyState,
    request: Request<Incoming>,
) -> std::result::Result<Response<ProxyBody>, ForwardFailure> {
    let (parts, body) = request.into_parts();
    if !has_expected_proxy_origin(&parts.uri, &state.endpoint)
        && (parts.uri.scheme().is_some() || parts.uri.authority().is_some())
    {
        return Err(ForwardFailure::Rejected);
    }
    let path_and_query = parts
        .uri
        .path_and_query()
        .map(|value| value.as_str())
        .unwrap_or("/");
    let upstream_uri =
        upstream_uri(&state.upstream, path_and_query).map_err(|_| ForwardFailure::Rejected)?;
    let request_body = Limited::new(body, state.max_body_bytes)
        .collect()
        .await
        .map_err(|_| ForwardFailure::Rejected)?
        .to_bytes();
    let mut upstream = Request::builder().method(parts.method).uri(upstream_uri);
    copy_headers(
        upstream.headers_mut().ok_or(ForwardFailure::Rejected)?,
        &parts.headers,
        false,
    )
    .map_err(|_| ForwardFailure::Rejected)?;
    let authority = upstream_authority(&state.upstream).map_err(|_| ForwardFailure::Rejected)?;
    upstream = upstream.header(HOST, authority);
    let upstream = upstream
        .body(Full::new(request_body))
        .map_err(|_| ForwardFailure::Rejected)?;
    let response = state.client.request(upstream).await?;
    let (parts, body) = response.into_parts();
    let response_body = Limited::new(body, state.max_body_bytes)
        .collect()
        .await
        .map_err(|_| ForwardFailure::Failed)?
        .to_bytes();
    let mut downstream = Response::builder()
        .status(parts.status)
        .version(parts.version);
    copy_headers(
        downstream.headers_mut().ok_or(ForwardFailure::Rejected)?,
        &parts.headers,
        true,
    )
    .map_err(|_| ForwardFailure::Rejected)?;
    if let Some(location) = parts.headers.get(LOCATION) {
        let location = rewrite_location(location, &state.upstream, &state.endpoint)
            .map_err(|_| ForwardFailure::Rejected)?;
        downstream = downstream.header(LOCATION, location);
    }
    downstream
        .body(Full::new(response_body))
        .map_err(|_| ForwardFailure::Rejected)
}

fn request_category(request: &Request<Incoming>) -> CallCategory {
    request
        .headers()
        .get(CALL_CATEGORY_HEADER)
        .and_then(|value| value.to_str().ok())
        .and_then(parse_call_category)
        .unwrap_or(CallCategory::GeneratedCase)
}

fn parse_call_category(value: &str) -> Option<CallCategory> {
    match value {
        "setup" => Some(CallCategory::Setup),
        "readiness" => Some(CallCategory::Readiness),
        "authentication" => Some(CallCategory::Authentication),
        "generated_case" => Some(CallCategory::GeneratedCase),
        "stateful_step" => Some(CallCategory::StatefulStep),
        "reduction" => Some(CallCategory::Reduction),
        "retry" => Some(CallCategory::Retry),
        "validation" => Some(CallCategory::Validation),
        "cleanup" => None,
        _ => None,
    }
}

fn validate_upstream(upstream: &Url) -> Result<()> {
    if !matches!(upstream.scheme(), "http" | "https")
        || upstream.host_str().is_none()
        || !upstream.username().is_empty()
        || upstream.password().is_some()
        || upstream.query().is_some()
        || upstream.fragment().is_some()
    {
        anyhow::bail!("Enforcing proxy upstream must be an exact credential-free HTTP origin");
    }
    Ok(())
}

fn upstream_uri(upstream: &Url, path_and_query: &str) -> Result<Uri> {
    let authority = upstream_authority(upstream)?;
    format!("{}://{authority}{path_and_query}", upstream.scheme())
        .parse()
        .context("Could not build exact upstream URI")
}

fn has_expected_proxy_origin(uri: &Uri, endpoint: &Url) -> bool {
    match (uri.scheme_str(), uri.authority()) {
        (None, None) => true,
        (Some(scheme), Some(authority)) => {
            scheme == endpoint.scheme()
                && upstream_authority(endpoint).is_ok_and(|expected| authority.as_str() == expected)
        }
        _ => false,
    }
}

fn upstream_authority(upstream: &Url) -> Result<String> {
    let host = upstream
        .host_str()
        .context("Enforcing proxy upstream has no host")?;
    let host = if host.contains(':') {
        format!("[{host}]")
    } else {
        host.to_string()
    };
    Ok(match upstream.port_or_known_default() {
        Some(port) => format!("{host}:{port}"),
        None => host,
    })
}

fn copy_headers(
    output: &mut hyper::HeaderMap,
    headers: &hyper::HeaderMap,
    response: bool,
) -> Result<()> {
    let connection_headers = connection_header_names(headers)?;
    for (name, value) in headers {
        if is_hop_by_hop(name)
            || connection_headers.contains(name)
            || name == HOST
            || name == CONTENT_LENGTH
            || (response && name == LOCATION)
        {
            continue;
        }
        output.append(name, value.clone());
    }
    Ok(())
}

fn connection_header_names(headers: &hyper::HeaderMap) -> Result<HashSet<HeaderName>> {
    let mut names = HashSet::new();
    for value in headers.get_all(CONNECTION) {
        for name in value
            .to_str()
            .context("Connection header is not text")?
            .split(',')
        {
            let name = name.trim();
            if !name.is_empty() {
                names.insert(
                    HeaderName::from_bytes(name.as_bytes())
                        .context("Connection header names an invalid field")?,
                );
            }
        }
    }
    Ok(names)
}

fn is_hop_by_hop(name: &HeaderName) -> bool {
    name == CONNECTION
        || name == TE
        || matches!(
            name.as_str(),
            "keep-alive"
                | "proxy-authenticate"
                | "proxy-authorization"
                | "proxy-connection"
                | "trailer"
                | "transfer-encoding"
                | "upgrade"
                | CALL_CATEGORY_HEADER
        )
}

fn rewrite_location(value: &HeaderValue, upstream: &Url, endpoint: &Url) -> Result<HeaderValue> {
    let value = value.to_str().context("Redirect location is not text")?;
    let destination = upstream
        .join(value)
        .context("Redirect location is not a valid URL")?;
    if !same_origin(&destination, upstream) {
        anyhow::bail!("Redirect leaves the exact planned destination");
    }
    let mut rewritten = endpoint.clone();
    rewritten.set_path(destination.path());
    rewritten.set_query(destination.query());
    HeaderValue::from_str(rewritten.as_str()).context("Could not rewrite safe redirect")
}

fn same_origin(left: &Url, right: &Url) -> bool {
    left.scheme() == right.scheme()
        && left.host_str() == right.host_str()
        && left.port_or_known_default() == right.port_or_known_default()
}

fn create_upstream_client() -> Result<UpstreamClient> {
    let connector = HttpsConnectorBuilder::new()
        .with_native_roots()
        .context("Could not load native roots for enforcing proxy upstream TLS")?
        .https_or_http()
        .enable_http1()
        .enable_http2()
        .build();
    Ok(Client::builder(TokioExecutor::new()).build(connector))
}

fn create_proxy_tls_identity() -> Result<ProxyTlsIdentity> {
    let mut ca_params = CertificateParams::new(Vec::<String>::new())?;
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca_params
        .distinguished_name
        .push(DnType::CommonName, "CodeAtlas ephemeral execution CA");
    let ca_key = KeyPair::generate()?;
    let ca = CertifiedIssuer::self_signed(ca_params, ca_key)?;

    let mut server_params = CertificateParams::new(vec!["localhost".to_string()])?;
    server_params
        .subject_alt_names
        .push(SanType::IpAddress(IpAddr::V4(Ipv4Addr::LOCALHOST)));
    server_params
        .distinguished_name
        .push(DnType::CommonName, "CodeAtlas enforcing proxy");
    let server_key = KeyPair::generate()?;
    let server = server_params.signed_by(&server_key, &ca)?;
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(server_key.serialize_der()));
    let mut config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![server.der().clone()], key)
        .context("Could not configure enforcing proxy TLS")?;
    config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    Ok(ProxyTlsIdentity {
        server: Arc::new(config),
        ca_pem: ca.pem(),
        #[cfg(test)]
        ca_der: ca.der().to_vec(),
    })
}

fn error_response(status: StatusCode) -> Response<ProxyBody> {
    Response::builder()
        .status(status)
        .body(Full::new(Bytes::new()))
        .expect("static proxy error response")
}

#[cfg(test)]
mod tests {
    use super::{
        copy_headers, has_expected_proxy_origin, EnforcingProxy, ProxyBody, ProxyEndpoint,
        ProxyUpstream, CALL_CATEGORY_HEADER,
    };
    use crate::execution::budget::CallDisposition;
    use crate::execution::model::{CallCategory, ExecutionLimits};
    use crate::execution::scheduler::ExecutionScheduler;
    use anyhow::{Context, Result};
    use bytes::Bytes;
    use http_body_util::{BodyExt, Full};
    use hyper::body::Incoming;
    #[cfg(unix)]
    use hyper::client::conn::http1;
    use hyper::service::service_fn;
    use hyper::{HeaderMap, Request, Response, StatusCode, Uri};
    use hyper_rustls::{HttpsConnector, HttpsConnectorBuilder};
    use hyper_util::client::legacy::connect::HttpConnector;
    use hyper_util::client::legacy::Client;
    use hyper_util::rt::{TokioExecutor, TokioIo};
    use hyper_util::server::conn::auto::Builder as ServerBuilder;
    use rustls::pki_types::CertificateDer;
    #[cfg(unix)]
    use rustls::pki_types::ServerName;
    use rustls::{ClientConfig, RootCertStore};
    use std::collections::BTreeMap;
    use std::convert::Infallible;
    use std::net::Ipv4Addr;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::Arc;
    use tokio::net::TcpListener;
    #[cfg(unix)]
    use tokio::net::{UnixListener, UnixStream};
    use tokio::sync::{watch, Notify};
    use tokio::task::{JoinHandle, JoinSet};
    use tokio_rustls::TlsAcceptor;
    #[cfg(unix)]
    use tokio_rustls::TlsConnector;
    use url::Url;

    type ProxyClient = Client<HttpsConnector<HttpConnector>, ProxyBody>;

    #[derive(Clone, Copy)]
    enum TargetBehavior {
        Success,
        ExternalRedirect,
        Oversized,
        Pending,
    }

    struct TestTarget {
        url: Url,
        calls: Arc<AtomicU64>,
        saw_call_category_header: Arc<AtomicBool>,
        observed_call: Arc<Notify>,
        shutdown: watch::Sender<bool>,
        task: JoinHandle<()>,
    }

    impl TestTarget {
        async fn start(behavior: TargetBehavior) -> Result<Self> {
            Self::start_with_tls(behavior, None).await
        }

        async fn start_untrusted_tls(behavior: TargetBehavior) -> Result<Self> {
            let tls = super::create_proxy_tls_identity()?;
            Self::start_with_tls(behavior, Some(TlsAcceptor::from(tls.server))).await
        }

        async fn start_with_tls(
            behavior: TargetBehavior,
            acceptor: Option<TlsAcceptor>,
        ) -> Result<Self> {
            let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
            let address = listener.local_addr()?;
            let calls = Arc::new(AtomicU64::new(0));
            let observed = Arc::clone(&calls);
            let saw_call_category_header = Arc::new(AtomicBool::new(false));
            let observed_header = Arc::clone(&saw_call_category_header);
            let observed_call = Arc::new(Notify::new());
            let call_notification = Arc::clone(&observed_call);
            let uses_tls = acceptor.is_some();
            let (shutdown, mut receiver) = watch::channel(false);
            let task = tokio::spawn(async move {
                let mut connections = JoinSet::new();
                loop {
                    while connections.try_join_next().is_some() {}
                    let (stream, _) = tokio::select! {
                        changed = receiver.changed() => {
                            let _ = changed;
                            break;
                        }
                        accepted = listener.accept() => match accepted {
                            Ok(value) => value,
                            Err(_) => break,
                        }
                    };
                    let observed = Arc::clone(&observed);
                    let observed_header = Arc::clone(&observed_header);
                    let call_notification = Arc::clone(&call_notification);
                    let acceptor = acceptor.clone();
                    connections.spawn(async move {
                        if let Some(acceptor) = acceptor {
                            if let Ok(stream) = acceptor.accept(stream).await {
                                serve_target_connection(
                                    stream,
                                    observed,
                                    observed_header,
                                    call_notification,
                                    behavior,
                                )
                                .await;
                            }
                        } else {
                            serve_target_connection(
                                stream,
                                observed,
                                observed_header,
                                call_notification,
                                behavior,
                            )
                            .await;
                        }
                    });
                }
                connections.abort_all();
                while connections.join_next().await.is_some() {}
            });
            Ok(Self {
                url: Url::parse(&format!(
                    "{}://{address}/",
                    if uses_tls { "https" } else { "http" }
                ))?,
                calls,
                saw_call_category_header,
                observed_call,
                shutdown,
                task,
            })
        }

        async fn shutdown(self) -> Result<()> {
            self.shutdown.send_replace(true);
            self.task.await.context("fixture target task")?;
            Ok(())
        }
    }

    async fn serve_target_connection<I>(
        stream: I,
        observed: Arc<AtomicU64>,
        observed_header: Arc<AtomicBool>,
        call_notification: Arc<Notify>,
        behavior: TargetBehavior,
    ) where
        I: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    {
        let service = service_fn(move |request: Request<Incoming>| {
            let observed = Arc::clone(&observed);
            let observed_header = Arc::clone(&observed_header);
            let call_notification = Arc::clone(&call_notification);
            async move {
                observed.fetch_add(1, Ordering::SeqCst);
                if request.headers().contains_key(CALL_CATEGORY_HEADER) {
                    observed_header.store(true, Ordering::SeqCst);
                }
                call_notification.notify_one();
                let response = match behavior {
                    TargetBehavior::Success => {
                        Response::new(Full::new(Bytes::from_static(b"target reached")))
                    }
                    TargetBehavior::ExternalRedirect => Response::builder()
                        .status(StatusCode::FOUND)
                        .header("location", "https://outside.invalid/escape")
                        .body(Full::new(Bytes::new()))
                        .expect("fixture redirect"),
                    TargetBehavior::Oversized => {
                        Response::new(Full::new(Bytes::from(vec![b'x'; 64])))
                    }
                    TargetBehavior::Pending => std::future::pending().await,
                };
                Ok::<_, Infallible>(response)
            }
        });
        let builder = ServerBuilder::new(TokioExecutor::new());
        let _ = builder
            .serve_connection(TokioIo::new(stream), service)
            .await;
    }

    fn limits(max_calls: u64, max_body_bytes: u64) -> ExecutionLimits {
        ExecutionLimits {
            max_calls,
            calls_per_second: 1_000_000_000,
            max_concurrency: 2.min(max_calls),
            run_timeout_ms: 10_000,
            max_cpu_time_ms: 9_000,
            max_rss_bytes: 1024 * 1024,
            max_processes: 2,
            max_open_files: 32,
            max_call_result_bytes: max_body_bytes,
            max_output_bytes: 1024,
            max_artifact_bytes: 1024,
        }
    }

    fn client(endpoint: &ProxyEndpoint) -> Result<ProxyClient> {
        let mut roots = RootCertStore::empty();
        roots.add(CertificateDer::from(endpoint.ca_der.clone()))?;
        let config = ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        let connector = HttpsConnectorBuilder::new()
            .with_tls_config(config)
            .https_only()
            .enable_http1()
            .enable_http2()
            .build();
        Ok(Client::builder(TokioExecutor::new()).build(connector))
    }

    async fn request(
        client: &ProxyClient,
        endpoint: &ProxyEndpoint,
        category: Option<&str>,
    ) -> Result<(StatusCode, Bytes)> {
        request_with_body(client, endpoint, category, Bytes::new()).await
    }

    async fn request_with_body(
        client: &ProxyClient,
        endpoint: &ProxyEndpoint,
        category: Option<&str>,
        body: Bytes,
    ) -> Result<(StatusCode, Bytes)> {
        let mut request = Request::builder()
            .method("GET")
            .uri(endpoint.base_url.as_str());
        if let Some(category) = category {
            request = request.header(CALL_CATEGORY_HEADER, category);
        }
        let response = client.request(request.body(Full::new(body))?).await?;
        let status = response.status();
        let body = response.into_body().collect().await?.to_bytes();
        Ok((status, body))
    }

    #[cfg(unix)]
    async fn request_unix(
        sender: &mut http1::SendRequest<ProxyBody>,
        endpoint: &ProxyEndpoint,
    ) -> Result<StatusCode> {
        let response = sender
            .send_request(
                Request::builder()
                    .method("GET")
                    .uri(endpoint.base_url.as_str())
                    .body(Full::new(Bytes::new()))?,
            )
            .await?;
        let status = response.status();
        response.into_body().collect().await?;
        Ok(status)
    }

    #[cfg(unix)]
    async fn unix_client(
        socket: &std::path::Path,
        endpoint: &ProxyEndpoint,
    ) -> Result<(
        http1::SendRequest<ProxyBody>,
        JoinHandle<std::result::Result<(), hyper::Error>>,
    )> {
        let mut roots = RootCertStore::empty();
        roots.add(CertificateDer::from(endpoint.ca_der.clone()))?;
        let config = ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        let socket_address = super::UnixSocketAddress::new(socket)?;
        let tls = TlsConnector::from(Arc::new(config))
            .connect(
                ServerName::try_from("localhost")?,
                UnixStream::connect(socket_address.path()).await?,
            )
            .await?;
        let (sender, connection) = http1::handshake(TokioIo::new(tls)).await?;
        Ok((sender, tokio::spawn(connection)))
    }

    #[test]
    fn proxy_blocks_call_max_plus_one_before_the_target_observes_it() {
        let limits = limits(2, 1024);
        let scheduler = ExecutionScheduler::new(&limits, 0).expect("scheduler");
        scheduler
            .run(|context| async move {
                let target = TestTarget::start(TargetBehavior::Success).await?;
                let proxy =
                    EnforcingProxy::start(&context, target.url.clone(), &limits, 3_000).await?;
                assert!(proxy
                    .endpoint()
                    .ca_pem
                    .starts_with("-----BEGIN CERTIFICATE-----"));
                let client = client(proxy.endpoint())?;

                let first = request(&client, proxy.endpoint(), Some("authentication")).await?;
                let second = request(&client, proxy.endpoint(), None).await?;
                let rejected = request(&client, proxy.endpoint(), Some("validation")).await?;
                assert_eq!(first.0, StatusCode::OK);
                assert_eq!(second.0, StatusCode::OK);
                assert_eq!(rejected.0, StatusCode::TOO_MANY_REQUESTS);
                assert_eq!(target.calls.load(Ordering::SeqCst), 2);
                assert!(!target.saw_call_category_header.load(Ordering::SeqCst));

                let snapshot = context.budget().snapshot();
                assert_eq!(snapshot.usage.consumed, 2);
                assert_eq!(snapshot.records[0].sequence, 1);
                assert_eq!(snapshot.records[1].sequence, 2);
                proxy.shutdown().await?;
                target.shutdown().await?;
                Ok(())
            })
            .expect("proxy conformance");
    }

    #[cfg(unix)]
    #[test]
    fn unix_listener_uses_the_same_tls_and_call_budget_boundary() {
        let root = std::env::temp_dir().join(format!(
            "codeatlas-proxy-unix-{}-{}",
            std::process::id(),
            "long-state-root".repeat(8)
        ));
        std::fs::create_dir(&root).expect("private Unix proxy fixture root");
        let socket = root.join("client.sock");
        let limits = limits(1, 1024);
        let scheduler = ExecutionScheduler::new(&limits, 0).expect("scheduler");
        let result = scheduler.run(|context| {
            let socket = socket.clone();
            async move {
                let target = TestTarget::start(TargetBehavior::Success).await?;
                let proxy = EnforcingProxy::start_unix(
                    &context,
                    target.url.clone(),
                    ProxyUpstream::Network,
                    &limits,
                    3_000,
                    &socket,
                    8443,
                )
                .await?;

                let (mut sender, connection) = unix_client(&socket, proxy.endpoint()).await?;

                assert_eq!(
                    request_unix(&mut sender, proxy.endpoint()).await?,
                    StatusCode::OK
                );
                assert_eq!(
                    request_unix(&mut sender, proxy.endpoint()).await?,
                    StatusCode::TOO_MANY_REQUESTS
                );
                assert_eq!(target.calls.load(Ordering::SeqCst), 1);
                drop(sender);
                proxy.shutdown().await?;
                let _ = connection.await;
                assert!(!socket.exists(), "Unix proxy socket must be removed");
                target.shutdown().await?;
                Ok(())
            }
        });
        std::fs::remove_dir(&root).expect("remove Unix proxy fixture root");
        result.expect("Unix listener conformance");
    }

    #[cfg(unix)]
    #[test]
    fn managed_server_socket_separates_connection_and_call_limits() {
        const MANAGED_REQUESTS: u64 = 2;
        let root = std::env::temp_dir().join(format!(
            "codeatlas-proxy-managed-{}-{}",
            std::process::id(),
            "long-state-root".repeat(8)
        ));
        std::fs::create_dir(&root).expect("private managed-server fixture root");
        let client_socket = root.join("client.sock");
        let server_socket = root.join("server.sock");
        let mut limits = limits(MANAGED_REQUESTS + 1, 1024);
        limits.max_concurrency = 1;
        let scheduler = ExecutionScheduler::new(&limits, 0).expect("scheduler");
        let result = scheduler.run(|context| {
            let client_socket = client_socket.clone();
            let server_socket = server_socket.clone();
            async move {
                let server_address = super::UnixSocketAddress::new(&server_socket)?;
                let listener = UnixListener::bind(server_address.path())?;
                let calls = Arc::new(AtomicU64::new(0));
                let observed = Arc::clone(&calls);
                let target = tokio::spawn(async move {
                    for _ in 0..MANAGED_REQUESTS {
                        let (stream, _) = listener.accept().await?;
                        serve_target_connection(
                            stream,
                            Arc::clone(&observed),
                            Arc::new(AtomicBool::new(false)),
                            Arc::new(Notify::new()),
                            TargetBehavior::Success,
                        )
                        .await;
                    }
                    Ok::<_, anyhow::Error>(())
                });
                let upstream = Url::parse("http://127.0.0.1:41002/")?;
                let proxy = EnforcingProxy::start_unix(
                    &context,
                    upstream,
                    ProxyUpstream::ManagedServerSocket {
                        host_path: server_socket.clone(),
                    },
                    &limits,
                    3_000,
                    &client_socket,
                    41001,
                )
                .await?;
                let (mut sender, connection) =
                    unix_client(&client_socket, proxy.endpoint()).await?;

                assert_eq!(
                    request_unix(&mut sender, proxy.endpoint()).await?,
                    StatusCode::BAD_GATEWAY
                );
                assert_eq!(calls.load(Ordering::SeqCst), 0);
                proxy.corroborate_managed_server_peer(std::process::id())?;
                assert_eq!(
                    request_unix(&mut sender, proxy.endpoint()).await?,
                    StatusCode::OK
                );
                let second_connection = tokio::time::timeout(
                    std::time::Duration::from_secs(1),
                    unix_client(&client_socket, proxy.endpoint()),
                )
                .await;
                let (mut second_sender, second_connection) = match second_connection {
                    Ok(connection) => connection?,
                    Err(_) => {
                        drop(sender);
                        proxy.shutdown().await?;
                        let _ = connection.await;
                        target.abort();
                        let _ = target.await;
                        std::fs::remove_file(&server_socket)?;
                        anyhow::bail!("second managed proxy connection was not admitted");
                    }
                };
                assert_eq!(
                    request_unix(&mut second_sender, proxy.endpoint()).await?,
                    StatusCode::OK
                );
                assert_eq!(
                    request_unix(&mut sender, proxy.endpoint()).await?,
                    StatusCode::TOO_MANY_REQUESTS
                );
                assert_eq!(calls.load(Ordering::SeqCst), MANAGED_REQUESTS);
                drop(sender);
                drop(second_sender);
                proxy.shutdown().await?;
                let _ = connection.await;
                let _ = second_connection.await;
                target.await.context("managed-server fixture task")??;
                assert!(!client_socket.exists());
                std::fs::remove_file(&server_socket)?;
                Ok(())
            }
        });
        std::fs::remove_dir(&root).expect("remove managed-server fixture root");
        result.expect("managed-server proxy conformance");
    }

    #[test]
    fn proxy_counts_supported_categories_and_denies_cleanup_reclassification() {
        let limits = limits(9, 1024);
        let scheduler = ExecutionScheduler::new(&limits, 0).expect("scheduler");
        scheduler
            .run(|context| async move {
                let target = TestTarget::start(TargetBehavior::Success).await?;
                let proxy =
                    EnforcingProxy::start(&context, target.url.clone(), &limits, 3_000).await?;
                let client = client(proxy.endpoint())?;
                for category in [
                    "setup",
                    "readiness",
                    "authentication",
                    "generated_case",
                    "stateful_step",
                    "reduction",
                    "retry",
                    "validation",
                    "cleanup",
                ] {
                    assert_eq!(
                        request(&client, proxy.endpoint(), Some(category)).await?.0,
                        StatusCode::OK
                    );
                }

                let counts = context
                    .budget()
                    .snapshot()
                    .usage
                    .by_category
                    .into_iter()
                    .map(|count| (count.category, count.count))
                    .collect::<BTreeMap<_, _>>();
                for category in [
                    CallCategory::Setup,
                    CallCategory::Readiness,
                    CallCategory::Authentication,
                    CallCategory::StatefulStep,
                    CallCategory::Reduction,
                    CallCategory::Retry,
                    CallCategory::Validation,
                ] {
                    assert_eq!(counts.get(&category), Some(&1));
                }
                assert_eq!(counts.get(&CallCategory::GeneratedCase), Some(&2));
                assert!(!counts.contains_key(&CallCategory::Cleanup));
                assert_eq!(target.calls.load(Ordering::SeqCst), 9);
                assert!(!target.saw_call_category_header.load(Ordering::SeqCst));
                proxy.shutdown().await?;
                target.shutdown().await?;
                Ok(())
            })
            .expect("call-category conformance");
    }

    #[test]
    fn cancellation_terminates_an_in_flight_proxy_call() {
        let limits = limits(1, 1024);
        let scheduler = ExecutionScheduler::new(&limits, 0).expect("scheduler");
        scheduler
            .run(|context| async move {
                let target = TestTarget::start(TargetBehavior::Pending).await?;
                let proxy =
                    EnforcingProxy::start(&context, target.url.clone(), &limits, 3_000).await?;
                let endpoint = proxy.endpoint().clone();
                let client = client(&endpoint)?;
                let observed = target.observed_call.notified();
                let call = tokio::spawn(async move { request(&client, &endpoint, None).await });
                observed.await;
                context.budget().cancel();
                let response = call.await.context("proxy request task")??;

                assert_eq!(response.0, StatusCode::GATEWAY_TIMEOUT);
                assert_eq!(target.calls.load(Ordering::SeqCst), 1);
                let snapshot = context.budget().snapshot();
                assert_eq!(snapshot.records[0].disposition, CallDisposition::Cancelled);
                assert_eq!(
                    snapshot.termination,
                    Some(crate::execution::budget::BudgetTermination::Cancelled)
                );
                proxy.shutdown().await?;
                target.shutdown().await?;
                Ok(())
            })
            .expect("in-flight cancellation");
    }

    #[test]
    fn proxy_rejects_redirects_outside_the_planned_origin() {
        let limits = limits(1, 1024);
        let scheduler = ExecutionScheduler::new(&limits, 0).expect("scheduler");
        scheduler
            .run(|context| async move {
                let target = TestTarget::start(TargetBehavior::ExternalRedirect).await?;
                let proxy =
                    EnforcingProxy::start(&context, target.url.clone(), &limits, 3_000).await?;
                let response = request(&client(proxy.endpoint())?, proxy.endpoint(), None).await?;
                assert_eq!(response.0, StatusCode::BAD_GATEWAY);
                assert_eq!(target.calls.load(Ordering::SeqCst), 1);
                assert_eq!(
                    context.budget().snapshot().records[0].disposition,
                    CallDisposition::Rejected
                );
                proxy.shutdown().await?;
                target.shutdown().await?;
                Ok(())
            })
            .expect("redirect enforcement");
    }

    #[test]
    fn proxy_bounds_upstream_response_bytes() {
        let limits = limits(1, 8);
        let scheduler = ExecutionScheduler::new(&limits, 0).expect("scheduler");
        scheduler
            .run(|context| async move {
                let target = TestTarget::start(TargetBehavior::Oversized).await?;
                let proxy =
                    EnforcingProxy::start(&context, target.url.clone(), &limits, 3_000).await?;
                let response = request(&client(proxy.endpoint())?, proxy.endpoint(), None).await?;
                assert_eq!(response.0, StatusCode::BAD_GATEWAY);
                assert!(response.1.len() < 128);
                assert_eq!(target.calls.load(Ordering::SeqCst), 1);
                assert_eq!(
                    context.budget().snapshot().records[0].disposition,
                    CallDisposition::Failed
                );
                proxy.shutdown().await?;
                target.shutdown().await?;
                Ok(())
            })
            .expect("response ceiling");
    }

    #[test]
    fn proxy_rejects_oversized_request_bytes_before_the_target_observes_them() {
        let limits = limits(1, 8);
        let scheduler = ExecutionScheduler::new(&limits, 0).expect("scheduler");
        scheduler
            .run(|context| async move {
                let target = TestTarget::start(TargetBehavior::Success).await?;
                let proxy =
                    EnforcingProxy::start(&context, target.url.clone(), &limits, 3_000).await?;
                let response = request_with_body(
                    &client(proxy.endpoint())?,
                    proxy.endpoint(),
                    None,
                    Bytes::from(vec![b'x'; 64]),
                )
                .await?;
                assert_eq!(response.0, StatusCode::BAD_GATEWAY);
                assert_eq!(
                    target.calls.load(Ordering::SeqCst),
                    0,
                    "an oversized request must be rejected before upstream dispatch"
                );
                assert_eq!(
                    context.budget().snapshot().records[0].disposition,
                    CallDisposition::Rejected
                );
                proxy.shutdown().await?;
                target.shutdown().await?;
                Ok(())
            })
            .expect("request ceiling");
    }

    #[test]
    fn proxy_never_bypasses_upstream_certificate_verification() {
        let limits = limits(1, 1024);
        let scheduler = ExecutionScheduler::new(&limits, 0).expect("scheduler");
        scheduler
            .run(|context| async move {
                let target = TestTarget::start_untrusted_tls(TargetBehavior::Success).await?;
                let proxy =
                    EnforcingProxy::start(&context, target.url.clone(), &limits, 3_000).await?;
                let response = request(&client(proxy.endpoint())?, proxy.endpoint(), None).await?;
                assert_eq!(response.0, StatusCode::BAD_GATEWAY);
                assert_eq!(
                    target.calls.load(Ordering::SeqCst),
                    0,
                    "an untrusted TLS target must not receive a decoded HTTP request"
                );
                assert_eq!(
                    context.budget().snapshot().records[0].disposition,
                    CallDisposition::Failed
                );
                proxy.shutdown().await?;
                target.shutdown().await?;
                Ok(())
            })
            .expect("upstream TLS verification");
    }

    #[test]
    fn absolute_proxy_requests_require_the_exact_tls_origin() {
        let endpoint = Url::parse("https://127.0.0.1:8443/api").expect("endpoint");
        let exact: Uri = "https://127.0.0.1:8443/widgets".parse().expect("exact URI");
        let wrong_port: Uri = "https://127.0.0.1:9443/widgets"
            .parse()
            .expect("wrong-port URI");
        let wrong_scheme: Uri = "http://127.0.0.1:8443/widgets"
            .parse()
            .expect("wrong-scheme URI");
        assert!(has_expected_proxy_origin(&exact, &endpoint));
        assert!(!has_expected_proxy_origin(&wrong_port, &endpoint));
        assert!(!has_expected_proxy_origin(&wrong_scheme, &endpoint));
    }

    #[test]
    fn connection_nominated_hop_headers_are_not_forwarded() {
        let mut input = HeaderMap::new();
        input.insert("connection", "x-private".parse().expect("connection value"));
        input.insert("x-private", "secret".parse().expect("private value"));
        input.insert("x-retained", "visible".parse().expect("retained value"));
        let mut output = HeaderMap::new();
        copy_headers(&mut output, &input, false).expect("filtered headers");
        assert!(!output.contains_key("connection"));
        assert!(!output.contains_key("x-private"));
        assert_eq!(output["x-retained"], "visible");
    }
}

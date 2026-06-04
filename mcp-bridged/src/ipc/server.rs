//! Local IPC server for the Bridge Console and CLI.
//!
//! Transport is per-platform:
//! - Unix: a Unix-domain socket, mode 0600, under the daemon's data dir.
//! - Windows: a named pipe at `\\.\pipe\mcp-bridge-control`, with the
//!   default DACL (creator + LocalSystem). Hardening the DACL down to
//!   the current user only is tracked as a follow-up.
//!
//! Wire format and dispatcher are identical across platforms — only the
//! accept loop and connection handler differ.

use std::path::{Path, PathBuf};

use thiserror::Error;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

use super::methods::{Context, dispatch};

/// Failure modes the IPC server surfaces to the daemon main loop.
#[derive(Debug, Error)]
pub enum IpcError {
    #[error("could not bind IPC socket at {path:?}: {source}")]
    Bind {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not set IPC socket permissions: {0}")]
    Permissions(#[source] std::io::Error),
    #[error("could not create parent directory for IPC socket: {0}")]
    ParentDir(#[source] std::io::Error),
    #[error("IPC accept loop failed: {0}")]
    Accept(#[source] std::io::Error),
}

/// Run the IPC server until `cancel` is signalled.
///
/// Dispatches to the platform-specific impl: UDS on Unix, named pipe
/// on Windows. The `socket_path` value comes from
/// [`crate::config::Config::ipc_socket_path`] — its meaning is the file
/// path on Unix and the pipe name on Windows.
pub async fn serve(
    socket_path: PathBuf,
    ctx: Context,
    cancel: CancellationToken,
) -> Result<(), IpcError> {
    #[cfg(unix)]
    {
        serve_unix(socket_path, ctx, cancel).await
    }
    #[cfg(windows)]
    {
        serve_windows(socket_path, ctx, cancel).await
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (socket_path, ctx, cancel);
        warn!("IPC server is not implemented on this platform");
        Ok(())
    }
}

#[cfg(unix)]
async fn serve_unix(
    socket_path: PathBuf,
    ctx: Context,
    cancel: CancellationToken,
) -> Result<(), IpcError> {
    use std::os::unix::fs::PermissionsExt;

    use tokio::net::UnixListener;

    if let Some(parent) = socket_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(IpcError::ParentDir)?;
    }

    // Remove any stale socket file from a previous run. UnixListener::bind
    // refuses to overwrite, so we clean up first.
    if let Err(e) = tokio::fs::remove_file(&socket_path).await {
        if e.kind() != std::io::ErrorKind::NotFound {
            warn!(error = ?e, path = ?socket_path, "could not remove stale IPC socket; bind may fail");
        }
    }

    let listener = UnixListener::bind(&socket_path).map_err(|e| IpcError::Bind {
        path: socket_path.clone(),
        source: e,
    })?;

    // Restrict to the owning user (mode 0600).
    let perms = std::fs::Permissions::from_mode(0o600);
    tokio::fs::set_permissions(&socket_path, perms)
        .await
        .map_err(IpcError::Permissions)?;

    info!(path = ?socket_path, "IPC server bound");

    let result = accept_loop(&listener, &ctx, &cancel).await;

    // Clean shutdown: remove the socket file so the next run starts clean.
    if let Err(e) = tokio::fs::remove_file(&socket_path).await {
        if e.kind() != std::io::ErrorKind::NotFound {
            warn!(error = ?e, path = ?socket_path, "could not remove IPC socket on shutdown");
        }
    }

    result
}

#[cfg(unix)]
async fn accept_loop(
    listener: &tokio::net::UnixListener,
    ctx: &Context,
    cancel: &CancellationToken,
) -> Result<(), IpcError> {
    loop {
        tokio::select! {
            () = cancel.cancelled() => {
                info!("IPC server shutting down");
                return Ok(());
            }
            res = listener.accept() => {
                match res {
                    Ok((stream, _addr)) => {
                        let ctx = ctx.clone();
                        let cancel = cancel.clone();
                        tokio::spawn(async move {
                            handle_unix_connection(stream, ctx, cancel).await;
                        });
                    }
                    Err(e) => {
                        error!(error = ?e, "IPC accept failed");
                        return Err(IpcError::Accept(e));
                    }
                }
            }
        }
    }
}

#[cfg(unix)]
async fn handle_unix_connection(
    mut stream: tokio::net::UnixStream,
    ctx: Context,
    cancel: CancellationToken,
) {
    use super::wire::{FrameError, JsonRpcRequest, read_frame, write_frame};

    let (reader, writer) = stream.split();
    let mut reader = tokio::io::BufReader::new(reader);
    let mut writer = tokio::io::BufWriter::new(writer);

    loop {
        let req: JsonRpcRequest = tokio::select! {
            () = cancel.cancelled() => return,
            r = read_frame(&mut reader) => match r {
                Ok(req) => req,
                Err(FrameError::Io(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                    // Peer closed cleanly.
                    debug!("IPC client disconnected");
                    return;
                }
                Err(e) => {
                    warn!(error = ?e, "IPC framing error; closing connection");
                    return;
                }
            },
        };

        // Subscription methods short-circuit the dispatch path —
        // after the initial success response the server pushes
        // notification frames until the client disconnects.
        if req.method == super::methods::method_names::LOG_SUBSCRIBE {
            run_log_subscribe(&mut writer, req.id, &ctx, &cancel).await;
            return;
        }

        let resp = dispatch(req, &ctx).await;

        if let Err(e) = write_frame(&mut writer, &resp).await {
            warn!(error = ?e, "IPC write failed; closing connection");
            return;
        }
        // Loop — keep the connection open for further requests until
        // the peer closes (which surfaces as UnexpectedEof on the next
        // read_frame).
    }
}

/// Drive a `log.subscribe` session: write the initial success
/// response, then push one [`super::wire::JsonRpcNotification`] frame
/// per broadcast event until the client disconnects or `cancel`
/// fires. Used by both the Unix-socket and Windows-named-pipe
/// connection handlers — the writer trait bounds keep them generic.
async fn run_log_subscribe<W>(
    writer: &mut W,
    req_id: serde_json::Value,
    ctx: &Context,
    cancel: &CancellationToken,
) where
    W: tokio::io::AsyncWriteExt + Unpin,
{
    use super::wire::{JsonRpcNotification, JsonRpcResponse, write_frame};

    // Initial response: empty object, ack. The client knows the
    // method is a subscription and now expects notification frames.
    let initial = JsonRpcResponse::success(req_id, serde_json::json!({}));
    if let Err(e) = write_frame(writer, &initial).await {
        warn!(error = ?e, "log.subscribe initial response failed");
        return;
    }

    let Some(recorder) = ctx.recorder.as_ref() else {
        // No recorder installed (some integration tests skip the
        // observability layer). Politely close.
        debug!("log.subscribe: no recorder installed; closing connection");
        return;
    };
    let mut rx = recorder.subscribe();

    loop {
        let event = tokio::select! {
            () = cancel.cancelled() => {
                debug!("log.subscribe: daemon shutting down");
                return;
            }
            recv = rx.recv() => recv,
        };

        let event = match event {
            Ok(e) => e,
            Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                // Subscriber fell behind. Send a synthetic event
                // telling the client some entries were skipped, then
                // continue. This is preferable to dropping the
                // connection: a busy console might briefly fall
                // behind during a log burst.
                let synth = JsonRpcNotification::new(
                    super::methods::method_names::LOG_ENTRY_NOTIFICATION,
                    serde_json::json!({
                        "seq": 0,
                        "timestamp": 0,
                        "level": "WARN",
                        "target": "ipc.log_subscribe",
                        "message": format!("log.subscribe lagged: {skipped} events dropped"),
                    }),
                );
                if write_frame(writer, &synth).await.is_err() {
                    return;
                }
                continue;
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                debug!("log.subscribe: broadcast channel closed");
                return;
            }
        };

        let params = match serde_json::to_value(&event) {
            Ok(v) => v,
            Err(e) => {
                warn!(error = ?e, "could not serialize log event for subscribe");
                continue;
            }
        };
        let frame = JsonRpcNotification::new(
            super::methods::method_names::LOG_ENTRY_NOTIFICATION,
            params,
        );
        if write_frame(writer, &frame).await.is_err() {
            // Client closed or pipe broken — exit cleanly.
            debug!("log.subscribe: write failed; assuming client disconnected");
            return;
        }
    }
}

/// Open a raw, bidirectional connection to the running daemon's IPC
/// endpoint. Used by callers that need to keep the connection alive
/// for multiple frames (e.g. `log.subscribe` subscriptions). Picks the
/// transport at compile time to match [`serve`].
pub async fn open_local(path: &Path) -> Result<LocalStream, super::wire::FrameError> {
    #[cfg(unix)]
    {
        let stream = tokio::net::UnixStream::connect(path)
            .await
            .map_err(super::wire::FrameError::Io)?;
        Ok(LocalStream::Unix(stream))
    }
    #[cfg(windows)]
    {
        use tokio::net::windows::named_pipe::ClientOptions;
        let pipe = ClientOptions::new()
            .open(path.as_os_str())
            .map_err(super::wire::FrameError::Io)?;
        Ok(LocalStream::Windows(pipe))
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = path;
        Err(super::wire::FrameError::Io(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "IPC client is not implemented on this platform",
        )))
    }
}

/// Owned bidirectional handle to the daemon's IPC endpoint. Returned
/// by [`open_local`]; the inner transport is platform-specific.
#[allow(missing_debug_implementations)] // tokio stream types aren't Debug.
pub enum LocalStream {
    #[cfg(unix)]
    Unix(tokio::net::UnixStream),
    #[cfg(windows)]
    Windows(tokio::net::windows::named_pipe::NamedPipeClient),
}

// AsyncRead / AsyncWrite forwarding so `read_frame` / `write_frame`
// can operate on LocalStream directly.
impl tokio::io::AsyncRead for LocalStream {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.get_mut() {
            #[cfg(unix)]
            Self::Unix(s) => std::pin::Pin::new(s).poll_read(cx, buf),
            #[cfg(windows)]
            Self::Windows(s) => std::pin::Pin::new(s).poll_read(cx, buf),
        }
    }
}

impl tokio::io::AsyncWrite for LocalStream {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        match self.get_mut() {
            #[cfg(unix)]
            Self::Unix(s) => std::pin::Pin::new(s).poll_write(cx, buf),
            #[cfg(windows)]
            Self::Windows(s) => std::pin::Pin::new(s).poll_write(cx, buf),
        }
    }
    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.get_mut() {
            #[cfg(unix)]
            Self::Unix(s) => std::pin::Pin::new(s).poll_flush(cx),
            #[cfg(windows)]
            Self::Windows(s) => std::pin::Pin::new(s).poll_flush(cx),
        }
    }
    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.get_mut() {
            #[cfg(unix)]
            Self::Unix(s) => std::pin::Pin::new(s).poll_shutdown(cx),
            #[cfg(windows)]
            Self::Windows(s) => std::pin::Pin::new(s).poll_shutdown(cx),
        }
    }
}

/// Connect to the running daemon's IPC endpoint, send one JSON-RPC
/// request, read one response, return it. Picks the transport at
/// compile time to match [`serve`]:
/// - Unix: UDS at the given file-system path.
/// - Windows: named pipe whose full name is the OS-string view of `path`.
pub async fn call_local(
    path: &Path,
    request: &super::wire::JsonRpcRequest,
) -> Result<super::wire::JsonRpcResponse, super::wire::FrameError> {
    #[cfg(unix)]
    {
        call_unix(path, request).await
    }
    #[cfg(windows)]
    {
        call_windows(path, request).await
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (path, request);
        Err(super::wire::FrameError::Io(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "IPC client is not implemented on this platform",
        )))
    }
}

/// Connect to the IPC socket, send one JSON-RPC request, read one
/// response, and return it. Unix-only sibling of [`call_local`]; most
/// callers want [`call_local`] for portability.
#[cfg(unix)]
pub async fn call_unix(
    socket_path: &Path,
    request: &super::wire::JsonRpcRequest,
) -> Result<super::wire::JsonRpcResponse, super::wire::FrameError> {
    use tokio::net::UnixStream;

    use super::wire::{read_frame, write_frame};

    let mut stream = UnixStream::connect(socket_path)
        .await
        .map_err(super::wire::FrameError::Io)?;
    write_frame(&mut stream, request).await?;
    read_frame(&mut stream).await
}

// ---------------------------------------------------------------------
// Windows named-pipe transport
// ---------------------------------------------------------------------

/// Pipe name used by both server and client. `Path` carries the same
/// OS-string the daemon's `Config::ipc_socket_path()` returns —
/// `\\.\pipe\mcp-bridge-control` by default.
#[cfg(windows)]
async fn serve_windows(
    pipe_path: PathBuf,
    ctx: Context,
    cancel: CancellationToken,
) -> Result<(), IpcError> {
    use tokio::net::windows::named_pipe::ServerOptions;

    let pipe_name = pipe_path.as_os_str();

    // First instance: created with `first_pipe_instance(true)` so a
    // second daemon trying to bind the same pipe gets a clean ERROR
    // instead of silently sharing the namespace.
    let mut server = ServerOptions::new()
        .first_pipe_instance(true)
        .create(pipe_name)
        .map_err(|e| IpcError::Bind {
            path: pipe_path.clone(),
            source: e,
        })?;

    info!(pipe = ?pipe_path, "IPC named-pipe server bound");

    loop {
        tokio::select! {
            () = cancel.cancelled() => {
                info!("IPC server shutting down");
                return Ok(());
            }
            res = server.connect() => {
                if let Err(e) = res {
                    error!(error = ?e, "named-pipe connect failed");
                    return Err(IpcError::Accept(e));
                }

                // Hand the connected pipe instance off to a task and
                // immediately create a new instance for the next caller.
                let connected = server;
                server = ServerOptions::new()
                    .create(pipe_name)
                    .map_err(|e| IpcError::Bind {
                        path: pipe_path.clone(),
                        source: e,
                    })?;

                let ctx = ctx.clone();
                let cancel = cancel.clone();
                tokio::spawn(async move {
                    handle_windows_connection(connected, ctx, cancel).await;
                });
            }
        }
    }
}

#[cfg(windows)]
async fn handle_windows_connection(
    mut pipe: tokio::net::windows::named_pipe::NamedPipeServer,
    ctx: Context,
    cancel: CancellationToken,
) {
    use super::wire::{FrameError, JsonRpcRequest, read_frame, write_frame};

    let (reader, writer) = tokio::io::split(&mut pipe);
    let mut reader = tokio::io::BufReader::new(reader);
    let mut writer = tokio::io::BufWriter::new(writer);

    loop {
        let req: JsonRpcRequest = tokio::select! {
            () = cancel.cancelled() => return,
            r = read_frame(&mut reader) => match r {
                Ok(req) => req,
                Err(FrameError::Io(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                    debug!("IPC client disconnected");
                    return;
                }
                Err(e) => {
                    warn!(error = ?e, "IPC framing error; closing connection");
                    return;
                }
            },
        };

        if req.method == super::methods::method_names::LOG_SUBSCRIBE {
            run_log_subscribe(&mut writer, req.id, &ctx, &cancel).await;
            return;
        }

        let resp = dispatch(req, &ctx).await;

        if let Err(e) = write_frame(&mut writer, &resp).await {
            warn!(error = ?e, "IPC write failed; closing connection");
            return;
        }
    }
}

#[cfg(windows)]
async fn call_windows(
    pipe_path: &Path,
    request: &super::wire::JsonRpcRequest,
) -> Result<super::wire::JsonRpcResponse, super::wire::FrameError> {
    use tokio::net::windows::named_pipe::ClientOptions;

    use super::wire::{read_frame, write_frame};

    let mut pipe = ClientOptions::new()
        .open(pipe_path.as_os_str())
        .map_err(super::wire::FrameError::Io)?;
    write_frame(&mut pipe, request).await?;
    read_frame(&mut pipe).await
}

// Every test in this module exercises the Unix-domain-socket path
// (UDS-specific tooling, mode-0600 perms). Windows tests would need
// a different harness using ServerOptions/ClientOptions and live in a
// follow-up commit.
#[cfg(all(test, unix))]
mod tests {
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use tokio::sync::RwLock;

    use super::*;
    use crate::adapters::Sentinel;
    use crate::identity::{DisplayName, Keypair, Keystore};
    use crate::ipc::methods::method_names;
    use crate::ipc::wire::JsonRpcRequest;
    use crate::pair::invite_register::InviteRegister;
    use crate::registry::Registry;

    fn make_ctx() -> Context {
        let cancel = CancellationToken::new();
        let invites = InviteRegister::spawn(cancel);
        crate::identity::keystore::install_mock_backend();
        let service = format!(
            "ipc-srv-tests-{}-{}",
            std::process::id(),
            std::time::Instant::now().elapsed().as_nanos()
        );
        let keystore = Arc::new(Keystore::for_service(&service).expect("keystore"));
        let tmp = tempfile::tempdir().expect("tmpdir");
        let registry_path = tmp.path().join("registry.json");
        std::mem::forget(tmp);
        Context {
            start: Instant::now(),
            registry: Arc::new(RwLock::new(Registry::new())),
            pair_endpoint_addr: "10.0.0.5:8765".parse().unwrap(),
            resolver: crate::identity::shared_keypair(Keypair::generate()),
            display_name: DisplayName::new("Test Bridge").unwrap(),
            invites,
            keystore,
            registry_path,
            sentinel: Sentinel::random(),
            adapters: Arc::new(Vec::new()),
            recorder: Some(crate::observability::EventRecorder::new()),
            connector_cache: Arc::new(crate::proxy::ConnectorCache::new()),
            rotation_signal: Arc::new(tokio::sync::Notify::new()),
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn round_trip_daemon_status() {
        let tmp = tempfile::tempdir().unwrap();
        let socket = tmp.path().join("control.sock");

        let ctx = make_ctx();
        let cancel = CancellationToken::new();
        let cancel_clone = cancel.clone();
        let socket_clone = socket.clone();
        let task = tokio::spawn(async move {
            serve(socket_clone, ctx, cancel_clone).await.unwrap();
        });

        // Let the listener bind.
        tokio::time::sleep(Duration::from_millis(50)).await;

        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: serde_json::json!("client-1"),
            method: method_names::DAEMON_STATUS.to_owned(),
            params: None,
        };
        let resp = call_unix(&socket, &req).await.unwrap();
        assert_eq!(resp.id, serde_json::json!("client-1"));
        let result = resp.result.expect("daemon.status returns a result");
        assert_eq!(result["pair_endpoint"], "10.0.0.5:8765");
        assert_eq!(result["pin_count"], 0);

        cancel.cancel();
        let _ = task.await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn unknown_method_returns_method_not_found_over_wire() {
        let tmp = tempfile::tempdir().unwrap();
        let socket = tmp.path().join("control.sock");

        let ctx = make_ctx();
        let cancel = CancellationToken::new();
        let cancel_clone = cancel.clone();
        let socket_clone = socket.clone();
        let task = tokio::spawn(async move {
            serve(socket_clone, ctx, cancel_clone).await.unwrap();
        });
        tokio::time::sleep(Duration::from_millis(50)).await;

        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: serde_json::json!(99),
            method: "no.such.method".to_owned(),
            params: None,
        };
        let resp = call_unix(&socket, &req).await.unwrap();
        let err = resp.error.expect("unknown method must return error");
        assert_eq!(err.code, crate::ipc::methods::error_codes::METHOD_NOT_FOUND);

        cancel.cancel();
        let _ = task.await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn socket_has_mode_0600() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::tempdir().unwrap();
        let socket = tmp.path().join("control.sock");

        let ctx = make_ctx();
        let cancel = CancellationToken::new();
        let cancel_clone = cancel.clone();
        let socket_clone = socket.clone();
        let task = tokio::spawn(async move {
            serve(socket_clone, ctx, cancel_clone).await.unwrap();
        });
        tokio::time::sleep(Duration::from_millis(50)).await;

        let perms = std::fs::metadata(&socket).unwrap().permissions();
        assert_eq!(perms.mode() & 0o777, 0o600);

        cancel.cancel();
        let _ = task.await;
    }
}

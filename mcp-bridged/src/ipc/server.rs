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

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use tokio::sync::RwLock;

    use super::*;
    use crate::ipc::methods::method_names;
    use crate::ipc::wire::JsonRpcRequest;
    use crate::registry::Registry;

    fn make_ctx() -> Context {
        Context {
            start: Instant::now(),
            registry: Arc::new(RwLock::new(Registry::new())),
            pair_endpoint_addr: "127.0.0.1:8765".parse().unwrap(),
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
        assert_eq!(result["pair_endpoint"], "127.0.0.1:8765");
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

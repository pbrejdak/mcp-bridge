//! UDS-based IPC server. Unix only at Phase 1; Windows named-pipe
//! support lands in a follow-up commit.

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
/// On Unix: binds a `UnixListener` at `socket_path`, accepts connections,
/// dispatches JSON-RPC requests against `ctx`.
///
/// On Windows: logs that IPC is unavailable and returns immediately. The
/// named-pipe implementation lands in a follow-up commit.
pub async fn serve(
    socket_path: PathBuf,
    ctx: Context,
    cancel: CancellationToken,
) -> Result<(), IpcError> {
    #[cfg(unix)]
    {
        serve_unix(socket_path, ctx, cancel).await
    }
    #[cfg(not(unix))]
    {
        let _ = (socket_path, ctx, cancel);
        warn!("IPC server is not yet implemented on this platform");
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

/// Connect to the IPC socket, send one JSON-RPC request, read one
/// response, and return it. Convenience for the CLI side.
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

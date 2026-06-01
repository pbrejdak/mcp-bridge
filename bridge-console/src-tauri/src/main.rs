//! MCP Bridge Console — Tauri shell. Renders a Svelte frontend that
//! talks to the running `mcp-bridged` daemon over the same UDS / named-
//! pipe that the CLI uses.
//!
//! Single command for v0.1: [`daemon_call`] — a thin wrapper around
//! [`mcp_bridged::ipc::call_local`] that the renderer invokes with a
//! JSON-RPC method name and params. Method-specific commands (e.g.
//! `list_servers`, `start_pair`) land as the tray + pair windows do.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;

use mcp_bridged::config::Config;
use mcp_bridged::ipc;
use serde::{Deserialize, Serialize};

/// Output of [`daemon_call`]. Mirrors the JSON-RPC response envelope
/// but flattened so the renderer doesn't have to know about
/// `result`/`error`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum DaemonCallResult {
    /// JSON-RPC success — the daemon's `result` field as a JSON value.
    Ok { result: serde_json::Value },
    /// JSON-RPC error — code + message from the daemon.
    DaemonError { code: i32, message: String },
    /// We couldn't reach the daemon at all (socket missing, IPC
    /// framing error, etc.).
    Transport { message: String },
}

/// Invoke one JSON-RPC method against the running daemon and return
/// its response (or a structured error). Renderer-side TS sees this
/// as `invoke<DaemonCallResult>('daemon_call', { method, params })`.
#[tauri::command]
async fn daemon_call(
    method: String,
    params: Option<serde_json::Value>,
) -> Result<DaemonCallResult, String> {
    let socket = match Config::defaults() {
        Ok(c) => c.ipc_socket_path(),
        Err(e) => {
            return Ok(DaemonCallResult::Transport {
                message: format!("could not resolve IPC socket path: {e}"),
            });
        }
    };

    let request = ipc::JsonRpcRequest {
        jsonrpc: "2.0".to_owned(),
        id: serde_json::json!(uuid_like_id()),
        method,
        params,
    };

    let socket: PathBuf = socket;
    match ipc::call_local(&socket, &request).await {
        Ok(resp) => {
            if let Some(err) = resp.error {
                Ok(DaemonCallResult::DaemonError {
                    code: err.code,
                    message: err.message,
                })
            } else {
                Ok(DaemonCallResult::Ok {
                    result: resp.result.unwrap_or(serde_json::Value::Null),
                })
            }
        }
        Err(e) => Ok(DaemonCallResult::Transport {
            message: format!("{e}"),
        }),
    }
}

/// Cheap unique id for the JSON-RPC `id` field. Tauri commands don't
/// share state by default; using a counter would need a mutex shared
/// across commands. PID + nanosecond clock is good enough for
/// observability — the daemon doesn't enforce id uniqueness.
fn uuid_like_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("console-{}-{nanos}", std::process::id())
}

fn main() {
    tauri::Builder::default()
        // Single-instance: second invocation hands its argv (which may
        // include an mcp-bridge:// deep-link URL) to the existing
        // instance, which focuses the Console window and emits a
        // `deep-link://new-url` event the renderer subscribes to.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            use tauri::Manager;
            if let Some(window) = app.get_webview_window("console") {
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_deep_link::init())
        .invoke_handler(tauri::generate_handler![daemon_call])
        .run(tauri::generate_context!())
        .expect("Tauri console failed to start");
}

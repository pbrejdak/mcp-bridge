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
use tauri::Emitter;
use tauri::Manager;
use tauri::WindowEvent;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

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

/// Apply the platform-native backdrop effect to the Console window —
/// Sidebar vibrancy on macOS, Mica on Windows 11. Errors are warned-
/// to-log and ignored (the window still works, just without the
/// effect — common on Linux and on older Windows builds).
///
/// The visual becomes visible only when the webview content has a
/// transparent or translucent background. Today the body has a solid
/// fill, so this call is plumbing — CSS updates to expose the
/// effect land in a follow-up.
fn apply_window_effects(window: &tauri::WebviewWindow) {
    #[cfg(target_os = "macos")]
    {
        use window_vibrancy::{NSVisualEffectMaterial, apply_vibrancy};
        if let Err(e) = apply_vibrancy(window, NSVisualEffectMaterial::Sidebar, None, None) {
            tracing::warn!(error = ?e, "could not apply macOS vibrancy");
        }
    }
    #[cfg(target_os = "windows")]
    {
        use window_vibrancy::apply_mica;
        if let Err(e) = apply_mica(window, None) {
            tracing::warn!(error = ?e, "could not apply Windows Mica");
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        // No native backdrop effect on Linux (the Wayland/X compositor
        // catalogue varies too much for a single API). The window is
        // still chrome-decorated and looks fine without the effect.
        let _ = window;
    }
}

/// Bring the Console window to the foreground; create it visible if
/// the user previously hid it via the close button.
fn show_console(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("console") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

/// Build the system-tray icon + menu. Right-click → menu; left-click →
/// show/focus the Console window. The "Pair new server" item emits a
/// `tray-action` event with payload `"pair"` that the renderer
/// listens for and switches into the pair view.
fn install_tray(app: &tauri::AppHandle) -> tauri::Result<()> {
    let open_item = MenuItem::with_id(app, "open", "Open Console", true, None::<&str>)?;
    let pair_item = MenuItem::with_id(app, "pair", "Pair new server", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit MCP Bridge", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open_item, &pair_item, &sep, &quit_item])?;

    let icon = app
        .default_window_icon()
        .ok_or_else(|| tauri::Error::AssetNotFound("default window icon".to_owned()))?
        .clone();

    TrayIconBuilder::new()
        .icon(icon)
        .menu(&menu)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show_console(app),
            "pair" => {
                show_console(app);
                let _ = app.emit("tray-action", "pair");
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_console(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

fn main() {
    tauri::Builder::default()
        // Single-instance: second invocation hands its argv (which may
        // include an mcp-bridge:// deep-link URL) to the existing
        // instance, which focuses the Console window and emits a
        // `deep-link://new-url` event the renderer subscribes to.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            show_console(app);
        }))
        .plugin(tauri_plugin_deep_link::init())
        .setup(|app| {
            install_tray(app.handle())?;

            // Keep the tray alive when the user closes the Console
            // window — hide the window instead of letting Tauri
            // tear it down (which on some platforms also exits the
            // process). Left-clicking the tray icon brings it back.
            if let Some(window) = app.get_webview_window("console") {
                apply_window_effects(&window);
                let w = window.clone();
                window.on_window_event(move |event| {
                    if let WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = w.hide();
                    }
                });
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![daemon_call])
        .run(tauri::generate_context!())
        .expect("Tauri console failed to start");
}

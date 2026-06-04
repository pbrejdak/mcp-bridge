//! Bridge Console IPC surface: JSON-RPC 2.0 over length-prefixed frames.
//! See [`docs/DAEMON.md`] §5.
//!
//! Submodules:
//! - [`wire`] — JSON-RPC envelopes + framing.
//! - [`methods`] — per-method handlers + dispatcher.
//! - [`server`] — UDS listener (Unix); Windows named-pipe is a follow-up.
//!
//! [`docs/DAEMON.md`]: ../../../../docs/DAEMON.md

pub mod methods;
pub mod server;
pub mod wire;

pub use methods::{
    Context, DaemonStatus, DiagnosticsBundleResult, IdentityInfo, IdentityRotateResult,
    LogRecentParams, LogRecentResult, ServerListEntry, UpdateCheckResult, dispatch, method_names,
};
#[cfg(unix)]
pub use server::call_unix;
pub use server::{IpcError, LocalStream, call_local, open_local, serve};
pub use wire::{
    FrameError, JsonRpcError, JsonRpcNotification, JsonRpcRequest, JsonRpcResponse,
    MAX_FRAME_LEN, read_frame, write_frame,
};

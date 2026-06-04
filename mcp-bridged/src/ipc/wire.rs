//! JSON-RPC 2.0 envelope + length-prefix framing for the IPC surface.
//!
//! Frame format per [`docs/DAEMON.md`](../../../../docs/DAEMON.md) §5:
//! `<4-byte big-endian length><JSON bytes>`. Length-prefix bounds the
//! parser memory and removes any need to scan for delimiters.

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Maximum size of a single framed message (bytes). 64 KiB is far above
/// anything the daemon's IPC methods produce; rejecting larger frames
/// keeps a malformed sender from forcing unbounded allocation.
pub const MAX_FRAME_LEN: usize = 64 * 1024;

/// JSON-RPC 2.0 request envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: serde_json::Value,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

/// JSON-RPC 2.0 notification envelope — a request without an `id`,
/// flowing server → client. Used by [`log.subscribe`](super::methods)
/// to push events as they arrive without a paired request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcNotification {
    pub jsonrpc: String,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

impl JsonRpcNotification {
    #[must_use]
    pub fn new(method: impl Into<String>, params: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0".to_owned(),
            method: method.into(),
            params: Some(params),
        }
    }
}

/// JSON-RPC 2.0 successful response envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

/// JSON-RPC 2.0 error object.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl JsonRpcResponse {
    /// Build a successful response carrying `result`.
    #[must_use]
    pub fn success(id: serde_json::Value, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0".to_owned(),
            id,
            result: Some(result),
            error: None,
        }
    }

    /// Build an error response.
    #[must_use]
    pub fn error(id: serde_json::Value, code: i32, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0".to_owned(),
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.into(),
                data: None,
            }),
        }
    }
}

/// Read one length-prefixed frame from `r` and parse it as JSON.
pub async fn read_frame<R: AsyncReadExt + Unpin, T: for<'de> Deserialize<'de>>(
    r: &mut R,
) -> Result<T, FrameError> {
    let mut len_buf = [0u8; 4];
    r.read_exact(&mut len_buf).await.map_err(FrameError::Io)?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > MAX_FRAME_LEN {
        return Err(FrameError::FrameTooLarge { len });
    }
    let mut body = vec![0u8; len];
    r.read_exact(&mut body).await.map_err(FrameError::Io)?;
    serde_json::from_slice(&body).map_err(FrameError::Parse)
}

/// Encode `value` as JSON and write it as a length-prefixed frame.
pub async fn write_frame<W: AsyncWriteExt + Unpin, T: Serialize>(
    w: &mut W,
    value: &T,
) -> Result<(), FrameError> {
    let body = serde_json::to_vec(value).map_err(FrameError::Parse)?;
    if body.len() > MAX_FRAME_LEN {
        return Err(FrameError::FrameTooLarge { len: body.len() });
    }
    let len =
        u32::try_from(body.len()).expect("body length fits in u32 (bounded by MAX_FRAME_LEN)");
    w.write_all(&len.to_be_bytes())
        .await
        .map_err(FrameError::Io)?;
    w.write_all(&body).await.map_err(FrameError::Io)?;
    w.flush().await.map_err(FrameError::Io)?;
    Ok(())
}

/// Failure modes for [`read_frame`] / [`write_frame`].
#[derive(Debug, Error)]
pub enum FrameError {
    #[error("IPC I/O error: {0}")]
    Io(#[source] std::io::Error),
    #[error("JSON parse error: {0}")]
    Parse(#[source] serde_json::Error),
    #[error("frame is {len} bytes; the maximum is {MAX_FRAME_LEN}")]
    FrameTooLarge { len: usize },
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncSeekExt;

    #[tokio::test]
    async fn round_trip_request_and_response() {
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: serde_json::json!(7),
            method: "daemon.status".to_owned(),
            params: None,
        };

        let mut buf: Vec<u8> = Vec::new();
        write_frame(&mut buf, &req).await.unwrap();

        let mut cursor = std::io::Cursor::new(buf);
        let parsed: JsonRpcRequest = read_frame(&mut cursor).await.unwrap();
        assert_eq!(parsed.method, "daemon.status");
        assert_eq!(parsed.id, serde_json::json!(7));
    }

    #[tokio::test]
    async fn read_rejects_oversized_frame() {
        let mut buf: Vec<u8> = Vec::new();
        let oversize_len = u32::try_from(MAX_FRAME_LEN).expect("MAX_FRAME_LEN fits in u32") + 1;
        buf.extend_from_slice(&oversize_len.to_be_bytes());
        // We don't need to actually supply the body; the length check
        // fires first.
        let mut cursor = std::io::Cursor::new(buf);
        let result: Result<JsonRpcRequest, _> = read_frame(&mut cursor).await;
        assert!(matches!(result, Err(FrameError::FrameTooLarge { .. })));
    }

    #[tokio::test]
    async fn success_response_serializes_with_result_field() {
        let resp = JsonRpcResponse::success(serde_json::json!(1), serde_json::json!({"ok": true}));
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"result\":"));
        assert!(!json.contains("\"error\":"));
    }

    #[tokio::test]
    async fn error_response_serializes_with_error_field() {
        let resp = JsonRpcResponse::error(serde_json::json!(1), -32601, "method not found");
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"error\":"));
        assert!(!json.contains("\"result\":"));
    }

    #[tokio::test]
    async fn streaming_works_via_async_seek() {
        // Sanity check: tokio::io::Cursor supports our read/write paths
        // even when the writer doesn't seek between operations.
        let mut buf: Vec<u8> = Vec::new();
        write_frame(
            &mut buf,
            &JsonRpcResponse::success(serde_json::json!(2), serde_json::json!("ok")),
        )
        .await
        .unwrap();
        write_frame(
            &mut buf,
            &JsonRpcResponse::success(serde_json::json!(3), serde_json::json!("ok")),
        )
        .await
        .unwrap();
        let mut cursor = std::io::Cursor::new(buf);
        let _a: JsonRpcResponse = read_frame(&mut cursor).await.unwrap();
        let _b: JsonRpcResponse = read_frame(&mut cursor).await.unwrap();
        cursor.seek(std::io::SeekFrom::End(0)).await.unwrap();
    }
}

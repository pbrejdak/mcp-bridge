//! End-to-end test for the streaming `log.subscribe` IPC method.
//!
//! Boots the IPC server with a fresh `EventRecorder`, opens a client
//! connection, sends `log.subscribe`, then pushes events through the
//! recorder and asserts they arrive as JSON-RPC notification frames.

#![cfg(unix)]

use std::sync::Arc;
use std::time::{Duration, Instant};

use mcp_bridged::adapters::Sentinel;
use mcp_bridged::identity::keystore::install_mock_backend;
use mcp_bridged::identity::{DisplayName, Keypair, Keystore, shared_keypair};
use mcp_bridged::ipc;
use mcp_bridged::observability::EventRecorder;
use mcp_bridged::observability::recorder::LogEvent;
use mcp_bridged::pair::invite_register::InviteRegister;
use mcp_bridged::registry::Registry;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

fn build_context(recorder: EventRecorder) -> ipc::Context {
    install_mock_backend();
    let cancel = CancellationToken::new();
    let invites = InviteRegister::spawn(cancel.clone());
    let svc = format!("logsub-{}", std::process::id());
    let keystore = Arc::new(Keystore::for_service(&svc).expect("keystore"));
    let tmp = tempfile::tempdir().expect("tmpdir");
    let registry_path = tmp.path().join("registry.json");
    // Leak the tempdir for the test lifetime.
    std::mem::forget(tmp);

    ipc::Context {
        start: Instant::now(),
        registry: Arc::new(RwLock::new(Registry::new())),
        pair_endpoint_addr: "10.0.0.5:8765".parse().unwrap(),
        resolver: shared_keypair(Keypair::generate()),
        display_name: DisplayName::new("Test Bridge").unwrap(),
        invites,
        keystore,
        registry_path,
        sentinel: Sentinel::random(),
        adapters: Arc::new(Vec::new()),
        recorder: Some(recorder),
        connector_cache: Arc::new(mcp_bridged::proxy::ConnectorCache::new()),
        rotation_signal: Arc::new(tokio::sync::Notify::new()),
    }
}

#[tokio::test]
async fn log_subscribe_streams_recorded_events() {
    let recorder = EventRecorder::new();
    let recorder_for_ctx = recorder.clone();
    let ctx = build_context(recorder_for_ctx);

    // Spin up the IPC server in a tempdir UDS.
    let tmp = tempfile::tempdir().unwrap();
    let socket = tmp.path().join("control.sock");
    let cancel = CancellationToken::new();
    let server_socket = socket.clone();
    let server_cancel = cancel.clone();
    let server = tokio::spawn(async move {
        ipc::serve(server_socket, ctx, server_cancel).await.unwrap();
    });
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Open the subscription.
    let mut stream = ipc::open_local(&socket).await.unwrap();
    let req = ipc::JsonRpcRequest {
        jsonrpc: "2.0".to_owned(),
        id: serde_json::json!("sub-1"),
        method: ipc::method_names::LOG_SUBSCRIBE.to_owned(),
        params: None,
    };
    ipc::write_frame(&mut stream, &req).await.unwrap();
    let ack: ipc::JsonRpcResponse = ipc::read_frame(&mut stream).await.unwrap();
    assert!(ack.error.is_none(), "subscribe must ack without error");

    // Give the server's broadcast subscription a moment to install.
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Push three events. Each one should land as a notification frame.
    for i in 0..3 {
        recorder.push(LogEvent {
            seq: 0,
            timestamp: 1_700_000_000 + i,
            level: "INFO".to_owned(),
            target: "test".to_owned(),
            message: format!("subscription event {i}"),
        });
    }

    let mut received: Vec<LogEvent> = Vec::new();
    while received.len() < 3 {
        let frame: ipc::JsonRpcNotification =
            tokio::time::timeout(Duration::from_secs(2), ipc::read_frame(&mut stream))
                .await
                .expect("notification within 2s")
                .expect("frame decodes");
        assert_eq!(frame.method, ipc::method_names::LOG_ENTRY_NOTIFICATION);
        let event: LogEvent = serde_json::from_value(frame.params.unwrap()).unwrap();
        received.push(event);
    }

    let messages: Vec<&str> = received.iter().map(|e| e.message.as_str()).collect();
    assert_eq!(
        messages,
        vec![
            "subscription event 0",
            "subscription event 1",
            "subscription event 2",
        ]
    );

    drop(stream);
    cancel.cancel();
    let _ = server.await;
}

#[tokio::test]
async fn log_subscribe_initial_ack_carries_request_id() {
    let recorder = EventRecorder::new();
    let ctx = build_context(recorder);

    let tmp = tempfile::tempdir().unwrap();
    let socket = tmp.path().join("control.sock");
    let cancel = CancellationToken::new();
    let server_socket = socket.clone();
    let server_cancel = cancel.clone();
    let server = tokio::spawn(async move {
        ipc::serve(server_socket, ctx, server_cancel).await.unwrap();
    });
    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut stream = ipc::open_local(&socket).await.unwrap();
    let req = ipc::JsonRpcRequest {
        jsonrpc: "2.0".to_owned(),
        id: serde_json::json!(42),
        method: ipc::method_names::LOG_SUBSCRIBE.to_owned(),
        params: None,
    };
    ipc::write_frame(&mut stream, &req).await.unwrap();
    let ack: ipc::JsonRpcResponse = ipc::read_frame(&mut stream).await.unwrap();
    assert_eq!(ack.id, serde_json::json!(42));
    assert!(ack.result.is_some());

    drop(stream);
    cancel.cancel();
    let _ = server.await;
}

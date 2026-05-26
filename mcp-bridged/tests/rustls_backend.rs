//! Integration test for [`RustlsBackendVerifier`].
//!
//! Spins up a minimal `tokio_rustls` TLS acceptor with the same
//! self-signed cert generator the Resolver's pair endpoint uses,
//! computes the cert's SHA-256, and exercises:
//!
//! - matching fingerprint → `Ok(())`
//! - mismatched fingerprint → `FingerprintMismatch`
//! - unbound port → `Unreachable`

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;

use mcp_bridged::identity::{SelfSignedCert, generate_self_signed_cert};
use mcp_bridged::pair::backend_url::BackendUrl;
use mcp_bridged::pair::backend_verifier::{BackendVerifier, BackendVerifyError};
use mcp_bridged::pair::cert_fingerprint::CertFingerprint;
use mcp_bridged::pair::endpoint::ensure_crypto_provider;
use mcp_bridged::pair::rustls_backend_verifier::RustlsBackendVerifier;
use rustls::ServerConfig;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use sha2::{Digest, Sha256};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;

fn cert_fp(cert: &SelfSignedCert) -> CertFingerprint {
    let digest = Sha256::digest(&cert.cert_der);
    CertFingerprint::from_bytes(digest.into())
}

async fn start_tls_server(cert: &SelfSignedCert) -> SocketAddr {
    ensure_crypto_provider();

    let cert_der = CertificateDer::from(cert.cert_der.clone());
    let key_der = PrivatePkcs8KeyDer::from(cert.key_der.to_vec());
    let server_config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert_der], PrivateKeyDer::Pkcs8(key_der))
        .expect("server config");
    let acceptor = TlsAcceptor::from(Arc::new(server_config));

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local_addr");

    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                // Drive the handshake to completion, then drop the stream.
                let _ = acceptor.accept(stream).await;
            });
        }
    });

    addr
}

fn url_for(addr: SocketAddr) -> BackendUrl {
    let port = addr.port();
    BackendUrl::new(format!("https://127.0.0.1:{port}/")).expect("backend url")
}

#[tokio::test]
async fn matching_fingerprint_succeeds() {
    let cert = generate_self_signed_cert(IpAddr::V4(Ipv4Addr::LOCALHOST)).expect("cert");
    let fp = cert_fp(&cert);
    let addr = start_tls_server(&cert).await;

    let verifier = RustlsBackendVerifier::new();
    let url = url_for(addr);
    verifier
        .verify(&url, &fp)
        .await
        .expect("verify should succeed");
}

#[tokio::test]
async fn mismatched_fingerprint_is_rejected() {
    let cert = generate_self_signed_cert(IpAddr::V4(Ipv4Addr::LOCALHOST)).expect("cert");
    let _real_fp = cert_fp(&cert);
    let addr = start_tls_server(&cert).await;

    let wrong_fp = CertFingerprint::from_bytes([0u8; 32]);
    let verifier = RustlsBackendVerifier::new();
    let url = url_for(addr);
    let err = verifier.verify(&url, &wrong_fp).await.unwrap_err();
    assert!(matches!(
        err,
        BackendVerifyError::FingerprintMismatch { .. }
    ));
}

#[tokio::test]
async fn unreachable_backend_is_rejected() {
    // Bind a TCP listener on an ephemeral port, get its address, then
    // immediately drop it. The port is now almost certainly free; the
    // connect attempt should refuse.
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);

    let verifier = RustlsBackendVerifier::new();
    let url = url_for(addr);
    let fp = CertFingerprint::from_bytes([0u8; 32]);
    let err = verifier.verify(&url, &fp).await.unwrap_err();
    assert!(matches!(err, BackendVerifyError::Unreachable { .. }));
}

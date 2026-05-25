//! Rule 6 — verify that the leaf certificate the Origin backend presents
//! over TLS matches the fingerprint claimed in the pair payload.
//!
//! Per [`docs/SPEC.md`](../../../../docs/SPEC.md) §4.5 (Direction-B, rule 6):
//! "`backend.fp` matches the leaf certificate the Origin actually presents
//! on a TLS handshake to `backend.url` initiated within the pairing flow."
//!
//! This module defines the trait and ships two stub implementations for
//! testing. The real rustls-backed implementation lives elsewhere — the
//! trait separation lets the HTTP listener and `accept_direction_b` stay
//! free of TLS dependencies in tests and in alternate-transport setups.

use async_trait::async_trait;
use thiserror::Error;

use super::backend_url::BackendUrl;
use super::cert_fingerprint::CertFingerprint;

/// Anything that can establish "the leaf cert at this URL matches this
/// fingerprint."
///
/// Implementations MUST be `Send + Sync` so the HTTP listener can share
/// one verifier across request-handling tasks.
///
/// Object-safe via `#[async_trait]` so the daemon can hold an
/// `Arc<dyn BackendVerifier>` and swap implementations at runtime
/// (real rustls in production, stubs in tests).
#[async_trait]
pub trait BackendVerifier: Send + Sync {
    /// Open a TLS handshake against `url`, capture the leaf certificate,
    /// and compare its SHA-256 fingerprint to `expected_fp`. Returns
    /// `Ok(())` only when the two match exactly.
    async fn verify(
        &self,
        url: &BackendUrl,
        expected_fp: &CertFingerprint,
    ) -> Result<(), BackendVerifyError>;
}

/// Failure modes that any [`BackendVerifier`] implementation can return.
#[derive(Debug, Error)]
pub enum BackendVerifyError {
    #[error("backend at {url} is unreachable: {reason}")]
    Unreachable { url: String, reason: String },
    #[error("backend at {url} presented a TLS cert that does not match the pinned fingerprint")]
    FingerprintMismatch { url: String },
    #[error("backend at {url} did not present any leaf certificate")]
    MissingLeafCert { url: String },
}

/// Test stub that accepts every backend without doing any network work.
///
/// Use in unit tests that exercise the orchestration logic, not the TLS
/// fingerprint matching itself.
#[derive(Debug, Default, Clone, Copy)]
pub struct AlwaysAccept;

#[async_trait]
impl BackendVerifier for AlwaysAccept {
    async fn verify(
        &self,
        _url: &BackendUrl,
        _expected_fp: &CertFingerprint,
    ) -> Result<(), BackendVerifyError> {
        Ok(())
    }
}

/// Test stub that always returns a fingerprint-mismatch error.
#[derive(Debug, Default, Clone, Copy)]
pub struct AlwaysFingerprintMismatch;

#[async_trait]
impl BackendVerifier for AlwaysFingerprintMismatch {
    async fn verify(
        &self,
        url: &BackendUrl,
        _expected_fp: &CertFingerprint,
    ) -> Result<(), BackendVerifyError> {
        Err(BackendVerifyError::FingerprintMismatch {
            url: url.as_str().to_owned(),
        })
    }
}

/// Test stub that always reports the backend as unreachable.
#[derive(Debug, Default, Clone, Copy)]
pub struct AlwaysUnreachable;

#[async_trait]
impl BackendVerifier for AlwaysUnreachable {
    async fn verify(
        &self,
        url: &BackendUrl,
        _expected_fp: &CertFingerprint,
    ) -> Result<(), BackendVerifyError> {
        Err(BackendVerifyError::Unreachable {
            url: url.as_str().to_owned(),
            reason: "stub".to_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn always_accept_returns_ok() {
        let v = AlwaysAccept;
        let url = BackendUrl::new("https://10.0.0.42:54321/").unwrap();
        let fp = CertFingerprint::from_bytes([0xab; 32]);
        v.verify(&url, &fp).await.unwrap();
    }

    #[tokio::test]
    async fn always_fingerprint_mismatch_returns_mismatch() {
        let v = AlwaysFingerprintMismatch;
        let url = BackendUrl::new("https://10.0.0.42:54321/").unwrap();
        let fp = CertFingerprint::from_bytes([0xab; 32]);
        let err = v.verify(&url, &fp).await.unwrap_err();
        assert!(matches!(
            err,
            BackendVerifyError::FingerprintMismatch { .. }
        ));
    }

    #[tokio::test]
    async fn always_unreachable_returns_unreachable() {
        let v = AlwaysUnreachable;
        let url = BackendUrl::new("https://10.0.0.42:54321/").unwrap();
        let fp = CertFingerprint::from_bytes([0xab; 32]);
        let err = v.verify(&url, &fp).await.unwrap_err();
        assert!(matches!(err, BackendVerifyError::Unreachable { .. }));
    }
}

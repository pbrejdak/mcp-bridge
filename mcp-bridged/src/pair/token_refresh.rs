//! Bearer-token rotation per SPEC §5.7.
//!
//! When an announce arrives with `auth_rotated_at` strictly greater
//! than the Pin's recorded value, the Resolver MUST:
//!
//!   1. Open a control call over the existing pinned backend TLS
//!      connection to fetch the new bearer token.
//!   2. Atomically replace the stored token.
//!   3. Close all pooled Origin Connector connections and re-create
//!      them with the new credential.
//!
//! SPEC §5.7 explicitly leaves the *control-call shape* out of scope —
//! it's "governed by the Origin host application's API." This module
//! defines a trait that captures step 1 so the daemon can be wired
//! against any concrete shape, and ships one default implementation
//! that follows a well-known convention:
//!
//!   GET `<backend_url>/.well-known/mcp-bridge/refresh-token`
//!   Authorization: Bearer <current_token>
//!   →  200 with JSON body `{"token": "<new bearer>"}`
//!
//! Origin host applications that want auth rotation to work
//! out-of-the-box implement this endpoint. Those that don't keep
//! today's behaviour — the announce is accepted, the new
//! `auth_rotated_at` is recorded in the registry, but every subsequent
//! request still uses the old bearer (until the operator manually
//! re-pairs).

use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use reqwest::{Client, Method};
use rustls::ClientConfig;
use serde::Deserialize;
use std::sync::Arc;
use thiserror::Error;

use crate::pair::backend_url::BackendUrl;
use crate::pair::bearer_token::{BearerToken, BearerTokenError};
use crate::pair::cert_fingerprint::CertFingerprint;
use crate::proxy::PinningVerifier;

/// Conventional path under the pinned backend that returns a fresh
/// bearer token to a caller authenticating with the current one.
pub const WELL_KNOWN_REFRESH_PATH: &str = "/.well-known/mcp-bridge/refresh-token";

/// Source for a freshly-rotated bearer token. Implementations are
/// stateless and may be called concurrently from multiple announce
/// handlers — synchronisation around the keystore write happens at the
/// caller in [`crate::pair::token_refresh::run_rotation`].
#[async_trait]
pub trait BearerTokenRefresher: Send + Sync {
    /// Fetch a new bearer token from the backend pinned by
    /// `(backend_url, backend_fp)`. The current bearer is supplied so
    /// the implementation can authenticate the refresh call.
    async fn refresh(
        &self,
        backend_url: &BackendUrl,
        backend_fp: CertFingerprint,
        current: &BearerToken,
    ) -> Result<BearerToken, RefreshError>;
}

/// Default [`BearerTokenRefresher`] — HTTPS GET to the well-known path
/// using the same `rustls` + `PinningVerifier` pipeline the proxy uses
/// for forwarded requests, so the same cert pin holds and the same
/// libraries are exercised.
#[allow(missing_debug_implementations)] // reqwest::Client is not Debug.
pub struct WellKnownPathRefresher;

#[async_trait]
impl BearerTokenRefresher for WellKnownPathRefresher {
    async fn refresh(
        &self,
        backend_url: &BackendUrl,
        backend_fp: CertFingerprint,
        current: &BearerToken,
    ) -> Result<BearerToken, RefreshError> {
        crate::pair::endpoint::ensure_crypto_provider();

        let tls = ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(PinningVerifier::new(backend_fp)))
            .with_no_client_auth();
        let client = Client::builder()
            .use_preconfigured_tls(tls)
            .build()
            .map_err(RefreshError::Build)?;

        let url = format!(
            "{}{WELL_KNOWN_REFRESH_PATH}",
            backend_url.as_str().trim_end_matches('/')
        );
        let mut headers = HeaderMap::new();
        let auth = format!("Bearer {}", current.expose());
        headers.insert(
            HeaderName::from_static("authorization"),
            HeaderValue::from_str(&auth).map_err(|_| RefreshError::InvalidBearer)?,
        );

        let resp = client
            .request(Method::GET, &url)
            .headers(headers)
            .send()
            .await
            .map_err(RefreshError::Send)?;
        if !resp.status().is_success() {
            return Err(RefreshError::Status {
                code: resp.status().as_u16(),
            });
        }

        // `reqwest`'s `Response::json` requires the `json` feature.
        // We keep default-features off (per CLAUDE.md §10) so decode
        // the body via serde_json directly.
        let bytes = resp.bytes().await.map_err(RefreshError::Decode)?;
        let body: RefreshResponseBody =
            serde_json::from_slice(&bytes).map_err(RefreshError::DecodeJson)?;
        BearerToken::new(&body.token).map_err(RefreshError::InvalidNewBearer)
    }
}

/// Shape the well-known endpoint MUST return on success.
#[derive(Debug, Deserialize)]
struct RefreshResponseBody {
    token: String,
}

/// Failure modes for [`BearerTokenRefresher::refresh`].
#[derive(Debug, Error)]
pub enum RefreshError {
    #[error("could not build refresh HTTPS client: {0}")]
    Build(#[source] reqwest::Error),
    #[error("current bearer token contains characters that are not valid in an HTTP header")]
    InvalidBearer,
    #[error("refresh GET to the backend failed: {0}")]
    Send(#[source] reqwest::Error),
    #[error("refresh endpoint returned status {code}")]
    Status { code: u16 },
    #[error("could not read refresh response body: {0}")]
    Decode(#[source] reqwest::Error),
    #[error("could not parse refresh response JSON: {0}")]
    DecodeJson(#[source] serde_json::Error),
    #[error("backend returned a bearer that failed validation: {0}")]
    InvalidNewBearer(#[from] BearerTokenError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pair::backend_url::BackendUrl;

    /// Hand-coded happy-path encoding/decoding check. The full live
    /// HTTPS path lives in `tests/token_refresh.rs` since it needs an
    /// axum test server.
    #[test]
    fn well_known_path_is_correct() {
        // Sanity: SPEC text + comment + endpoint constant agree.
        assert_eq!(WELL_KNOWN_REFRESH_PATH, "/.well-known/mcp-bridge/refresh-token");
    }

    #[test]
    fn refresh_response_body_round_trips() {
        let json = r#"{"token":"abc-123"}"#;
        let parsed: RefreshResponseBody = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.token, "abc-123");
    }

    #[test]
    fn refresh_url_is_built_from_backend_root() {
        // The trim_end_matches('/') logic ensures a trailing slash on
        // backend_url doesn't produce a double slash before the well-
        // known path.
        let with_slash = BackendUrl::new("https://10.0.0.42:54321/").unwrap();
        let composed = format!(
            "{}{}",
            with_slash.as_str().trim_end_matches('/'),
            WELL_KNOWN_REFRESH_PATH
        );
        assert_eq!(
            composed,
            "https://10.0.0.42:54321/.well-known/mcp-bridge/refresh-token"
        );

        let without_slash = BackendUrl::new("https://10.0.0.42:54321").unwrap();
        let composed = format!(
            "{}{}",
            without_slash.as_str().trim_end_matches('/'),
            WELL_KNOWN_REFRESH_PATH
        );
        assert_eq!(
            composed,
            "https://10.0.0.42:54321/.well-known/mcp-bridge/refresh-token"
        );
    }
}

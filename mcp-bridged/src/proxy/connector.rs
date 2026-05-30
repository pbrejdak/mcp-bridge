//! Outbound HTTPS client to one Origin's backend, pinned to its
//! certificate SHA-256 and authenticated with its bearer token.
//!
//! Per [`docs/ARCHITECTURE.md`](../../../../docs/ARCHITECTURE.md) §3.1:
//! one [`OriginConnector`] per registered Server Pin. The request body
//! is buffered up to a small cap (MCP requests are JSON-shaped and
//! small); the response body is streamed so SSE `text/event-stream`
//! responses pass through without blocking on full-body completion.

use std::pin::Pin;
use std::sync::Arc;

use bytes::Bytes;
use futures_core::Stream;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use reqwest::{Client, Method};
use rustls::ClientConfig;
use thiserror::Error;

use crate::pair::backend_url::BackendUrl;
use crate::pair::bearer_token::BearerToken;
use crate::pair::cert_fingerprint::CertFingerprint;

use super::pinning_verifier::PinningVerifier;

/// Stream of body chunks coming back from the pinned backend. One
/// boxed item per chunk reqwest hands us — SSE messages, ordinary
/// body bytes, or the trailing transfer-encoding fragment.
pub type ResponseBodyStream =
    Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static>>;

/// Forward-request shape — the listener's handler boils its incoming
/// `axum::Request` down to this before calling [`OriginConnector::forward`].
#[derive(Debug)]
pub struct ForwardRequest {
    pub method: Method,
    /// The path-and-query portion to append to the backend URL. Must
    /// include the leading `/`.
    pub path_and_query: String,
    pub headers: HeaderMap,
    pub body: Vec<u8>,
}

/// Forward-response shape — what the listener's handler turns back into
/// an `axum::Response`. The body is a stream so SSE pass-through
/// doesn't have to wait for the backend to close the connection.
#[allow(missing_debug_implementations)] // Stream trait object is not Debug.
pub struct ForwardResponse {
    pub status: reqwest::StatusCode,
    pub headers: HeaderMap,
    pub body: ResponseBodyStream,
}

/// Headers we strip from the incoming Consumer request before forwarding.
/// These are either rewritten by us (`host`, `authorization`) or are
/// hop-by-hop per RFC 7230 §6.1.
const STRIP_HEADERS: &[&str] = &[
    "host",
    "authorization",
    "connection",
    "proxy-connection",
    "transfer-encoding",
    "keep-alive",
    "te",
    "trailer",
    "upgrade",
];

/// One outbound HTTPS client per Server Pin.
///
/// Holds a `reqwest::Client` whose rustls config replaces the chain
/// verifier with [`PinningVerifier`]. The bearer token is in
/// [`BearerToken`] (zeroizing); requests rewrite the `Authorization`
/// header to `Bearer <token>` before sending.
#[allow(missing_debug_implementations)] // reqwest::Client is not Debug.
pub struct OriginConnector {
    client: Client,
    backend_url: BackendUrl,
    bearer_token: BearerToken,
}

impl OriginConnector {
    /// Build a connector pinned to `backend_fp` and authenticated with
    /// `bearer_token`. Use one connector per `(Pin, lifetime-of-token)`.
    ///
    /// Idempotently installs the rustls ring crypto provider as the
    /// process default, mirroring [`crate::pair::endpoint`].
    pub fn new(
        backend_url: BackendUrl,
        backend_fp: CertFingerprint,
        bearer_token: BearerToken,
    ) -> Result<Self, ConnectorError> {
        crate::pair::endpoint::ensure_crypto_provider();

        let tls = ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(PinningVerifier::new(backend_fp)))
            .with_no_client_auth();
        let client = Client::builder()
            .use_preconfigured_tls(tls)
            .build()
            .map_err(ConnectorError::Build)?;
        Ok(Self {
            client,
            backend_url,
            bearer_token,
        })
    }

    /// Forward `req` to the pinned backend and return the response.
    pub async fn forward(&self, req: ForwardRequest) -> Result<ForwardResponse, ConnectorError> {
        let url = compose_url(self.backend_url.as_str(), &req.path_and_query)?;

        let mut builder = self.client.request(req.method.clone(), url);

        // Copy the incoming headers minus the ones we own or are
        // hop-by-hop per RFC 7230.
        for (name, value) in &req.headers {
            let name_str = name.as_str();
            if STRIP_HEADERS
                .iter()
                .any(|stripped| stripped.eq_ignore_ascii_case(name_str))
            {
                continue;
            }
            builder = builder.header(name, value);
        }

        // Authorization: Bearer <token>. The exposed string never
        // leaves this scope; reqwest copies it into the request frame
        // and the local variable is dropped at the end of the call.
        let auth = format!("Bearer {}", self.bearer_token.expose());
        builder = builder.header(
            HeaderName::from_static("authorization"),
            HeaderValue::from_str(&auth).map_err(|_| ConnectorError::InvalidBearer)?,
        );

        if !req.body.is_empty() {
            builder = builder.body(req.body);
        }

        let resp = builder.send().await.map_err(ConnectorError::Send)?;
        let status = resp.status();
        let headers = resp.headers().clone();
        let body: ResponseBodyStream = Box::pin(resp.bytes_stream());

        Ok(ForwardResponse {
            status,
            headers,
            body,
        })
    }
}

/// Append `path_and_query` (must start with `/`) to the backend URL,
/// preserving the backend's own base path.
fn compose_url(backend: &str, path_and_query: &str) -> Result<String, ConnectorError> {
    if !path_and_query.starts_with('/') {
        return Err(ConnectorError::BadPath {
            got: path_and_query.to_owned(),
        });
    }
    // Strip any trailing slash on the backend so we never produce //.
    let base = backend.trim_end_matches('/');
    Ok(format!("{base}{path_and_query}"))
}

/// Failure modes for [`OriginConnector`].
#[derive(Debug, Error)]
pub enum ConnectorError {
    #[error("could not build HTTPS client: {0}")]
    Build(#[source] reqwest::Error),
    #[error("bearer token contains characters that are not valid in an HTTP header")]
    InvalidBearer,
    #[error("forward path must start with '/'; got `{got}`")]
    BadPath { got: String },
    #[error("request to backend failed: {0}")]
    Send(#[source] reqwest::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compose_url_appends_path() {
        let got = compose_url("https://10.0.0.42:54321/", "/tools/call").unwrap();
        assert_eq!(got, "https://10.0.0.42:54321/tools/call");
    }

    #[test]
    fn compose_url_handles_backend_without_trailing_slash() {
        let got = compose_url("https://10.0.0.42:54321", "/tools/call").unwrap();
        assert_eq!(got, "https://10.0.0.42:54321/tools/call");
    }

    #[test]
    fn compose_url_preserves_backend_base_path() {
        let got = compose_url("https://10.0.0.42:54321/mcp", "/tools/call").unwrap();
        assert_eq!(got, "https://10.0.0.42:54321/mcp/tools/call");
    }

    #[test]
    fn compose_url_rejects_missing_leading_slash() {
        let r = compose_url("https://10.0.0.42:54321/", "tools/call");
        assert!(matches!(r, Err(ConnectorError::BadPath { .. })));
    }
}

//! Real rustls-backed [`BackendVerifier`] — opens a TLS connection to the
//! Origin's backend URL, captures the leaf certificate from the
//! handshake, computes its SHA-256, and compares to the fingerprint the
//! pair payload claims.
//!
//! Per [`docs/SPEC.md`](../../../../docs/SPEC.md) §4.5 rule 6.
//!
//! Cert-chain validation is intentionally bypassed: an Origin self-signs
//! its TLS cert, and the pair payload's `sig` field (verified by
//! [`super::PairPayload::validate_direction_b`]) is what authorizes the
//! Resolver to trust the fingerprint claim. The TLS handshake exists
//! purely to extract the cert the Origin actually serves.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::str::FromStr;
use std::sync::Arc;

use async_trait::async_trait;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, Error as RustlsError, SignatureScheme};
use sha2::{Digest, Sha256};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;

use super::backend_url::BackendUrl;
use super::backend_verifier::{BackendVerifier, BackendVerifyError};
use super::cert_fingerprint::CertFingerprint;
use super::endpoint::ensure_crypto_provider;

/// Real backend-fingerprint verifier. Holds a reusable rustls
/// `ClientConfig` so repeated checks don't rebuild it.
#[derive(Debug, Clone)]
pub struct RustlsBackendVerifier {
    client_config: Arc<ClientConfig>,
}

impl RustlsBackendVerifier {
    /// Construct a new verifier. Idempotently installs the rustls ring
    /// crypto provider as the process default before building the
    /// client config.
    #[must_use]
    pub fn new() -> Self {
        ensure_crypto_provider();
        let config = ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(NoChainVerifier))
            .with_no_client_auth();
        Self {
            client_config: Arc::new(config),
        }
    }
}

impl Default for RustlsBackendVerifier {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl BackendVerifier for RustlsBackendVerifier {
    async fn verify(
        &self,
        url: &BackendUrl,
        expected_fp: &CertFingerprint,
    ) -> Result<(), BackendVerifyError> {
        let url_str = url.as_str().to_owned();

        // Parse host + port.
        let host_str = url.host_str();
        let stripped = host_str.trim_start_matches('[').trim_end_matches(']');
        let server_name: ServerName<'static> = if let Ok(v4) = Ipv4Addr::from_str(stripped) {
            ServerName::IpAddress(IpAddr::V4(v4).into())
        } else if let Ok(v6) = Ipv6Addr::from_str(stripped) {
            ServerName::IpAddress(IpAddr::V6(v6).into())
        } else {
            ServerName::try_from(host_str.to_owned()).map_err(|e| {
                BackendVerifyError::Unreachable {
                    url: url_str.clone(),
                    reason: format!("invalid server name: {e}"),
                }
            })?
        };

        // Connect.
        let addr = format!("{stripped}:{}", url.port_or_default());
        let tcp = TcpStream::connect(&addr)
            .await
            .map_err(|e| BackendVerifyError::Unreachable {
                url: url_str.clone(),
                reason: format!("tcp connect: {e}"),
            })?;

        // TLS handshake.
        let connector = TlsConnector::from(self.client_config.clone());
        let tls = connector.connect(server_name, tcp).await.map_err(|e| {
            BackendVerifyError::Unreachable {
                url: url_str.clone(),
                reason: format!("tls handshake: {e}"),
            }
        })?;

        // Pull the leaf cert from the completed handshake.
        let (_, conn) = tls.get_ref();
        let certs =
            conn.peer_certificates()
                .ok_or_else(|| BackendVerifyError::MissingLeafCert {
                    url: url_str.clone(),
                })?;
        let leaf = certs
            .first()
            .ok_or_else(|| BackendVerifyError::MissingLeafCert {
                url: url_str.clone(),
            })?;

        // SHA-256 of the DER encoding, compared to the pinned value.
        let actual = Sha256::digest(leaf.as_ref());
        if actual.as_slice() != expected_fp.as_bytes().as_slice() {
            return Err(BackendVerifyError::FingerprintMismatch { url: url_str });
        }

        Ok(())
    }
}

/// rustls verifier that accepts any cert presented by the server. We do
/// not trust the chain — only the fingerprint, checked separately by the
/// surrounding code. Required because rustls demands SOME verifier; we
/// supply the one that defers the actual trust decision to our own code.
#[derive(Debug)]
struct NoChainVerifier;

impl ServerCertVerifier for NoChainVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, RustlsError> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![
            SignatureScheme::RSA_PKCS1_SHA1,
            SignatureScheme::ECDSA_SHA1_Legacy,
            SignatureScheme::RSA_PKCS1_SHA256,
            SignatureScheme::ECDSA_NISTP256_SHA256,
            SignatureScheme::RSA_PKCS1_SHA384,
            SignatureScheme::ECDSA_NISTP384_SHA384,
            SignatureScheme::RSA_PKCS1_SHA512,
            SignatureScheme::ECDSA_NISTP521_SHA512,
            SignatureScheme::RSA_PSS_SHA256,
            SignatureScheme::RSA_PSS_SHA384,
            SignatureScheme::RSA_PSS_SHA512,
            SignatureScheme::ED25519,
            SignatureScheme::ED448,
        ]
    }
}

//! rustls `ServerCertVerifier` that enforces a SHA-256 fingerprint on
//! the leaf certificate during the handshake.
//!
//! Per [`docs/SPEC.md`](../../../../docs/SPEC.md) §5.8 + §6: the proxy
//! pins each Origin's TLS leaf cert by its SHA-256. Chain validation is
//! intentionally bypassed — the Origin self-signs its cert and the pair
//! payload's `sig` (verified at pair time by
//! [`crate::pair::accept`]) is what authorised the Resolver to trust
//! the fingerprint claim in the first place.

use rustls::DigitallySignedStruct;
use rustls::Error as RustlsError;
use rustls::SignatureScheme;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use sha2::{Digest, Sha256};

use crate::pair::cert_fingerprint::CertFingerprint;

/// rustls verifier that compares the leaf cert's SHA-256 to a pinned
/// fingerprint and rejects anything else.
#[derive(Debug)]
pub struct PinningVerifier {
    expected: CertFingerprint,
}

impl PinningVerifier {
    #[must_use]
    pub fn new(expected: CertFingerprint) -> Self {
        Self { expected }
    }
}

impl ServerCertVerifier for PinningVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, RustlsError> {
        let actual = Sha256::digest(end_entity.as_ref());
        if actual.as_slice() == self.expected.as_bytes().as_slice() {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(RustlsError::General(
                "backend leaf certificate does not match pinned fingerprint".into(),
            ))
        }
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

//! Self-signed TLS certificate for the Resolver's LAN pair endpoint.
//!
//! Per [`docs/SPEC.md`](../../../../docs/SPEC.md) §4.2:
//! "The Resolver's TLS certificate MAY be self-signed for this address
//! (the seal authenticates the payload independently of TLS)."
//!
//! The pair endpoint uses HTTPS purely as the transport. Phone clients
//! do not verify the chain — they trust the sealed envelope per SPEC
//! §4.5 rule 7. The certificate exists to make standard HTTPS clients
//! (curl, browsers, the SDK's HTTP layer) speak TLS at all.
//!
//! Key material zeroizes on drop via [`zeroize::Zeroizing`].

use std::net::IpAddr;

use rcgen::{CertificateParams, DistinguishedName, DnType, KeyPair, SanType};
use thiserror::Error;
use zeroize::Zeroizing;

/// A freshly-generated self-signed cert paired with its private key.
pub struct SelfSignedCert {
    /// DER-encoded X.509 certificate. Public, not sensitive.
    pub cert_der: Vec<u8>,
    /// DER-encoded PKCS#8 private key. Zeroized on drop.
    pub key_der: Zeroizing<Vec<u8>>,
}

impl std::fmt::Debug for SelfSignedCert {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SelfSignedCert")
            .field("cert_der", &format_args!("<{} bytes>", self.cert_der.len()))
            .finish_non_exhaustive()
    }
}

/// Generate a fresh self-signed certificate whose only Subject Alternative
/// Name is `ip`. CN = "MCP Bridge Resolver".
pub fn generate_for(ip: IpAddr) -> Result<SelfSignedCert, GenerateError> {
    let mut params = CertificateParams::new(Vec::<String>::new()).map_err(GenerateError::Params)?;
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, "MCP Bridge Resolver");
    params.distinguished_name = dn;
    params.subject_alt_names = vec![SanType::IpAddress(ip)];

    let key_pair = KeyPair::generate().map_err(GenerateError::Key)?;
    let cert = params.self_signed(&key_pair).map_err(GenerateError::Sign)?;

    Ok(SelfSignedCert {
        cert_der: cert.der().to_vec(),
        key_der: Zeroizing::new(key_pair.serialize_der()),
    })
}

/// Failure modes for [`generate_for`].
#[derive(Debug, Error)]
pub enum GenerateError {
    #[error("could not build certificate params: {0}")]
    Params(#[source] rcgen::Error),
    #[error("could not generate key pair: {0}")]
    Key(#[source] rcgen::Error),
    #[error("could not self-sign certificate: {0}")]
    Sign(#[source] rcgen::Error),
}

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, Ipv6Addr};

    use super::*;

    #[test]
    fn generate_for_ipv4_returns_non_empty_cert_and_key() {
        let c = generate_for(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5))).unwrap();
        assert!(!c.cert_der.is_empty(), "cert DER must not be empty");
        assert!(!c.key_der.is_empty(), "key DER must not be empty");
    }

    #[test]
    fn generate_for_ipv6_returns_non_empty_cert_and_key() {
        let c = generate_for(IpAddr::V6(Ipv6Addr::LOCALHOST)).unwrap();
        assert!(!c.cert_der.is_empty());
        assert!(!c.key_der.is_empty());
    }

    #[test]
    fn each_generation_produces_a_distinct_key() {
        let a = generate_for(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5))).unwrap();
        let b = generate_for(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5))).unwrap();
        // Same SAN, different keypair → different cert + different key.
        assert_ne!(&*a.key_der, &*b.key_der);
        assert_ne!(a.cert_der, b.cert_der);
    }

    #[test]
    fn debug_redacts_key_material() {
        let c = generate_for(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5))).unwrap();
        let dbg = format!("{c:?}");
        // The key bytes (as hex or otherwise) must not leak through Debug.
        // Easiest check: the formatted size string should appear; the raw
        // bytes (rendered as numbers) should not flood the output.
        assert!(dbg.contains("cert_der"));
        assert!(!dbg.contains("key_der"));
    }
}

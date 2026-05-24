//! `https://`-only LAN address with IP-literal or `.local`-hostname host.
//!
//! See [`docs/SPEC.md`](../../../../docs/SPEC.md) §4.2:
//! "MUST be a `https://` URL with an IP literal or RFC 6762 `.local`
//! hostname; MUST NOT be a public DNS name."
//!
//! Public material. No zeroize.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;
use url::{Host, Url};

/// A LAN-scoped HTTPS endpoint advertised in a pairing invite.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct LanAddr(Url);

impl LanAddr {
    /// Validate and construct from a URL string.
    pub fn new(s: impl AsRef<str>) -> Result<Self, LanAddrError> {
        let url = Url::parse(s.as_ref()).map_err(LanAddrError::Parse)?;
        if url.scheme() != "https" {
            return Err(LanAddrError::NotHttps {
                got: url.scheme().to_owned(),
            });
        }
        let host = url.host().ok_or(LanAddrError::MissingHost)?;
        if !is_acceptable_host(&host) {
            return Err(LanAddrError::ForbiddenHost {
                host: host.to_string(),
            });
        }
        Ok(Self(url))
    }

    /// Borrow the encoded URL.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

/// Accept only IP literals (v4 or v6) and `.local`-suffixed hostnames per
/// SPEC.md §4.2. Public-DNS names are rejected.
fn is_acceptable_host(host: &Host<&str>) -> bool {
    match host {
        Host::Ipv4(_) | Host::Ipv6(_) => true,
        // RFC 6762 §3: ".local" is the link-local DNS suffix. This is a
        // hostname check, not a file-extension check — clippy's pedantic
        // lint for the latter is a false positive here.
        #[allow(clippy::case_sensitive_file_extension_comparisons)]
        Host::Domain(d) => d.ends_with(".local") && d.len() > ".local".len(),
    }
}

impl fmt::Display for LanAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0.as_str())
    }
}

impl fmt::Debug for LanAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("LanAddr").field(&self.0.as_str()).finish()
    }
}

impl FromStr for LanAddr {
    type Err = LanAddrError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

impl Serialize for LanAddr {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.0.as_str())
    }
}

impl<'de> Deserialize<'de> for LanAddr {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = <&str>::deserialize(deserializer)?;
        Self::new(s).map_err(serde::de::Error::custom)
    }
}

/// Failure modes for [`LanAddr::new`].
#[derive(Debug, Error)]
pub enum LanAddrError {
    #[error("invalid URL: {0}")]
    Parse(#[from] url::ParseError),
    #[error("scheme must be https, got `{got}`")]
    NotHttps { got: String },
    #[error("URL has no host")]
    MissingHost,
    #[error("host `{host}` is not an IP literal or .local hostname")]
    ForbiddenHost { host: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_ipv4_with_port_and_path() {
        let a = LanAddr::new("https://10.0.0.5:8765/pair").unwrap();
        assert_eq!(a.as_str(), "https://10.0.0.5:8765/pair");
    }

    #[test]
    fn accepts_ipv6_literal() {
        let a = LanAddr::new("https://[fe80::1]:8765/pair").unwrap();
        assert_eq!(a.as_str(), "https://[fe80::1]:8765/pair");
    }

    #[test]
    fn accepts_local_hostname() {
        let a = LanAddr::new("https://patryk.local:8765/pair").unwrap();
        assert_eq!(a.as_str(), "https://patryk.local:8765/pair");
    }

    #[test]
    fn rejects_http_scheme() {
        let r = LanAddr::new("http://10.0.0.5:8765/pair");
        assert!(matches!(r, Err(LanAddrError::NotHttps { .. })));
    }

    #[test]
    fn rejects_public_dns_hostname() {
        let r = LanAddr::new("https://example.com/pair");
        assert!(matches!(r, Err(LanAddrError::ForbiddenHost { .. })));
    }

    #[test]
    fn rejects_bare_local_suffix() {
        // Just ".local" without a label in front.
        let r = LanAddr::new("https://.local/pair");
        assert!(matches!(
            r,
            Err(LanAddrError::Parse(_) | LanAddrError::ForbiddenHost { .. })
        ));
    }

    #[test]
    fn rejects_garbage() {
        assert!(matches!(
            LanAddr::new("not a url"),
            Err(LanAddrError::Parse(_))
        ));
    }

    #[test]
    fn serde_round_trip() {
        let a = LanAddr::new("https://10.0.0.5:8765/pair").unwrap();
        let json = serde_json::to_string(&a).unwrap();
        assert_eq!(json, "\"https://10.0.0.5:8765/pair\"");
        let back: LanAddr = serde_json::from_str(&json).unwrap();
        assert_eq!(back, a);
    }

    #[test]
    fn serde_rejects_invalid_on_deserialize() {
        let r: Result<LanAddr, _> = serde_json::from_str("\"https://example.com/pair\"");
        assert!(r.is_err());
    }
}

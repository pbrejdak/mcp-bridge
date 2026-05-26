//! Origin backend HTTPS URL.
//!
//! Per [`docs/SPEC.md`](../../../../docs/SPEC.md) §4.4:
//! "`backend.url` MUST be a `https://` URL. `http://` backends MUST be
//! rejected by the Resolver."
//!
//! Backend URLs are NOT subject to the IP-literal-or-`.local` restriction
//! that applies to `resolver.lan_addr` ([`super::LanAddr`]) — Origins may
//! reasonably present a hostname or any address the Resolver can reach.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;
use url::Url;

/// An Origin backend HTTPS URL.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct BackendUrl(Url);

impl BackendUrl {
    /// Validate and construct from a URL string.
    pub fn new(s: impl AsRef<str>) -> Result<Self, BackendUrlError> {
        let url = Url::parse(s.as_ref()).map_err(BackendUrlError::Parse)?;
        if url.scheme() != "https" {
            return Err(BackendUrlError::NotHttps {
                got: url.scheme().to_owned(),
            });
        }
        if url.host().is_none() {
            return Err(BackendUrlError::MissingHost);
        }
        Ok(Self(url))
    }

    /// Borrow the encoded URL.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// Host portion of the URL (IPv4 literal, IPv6 literal with surrounding
    /// brackets, or a hostname).
    #[must_use]
    pub fn host_str(&self) -> &str {
        self.0.host_str().expect("BackendUrl::new requires a host")
    }

    /// Explicit port from the URL, or 443 (the HTTPS default) if omitted.
    #[must_use]
    pub fn port_or_default(&self) -> u16 {
        self.0.port().unwrap_or(443)
    }
}

impl fmt::Display for BackendUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0.as_str())
    }
}

impl fmt::Debug for BackendUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("BackendUrl").field(&self.0.as_str()).finish()
    }
}

impl FromStr for BackendUrl {
    type Err = BackendUrlError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

impl Serialize for BackendUrl {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.0.as_str())
    }
}

impl<'de> Deserialize<'de> for BackendUrl {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Self::new(&s).map_err(serde::de::Error::custom)
    }
}

/// Failure modes for [`BackendUrl::new`].
#[derive(Debug, Error)]
pub enum BackendUrlError {
    #[error("invalid URL: {0}")]
    Parse(#[from] url::ParseError),
    #[error("scheme must be https, got `{got}`")]
    NotHttps { got: String },
    #[error("URL has no host")]
    MissingHost,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_ip_literal_with_port_and_path() {
        let u = BackendUrl::new("https://10.0.0.42:54321/").unwrap();
        assert_eq!(u.as_str(), "https://10.0.0.42:54321/");
    }

    #[test]
    fn accepts_hostname() {
        // Unlike LanAddr, BackendUrl permits public DNS hostnames.
        let u = BackendUrl::new("https://example.com/").unwrap();
        assert_eq!(u.as_str(), "https://example.com/");
    }

    #[test]
    fn accepts_path() {
        let u = BackendUrl::new("https://10.0.0.42:54321/mcp").unwrap();
        assert_eq!(u.as_str(), "https://10.0.0.42:54321/mcp");
    }

    #[test]
    fn rejects_http_scheme() {
        assert!(matches!(
            BackendUrl::new("http://10.0.0.42:54321/"),
            Err(BackendUrlError::NotHttps { .. })
        ));
    }

    #[test]
    fn rejects_garbage() {
        assert!(matches!(
            BackendUrl::new("not a url"),
            Err(BackendUrlError::Parse(_))
        ));
    }

    #[test]
    fn serde_round_trip() {
        let u = BackendUrl::new("https://10.0.0.42:54321/").unwrap();
        let json = serde_json::to_string(&u).unwrap();
        assert_eq!(json, "\"https://10.0.0.42:54321/\"");
        let back: BackendUrl = serde_json::from_str(&json).unwrap();
        assert_eq!(back, u);
    }

    #[test]
    fn serde_rejects_invalid_on_deserialize() {
        let r: Result<BackendUrl, _> = serde_json::from_str("\"http://10.0.0.42/\"");
        assert!(r.is_err());
    }
}

//! Per-install sentinel UUID used to tag adapter-written config entries.
//!
//! Per [`docs/CONTRIBUTING.md`](../../../../docs/CONTRIBUTING.md) §3.5:
//! every entry an adapter writes into a Consumer's config file carries
//! this UUID under a `_mcp_bridge_managed` key. On revoke/uninstall the
//! adapter reads back the file and removes only entries whose marker
//! matches our sentinel — so we never touch lines the user added by
//! hand or that another tool wrote.
//!
//! Lifetime: generated at first daemon launch, persisted in the OS
//! keychain (alongside the Resolver keypair), stable across restarts.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;
use uuid::Uuid;

/// Newtype around a v4 UUID. JSON shape: standard hyphenated lowercase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Sentinel(Uuid);

impl Sentinel {
    /// Generate a fresh sentinel.
    #[must_use]
    pub fn random() -> Self {
        Self(Uuid::new_v4())
    }

    /// Borrow the inner UUID.
    #[must_use]
    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl fmt::Display for Sentinel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for Sentinel {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s).map_err(ParseError::InvalidUuid)?))
    }
}

impl Serialize for Sentinel {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for Sentinel {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

/// Failure modes for [`Sentinel::from_str`].
#[derive(Debug, Error)]
pub enum ParseError {
    #[error("invalid UUID: {0}")]
    InvalidUuid(uuid::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn random_produces_distinct_sentinels() {
        let a = Sentinel::random();
        let b = Sentinel::random();
        assert_ne!(a, b);
    }

    #[test]
    fn display_round_trips_through_from_str() {
        let s = Sentinel::random();
        let back: Sentinel = s.to_string().parse().unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn serde_round_trips_as_hyphenated_string() {
        let s = Sentinel::random();
        let json = serde_json::to_string(&s).unwrap();
        // 36 hyphenated chars + 2 quotes
        assert_eq!(json.len(), 38);
        let back: Sentinel = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn from_str_rejects_invalid() {
        assert!(matches!(
            "not-a-uuid".parse::<Sentinel>(),
            Err(ParseError::InvalidUuid(_))
        ));
    }
}

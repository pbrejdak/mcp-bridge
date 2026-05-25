//! Per-Origin bearer token carried in the pair payload.
//!
//! Per [`docs/SPEC.md`](../../../../docs/SPEC.md) §4.4:
//! "`auth.value` — token string, 1-2048 chars."
//!
//! Secret material — wrapped in [`secrecy::SecretString`] so callers must
//! `expose_secret()` explicitly at every use site, and so the bytes are
//! zeroized on drop. `Debug` redacts; `Serialize` emits the cleartext
//! token because that is exactly what the wire protocol requires inside
//! the sealed pair-payload envelope — the redaction layer in
//! `observability/` is what prevents this from reaching logs.

use std::fmt;

use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;

/// Minimum length per SPEC.md §4.4.
pub const MIN_LEN: usize = 1;

/// Maximum length per SPEC.md §4.4.
pub const MAX_LEN: usize = 2048;

/// An Origin-issued bearer token.
pub struct BearerToken(SecretString);

impl BearerToken {
    /// Validate and wrap an owned string.
    pub fn new(s: impl Into<String>) -> Result<Self, BearerTokenError> {
        let s = s.into();
        let len = s.len();
        if len < MIN_LEN {
            return Err(BearerTokenError::Empty);
        }
        if len > MAX_LEN {
            return Err(BearerTokenError::TooLong { got: len });
        }
        Ok(Self(SecretString::from(s)))
    }

    /// Expose the inner token. Every call to this method is intentional
    /// secret-handling — `git grep` it during security review.
    #[must_use]
    pub fn expose(&self) -> &str {
        self.0.expose_secret()
    }
}

impl Clone for BearerToken {
    fn clone(&self) -> Self {
        // Cannot derive — SecretString doesn't implement Clone.
        Self(SecretString::from(self.0.expose_secret().to_owned()))
    }
}

impl PartialEq for BearerToken {
    fn eq(&self, other: &Self) -> bool {
        // Constant-time would matter if bearer-token comparison were
        // attacker-driven; today this is only used in tests. Switch to
        // constant_time_eq if/when a non-test comparison path appears.
        self.0.expose_secret() == other.0.expose_secret()
    }
}

impl Eq for BearerToken {}

impl fmt::Debug for BearerToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Never print the raw token. Length is non-secret and useful.
        write!(
            f,
            "BearerToken(<redacted, {} bytes>)",
            self.0.expose_secret().len()
        )
    }
}

impl Serialize for BearerToken {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // Required for wire output of the pair payload. Redaction at the
        // logging layer is what keeps this from being a leak.
        serializer.serialize_str(self.0.expose_secret())
    }
}

impl<'de> Deserialize<'de> for BearerToken {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Self::new(s).map_err(serde::de::Error::custom)
    }
}

/// Failure modes for [`BearerToken::new`].
#[derive(Debug, Error)]
pub enum BearerTokenError {
    #[error("bearer token must be at least {MIN_LEN} character")]
    Empty,
    #[error("bearer token is {got} bytes; max is {MAX_LEN}")]
    TooLong { got: usize },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_typical() {
        let t = BearerToken::new("abc123").unwrap();
        assert_eq!(t.expose(), "abc123");
    }

    #[test]
    fn accepts_max_length() {
        let s = "a".repeat(MAX_LEN);
        let t = BearerToken::new(s.clone()).unwrap();
        assert_eq!(t.expose().len(), MAX_LEN);
    }

    #[test]
    fn rejects_empty() {
        assert!(matches!(BearerToken::new(""), Err(BearerTokenError::Empty)));
    }

    #[test]
    fn rejects_too_long() {
        let s = "a".repeat(MAX_LEN + 1);
        assert!(matches!(
            BearerToken::new(s),
            Err(BearerTokenError::TooLong { .. })
        ));
    }

    #[test]
    fn debug_redacts_value() {
        let t = BearerToken::new("supersecret-token").unwrap();
        let dbg = format!("{t:?}");
        assert!(!dbg.contains("supersecret-token"));
        assert!(dbg.contains("redacted"));
    }

    #[test]
    fn serde_round_trip_preserves_value() {
        // Round-trip emits the cleartext token by design — required for the
        // wire protocol. The redaction layer keeps this out of logs.
        let t = BearerToken::new("abc123").unwrap();
        let json = serde_json::to_string(&t).unwrap();
        assert_eq!(json, "\"abc123\"");
        let back: BearerToken = serde_json::from_str(&json).unwrap();
        assert_eq!(back, t);
    }
}

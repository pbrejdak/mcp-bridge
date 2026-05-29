//! Per-Pin (and eventually per-`(Pin, Consumer)`) loopback key.
//!
//! Per [`docs/SPEC.md`](../../../../docs/SPEC.md) §6.1: 256-bit random
//! secret embedded in the loopback URL as a query parameter. The
//! [`Loopback Listener`](super::super::proxy) compares this in constant
//! time on every request — missing or wrong → `401 Unauthorized` with
//! no body.
//!
//! Secret material — Debug redacts, eq is constant-time.

use std::fmt;
use std::str::FromStr;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use constant_time_eq::constant_time_eq;
use rand::RngCore;
use rand::rngs::OsRng;
use thiserror::Error;
use zeroize::{Zeroize, ZeroizeOnDrop};

/// Length in bytes (256 bits) per SPEC.md §6.1.
pub const LEN: usize = 32;

/// A per-Pin loopback authentication key.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct LoopbackKey(Box<[u8; LEN]>);

impl LoopbackKey {
    /// Generate a fresh loopback key from the OS CSPRNG.
    #[must_use]
    pub fn random() -> Self {
        let mut bytes = Box::new([0u8; LEN]);
        OsRng.fill_bytes(bytes.as_mut_slice());
        Self(bytes)
    }

    /// Construct from raw bytes (keystore reconstruction, tests).
    #[must_use]
    pub fn from_bytes(bytes: [u8; LEN]) -> Self {
        Self(Box::new(bytes))
    }

    /// Borrow the raw bytes. Crate-internal — only the keystore writes
    /// them out, and even then through the encoded form.
    pub(crate) fn as_bytes(&self) -> &[u8; LEN] {
        &self.0
    }

    /// Base64url-no-pad encoding of the key. Used as the value of the
    /// `?key=` query parameter on the loopback URL.
    #[must_use]
    pub fn encoded(&self) -> String {
        URL_SAFE_NO_PAD.encode(self.0.as_slice())
    }

    /// Constant-time comparison against a candidate string supplied by
    /// a Consumer's HTTP request. Returns `true` on exact match.
    #[must_use]
    pub fn matches_encoded(&self, candidate: &str) -> bool {
        let Ok(decoded) = URL_SAFE_NO_PAD.decode(candidate) else {
            return false;
        };
        if decoded.len() != LEN {
            return false;
        }
        constant_time_eq(decoded.as_slice(), self.0.as_slice())
    }
}

impl fmt::Debug for LoopbackKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LoopbackKey")
            .field("len", &LEN)
            .finish_non_exhaustive()
    }
}

impl PartialEq for LoopbackKey {
    fn eq(&self, other: &Self) -> bool {
        constant_time_eq(self.0.as_slice(), other.0.as_slice())
    }
}

impl Eq for LoopbackKey {}

impl FromStr for LoopbackKey {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let decoded = URL_SAFE_NO_PAD.decode(s).map_err(ParseError::Base64)?;
        let bytes: [u8; LEN] = decoded
            .as_slice()
            .try_into()
            .map_err(|_| ParseError::WrongLength { got: decoded.len() })?;
        Ok(Self::from_bytes(bytes))
    }
}

/// Failure modes for [`LoopbackKey::from_str`].
#[derive(Debug, Error)]
pub enum ParseError {
    #[error("invalid base64url encoding: {0}")]
    Base64(#[from] base64::DecodeError),
    #[error("decoded key is {got} bytes; expected {LEN}")]
    WrongLength { got: usize },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn random_produces_distinct_keys() {
        let a = LoopbackKey::random();
        let b = LoopbackKey::random();
        assert_ne!(a.as_bytes(), b.as_bytes());
    }

    #[test]
    fn encoded_round_trips_through_from_str() {
        let key = LoopbackKey::random();
        let encoded = key.encoded();
        let back: LoopbackKey = encoded.parse().unwrap();
        assert!(key.matches_encoded(&encoded));
        assert_eq!(back, key);
    }

    #[test]
    fn matches_encoded_accepts_exact() {
        let key = LoopbackKey::from_bytes([0xAB; LEN]);
        assert!(key.matches_encoded(&key.encoded()));
    }

    #[test]
    fn matches_encoded_rejects_wrong_value() {
        let a = LoopbackKey::from_bytes([0xAA; LEN]);
        let b = LoopbackKey::from_bytes([0xBB; LEN]);
        assert!(!a.matches_encoded(&b.encoded()));
    }

    #[test]
    fn matches_encoded_rejects_wrong_length() {
        let key = LoopbackKey::from_bytes([0xAA; LEN]);
        // 31 zero bytes encoded — length check fires.
        let shorter = URL_SAFE_NO_PAD.encode([0u8; 31]);
        assert!(!key.matches_encoded(&shorter));
    }

    #[test]
    fn matches_encoded_rejects_garbage_base64() {
        let key = LoopbackKey::from_bytes([0xAA; LEN]);
        assert!(!key.matches_encoded("!!!not-base64!!!"));
    }

    #[test]
    fn debug_redacts_key_bytes() {
        let key = LoopbackKey::from_bytes([0xCC; LEN]);
        let dbg = format!("{key:?}");
        assert!(dbg.contains("LoopbackKey"));
        assert!(dbg.contains(".."));
        // The encoded form must not appear in Debug output.
        assert!(!dbg.contains(&key.encoded()));
    }

    #[test]
    fn parse_rejects_wrong_length() {
        let encoded = URL_SAFE_NO_PAD.encode([0u8; 31]);
        assert!(matches!(
            encoded.parse::<LoopbackKey>(),
            Err(ParseError::WrongLength { got: 31 })
        ));
    }

    #[test]
    fn parse_rejects_invalid_base64() {
        assert!(matches!(
            "!!!not-base64!!!".parse::<LoopbackKey>(),
            Err(ParseError::Base64(_))
        ));
    }
}

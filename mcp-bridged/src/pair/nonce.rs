//! Single-use 16-byte nonce for pairing invites.
//!
//! See [`docs/SPEC.md`](../../../../docs/SPEC.md) §4.2-§4.3:
//! "16 cryptographically random bytes. Each invite uses a fresh nonce. The
//! Resolver MUST record consumed nonces for the invite-lifetime window."
//!
//! Public material — nonces are part of the QR payload and travel in the
//! clear over the OOB channel. No zeroize required.

use std::fmt;
use std::str::FromStr;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rand::RngCore;
use rand::rngs::OsRng;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;

/// Length of a pairing nonce in bytes.
pub const LEN: usize = 16;

/// A pairing nonce — 16 cryptographically random bytes.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Nonce([u8; LEN]);

impl Nonce {
    /// Generate a fresh nonce from the OS CSPRNG.
    #[must_use]
    pub fn random() -> Self {
        let mut bytes = [0u8; LEN];
        OsRng.fill_bytes(&mut bytes);
        Self(bytes)
    }

    /// Construct from raw bytes (for tests and protocol-level reconstruction).
    #[must_use]
    pub const fn from_bytes(bytes: [u8; LEN]) -> Self {
        Self(bytes)
    }

    /// Borrow the raw nonce bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; LEN] {
        &self.0
    }
}

impl fmt::Display for Nonce {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&URL_SAFE_NO_PAD.encode(self.0))
    }
}

impl fmt::Debug for Nonce {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Nonce").field(&self.to_string()).finish()
    }
}

impl FromStr for Nonce {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let decoded = URL_SAFE_NO_PAD.decode(s).map_err(ParseError::Base64)?;
        let bytes: [u8; LEN] = decoded
            .as_slice()
            .try_into()
            .map_err(|_| ParseError::WrongLength { got: decoded.len() })?;
        Ok(Self(bytes))
    }
}

impl Serialize for Nonce {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Nonce {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = <&str>::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

/// Failure modes for [`Nonce::from_str`].
#[derive(Debug, Error)]
pub enum ParseError {
    #[error("invalid base64url encoding: {0}")]
    Base64(#[from] base64::DecodeError),
    #[error("decoded nonce is {got} bytes; expected {LEN}")]
    WrongLength { got: usize },
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_BYTES: [u8; LEN] = [
        0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0xfe, 0xdc, 0xba, 0x98, 0x76, 0x54, 0x32,
        0x10,
    ];

    /// Independent computation:
    ///   `python3 -c "import base64;
    ///    print(base64.urlsafe_b64encode(bytes.fromhex('0123456789abcdeffedcba9876543210')).decode().rstrip('='))"`
    const SAMPLE_ENCODED: &str = "ASNFZ4mrze_-3LqYdlQyEA";

    #[test]
    fn random_produces_distinct_values() {
        // Cryptographically negligible probability of collision over 100 draws.
        let mut seen = std::collections::HashSet::new();
        for _ in 0..100 {
            let n = Nonce::random();
            assert!(seen.insert(*n.as_bytes()));
        }
    }

    #[test]
    fn display_emits_base64url_no_pad() {
        let n = Nonce::from_bytes(SAMPLE_BYTES);
        assert_eq!(n.to_string(), SAMPLE_ENCODED);
    }

    #[test]
    fn parse_round_trips() {
        let n: Nonce = SAMPLE_ENCODED.parse().unwrap();
        assert_eq!(n.as_bytes(), &SAMPLE_BYTES);
        assert_eq!(n.to_string(), SAMPLE_ENCODED);
    }

    #[test]
    fn parse_rejects_wrong_length() {
        let short = URL_SAFE_NO_PAD.encode([0u8; 15]);
        assert!(matches!(
            short.parse::<Nonce>(),
            Err(ParseError::WrongLength { got: 15 })
        ));
    }

    #[test]
    fn parse_rejects_invalid_base64() {
        assert!(matches!(
            "!!!not-base64!!!".parse::<Nonce>(),
            Err(ParseError::Base64(_))
        ));
    }

    #[test]
    fn debug_does_not_dump_raw_bytes() {
        let n = Nonce::from_bytes(SAMPLE_BYTES);
        let dbg = format!("{n:?}");
        assert!(dbg.contains(SAMPLE_ENCODED));
        assert!(!dbg.starts_with("Nonce(["));
    }

    #[test]
    fn serde_round_trip() {
        let n = Nonce::from_bytes(SAMPLE_BYTES);
        let json = serde_json::to_string(&n).unwrap();
        assert_eq!(json, format!("\"{SAMPLE_ENCODED}\""));
        let back: Nonce = serde_json::from_str(&json).unwrap();
        assert_eq!(back, n);
    }
}

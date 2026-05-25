//! Ed25519 signature newtype with base64url text encoding.
//!
//! See [`docs/SPEC.md`](../../../../docs/SPEC.md) §3.2:
//! "Signatures MUST NOT carry a prefix and are 64 bytes encoded." Pubkeys
//! carry the `ed25519:` prefix; signatures do not.

use std::fmt;
use std::str::FromStr;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;

/// Length of an Ed25519 signature in bytes.
pub const LEN: usize = 64;

/// An Ed25519 signature.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Signature([u8; LEN]);

impl Signature {
    /// Construct from raw signature bytes.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; LEN]) -> Self {
        Self(bytes)
    }

    /// Borrow the raw signature bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; LEN] {
        &self.0
    }
}

impl fmt::Display for Signature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&URL_SAFE_NO_PAD.encode(self.0))
    }
}

impl fmt::Debug for Signature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Signature").field(&self.to_string()).finish()
    }
}

impl FromStr for Signature {
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

impl Serialize for Signature {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Signature {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

/// Failure modes for [`Signature::from_str`].
#[derive(Debug, Error)]
pub enum ParseError {
    #[error("invalid base64url encoding: {0}")]
    Base64(#[from] base64::DecodeError),
    #[error("decoded signature is {got} bytes; expected {LEN}")]
    WrongLength { got: usize },
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_BYTES: [u8; LEN] = [0x42; LEN];

    #[test]
    fn display_emits_base64url_no_pad() {
        let s = Signature::from_bytes(SAMPLE_BYTES);
        assert_eq!(s.to_string(), URL_SAFE_NO_PAD.encode(SAMPLE_BYTES));
    }

    #[test]
    fn parse_round_trips() {
        let s = Signature::from_bytes(SAMPLE_BYTES);
        let parsed: Signature = s.to_string().parse().unwrap();
        assert_eq!(parsed, s);
    }

    #[test]
    fn parse_rejects_wrong_length() {
        let short = URL_SAFE_NO_PAD.encode([0u8; 63]);
        assert!(matches!(
            short.parse::<Signature>(),
            Err(ParseError::WrongLength { got: 63 })
        ));
    }

    #[test]
    fn parse_rejects_invalid_base64() {
        assert!(matches!(
            "!!!not-base64!!!".parse::<Signature>(),
            Err(ParseError::Base64(_))
        ));
    }

    #[test]
    fn debug_emits_encoded_form_not_raw_array() {
        let s = Signature::from_bytes(SAMPLE_BYTES);
        let dbg = format!("{s:?}");
        assert!(dbg.contains(&s.to_string()));
        assert!(!dbg.starts_with("Signature(["));
    }

    #[test]
    fn serde_round_trip() {
        let s = Signature::from_bytes(SAMPLE_BYTES);
        let json = serde_json::to_string(&s).unwrap();
        let back: Signature = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
    }
}

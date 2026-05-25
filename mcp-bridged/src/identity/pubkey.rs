//! Ed25519 public-key newtype with `ed25519:<base64url>` text encoding.
//!
//! Public material — not a secret. Used to pin Origin identities at pair
//! time and to advertise the Resolver's own pubkey in
//! [`docs/SPEC.md`](../../../../docs/SPEC.md) §4.2 invites.

use std::fmt;
use std::str::FromStr;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;

/// Length of an Ed25519 public key in bytes.
pub const LEN: usize = 32;

/// Text prefix that identifies the key algorithm in the encoded form.
pub const PREFIX: &str = "ed25519:";

/// An Ed25519 public key.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Ed25519Pubkey([u8; LEN]);

impl Ed25519Pubkey {
    /// Construct from raw key bytes.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; LEN]) -> Self {
        Self(bytes)
    }

    /// Borrow the raw key bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; LEN] {
        &self.0
    }
}

impl fmt::Display for Ed25519Pubkey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{PREFIX}{}", URL_SAFE_NO_PAD.encode(self.0))
    }
}

impl fmt::Debug for Ed25519Pubkey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Ed25519Pubkey")
            .field(&self.to_string())
            .finish()
    }
}

impl FromStr for Ed25519Pubkey {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let body = s.strip_prefix(PREFIX).ok_or(ParseError::MissingPrefix)?;
        let decoded = URL_SAFE_NO_PAD.decode(body).map_err(ParseError::Base64)?;
        let bytes: [u8; LEN] = decoded
            .as_slice()
            .try_into()
            .map_err(|_| ParseError::WrongLength { got: decoded.len() })?;
        Ok(Self(bytes))
    }
}

impl Serialize for Ed25519Pubkey {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Ed25519Pubkey {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // Owned String here, not `&str`: borrowed deserialization fails when
        // the value flows through `serde_json::Value` or any other deserializer
        // that does not buffer the input bytes.
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

/// Failure modes for [`Ed25519Pubkey::from_str`].
#[derive(Debug, Error)]
pub enum ParseError {
    #[error("missing `{PREFIX}` prefix")]
    MissingPrefix,
    #[error("invalid base64url encoding: {0}")]
    Base64(#[from] base64::DecodeError),
    #[error("decoded key is {got} bytes; expected {LEN}")]
    WrongLength { got: usize },
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_BYTES: [u8; LEN] = [
        0xd7, 0x5a, 0x98, 0x01, 0x82, 0xb1, 0x0a, 0xb7, 0xd5, 0x4b, 0xfe, 0xd3, 0xc9, 0x64, 0x07,
        0x3a, 0x0e, 0xe1, 0x72, 0xf3, 0xda, 0xa6, 0x23, 0x25, 0xaf, 0x02, 0x1a, 0x68, 0xf7, 0x07,
        0x51, 0x1a,
    ];

    /// Independently computed: `python3 -c "import base64;
    /// print(base64.urlsafe_b64encode(bytes.fromhex('d75a9801...511a')).decode().rstrip('='))"`
    const SAMPLE_ENCODED: &str = "ed25519:11qYAYKxCrfVS_7TyWQHOg7hcvPapiMlrwIaaPcHURo";

    #[test]
    fn display_emits_prefixed_base64url() {
        let pk = Ed25519Pubkey::from_bytes(SAMPLE_BYTES);
        assert_eq!(pk.to_string(), SAMPLE_ENCODED);
    }

    #[test]
    fn parse_round_trips() {
        let pk: Ed25519Pubkey = SAMPLE_ENCODED.parse().unwrap();
        assert_eq!(pk.as_bytes(), &SAMPLE_BYTES);
        assert_eq!(pk.to_string(), SAMPLE_ENCODED);
    }

    #[test]
    fn parse_rejects_missing_prefix() {
        let unprefixed = SAMPLE_ENCODED.strip_prefix(PREFIX).unwrap();
        assert!(matches!(
            unprefixed.parse::<Ed25519Pubkey>(),
            Err(ParseError::MissingPrefix)
        ));
    }

    #[test]
    fn parse_rejects_wrong_length() {
        // 31 zero bytes, base64url encoded.
        let short = format!("{PREFIX}{}", URL_SAFE_NO_PAD.encode([0u8; 31]));
        assert!(matches!(
            short.parse::<Ed25519Pubkey>(),
            Err(ParseError::WrongLength { got: 31 })
        ));
    }

    #[test]
    fn parse_rejects_invalid_base64() {
        let bad = format!("{PREFIX}!!!not-base64!!!");
        assert!(matches!(
            bad.parse::<Ed25519Pubkey>(),
            Err(ParseError::Base64(_))
        ));
    }

    #[test]
    fn debug_does_not_dump_raw_bytes() {
        let pk = Ed25519Pubkey::from_bytes(SAMPLE_BYTES);
        let dbg = format!("{pk:?}");
        assert!(dbg.contains(SAMPLE_ENCODED));
        // Sanity: not the raw [u8; 32] derived-Debug output.
        assert!(!dbg.starts_with("Ed25519Pubkey(["));
    }

    #[test]
    fn serde_round_trip() {
        let pk = Ed25519Pubkey::from_bytes(SAMPLE_BYTES);
        let json = serde_json::to_string(&pk).unwrap();
        assert_eq!(json, format!("\"{SAMPLE_ENCODED}\""));
        let back: Ed25519Pubkey = serde_json::from_str(&json).unwrap();
        assert_eq!(back, pk);
    }
}

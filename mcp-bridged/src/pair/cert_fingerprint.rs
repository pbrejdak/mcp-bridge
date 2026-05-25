//! SHA-256 fingerprint of an Origin backend's leaf TLS certificate.
//!
//! See [`docs/SPEC.md`](../../../../docs/SPEC.md) §3.2:
//! "Certificate fingerprints MUST carry the prefix `sha256:` followed by the
//! hex (lowercase) encoding of the digest of the certificate's DER encoding."

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;

/// Length of a SHA-256 digest in bytes.
pub const DIGEST_LEN: usize = 32;

/// Text prefix that identifies the digest algorithm.
pub const PREFIX: &str = "sha256:";

/// A certificate fingerprint.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct CertFingerprint([u8; DIGEST_LEN]);

impl CertFingerprint {
    /// Construct from a raw 32-byte digest.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; DIGEST_LEN]) -> Self {
        Self(bytes)
    }

    /// Borrow the raw 32-byte digest.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; DIGEST_LEN] {
        &self.0
    }
}

impl fmt::Display for CertFingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(PREFIX)?;
        for b in self.0 {
            write!(f, "{b:02x}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for CertFingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("CertFingerprint")
            .field(&self.to_string())
            .finish()
    }
}

impl FromStr for CertFingerprint {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let hex = s.strip_prefix(PREFIX).ok_or(ParseError::MissingPrefix)?;
        if hex.len() != DIGEST_LEN * 2 {
            return Err(ParseError::WrongLength { got: hex.len() });
        }
        let mut out = [0u8; DIGEST_LEN];
        for (i, byte) in out.iter_mut().enumerate() {
            let hi = decode_nibble(hex.as_bytes()[i * 2])?;
            let lo = decode_nibble(hex.as_bytes()[i * 2 + 1])?;
            *byte = (hi << 4) | lo;
        }
        Ok(Self(out))
    }
}

const fn decode_nibble(c: u8) -> Result<u8, ParseError> {
    match c {
        b'0'..=b'9' => Ok(c - b'0'),
        b'a'..=b'f' => Ok(c - b'a' + 10),
        // SPEC.md §3.2 says lowercase hex specifically; reject upper.
        _ => Err(ParseError::InvalidHex { ch: c }),
    }
}

impl Serialize for CertFingerprint {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for CertFingerprint {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

/// Failure modes for [`CertFingerprint::from_str`].
#[derive(Debug, Error)]
pub enum ParseError {
    #[error("missing `{PREFIX}` prefix")]
    MissingPrefix,
    #[error("hex body must be {} chars, got {got}", DIGEST_LEN * 2)]
    WrongLength { got: usize },
    #[error("invalid lowercase-hex character: {ch:?}")]
    InvalidHex { ch: u8 },
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_BYTES: [u8; DIGEST_LEN] = [
        0xde, 0xad, 0xbe, 0xef, 0xca, 0xfe, 0xba, 0xbe, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd,
        0xef, 0xfe, 0xdc, 0xba, 0x98, 0x76, 0x54, 0x32, 0x10, 0xa1, 0xb2, 0xc3, 0xd4, 0xe5, 0xf6,
        0x07, 0x18,
    ];
    const SAMPLE_ENCODED: &str =
        "sha256:deadbeefcafebabe0123456789abcdeffedcba9876543210a1b2c3d4e5f60718";

    #[test]
    fn display_emits_prefixed_lowercase_hex() {
        let fp = CertFingerprint::from_bytes(SAMPLE_BYTES);
        assert_eq!(fp.to_string(), SAMPLE_ENCODED);
    }

    #[test]
    fn parse_round_trips() {
        let fp: CertFingerprint = SAMPLE_ENCODED.parse().unwrap();
        assert_eq!(fp.as_bytes(), &SAMPLE_BYTES);
        assert_eq!(fp.to_string(), SAMPLE_ENCODED);
    }

    #[test]
    fn parse_rejects_missing_prefix() {
        let unprefixed = SAMPLE_ENCODED.strip_prefix(PREFIX).unwrap();
        assert!(matches!(
            unprefixed.parse::<CertFingerprint>(),
            Err(ParseError::MissingPrefix)
        ));
    }

    #[test]
    fn parse_rejects_uppercase_hex() {
        let upper = format!("{PREFIX}DEADBEEF{}", &SAMPLE_ENCODED[15..]);
        assert!(matches!(
            upper.parse::<CertFingerprint>(),
            Err(ParseError::InvalidHex { .. })
        ));
    }

    #[test]
    fn parse_rejects_wrong_length() {
        let short = format!("{PREFIX}deadbeef");
        assert!(matches!(
            short.parse::<CertFingerprint>(),
            Err(ParseError::WrongLength { got: 8 })
        ));
    }

    #[test]
    fn parse_rejects_non_hex_character() {
        // Exactly 64 hex chars, with the last two replaced by `zz` so the
        // length check passes and the hex check is what fires.
        let mut body: String = "0123456789abcdef".repeat(4);
        body.replace_range(62..64, "zz");
        let bad = format!("{PREFIX}{body}");
        assert!(matches!(
            bad.parse::<CertFingerprint>(),
            Err(ParseError::InvalidHex { ch: b'z' })
        ));
    }

    #[test]
    fn serde_round_trip() {
        let fp = CertFingerprint::from_bytes(SAMPLE_BYTES);
        let json = serde_json::to_string(&fp).unwrap();
        assert_eq!(json, format!("\"{SAMPLE_ENCODED}\""));
        let back: CertFingerprint = serde_json::from_str(&json).unwrap();
        assert_eq!(back, fp);
    }
}

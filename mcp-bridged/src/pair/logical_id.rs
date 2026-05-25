//! Origin-chosen identifier, stable across IP / port / token / cert
//! rotation. See [`docs/SPEC.md`](../../../../docs/SPEC.md) §4.4:
//! "ASCII, 1-128 chars, `[a-z0-9_-]`".

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;

/// Minimum length per SPEC.md §4.4.
pub const MIN_LEN: usize = 1;

/// Maximum length per SPEC.md §4.4.
pub const MAX_LEN: usize = 128;

/// An Origin's logical identifier.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct LogicalId(String);

impl LogicalId {
    /// Validate and construct from a string.
    pub fn new(s: impl Into<String>) -> Result<Self, LogicalIdError> {
        let s = s.into();
        if s.len() < MIN_LEN {
            return Err(LogicalIdError::TooShort);
        }
        if s.len() > MAX_LEN {
            return Err(LogicalIdError::TooLong { got: s.len() });
        }
        if let Some((idx, ch)) = s.char_indices().find(|(_, c)| !is_legal_char(*c)) {
            return Err(LogicalIdError::IllegalCharacter { at: idx, ch });
        }
        Ok(Self(s))
    }

    /// Borrow the encoded form.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

const fn is_legal_char(c: char) -> bool {
    matches!(c, 'a'..='z' | '0'..='9' | '_' | '-')
}

impl fmt::Display for LogicalId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for LogicalId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("LogicalId").field(&self.0).finish()
    }
}

impl FromStr for LogicalId {
    type Err = LogicalIdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

impl Serialize for LogicalId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for LogicalId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Self::new(s).map_err(serde::de::Error::custom)
    }
}

/// Failure modes for [`LogicalId::new`].
#[derive(Debug, Error)]
pub enum LogicalIdError {
    #[error("logical id must be at least {MIN_LEN} character")]
    TooShort,
    #[error("logical id is {got} chars; max is {MAX_LEN}")]
    TooLong { got: usize },
    #[error("illegal character {ch:?} at byte offset {at}")]
    IllegalCharacter { at: usize, ch: char },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_typical() {
        let id = LogicalId::new("bodylog-7f3a").unwrap();
        assert_eq!(id.as_str(), "bodylog-7f3a");
    }

    #[test]
    fn accepts_single_char() {
        let id = LogicalId::new("a").unwrap();
        assert_eq!(id.as_str(), "a");
    }

    #[test]
    fn accepts_full_alphabet_range() {
        let id = LogicalId::new("abcdefghijklmnopqrstuvwxyz0123456789_-").unwrap();
        assert_eq!(id.as_str(), "abcdefghijklmnopqrstuvwxyz0123456789_-");
    }

    #[test]
    fn rejects_empty() {
        assert!(matches!(LogicalId::new(""), Err(LogicalIdError::TooShort)));
    }

    #[test]
    fn rejects_uppercase() {
        assert!(matches!(
            LogicalId::new("BodyLog"),
            Err(LogicalIdError::IllegalCharacter { ch: 'B', at: 0 })
        ));
    }

    #[test]
    fn rejects_slash() {
        assert!(matches!(
            LogicalId::new("bodylog/v1"),
            Err(LogicalIdError::IllegalCharacter { ch: '/', .. })
        ));
    }

    #[test]
    fn rejects_dot() {
        assert!(matches!(
            LogicalId::new("body.log"),
            Err(LogicalIdError::IllegalCharacter { ch: '.', .. })
        ));
    }

    #[test]
    fn rejects_over_128_chars() {
        let s = "a".repeat(129);
        assert!(matches!(
            LogicalId::new(s),
            Err(LogicalIdError::TooLong { got: 129 })
        ));
    }

    #[test]
    fn serde_round_trip() {
        let id = LogicalId::new("bodylog-7f3a").unwrap();
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "\"bodylog-7f3a\"");
        let back: LogicalId = serde_json::from_str(&json).unwrap();
        assert_eq!(back, id);
    }
}

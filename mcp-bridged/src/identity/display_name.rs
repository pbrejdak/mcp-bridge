//! Resolver display-name newtype with grapheme-cluster length validation.
//!
//! See [`docs/SPEC.md`](../../../../docs/SPEC.md) §4.2:
//! "UTF-8 string, 1-64 grapheme clusters".

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;
use unicode_segmentation::UnicodeSegmentation;

/// Minimum grapheme-cluster count per SPEC.md §4.2.
pub const MIN_GRAPHEMES: usize = 1;

/// Maximum grapheme-cluster count per SPEC.md §4.2.
pub const MAX_GRAPHEMES: usize = 64;

/// A Resolver's user-facing display name.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct DisplayName(String);

impl DisplayName {
    /// Construct from an owned string, validating the grapheme range.
    pub fn new(s: impl Into<String>) -> Result<Self, DisplayNameError> {
        let s = s.into();
        let count = s.graphemes(true).count();
        if count < MIN_GRAPHEMES {
            return Err(DisplayNameError::TooShort);
        }
        if count > MAX_GRAPHEMES {
            return Err(DisplayNameError::TooLong { got: count });
        }
        Ok(Self(s))
    }

    /// Borrow the inner UTF-8 string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DisplayName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for DisplayName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("DisplayName").field(&self.0).finish()
    }
}

impl Serialize for DisplayName {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for DisplayName {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Self::new(s).map_err(serde::de::Error::custom)
    }
}

/// Failure modes for [`DisplayName::new`].
#[derive(Debug, Error)]
pub enum DisplayNameError {
    #[error("display name must contain at least {MIN_GRAPHEMES} grapheme cluster")]
    TooShort,
    #[error("display name is {got} grapheme clusters; maximum is {MAX_GRAPHEMES}")]
    TooLong { got: usize },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_typical_name() {
        let d = DisplayName::new("Patryk's MacBook Pro").unwrap();
        assert_eq!(d.as_str(), "Patryk's MacBook Pro");
    }

    #[test]
    fn accepts_unicode_emoji_as_single_grapheme() {
        // ZWJ-joined family is one grapheme cluster but many code points.
        let d = DisplayName::new("\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}").unwrap();
        assert_eq!(d.as_str().graphemes(true).count(), 1);
    }

    #[test]
    fn rejects_empty() {
        assert!(matches!(
            DisplayName::new(""),
            Err(DisplayNameError::TooShort)
        ));
    }

    #[test]
    fn rejects_above_64_graphemes() {
        let s = "a".repeat(65);
        assert!(matches!(
            DisplayName::new(s),
            Err(DisplayNameError::TooLong { got: 65 })
        ));
    }

    #[test]
    fn accepts_exactly_64_graphemes() {
        let s = "a".repeat(64);
        let d = DisplayName::new(s).unwrap();
        assert_eq!(d.as_str().chars().count(), 64);
    }

    #[test]
    fn serde_round_trip() {
        let d = DisplayName::new("Patryk's MacBook Pro").unwrap();
        let json = serde_json::to_string(&d).unwrap();
        assert_eq!(json, "\"Patryk's MacBook Pro\"");
        let back: DisplayName = serde_json::from_str(&json).unwrap();
        assert_eq!(back, d);
    }

    #[test]
    fn serde_rejects_invalid_on_deserialize() {
        let r: Result<DisplayName, _> = serde_json::from_str("\"\"");
        assert!(r.is_err());
    }
}

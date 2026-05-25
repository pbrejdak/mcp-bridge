//! Origin authentication block of a pair payload.
//!
//! Per [`docs/SPEC.md`](../../../../docs/SPEC.md) §4.4 + §8.1: `auth.type`
//! is one of `bearer` or `none`; when `bearer`, `auth.value` carries the
//! token. Future versions reserve `oauth2`, `mtls`, `hmac-sig`.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::bearer_token::BearerToken;

/// Origin authentication type enumerated by SPEC.md §8.1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuthType {
    Bearer,
    None,
}

/// The full `auth` block.
///
/// We accept the SPEC.md §4.4 example shape with separate `type` and
/// optional `value` fields and reconcile them via [`Auth::validated`].
/// `Bearer` requires `value` to be present; `None` requires it absent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Auth {
    #[serde(rename = "type")]
    pub ty: AuthType,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub value: Option<BearerToken>,
}

impl Auth {
    /// Run the cross-field consistency check that serde alone cannot.
    ///
    /// Pure shape validation; no signature involvement.
    pub fn validate(&self) -> Result<(), AuthError> {
        match (self.ty, &self.value) {
            (AuthType::Bearer, Some(_)) | (AuthType::None, None) => Ok(()),
            (AuthType::Bearer, None) => Err(AuthError::BearerWithoutValue),
            (AuthType::None, Some(_)) => Err(AuthError::NoneWithValue),
        }
    }
}

/// Failure modes for [`Auth::validate`].
#[derive(Debug, Error)]
pub enum AuthError {
    #[error("auth.type=bearer requires auth.value to be present")]
    BearerWithoutValue,
    #[error("auth.type=none must not carry auth.value")]
    NoneWithValue,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bearer_with_value_validates() {
        let a = Auth {
            ty: AuthType::Bearer,
            value: Some(BearerToken::new("abc").unwrap()),
        };
        a.validate().unwrap();
    }

    #[test]
    fn none_without_value_validates() {
        let a = Auth {
            ty: AuthType::None,
            value: None,
        };
        a.validate().unwrap();
    }

    #[test]
    fn bearer_without_value_is_rejected() {
        let a = Auth {
            ty: AuthType::Bearer,
            value: None,
        };
        assert!(matches!(a.validate(), Err(AuthError::BearerWithoutValue)));
    }

    #[test]
    fn none_with_value_is_rejected() {
        let a = Auth {
            ty: AuthType::None,
            value: Some(BearerToken::new("abc").unwrap()),
        };
        assert!(matches!(a.validate(), Err(AuthError::NoneWithValue)));
    }

    #[test]
    fn auth_type_serializes_lowercase() {
        let json = serde_json::to_string(&AuthType::Bearer).unwrap();
        assert_eq!(json, "\"bearer\"");
    }

    #[test]
    fn unknown_auth_type_rejected() {
        let r: Result<AuthType, _> = serde_json::from_str("\"oauth2\"");
        assert!(r.is_err());
    }
}

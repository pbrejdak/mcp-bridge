//! Apply SPEC §5.5 acceptance rules to a sealed announce envelope.
//!
//! Composes [`super::super::pair::seal::open_with`], JSON deserialization,
//! [`AnnouncePayload::verify_sig`], and the registry lookup + ratchet
//! checks into a single async call: bytes → `AcceptedAnnounce` or
//! [`AcceptError`].
//!
//! Acceptance order:
//!   1. Open the sealed envelope (addressed to this Resolver).
//!   2. Deserialize the inner JSON.
//!   3. Check the freshness window (rule 5). Cheap; rejects unsigned
//!      replay attempts without ever touching the registry.
//!   4. Look up the Pin keyed by `lid` (rule 2). Unknown LIDs are
//!      dropped silently per SPEC — the Resolver MUST NOT auto-create.
//!   5. Verify the signature against the pinned origin pubkey (rule 3).
//!   6. Verify the seq is strictly greater than `last_seen_seq` (rule 4).
//!   7. Verify the fp ratchet (rule 7) and the auth-rotated-at
//!      monotonicity (§5.4).
//!   8. Update the registry pin atomically (last_seen_seq + any field
//!      changes implied by the accepted announce).
//!
//! Persistence — the registry write happens here under the write lock,
//! but the disk save is the caller's job (so multiple state changes can
//! be batched into one save call by the HTTP handler).

use std::sync::Arc;

use thiserror::Error;
use tokio::sync::RwLock;

use crate::identity::Keypair;
use crate::pair::seal::{self, OpenError};
use crate::registry::Registry;

use super::payload::{AnnouncePayload, ValidationError};

/// SPEC §5.4 clock-skew window: `now - 60 ≤ exp ≤ now + 60`.
pub const MAX_CLOCK_SKEW_SECS: u64 = 60;

/// Summary of what changed on the Pin after a successful announce.
#[derive(Debug, Clone)]
pub struct AcceptedAnnounce {
    /// The Pin updated.
    pub logical_id: crate::pair::logical_id::LogicalId,
    /// New `last_seen_seq`.
    pub new_seq: u64,
    /// `backend.fp` (and `backend.url` / `cert_rotated_at`) were updated.
    pub fp_changed: bool,
    /// `auth_rotated_at` strictly advanced — the caller is responsible
    /// for triggering the §5.7 token re-fetch.
    pub auth_rotated: bool,
}

/// Open the sealed envelope and deserialize the inner payload, without
/// touching the registry or running the signature check.
///
/// Split out so the HTTP handler can apply the per-LID rate limit
/// (SPEC §5.6) between this cheap step and the expensive sig verify
/// in [`apply_announce`]. [`accept_announce`] is the unsplit
/// convenience for callers that don't rate-limit.
pub fn parse_sealed_announce(
    sealed: &[u8],
    resolver: &Keypair,
) -> Result<AnnouncePayload, AcceptError> {
    let plaintext = seal::open_with(sealed, resolver)?;
    let payload: AnnouncePayload = serde_json::from_slice(&plaintext)?;
    Ok(payload)
}

/// Apply SPEC §5.5 rules to an already-parsed announce: freshness,
/// pin lookup, sig verify, seq, fp/auth ratchets, atomic registry
/// update.
pub async fn apply_announce(
    payload: AnnouncePayload,
    registry: &Arc<RwLock<Registry>>,
    now_unix: u64,
) -> Result<AcceptedAnnounce, AcceptError> {
    let (drift_abs, ahead) = if payload.exp >= now_unix {
        (payload.exp - now_unix, true)
    } else {
        (now_unix - payload.exp, false)
    };
    if drift_abs > MAX_CLOCK_SKEW_SECS {
        return Err(AcceptError::Stale {
            drift_secs: drift_abs,
            ahead,
        });
    }

    let mut registry = registry.write().await;
    let Some(pin) = registry.get(&payload.lid).cloned() else {
        return Err(AcceptError::UnknownLid);
    };

    payload.verify_sig(&pin.origin_pubkey)?;

    if payload.seq <= pin.last_seen_seq {
        return Err(AcceptError::ReplaySeq {
            last_seen: pin.last_seen_seq,
            got: payload.seq,
        });
    }

    let fp_changed = payload.backend.fp != pin.backend_fp;
    if fp_changed {
        let Some(new_rotated) = payload.cert_rotated_at else {
            return Err(AcceptError::FpChangedWithoutRatchet);
        };
        let prev_rotated = pin.cert_rotated_at.unwrap_or(0);
        if new_rotated <= prev_rotated {
            return Err(AcceptError::CertRatchetNotMonotonic {
                prev: prev_rotated,
                got: new_rotated,
            });
        }
    }

    if let (Some(new_auth), Some(prev_auth)) = (payload.auth_rotated_at, pin.auth_rotated_at) {
        if new_auth < prev_auth {
            return Err(AcceptError::AuthRatchetNonMonotonic {
                prev: prev_auth,
                got: new_auth,
            });
        }
    }

    let auth_rotated = match (pin.auth_rotated_at, payload.auth_rotated_at) {
        (Some(prev), Some(new)) => new > prev,
        (None, Some(_)) => true,
        _ => false,
    };

    let mut updated = pin;
    updated.last_seen_seq = payload.seq;
    updated.backend_url = payload.backend.url.clone();
    if fp_changed {
        updated.backend_fp = payload.backend.fp;
        updated.cert_rotated_at = payload.cert_rotated_at;
    }
    if let Some(a) = payload.auth_rotated_at {
        updated.auth_rotated_at = Some(a);
    }
    registry.insert(updated);

    Ok(AcceptedAnnounce {
        logical_id: payload.lid,
        new_seq: payload.seq,
        fp_changed,
        auth_rotated,
    })
}

/// Accept a sealed `mcp-announce/v0.1` envelope end-to-end. Returns the
/// summary of mutations on success; the caller persists the registry
/// to disk.
///
/// Equivalent to [`parse_sealed_announce`] then [`apply_announce`]; use
/// the split functions when you need to rate-limit per-LID between the
/// two steps (SPEC §5.6).
pub async fn accept_announce(
    sealed: &[u8],
    resolver: &Keypair,
    registry: &Arc<RwLock<Registry>>,
    now_unix: u64,
) -> Result<AcceptedAnnounce, AcceptError> {
    let payload = parse_sealed_announce(sealed, resolver)?;
    apply_announce(payload, registry, now_unix).await
}

/// Failure modes for [`accept_announce`]. Mapped to a bare `400 Bad
/// Request` (no body) at the HTTP layer per SPEC §5.3.
#[derive(Debug, Error)]
pub enum AcceptError {
    #[error("could not open sealed announce envelope: {0}")]
    Seal(#[from] OpenError),
    #[error("could not deserialize announce payload: {0}")]
    Deserialize(#[from] serde_json::Error),
    #[error(
        "exp is outside the ±{MAX_CLOCK_SKEW_SECS}s freshness window ({drift_secs}s {})",
        if *ahead { "ahead of" } else { "behind" }
    )]
    Stale { drift_secs: u64, ahead: bool },
    #[error("no Pin found for the announced logical_id")]
    UnknownLid,
    #[error(transparent)]
    Validation(#[from] ValidationError),
    #[error("seq {got} is not greater than last_seen_seq {last_seen}")]
    ReplaySeq { last_seen: u64, got: u64 },
    #[error("backend.fp changed but the announce carries no cert_rotated_at")]
    FpChangedWithoutRatchet,
    #[error("cert_rotated_at {got} is not strictly greater than previous {prev}")]
    CertRatchetNotMonotonic { prev: u64, got: u64 },
    #[error("auth_rotated_at {got} is less than previously accepted {prev}")]
    AuthRatchetNonMonotonic { prev: u64, got: u64 },
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::identity::{DisplayName, Keypair, Signature};
    use crate::pair::auth::{Auth, AuthType};
    use crate::pair::backend_url::BackendUrl;
    use crate::pair::bearer_token::BearerToken;
    use crate::pair::cert_fingerprint::CertFingerprint;
    use crate::pair::invite::{Direction, SpecVersion as PairSpecVersion};
    use crate::pair::logical_id::LogicalId;
    use crate::pair::nonce::Nonce;
    use crate::pair::payload::{BackendInfo, OriginInfo, PairPayload, Scope};
    use crate::pair::seal;
    use crate::registry::ServerPin;

    use super::super::payload::{AnnounceBackend, SpecVersion as AnnounceSpec};

    const NOW: u64 = 1_700_000_000;

    /// Build a Pin + matching origin keypair so the test can sign
    /// announces and the registry has something to look up.
    fn seed_pin(registry: &mut Registry, lid: &str) -> Keypair {
        let origin = Keypair::generate();
        let resolver = Keypair::generate();
        let mut payload = PairPayload {
            spec: PairSpecVersion::McpPairV0_1,
            direction: Direction::ResolverOffered,
            origin: OriginInfo {
                name: DisplayName::new("BodyLog").unwrap(),
                pubkey: *origin.pubkey(),
                logical_id: LogicalId::new(lid).unwrap(),
            },
            backend: BackendInfo {
                url: BackendUrl::new("https://10.0.0.42:54321/").unwrap(),
                fp: CertFingerprint::from_bytes([0xab; 32]),
                ca: None,
            },
            auth: Auth {
                ty: AuthType::Bearer,
                value: Some(BearerToken::new("token-abc").unwrap()),
            },
            scope: vec![Scope::Tools],
            nonce: Nonce::from_bytes([0u8; 16]),
            target_resolver_pubkey: Some(*resolver.pubkey()),
            sig: Signature::from_bytes([0u8; 64]),
        };
        let canonical = payload.canonical_signing_bytes().unwrap();
        payload.sig = origin.sign(&canonical);
        registry.insert(ServerPin::from_accepted_payload(&payload, NOW - 1_000));
        origin
    }

    fn signed_announce(
        origin: &Keypair,
        lid: &str,
        seq: u64,
        exp: u64,
        fp: [u8; 32],
        cert_rotated_at: Option<u64>,
        auth_rotated_at: Option<u64>,
    ) -> AnnouncePayload {
        let mut payload = AnnouncePayload {
            spec: AnnounceSpec::McpAnnounceV0_1,
            lid: LogicalId::new(lid).unwrap(),
            backend: AnnounceBackend {
                url: BackendUrl::new("https://10.0.0.42:54321/").unwrap(),
                fp: CertFingerprint::from_bytes(fp),
            },
            auth_rotated_at,
            cert_rotated_at,
            seq,
            exp,
            sig: Signature::from_bytes([0u8; 64]),
        };
        let canonical = payload.canonical_signing_bytes().unwrap();
        payload.sig = origin.sign(&canonical);
        payload
    }

    fn seal_for(payload: &AnnouncePayload, resolver: &Keypair) -> Vec<u8> {
        let bytes = serde_json::to_vec(payload).unwrap();
        seal::seal_to(&bytes, resolver.pubkey()).unwrap()
    }

    #[tokio::test]
    async fn happy_path_advances_seq_and_succeeds() {
        let resolver = Keypair::generate();
        let mut reg = Registry::new();
        let origin = seed_pin(&mut reg, "bodylog-7f3a");
        let registry = Arc::new(RwLock::new(reg));

        let payload = signed_announce(&origin, "bodylog-7f3a", 1, NOW, [0xab; 32], None, None);
        let sealed = seal_for(&payload, &resolver);

        let accepted = accept_announce(&sealed, &resolver, &registry, NOW)
            .await
            .unwrap();
        assert_eq!(accepted.new_seq, 1);
        assert!(!accepted.fp_changed);
        assert!(!accepted.auth_rotated);

        let reg = registry.read().await;
        let pin = reg.get(&LogicalId::new("bodylog-7f3a").unwrap()).unwrap();
        assert_eq!(pin.last_seen_seq, 1);
    }

    #[tokio::test]
    async fn sealed_to_wrong_resolver_fails_at_seal_open() {
        let actual = Keypair::generate();
        let imposter = Keypair::generate();
        let mut reg = Registry::new();
        let origin = seed_pin(&mut reg, "bodylog-7f3a");
        let registry = Arc::new(RwLock::new(reg));

        let payload = signed_announce(&origin, "bodylog-7f3a", 1, NOW, [0xab; 32], None, None);
        let sealed = seal_for(&payload, &actual);

        let err = accept_announce(&sealed, &imposter, &registry, NOW)
            .await
            .unwrap_err();
        assert!(matches!(err, AcceptError::Seal(_)));
    }

    #[tokio::test]
    async fn stale_exp_in_the_past_is_rejected() {
        let resolver = Keypair::generate();
        let mut reg = Registry::new();
        let origin = seed_pin(&mut reg, "bodylog-7f3a");
        let registry = Arc::new(RwLock::new(reg));

        let payload = signed_announce(
            &origin,
            "bodylog-7f3a",
            1,
            NOW - MAX_CLOCK_SKEW_SECS - 1,
            [0xab; 32],
            None,
            None,
        );
        let sealed = seal_for(&payload, &resolver);

        let err = accept_announce(&sealed, &resolver, &registry, NOW)
            .await
            .unwrap_err();
        assert!(matches!(err, AcceptError::Stale { .. }));
    }

    #[tokio::test]
    async fn future_exp_outside_window_is_rejected() {
        let resolver = Keypair::generate();
        let mut reg = Registry::new();
        let origin = seed_pin(&mut reg, "bodylog-7f3a");
        let registry = Arc::new(RwLock::new(reg));

        let payload = signed_announce(
            &origin,
            "bodylog-7f3a",
            1,
            NOW + MAX_CLOCK_SKEW_SECS + 1,
            [0xab; 32],
            None,
            None,
        );
        let sealed = seal_for(&payload, &resolver);

        let err = accept_announce(&sealed, &resolver, &registry, NOW)
            .await
            .unwrap_err();
        assert!(matches!(err, AcceptError::Stale { .. }));
    }

    #[tokio::test]
    async fn unknown_lid_is_rejected_without_registry_mutation() {
        let resolver = Keypair::generate();
        let registry = Arc::new(RwLock::new(Registry::new()));

        let stranger = Keypair::generate();
        let payload =
            signed_announce(&stranger, "no-such-lid-x", 1, NOW, [0xab; 32], None, None);
        let sealed = seal_for(&payload, &resolver);

        let err = accept_announce(&sealed, &resolver, &registry, NOW)
            .await
            .unwrap_err();
        assert!(matches!(err, AcceptError::UnknownLid));

        assert_eq!(registry.read().await.len(), 0);
    }

    #[tokio::test]
    async fn wrong_signer_is_rejected_at_verify_sig() {
        let resolver = Keypair::generate();
        let mut reg = Registry::new();
        let _origin = seed_pin(&mut reg, "bodylog-7f3a");
        let registry = Arc::new(RwLock::new(reg));

        let attacker = Keypair::generate();
        let payload = signed_announce(&attacker, "bodylog-7f3a", 1, NOW, [0xab; 32], None, None);
        let sealed = seal_for(&payload, &resolver);

        let err = accept_announce(&sealed, &resolver, &registry, NOW)
            .await
            .unwrap_err();
        assert!(matches!(err, AcceptError::Validation(_)));
    }

    #[tokio::test]
    async fn replay_seq_at_or_below_last_seen_is_rejected() {
        let resolver = Keypair::generate();
        let mut reg = Registry::new();
        let origin = seed_pin(&mut reg, "bodylog-7f3a");
        let registry = Arc::new(RwLock::new(reg));

        // Accept seq=5 first.
        let p1 = signed_announce(&origin, "bodylog-7f3a", 5, NOW, [0xab; 32], None, None);
        accept_announce(&seal_for(&p1, &resolver), &resolver, &registry, NOW)
            .await
            .unwrap();

        // Replay seq=5: must be rejected.
        let err = accept_announce(&seal_for(&p1, &resolver), &resolver, &registry, NOW)
            .await
            .unwrap_err();
        assert!(matches!(err, AcceptError::ReplaySeq { .. }));

        // Older seq=3: must be rejected too.
        let p_old = signed_announce(&origin, "bodylog-7f3a", 3, NOW, [0xab; 32], None, None);
        let err = accept_announce(&seal_for(&p_old, &resolver), &resolver, &registry, NOW)
            .await
            .unwrap_err();
        assert!(matches!(err, AcceptError::ReplaySeq { .. }));
    }

    #[tokio::test]
    async fn fp_change_without_cert_rotated_at_is_rejected() {
        let resolver = Keypair::generate();
        let mut reg = Registry::new();
        let origin = seed_pin(&mut reg, "bodylog-7f3a");
        let registry = Arc::new(RwLock::new(reg));

        let payload = signed_announce(&origin, "bodylog-7f3a", 1, NOW, [0xee; 32], None, None);
        let sealed = seal_for(&payload, &resolver);

        let err = accept_announce(&sealed, &resolver, &registry, NOW)
            .await
            .unwrap_err();
        assert!(matches!(err, AcceptError::FpChangedWithoutRatchet));
    }

    #[tokio::test]
    async fn fp_change_with_advancing_cert_rotated_at_succeeds() {
        let resolver = Keypair::generate();
        let mut reg = Registry::new();
        let origin = seed_pin(&mut reg, "bodylog-7f3a");
        let registry = Arc::new(RwLock::new(reg));

        let payload = signed_announce(
            &origin,
            "bodylog-7f3a",
            1,
            NOW,
            [0xee; 32],
            Some(1_699_900_000),
            None,
        );
        let sealed = seal_for(&payload, &resolver);

        let accepted = accept_announce(&sealed, &resolver, &registry, NOW)
            .await
            .unwrap();
        assert!(accepted.fp_changed);

        let reg = registry.read().await;
        let pin = reg.get(&LogicalId::new("bodylog-7f3a").unwrap()).unwrap();
        assert_eq!(pin.backend_fp, CertFingerprint::from_bytes([0xee; 32]));
        assert_eq!(pin.cert_rotated_at, Some(1_699_900_000));
    }

    #[tokio::test]
    async fn fp_change_with_non_advancing_cert_rotated_at_is_rejected() {
        let resolver = Keypair::generate();
        let mut reg = Registry::new();
        let origin = seed_pin(&mut reg, "bodylog-7f3a");
        let registry = Arc::new(RwLock::new(reg));

        // First fp ratchet succeeds at cert_rotated_at = 1_000.
        let p1 = signed_announce(
            &origin,
            "bodylog-7f3a",
            1,
            NOW,
            [0xee; 32],
            Some(1_000),
            None,
        );
        accept_announce(&seal_for(&p1, &resolver), &resolver, &registry, NOW)
            .await
            .unwrap();

        // Second tries to install a different fp but with the same
        // cert_rotated_at — must be rejected by the strict-greater rule.
        let p2 = signed_announce(
            &origin,
            "bodylog-7f3a",
            2,
            NOW,
            [0xcc; 32],
            Some(1_000),
            None,
        );
        let err = accept_announce(&seal_for(&p2, &resolver), &resolver, &registry, NOW)
            .await
            .unwrap_err();
        assert!(matches!(err, AcceptError::CertRatchetNotMonotonic { .. }));
    }

    #[tokio::test]
    async fn auth_rotated_at_strictly_advancing_signals_caller() {
        let resolver = Keypair::generate();
        let mut reg = Registry::new();
        let origin = seed_pin(&mut reg, "bodylog-7f3a");
        let registry = Arc::new(RwLock::new(reg));

        let payload = signed_announce(
            &origin,
            "bodylog-7f3a",
            1,
            NOW,
            [0xab; 32],
            None,
            Some(1_699_900_000),
        );
        let sealed = seal_for(&payload, &resolver);

        let accepted = accept_announce(&sealed, &resolver, &registry, NOW)
            .await
            .unwrap();
        assert!(accepted.auth_rotated);

        let reg = registry.read().await;
        let pin = reg.get(&LogicalId::new("bodylog-7f3a").unwrap()).unwrap();
        assert_eq!(pin.auth_rotated_at, Some(1_699_900_000));
    }

    #[tokio::test]
    async fn auth_rotated_at_going_backwards_is_rejected() {
        let resolver = Keypair::generate();
        let mut reg = Registry::new();
        let origin = seed_pin(&mut reg, "bodylog-7f3a");
        let registry = Arc::new(RwLock::new(reg));

        let p1 = signed_announce(
            &origin,
            "bodylog-7f3a",
            1,
            NOW,
            [0xab; 32],
            None,
            Some(1_000),
        );
        accept_announce(&seal_for(&p1, &resolver), &resolver, &registry, NOW)
            .await
            .unwrap();

        let p2 = signed_announce(
            &origin,
            "bodylog-7f3a",
            2,
            NOW,
            [0xab; 32],
            None,
            Some(500),
        );
        let err = accept_announce(&seal_for(&p2, &resolver), &resolver, &registry, NOW)
            .await
            .unwrap_err();
        assert!(matches!(err, AcceptError::AuthRatchetNonMonotonic { .. }));
    }

    #[tokio::test]
    async fn auth_rotated_at_equal_to_previous_is_a_silent_noop() {
        let resolver = Keypair::generate();
        let mut reg = Registry::new();
        let origin = seed_pin(&mut reg, "bodylog-7f3a");
        let registry = Arc::new(RwLock::new(reg));

        let p1 = signed_announce(
            &origin,
            "bodylog-7f3a",
            1,
            NOW,
            [0xab; 32],
            None,
            Some(1_000),
        );
        accept_announce(&seal_for(&p1, &resolver), &resolver, &registry, NOW)
            .await
            .unwrap();

        let p2 = signed_announce(
            &origin,
            "bodylog-7f3a",
            2,
            NOW,
            [0xab; 32],
            None,
            Some(1_000),
        );
        let accepted = accept_announce(&seal_for(&p2, &resolver), &resolver, &registry, NOW)
            .await
            .unwrap();
        assert!(
            !accepted.auth_rotated,
            "equal auth_rotated_at must NOT signal rotation"
        );
    }

    #[tokio::test]
    async fn garbage_inner_bytes_fail_at_deserialize() {
        let resolver = Keypair::generate();
        let mut reg = Registry::new();
        let _origin = seed_pin(&mut reg, "bodylog-7f3a");
        let registry = Arc::new(RwLock::new(reg));

        let sealed = seal::seal_to(b"not json", resolver.pubkey()).unwrap();
        let err = accept_announce(&sealed, &resolver, &registry, NOW)
            .await
            .unwrap_err();
        assert!(matches!(err, AcceptError::Deserialize(_)));
    }
}

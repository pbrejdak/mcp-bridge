//! mDNS / Bonjour carrier for `mcp-announce/v0.1`.
//!
//! Per [`docs/SPEC.md`](../../../../docs/SPEC.md) §5.2: the Origin
//! publishes its sealed announce body inside a TXT record under a
//! daily-rotated service type. The Resolver subscribes to the service
//! types for the current and previous UTC day (clock-skew tolerance),
//! decodes each TXT, and feeds the sealed bytes through the existing
//! [`super::accept::apply_announce`] pipeline.
//!
//! This module owns the protocol-level pieces:
//!
//! - [`service_type`] — `_mcp-bridge-<HMAC>._tcp.local` derivation.
//! - [`current_and_previous_service_types`] — the two-day subscribe set.
//! - [`decode_txt_body`] — pulls the `body=` entry, base64url-decodes.
//! - [`MdnsSubscriber`] / [`MdnsAnnouncement`] — the platform-trait
//!   abstraction every per-OS backend implements.
//! - [`FakeMdnsSubscriber`] — in-memory subscriber for tests.
//!
//! Platform-specific backends (Bonjour on macOS, zeroconf on
//! Linux/Windows) live in [`super::mdns_backend`] (still TBD) and
//! provide a real [`MdnsSubscriber`] implementation. The accept-bridge
//! task in [`super::mdns_serve`] (also TBD) connects a subscriber to
//! the announce::accept pipeline.

use std::collections::HashMap;
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use thiserror::Error;
use tokio::sync::{RwLock, mpsc};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use crate::identity::{Ed25519Pubkey, Keypair};
use crate::registry::Registry;

use super::accept::{apply_announce, parse_sealed_announce};
use super::rate_limit::AnnounceRateLimiter;

/// SPEC §5.2 TXT key carrying the sealed body.
pub const BODY_KEY: &str = "body";

/// Service-type prefix per SPEC §5.2. The HMAC suffix appended to this
/// rotates daily.
pub const SERVICE_TYPE_PREFIX: &str = "_mcp-bridge-";

/// Service-type suffix per SPEC §5.2: `._tcp.local`.
pub const SERVICE_TYPE_SUFFIX: &str = "._tcp.local";

/// Length of the HMAC prefix used as the service-type suffix
/// (SPEC §5.2: `HMAC[..8]` then base64url-no-pad encoded).
pub const HMAC_PREFIX_LEN: usize = 8;

/// Derive the per-day service type for `resolver_pubkey`.
///
/// `day_unix_seconds` is any timestamp in the UTC day the caller wants
/// to publish/subscribe under. Most callers want
/// [`current_and_previous_service_types`], which derives both days
/// from the current wall clock.
#[must_use]
pub fn service_type(resolver_pubkey: &Ed25519Pubkey, day_unix_seconds: u64) -> String {
    let (year, month, day) = unix_seconds_to_ymd_utc(day_unix_seconds);
    let salt = format!("mcp-bridge:{year:04}{month:02}{day:02}");

    let mut mac = Hmac::<Sha256>::new_from_slice(resolver_pubkey.as_bytes())
        .expect("HMAC-SHA256 accepts any key length");
    mac.update(salt.as_bytes());
    let tag = mac.finalize().into_bytes();
    let prefix = URL_SAFE_NO_PAD.encode(&tag[..HMAC_PREFIX_LEN]);

    format!("{SERVICE_TYPE_PREFIX}{prefix}{SERVICE_TYPE_SUFFIX}")
}

/// Return the two service types a Resolver subscribes to: today's and
/// yesterday's. SPEC §5.2 requires both for clock-skew tolerance.
#[must_use]
pub fn current_and_previous_service_types(
    resolver_pubkey: &Ed25519Pubkey,
) -> [String; 2] {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    [
        service_type(resolver_pubkey, now),
        service_type(resolver_pubkey, now.saturating_sub(86_400)),
    ]
}

/// Extract the sealed announce bytes from an mDNS TXT record set.
///
/// SPEC §5.2: exactly one `body=<base64url>` entry. Receivers MUST
/// ignore other keys; missing or malformed `body` is a hard reject.
///
/// Generic over the hasher so callers using foundation/zeroconf-style
/// maps (which sometimes ship custom hashers) don't have to copy into
/// `HashMap<_, _, RandomState>` first.
pub fn decode_txt_body<S: std::hash::BuildHasher>(
    txt: &HashMap<String, String, S>,
) -> Result<Vec<u8>, DecodeError> {
    let encoded = txt.get(BODY_KEY).ok_or(DecodeError::MissingBody)?;
    URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(DecodeError::Base64)
}

/// Failure modes for [`decode_txt_body`].
#[derive(Debug, Error)]
pub enum DecodeError {
    #[error("no `body` key in the TXT record")]
    MissingBody,
    #[error("base64url decode of `body` failed: {0}")]
    Base64(#[source] base64::DecodeError),
}

/// One announcement event observed by a [`MdnsSubscriber`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MdnsAnnouncement {
    /// Service instance name as reported by the OS layer. Useful for
    /// logging; the protocol itself doesn't read it.
    pub instance_name: String,
    /// TXT records as a map. Multi-value records (rare) are
    /// reduced to last-wins by the caller; the daemon only reads
    /// `body`.
    pub txt: HashMap<String, String>,
    /// IPv4 / IPv6 address of the publisher as observed at the
    /// network layer. Used for the SPEC §5.6 per-source-IP rate limit.
    pub source_addr: IpAddr,
}

/// Stream of mDNS announcements. Implementations bridge the platform's
/// mDNS API (Bonjour on macOS, zeroconf on Linux/Windows) to the
/// daemon's tokio runtime.
#[async_trait]
pub trait MdnsSubscriber: Send + Sync {
    /// Begin subscribing to `service_types`. Returns a receiver that
    /// yields one [`MdnsAnnouncement`] per resolved instance. The
    /// receiver closes when the subscriber is dropped or the OS layer
    /// reports a hard failure.
    async fn subscribe(
        &self,
        service_types: &[String],
    ) -> Result<mpsc::Receiver<MdnsAnnouncement>, SubscribeError>;
}

/// Failure modes any [`MdnsSubscriber`] backend may surface.
#[derive(Debug, Error)]
pub enum SubscribeError {
    #[error("mDNS backend is not available on this platform")]
    Unsupported,
    #[error("mDNS backend failed to start: {0}")]
    Backend(String),
}

/// In-memory [`MdnsSubscriber`] implementation. Tests inject
/// [`MdnsAnnouncement`] values via [`FakeMdnsSubscriber::push`] and the
/// accept-bridge task observes them as if a real publisher had emitted
/// them. [`FakeMdnsSubscriber::subscribe_count`] reports how many times
/// the bridge re-subscribed — useful for day-rollover tests.
#[derive(Debug, Default)]
pub struct FakeMdnsSubscriber {
    sender: tokio::sync::Mutex<Option<mpsc::Sender<MdnsAnnouncement>>>,
    subscribe_count: std::sync::atomic::AtomicUsize,
}

impl FakeMdnsSubscriber {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Push one announcement to the subscribed channel. No-op if no
    /// caller has subscribed yet.
    pub async fn push(&self, ann: MdnsAnnouncement) {
        let guard = self.sender.lock().await;
        if let Some(tx) = guard.as_ref() {
            let _ = tx.send(ann).await;
        }
    }

    /// Number of `subscribe` calls observed so far. Used by tests to
    /// confirm day-rollover re-subscribes.
    #[must_use]
    pub fn subscribe_count(&self) -> usize {
        self.subscribe_count
            .load(std::sync::atomic::Ordering::Relaxed)
    }
}

#[async_trait]
impl MdnsSubscriber for FakeMdnsSubscriber {
    async fn subscribe(
        &self,
        _service_types: &[String],
    ) -> Result<mpsc::Receiver<MdnsAnnouncement>, SubscribeError> {
        let (tx, rx) = mpsc::channel(32);
        *self.sender.lock().await = Some(tx);
        self.subscribe_count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Ok(rx)
    }
}

/// Long-running task: subscribe to the [today, yesterday] service
/// types, decode each TXT body, and feed the sealed bytes through the
/// same accept pipeline the HTTP `/announce` endpoint uses.
///
/// Failures at any step (rate-limit drop, malformed TXT, broken seal,
/// stale exp, replay seq) are logged at debug or warn and the loop
/// continues. The only way out is via `cancel`.
///
/// Day-rollover: the service-type HMAC rotates at UTC midnight. The
/// outer loop sleeps until the next UTC midnight (plus a 5s buffer),
/// drops the current subscription, and re-subscribes with the freshly
/// derived [today, yesterday] pair.
pub async fn serve_mdns(
    subscriber: Arc<dyn MdnsSubscriber>,
    resolver: Arc<Keypair>,
    registry: Arc<RwLock<Registry>>,
    registry_path: PathBuf,
    rate_limiter: Arc<AnnounceRateLimiter>,
    cancel: CancellationToken,
) -> Result<(), SubscribeError> {
    loop {
        let service_types = current_and_previous_service_types(resolver.pubkey()).to_vec();
        info!(?service_types, "mDNS subscriber (re)subscribing");
        let mut rx = subscriber.subscribe(&service_types).await?;
        let rollover_at = next_utc_midnight_instant();

        loop {
            tokio::select! {
                () = cancel.cancelled() => {
                    info!("mDNS subscriber shutting down");
                    return Ok(());
                }
                () = tokio::time::sleep_until(rollover_at) => {
                    info!("UTC day rolled over; refreshing mDNS subscription");
                    break;
                }
                ann = rx.recv() => {
                    let Some(ann) = ann else {
                        warn!("mDNS subscriber channel closed; exiting loop");
                        return Ok(());
                    };
                    handle_announcement(
                        ann,
                        resolver.as_ref(),
                        &registry,
                        &registry_path,
                        rate_limiter.as_ref(),
                    )
                    .await;
                }
            }
        }
        // Dropping `rx` here causes the per-service-type drain tasks
        // inside the subscriber backend to observe `tx.send` failure
        // and exit. The next outer-loop iteration creates a fresh
        // subscription with the new service types.
    }
}

/// `tokio::time::Instant` corresponding to the next UTC midnight,
/// plus a five-second buffer so we never refresh microseconds before
/// the date string actually rolls.
fn next_utc_midnight_instant() -> tokio::time::Instant {
    let now_wall = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let secs_into_day = now_wall % 86_400;
    let secs_to_next_midnight = 86_400 - secs_into_day + 5;
    tokio::time::Instant::now() + std::time::Duration::from_secs(secs_to_next_midnight)
}

/// Process one [`MdnsAnnouncement`]. Mirrors the HTTP `/announce`
/// handler's stage ordering — IP rate-limit, parse, LID rate-limit,
/// apply, persist — and pulls the sealed bytes out of the `body=`
/// TXT entry first.
async fn handle_announcement(
    ann: MdnsAnnouncement,
    resolver: &Keypair,
    registry: &Arc<RwLock<Registry>>,
    registry_path: &std::path::Path,
    rate_limiter: &AnnounceRateLimiter,
) {
    if !rate_limiter.check_ip(ann.source_addr) {
        debug!(peer = %ann.source_addr, "mDNS announce rejected: per-IP rate limit");
        return;
    }

    let sealed = match decode_txt_body(&ann.txt) {
        Ok(b) => b,
        Err(e) => {
            debug!(error = ?e, instance = %ann.instance_name, "mDNS TXT decode failed");
            return;
        }
    };

    let payload = match parse_sealed_announce(&sealed, resolver) {
        Ok(p) => p,
        Err(e) => {
            debug!(error = ?e, "mDNS announce rejected at parse");
            return;
        }
    };

    if !rate_limiter.check_lid(&payload.lid) {
        debug!(logical_id = %payload.lid, "mDNS announce rejected: per-LID rate limit");
        return;
    }

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let accepted = match apply_announce(payload, registry, now).await {
        Ok(a) => a,
        Err(e) => {
            debug!(error = ?e, "mDNS announce rejected");
            return;
        }
    };

    let registry_guard = registry.read().await;
    let save_result = registry_guard.save(registry_path).await;
    drop(registry_guard);
    if let Err(e) = save_result {
        warn!(
            error = ?e,
            logical_id = %accepted.logical_id,
            "registry save failed after mDNS announce accept",
        );
        return;
    }

    if accepted.auth_rotated {
        warn!(
            logical_id = %accepted.logical_id,
            "mDNS announce signalled bearer-token rotation; §5.7 re-fetch is not yet implemented",
        );
    }
    info!(
        logical_id = %accepted.logical_id,
        seq = accepted.new_seq,
        fp_changed = accepted.fp_changed,
        auth_rotated = accepted.auth_rotated,
        "mDNS announce accepted",
    );
}

/// Convert a unix-seconds timestamp to (year, month, day) in UTC.
///
/// Howard Hinnant's `civil_from_days` algorithm — branchless, no
/// external date crate. We always pass post-epoch values (today is
/// 1970+), so the algorithm runs entirely on non-negative integers.
///
/// <http://howardhinnant.github.io/date_algorithms.html#civil_from_days>
#[allow(clippy::many_single_char_names, clippy::similar_names)] // names mirror Hinnant's reference impl
fn unix_seconds_to_ymd_utc(unix_seconds: u64) -> (u32, u32, u32) {
    // Days since the unix epoch (1970-01-01).
    let days = unix_seconds / 86_400;
    // Hinnant uses days since the civil epoch (0000-03-01); the unix
    // epoch is 719_468 days after that.
    let z = days + 719_468;
    // Era: 400-year cycle. For non-negative z, `z / 146_097` is the
    // era index.
    let era = z / 146_097;
    let doe = z - era * 146_097; // [0, 146_097)
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 400)
    let year_full = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 366)
    let mp = (5 * doy + 2) / 153; // [0, 12)
    let day = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let month = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let year = year_full + u64::from(month <= 2);
    // Caller-visible types are u32 — Hinnant's algorithm gives Jan/Feb
    // months 13/14 in the previous year, which the +1 above corrects;
    // both year and day fit comfortably in u32.
    #[allow(clippy::cast_possible_truncation)]
    (year as u32, month as u32, day as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unix_to_ymd_known_dates() {
        // Reference values from a calendar.
        assert_eq!(unix_seconds_to_ymd_utc(0), (1970, 1, 1));
        assert_eq!(unix_seconds_to_ymd_utc(1_700_000_000), (2023, 11, 14));
        // Leap-day handling.
        assert_eq!(unix_seconds_to_ymd_utc(1_582_934_400), (2020, 2, 29));
        // Last second of a day rolls correctly.
        assert_eq!(unix_seconds_to_ymd_utc(86_399), (1970, 1, 1));
        assert_eq!(unix_seconds_to_ymd_utc(86_400), (1970, 1, 2));
    }

    #[test]
    fn service_type_shape_matches_spec_5_2() {
        let pk = Ed25519Pubkey::from_bytes([0u8; 32]);
        let st = service_type(&pk, 0);
        assert!(st.starts_with(SERVICE_TYPE_PREFIX));
        assert!(st.ends_with(SERVICE_TYPE_SUFFIX));
        // The HMAC prefix is 8 bytes → 11 base64url-no-pad chars.
        let suffix_chars = st.len() - SERVICE_TYPE_PREFIX.len() - SERVICE_TYPE_SUFFIX.len();
        assert_eq!(suffix_chars, 11, "HMAC prefix must be 11 chars (8 bytes b64url)");
    }

    #[test]
    fn service_type_is_deterministic_per_day() {
        let pk = Ed25519Pubkey::from_bytes([0x11; 32]);
        let a = service_type(&pk, 1_700_000_000);
        // Same day, any time within: identical output.
        let b = service_type(&pk, 1_700_000_500);
        assert_eq!(a, b);
        // Next day: different.
        let c = service_type(&pk, 1_700_000_000 + 86_400);
        assert_ne!(a, c);
    }

    #[test]
    fn service_type_differs_per_resolver() {
        let a = service_type(&Ed25519Pubkey::from_bytes([0xAA; 32]), 1_700_000_000);
        let b = service_type(&Ed25519Pubkey::from_bytes([0xBB; 32]), 1_700_000_000);
        assert_ne!(a, b);
    }

    #[test]
    fn current_and_previous_pair_differs_when_daily_rolls() {
        // Run the helper twice — both readings are on the same day,
        // so the [current, previous] tuple is stable.
        let pk = Ed25519Pubkey::from_bytes([0x42; 32]);
        let [a_now, a_prev] = current_and_previous_service_types(&pk);
        let [b_now, b_prev] = current_and_previous_service_types(&pk);
        assert_eq!(a_now, b_now);
        assert_eq!(a_prev, b_prev);
        // current ≠ previous (HMAC of different days)
        assert_ne!(a_now, a_prev);
    }

    #[test]
    fn decode_txt_body_extracts_base64url() {
        let mut txt = HashMap::new();
        txt.insert("body".to_owned(), URL_SAFE_NO_PAD.encode(b"hello"));
        txt.insert("ignored".to_owned(), "anything".to_owned());
        let decoded = decode_txt_body(&txt).unwrap();
        assert_eq!(decoded, b"hello");
    }

    #[test]
    fn decode_txt_body_without_body_key_is_an_error() {
        let txt = HashMap::new();
        assert!(matches!(
            decode_txt_body(&txt),
            Err(DecodeError::MissingBody)
        ));
    }

    #[test]
    fn decode_txt_body_with_malformed_base64_is_an_error() {
        let mut txt = HashMap::new();
        txt.insert("body".to_owned(), "!!!not-base64!!!".to_owned());
        assert!(matches!(decode_txt_body(&txt), Err(DecodeError::Base64(_))));
    }

    #[test]
    fn next_utc_midnight_is_within_one_day_plus_buffer() {
        let now = tokio::time::Instant::now();
        let at = next_utc_midnight_instant();
        let dur = at.saturating_duration_since(now);
        assert!(
            dur.as_secs() <= 86_400 + 5,
            "next midnight must be within a day + 5s buffer (got {}s)",
            dur.as_secs(),
        );
        assert!(
            dur.as_secs() >= 5,
            "next midnight must be at least the 5s buffer away (got {}s)",
            dur.as_secs(),
        );
    }

    #[tokio::test]
    async fn fake_subscriber_round_trips_an_announcement() {
        let sub = FakeMdnsSubscriber::new();
        let mut rx = sub.subscribe(&["_test._tcp.local".to_owned()]).await.unwrap();

        let ann = MdnsAnnouncement {
            instance_name: "phone".to_owned(),
            txt: HashMap::from([("body".to_owned(), "ZXg".to_owned())]),
            source_addr: "10.0.0.42".parse().unwrap(),
        };
        sub.push(ann.clone()).await;
        let got = rx.recv().await.unwrap();
        assert_eq!(got, ann);
    }
}

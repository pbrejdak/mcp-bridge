//! Pre-signature rate limiting for the announce carrier
//! (SPEC §5.6).
//!
//! Per-source-IP and per-LID token buckets that drop excess records
//! without ever running the (relatively expensive) Ed25519 verification.
//! Both buckets are checked: IP first (cheap, runs on every inbound
//! request) and LID after unseal+deserialize (so the LID is known) but
//! before the sig check.
//!
//! Constants come from SPEC §5.6 — the HTTP-POST row, since today the
//! HTTP carrier is the only one wired. The mDNS row's tighter limit
//! (4/s/IP) lands when the mDNS subscriber lands.

use std::collections::HashMap;
use std::hash::Hash;
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::Instant;

use crate::pair::logical_id::LogicalId;

/// HTTP-POST carrier per-source-IP budget (SPEC §5.6).
pub const HTTP_PER_IP_BURST: u32 = 8;
pub const HTTP_PER_IP_REFILL_PER_SEC: f64 = 8.0;

/// mDNS carrier per-source-IP budget (SPEC §5.6 — tighter than HTTP
/// because mDNS multicast invites every host on the LAN to drive the
/// verifier).
pub const MDNS_PER_IP_BURST: u32 = 4;
pub const MDNS_PER_IP_REFILL_PER_SEC: f64 = 4.0;

/// Per-LID budget (SPEC §5.6, same value for both carriers).
pub const PER_LID_BURST: u32 = 1;
pub const PER_LID_REFILL_PER_SEC: f64 = 1.0;

/// One token bucket: fills at `refill_per_sec` up to `burst` tokens;
/// `try_take` returns whether one token was available and (if so)
/// consumed.
#[derive(Debug)]
struct Bucket {
    burst: f64,
    refill_per_sec: f64,
    tokens: f64,
    last: Instant,
}

impl Bucket {
    fn new(burst: u32, refill_per_sec: f64) -> Self {
        Self {
            burst: f64::from(burst),
            refill_per_sec,
            tokens: f64::from(burst),
            last: Instant::now(),
        }
    }

    fn try_take(&mut self, now: Instant) -> bool {
        let elapsed = now.duration_since(self.last).as_secs_f64();
        self.tokens = (self.tokens + elapsed * self.refill_per_sec).min(self.burst);
        self.last = now;
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

/// A keyed token-bucket rate limiter. Buckets are created lazily on
/// first access; idle ones survive in the map until the daemon
/// restarts. The expected key space is tiny (number of distinct paired
/// LIDs or recent peer IPs), so unbounded growth in this scope is
/// acceptable for v0.1.
#[derive(Debug)]
struct KeyedRateLimiter<K: Eq + Hash + Clone> {
    burst: u32,
    refill_per_sec: f64,
    buckets: Mutex<HashMap<K, Bucket>>,
}

impl<K: Eq + Hash + Clone> KeyedRateLimiter<K> {
    fn new(burst: u32, refill_per_sec: f64) -> Self {
        Self {
            burst,
            refill_per_sec,
            buckets: Mutex::new(HashMap::new()),
        }
    }

    fn check(&self, key: &K, now: Instant) -> bool {
        let mut buckets = self.buckets.lock().expect("rate-limit mutex poisoned");
        let bucket = buckets
            .entry(key.clone())
            .or_insert_with(|| Bucket::new(self.burst, self.refill_per_sec));
        bucket.try_take(now)
    }
}

/// Rate limiter for the HTTP-POST announce carrier. Holds the SPEC
/// §5.6 per-IP and per-LID buckets.
#[derive(Debug)]
pub struct AnnounceRateLimiter {
    per_ip: KeyedRateLimiter<IpAddr>,
    per_lid: KeyedRateLimiter<LogicalId>,
}

impl Default for AnnounceRateLimiter {
    fn default() -> Self {
        Self::http_default()
    }
}

impl AnnounceRateLimiter {
    /// SPEC §5.6 HTTP-POST row: 8/s/IP and 1/s/LID.
    #[must_use]
    pub fn http_default() -> Self {
        Self {
            per_ip: KeyedRateLimiter::new(HTTP_PER_IP_BURST, HTTP_PER_IP_REFILL_PER_SEC),
            per_lid: KeyedRateLimiter::new(PER_LID_BURST, PER_LID_REFILL_PER_SEC),
        }
    }

    /// SPEC §5.6 mDNS row: 4/s/IP and 1/s/LID.
    #[must_use]
    pub fn mdns_default() -> Self {
        Self {
            per_ip: KeyedRateLimiter::new(MDNS_PER_IP_BURST, MDNS_PER_IP_REFILL_PER_SEC),
            per_lid: KeyedRateLimiter::new(PER_LID_BURST, PER_LID_REFILL_PER_SEC),
        }
    }

    /// First-stage check: does this source IP have budget? Called by
    /// the HTTP handler before unsealing.
    #[must_use]
    pub fn check_ip(&self, ip: IpAddr) -> bool {
        self.per_ip.check(&ip, Instant::now())
    }

    /// Second-stage check: does this LID have budget? Called after
    /// unseal+deserialize but before sig verify.
    #[must_use]
    pub fn check_lid(&self, lid: &LogicalId) -> bool {
        self.per_lid.check(lid, Instant::now())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::time::Duration;

    #[test]
    fn bucket_lets_burst_through_then_starves() {
        let mut b = Bucket::new(3, 1.0);
        let t = Instant::now();
        assert!(b.try_take(t));
        assert!(b.try_take(t));
        assert!(b.try_take(t));
        // Burst exhausted.
        assert!(!b.try_take(t));
    }

    #[test]
    fn bucket_refills_over_time() {
        let mut b = Bucket::new(1, 1.0);
        let t0 = Instant::now();
        assert!(b.try_take(t0));
        assert!(!b.try_take(t0));
        // One second of real time = one token of headroom.
        assert!(b.try_take(t0 + Duration::from_secs(1)));
    }

    #[test]
    fn bucket_refill_clamps_at_burst() {
        let mut b = Bucket::new(2, 1.0);
        let t0 = Instant::now();
        // Idle for an hour: the bucket should NOT grow past 2.
        let later = t0 + Duration::from_secs(3_600);
        assert!(b.try_take(later));
        assert!(b.try_take(later));
        assert!(!b.try_take(later));
    }

    #[test]
    fn per_ip_buckets_are_independent() {
        let limiter = AnnounceRateLimiter::http_default();
        let ip_a: IpAddr = "10.0.0.1".parse().unwrap();
        let ip_b: IpAddr = "10.0.0.2".parse().unwrap();
        for _ in 0..HTTP_PER_IP_BURST {
            assert!(limiter.check_ip(ip_a));
        }
        assert!(!limiter.check_ip(ip_a));
        // ip_b's bucket is fresh — it still has its full burst.
        assert!(limiter.check_ip(ip_b));
    }

    #[test]
    fn per_lid_buckets_are_independent() {
        let limiter = AnnounceRateLimiter::http_default();
        let lid_a = LogicalId::new("a-1234").unwrap();
        let lid_b = LogicalId::new("b-5678").unwrap();
        assert!(limiter.check_lid(&lid_a));
        // a's bucket is now drained.
        assert!(!limiter.check_lid(&lid_a));
        // b's bucket is fresh.
        assert!(limiter.check_lid(&lid_b));
    }

    #[test]
    fn ip_limit_matches_spec_5_6_http_row() {
        // SPEC §5.6 HTTP row: ≤ 8 verifications/sec per source IP.
        assert_eq!(HTTP_PER_IP_BURST, 8);
    }

    #[test]
    fn lid_limit_matches_spec_5_6() {
        // SPEC §5.6 (both rows): ≤ 1 verification/sec per LID.
        assert_eq!(PER_LID_BURST, 1);
    }
}

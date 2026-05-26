//! Tracing-subscriber setup for the daemon process.
//!
//! Per [`docs/DAEMON.md`](../../../../docs/DAEMON.md) §8: local-only logging,
//! structured fields preferred, no telemetry. Defaults are deliberately
//! narrow (request bodies, headers, secrets all never logged); verbose mode
//! is opt-in elsewhere and gated by a UI toggle that is not part of this
//! module.
//!
//! What this module owns: the one-time `tracing-subscriber` global
//! initialization that every binary entry point should call exactly once.

use std::sync::Once;

use tracing_subscriber::EnvFilter;

/// Default env-filter directive when `RUST_LOG` is unset.
///
/// `info` everywhere except our own crate where `debug` is helpful during
/// Phase 1 development. Bump the second clause down to `info` before the
/// first public release.
pub const DEFAULT_FILTER: &str = "info,mcp_bridged=debug";

/// Install the global `tracing` subscriber.
///
/// Idempotent — second and subsequent calls are no-ops. Safe to call from
/// the daemon entry point, from CLI subcommands, and from integration
/// tests without coordination.
pub fn init() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        let filter =
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(DEFAULT_FILTER));
        let subscriber = tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_target(true)
            .with_thread_ids(false)
            .with_level(true)
            .compact()
            .finish();
        // `set_global_default` can fail if a subscriber is already
        // installed (e.g. another component called init concurrently).
        // We ignore the error — being installed is the desired state.
        let _ = tracing::subscriber::set_global_default(subscriber);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_is_idempotent() {
        // Calling init twice from the same test must not panic.
        init();
        init();
    }
}

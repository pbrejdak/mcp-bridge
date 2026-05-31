//! Tracing-subscriber setup for the daemon process.
//!
//! Per [`docs/DAEMON.md`](../../../../docs/DAEMON.md) §8: local-only logging,
//! structured fields preferred, no telemetry. Defaults are deliberately
//! narrow (request bodies, headers, secrets all never logged); verbose mode
//! is opt-in elsewhere and gated by a UI toggle that is not part of this
//! module.
//!
//! What this module owns:
//! - the one-time `tracing-subscriber` global initialization
//! - the in-memory [`recorder::EventRecorder`] feeding the `log.recent`
//!   IPC method, installed as a sibling `Layer` to the fmt layer

pub mod recorder;

use std::sync::{Mutex, Once};

use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;

pub use recorder::EventRecorder;

/// Default env-filter directive when `RUST_LOG` is unset.
///
/// `info` everywhere except our own crate where `debug` is helpful during
/// Phase 1 development. Bump the second clause down to `info` before the
/// first public release.
pub const DEFAULT_FILTER: &str = "info,mcp_bridged=debug";

/// Globally-accessible handle to the recorder Layer installed by
/// [`init`]. `None` until `init` runs (e.g. very early in test
/// scaffolding). Once set it never changes.
static RECORDER: Mutex<Option<EventRecorder>> = Mutex::new(None);

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
        let recorder = EventRecorder::new();
        *RECORDER.lock().expect("recorder mutex poisoned") = Some(recorder.clone());

        let subscriber = tracing_subscriber::registry()
            .with(filter)
            .with(
                tracing_subscriber::fmt::layer()
                    .with_target(true)
                    .with_thread_ids(false)
                    .with_level(true)
                    .compact(),
            )
            .with(recorder);

        // `set_global_default` can fail if a subscriber is already
        // installed (e.g. another component called init concurrently).
        // We ignore the error — being installed is the desired state.
        let _ = tracing::subscriber::set_global_default(subscriber);
    });
}

/// Return the recorder installed by [`init`], if any. Callers that want
/// to expose `log.recent` over IPC pass this handle into the IPC
/// `Context`.
#[must_use]
pub fn recorder() -> Option<EventRecorder> {
    RECORDER.lock().expect("recorder mutex poisoned").clone()
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

    #[test]
    fn recorder_is_available_after_init() {
        init();
        assert!(recorder().is_some(), "init must populate the recorder slot");
    }
}

//! In-memory event recorder that exposes recent log entries to the IPC
//! surface (`log.recent`).
//!
//! Implemented as a `tracing_subscriber::Layer` that pushes every
//! formatted event into a bounded ring buffer. The IPC handler reads
//! back the tail. No streaming yet — the CLI's `--follow` flag polls
//! the same endpoint with a remembered cursor.
//!
//! Redaction posture: the daemon already declines to log secret fields
//! (per CLAUDE.md §5 and `docs/DAEMON.md` §8.2). The recorder serialises
//! whatever fields the rest of the daemon emits and adds no new
//! redaction of its own.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;

/// Maximum number of events retained in the ring buffer. The oldest
/// entries fall off the back as new ones arrive.
pub const BUFFER_CAP: usize = 1024;

/// Capacity of the broadcast channel for live subscribers. Lagging
/// receivers see `RecvError::Lagged` and the channel drops the oldest
/// queued events — preferred behaviour over blocking the producer.
pub const BROADCAST_CAP: usize = 256;

/// A single captured log event in the form the IPC surface returns.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEvent {
    /// Monotonic sequence number; the CLI uses this as a polling
    /// cursor for `--follow`.
    pub seq: u64,
    /// Unix-seconds wall-clock timestamp when the event was captured.
    pub timestamp: u64,
    /// Tracing level: `ERROR`, `WARN`, `INFO`, `DEBUG`, `TRACE`.
    pub level: String,
    /// Module path that emitted the event (e.g. `mcp_bridged::daemon`).
    pub target: String,
    /// The `message` field of the event. Other structured fields are
    /// folded in via Debug formatting after a `--` separator.
    pub message: String,
}

/// Handle to the recorder shared between the tracing Layer and the IPC
/// dispatcher.
#[derive(Debug, Clone)]
pub struct EventRecorder {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    buffer: Mutex<VecDeque<LogEvent>>,
    next_seq: AtomicU64,
    /// Broadcast sender for live `log.subscribe` clients. `send` is
    /// fire-and-forget — it never blocks; if no subscriber is alive
    /// the event is dropped on the broadcast side, the ring buffer
    /// still keeps it.
    tx: broadcast::Sender<LogEvent>,
}

impl Default for EventRecorder {
    fn default() -> Self {
        Self::new()
    }
}

impl EventRecorder {
    #[must_use]
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(BROADCAST_CAP);
        Self {
            inner: Arc::new(Inner {
                buffer: Mutex::new(VecDeque::with_capacity(BUFFER_CAP)),
                next_seq: AtomicU64::new(1),
                tx,
            }),
        }
    }

    /// Subscribe to the live event broadcast. Returns a `tokio` broadcast
    /// receiver yielding every event captured by the tracing Layer
    /// after the call; historical events stay in the ring buffer
    /// (see [`recent`](Self::recent)) and are NOT replayed.
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<LogEvent> {
        self.inner.tx.subscribe()
    }

    /// Return up to `limit` events whose seq is strictly greater than
    /// `after`. `None` for `after` returns the most recent `limit`
    /// events. Results are oldest-first so they print in chronological
    /// order.
    pub fn recent(&self, after: Option<u64>, limit: usize) -> Vec<LogEvent> {
        let buffer = self.inner.buffer.lock().expect("recorder mutex poisoned");
        let cursor = after.unwrap_or(0);
        let filtered: Vec<LogEvent> = buffer.iter().filter(|e| e.seq > cursor).cloned().collect();
        if filtered.len() > limit {
            // Keep the newest `limit` entries (slicing from the back),
            // then re-order oldest-first for display.
            let drop = filtered.len() - limit;
            filtered.into_iter().skip(drop).collect()
        } else {
            filtered
        }
    }

    /// Push one captured event directly into the ring buffer + the
    /// live broadcast. The Layer impl below is the production path;
    /// this is `pub` so integration tests can synthesize events
    /// without installing the tracing subscriber.
    #[doc(hidden)]
    pub fn push(&self, mut event: LogEvent) {
        event.seq = self.inner.next_seq.fetch_add(1, Ordering::Relaxed);
        let mut buffer = self.inner.buffer.lock().expect("recorder mutex poisoned");
        if buffer.len() == BUFFER_CAP {
            buffer.pop_front();
        }
        buffer.push_back(event.clone());
        drop(buffer);
        // Broadcast is fire-and-forget; `send` errors only when no
        // receivers are alive, which is the common case (nobody is
        // following) — log.subscribe clients are rare.
        let _ = self.inner.tx.send(event);
    }
}

/// `tracing_subscriber::Layer` implementation that funnels events into
/// the recorder. Install alongside the fmt layer in `observability::init`.
impl<S> tracing_subscriber::Layer<S> for EventRecorder
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let meta = event.metadata();
        let mut visitor = FieldCollector::default();
        event.record(&mut visitor);

        let message = if visitor.fields.is_empty() {
            visitor.message
        } else {
            // Append structured fields after a `--` so a Consumer can
            // glance and read both — same shape as tracing's compact
            // formatter.
            format!("{} -- {}", visitor.message, visitor.fields.join(" "))
        };

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        self.push(LogEvent {
            seq: 0, // overwritten in push()
            timestamp,
            level: meta.level().to_string(),
            target: meta.target().to_owned(),
            message,
        });
    }
}

/// Visit-pattern collector for an event's fields. We pull the
/// `message` field out specially and Debug-format the rest as
/// `name=value` pairs.
#[derive(Default)]
struct FieldCollector {
    message: String,
    fields: Vec<String>,
}

impl Visit for FieldCollector {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            value.clone_into(&mut self.message);
        } else {
            self.fields.push(format!("{}={value}", field.name()));
        }
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        if field.name() == "message" {
            self.message = value.to_string();
        } else {
            self.fields.push(format!("{}={value}", field.name()));
        }
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        if field.name() == "message" {
            self.message = value.to_string();
        } else {
            self.fields.push(format!("{}={value}", field.name()));
        }
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        if field.name() == "message" {
            self.message = value.to_string();
        } else {
            self.fields.push(format!("{}={value}", field.name()));
        }
    }

    fn record_f64(&mut self, field: &Field, value: f64) {
        if field.name() == "message" {
            self.message = value.to_string();
        } else {
            self.fields.push(format!("{}={value}", field.name()));
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = format!("{value:?}");
        } else {
            self.fields.push(format!("{}={value:?}", field.name()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_assigns_monotonic_seqs() {
        let r = EventRecorder::new();
        for i in 0..5 {
            r.push(LogEvent {
                seq: 0,
                timestamp: 100 + i,
                level: "INFO".to_owned(),
                target: "test".to_owned(),
                message: format!("event {i}"),
            });
        }
        let events = r.recent(None, 10);
        let seqs: Vec<u64> = events.iter().map(|e| e.seq).collect();
        assert_eq!(seqs, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn ring_buffer_drops_oldest_when_capacity_exceeded() {
        let r = EventRecorder::new();
        for i in 0..(BUFFER_CAP + 100) {
            r.push(LogEvent {
                seq: 0,
                timestamp: i as u64,
                level: "INFO".to_owned(),
                target: "test".to_owned(),
                message: format!("event {i}"),
            });
        }
        let events = r.recent(None, BUFFER_CAP * 2);
        assert_eq!(
            events.len(),
            BUFFER_CAP,
            "ring buffer must hold at most BUFFER_CAP events"
        );
        // First event in the result is the oldest still in the buffer:
        // we pushed `BUFFER_CAP + 100` events, so the first 100 are
        // gone — earliest surviving seq is 101.
        assert_eq!(events[0].seq, 101);
    }

    #[test]
    fn recent_after_filters_by_cursor() {
        let r = EventRecorder::new();
        for i in 0..5 {
            r.push(LogEvent {
                seq: 0,
                timestamp: 100 + i,
                level: "INFO".to_owned(),
                target: "test".to_owned(),
                message: format!("event {i}"),
            });
        }
        let after_seq_2 = r.recent(Some(2), 10);
        let seqs: Vec<u64> = after_seq_2.iter().map(|e| e.seq).collect();
        assert_eq!(seqs, vec![3, 4, 5]);
    }

    #[test]
    fn recent_limit_keeps_newest_when_over_cap() {
        let r = EventRecorder::new();
        for i in 0..10 {
            r.push(LogEvent {
                seq: 0,
                timestamp: i,
                level: "INFO".to_owned(),
                target: "test".to_owned(),
                message: format!("e{i}"),
            });
        }
        let last_three = r.recent(None, 3);
        let seqs: Vec<u64> = last_three.iter().map(|e| e.seq).collect();
        assert_eq!(seqs, vec![8, 9, 10]);
    }

    #[test]
    fn recent_on_empty_recorder_returns_empty() {
        let r = EventRecorder::new();
        assert!(r.recent(None, 100).is_empty());
        assert!(r.recent(Some(99), 100).is_empty());
    }

    /// Plumbing test: install the layer in a private subscriber, emit
    /// a tracing event, then read it back through `recent`. This
    /// proves the Layer wiring + Visit captures the message field.
    #[test]
    fn tracing_layer_records_an_emitted_event() {
        use tracing_subscriber::Layer;
        use tracing_subscriber::layer::SubscriberExt;

        let recorder = EventRecorder::new();
        let subscriber = tracing_subscriber::registry().with(recorder.clone().with_filter(
            tracing_subscriber::filter::LevelFilter::INFO,
        ));
        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(answer = 42, "hello from a tracing event");
        });

        let events = recorder.recent(None, 10);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].level, "INFO");
        assert!(events[0].message.contains("hello from a tracing event"));
        assert!(
            events[0].message.contains("answer=42"),
            "structured field must be appended; got {}",
            events[0].message
        );
    }
}

//! [`Adapter`] trait — the contract each Client-side adapter implements.
//!
//! Per [`docs/CONTRIBUTING.md`](../../../../docs/CONTRIBUTING.md) §3.5:
//! every adapter must support detect / write / remove operations, and
//! must tag every entry it writes with a sentinel UUID so revoke and
//! uninstall touch only Bridge-managed entries.

use std::path::PathBuf;

use async_trait::async_trait;
use thiserror::Error;

use crate::identity::DisplayName;
use crate::pair::logical_id::LogicalId;

use super::sentinel::Sentinel;

/// One entry the adapter should persist into its Consumer's config.
#[derive(Debug, Clone)]
pub struct AdapterEntry {
    /// Stable identifier for the entry (used by [`Adapter::remove_entry`]).
    pub logical_id: LogicalId,
    /// User-visible name (shown by the Consumer's UI when present).
    pub display_name: DisplayName,
    /// Full loopback URL including `?key=...`.
    pub url: String,
    /// Per-install sentinel that lets us recognise our own entries on
    /// revoke / uninstall.
    pub sentinel: Sentinel,
}

/// Detection result: where the Consumer's config file would live.
#[derive(Debug, Clone)]
pub struct Detected {
    pub config_path: PathBuf,
}

/// A Client Adapter — one impl per Consumer family
/// (`claude-desktop`, `cursor`, …).
#[async_trait]
pub trait Adapter: Send + Sync {
    /// Canonical name (stable identifier; matches the `ServerPin`'s
    /// per-Consumer entry name).
    fn name(&self) -> &'static str;

    /// Locate the Consumer's config file. Returns `Some(Detected)` if
    /// the Consumer appears installed (config file or parent directory
    /// present); `None` otherwise. Errors only for unexpected I/O.
    async fn detect(&self) -> Result<Option<Detected>, AdapterError>;

    /// Insert (or overwrite) an entry for `entry.logical_id`. Atomic
    /// per the adapter's own conventions (typically temp-file + rename).
    async fn write_entry(&self, entry: &AdapterEntry) -> Result<(), AdapterError>;

    /// Remove an entry by `logical_id`. Only entries carrying the
    /// adapter's own sentinel are removed; user-authored entries with
    /// the same key are left alone.
    async fn remove_entry(
        &self,
        logical_id: &LogicalId,
        sentinel: Sentinel,
    ) -> Result<(), AdapterError>;
}

/// Failure modes any [`Adapter`] implementation may surface.
#[derive(Debug, Error)]
pub enum AdapterError {
    #[error("I/O error on adapter config file: {0}")]
    Io(#[from] std::io::Error),
    #[error("could not parse adapter config JSON: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("adapter config has an unexpected shape: {0}")]
    UnexpectedShape(String),
}

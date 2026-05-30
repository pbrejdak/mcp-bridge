//! Client Adapters: per-Consumer config writers tagged with the sentinel
//! UUID so unattended removal touches only Bridge-managed entries. See
//! [`docs/CONTRIBUTING.md`] §3.5 for the adapter contract.
//!
//! Submodules:
//! - [`sentinel`] — the per-install UUID we tag every entry with.
//! - [`adapter`] — the trait every adapter implements.
//! - [`claude_desktop`] — Claude Desktop config writer.
//!
//! [`docs/CONTRIBUTING.md`]: ../../../../docs/CONTRIBUTING.md

pub mod adapter;
pub mod claude_desktop;
pub mod sentinel;

pub use adapter::{Adapter, AdapterEntry, AdapterError, Detected};
pub use claude_desktop::ClaudeDesktopAdapter;
pub use sentinel::Sentinel;

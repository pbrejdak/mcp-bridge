//! MCP Bridge daemon — Stable-Loopback Bridge implementation.
//!
//! Crate layout follows [`docs/DAEMON.md`] §3 module layout. Modules listed
//! here are scaffold-only at this revision; bodies land in Phase 1 work per
//! [`docs/ROADMAP.md`].
//!
//! [`docs/DAEMON.md`]: ../../docs/DAEMON.md
//! [`docs/ROADMAP.md`]: ../../docs/ROADMAP.md

pub mod adapters;
pub mod announce;
pub mod config;
pub mod identity;
pub mod ipc;
pub mod keystore;
pub mod observability;
pub mod pair;
pub mod proxy;
pub mod registry;
pub mod supervisor;
pub mod update;

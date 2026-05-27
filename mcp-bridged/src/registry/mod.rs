//! Server Registry: persistent store of Server Pins. See
//! [`docs/DAEMON.md`] §7.1.
//!
//! Submodules:
//! - [`pin`] — the [`ServerPin`] type and its on-disk shape.
//! - [`store`] — atomic JSON read/write of the pin set.
//!
//! [`docs/DAEMON.md`]: ../../../../docs/DAEMON.md

pub mod pin;
pub mod store;

pub use pin::{PinState, ServerPin};
pub use store::{Registry, RegistryError};

//! Runtime configuration for the daemon process.
//!
//! Per [`docs/DAEMON.md`](../../../docs/DAEMON.md) §7: paths follow OS
//! conventions via the [`directories`] crate. No hardcoded
//! `~/Library/...` or `%APPDATA%\...` — every path goes through
//! [`directories::ProjectDirs`].
//!
//! The [`Config`] struct is the single argument to the daemon entry
//! point. CLI flags / env vars merge into this struct in
//! [`crate::cli`] (forthcoming); this module owns the *shape* and the
//! defaults only.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use thiserror::Error;

use crate::identity::DisplayName;

/// Qualifier for `directories::ProjectDirs`. Matches the bundle ID
/// convention `dev.mcpbridge.mcp-bridged`.
const QUALIFIER: &str = "dev";
const ORGANIZATION: &str = "MCPBridge";
const APPLICATION: &str = "mcp-bridged";

/// Default pair-endpoint bind address. Loopback for Phase 1; the daemon
/// will grow LAN-interface auto-detection in a later commit.
pub const DEFAULT_BIND_ADDR: ([u8; 4], u16) = ([127, 0, 0, 1], 8765);

/// Default loopback-listener bind address per SPEC §6.1
/// recommendation (8765 is the pair endpoint default; loopback gets the
/// next port up).
pub const DEFAULT_LOOPBACK_ADDR: ([u8; 4], u16) = ([127, 0, 0, 1], 8766);

/// Runtime configuration handed to the daemon entry point.
#[derive(Debug, Clone)]
pub struct Config {
    /// Where the HTTPS pair endpoint binds.
    pub bind_addr: SocketAddr,
    /// Where the loopback listener binds. MUST be 127.0.0.1 (or ::1);
    /// the listener refuses to start on a non-loopback address.
    pub loopback_addr: SocketAddr,
    /// OS-appropriate data directory for persistent state (registry, …).
    pub data_dir: PathBuf,
    /// Operator-facing name carried in every pairing invite. Shown on
    /// the phone's confirm screen so the user can sanity-check they're
    /// pairing with the right desktop.
    pub display_name: DisplayName,
}

impl Config {
    /// Sensible defaults: loopback pair endpoint, OS data directory.
    ///
    /// Errors only on platforms where `directories::ProjectDirs::from`
    /// cannot determine a home directory (extremely unusual).
    pub fn defaults() -> Result<Self, ConfigError> {
        let dirs = ProjectDirs::from(QUALIFIER, ORGANIZATION, APPLICATION)
            .ok_or(ConfigError::NoProjectDirs)?;
        let (octets, port) = DEFAULT_BIND_ADDR;
        let (lb_octets, lb_port) = DEFAULT_LOOPBACK_ADDR;
        let display_name = default_display_name();
        Ok(Self {
            bind_addr: SocketAddr::from((octets, port)),
            loopback_addr: SocketAddr::from((lb_octets, lb_port)),
            data_dir: dirs.data_dir().to_path_buf(),
            display_name,
        })
    }

    /// Override the Resolver's display name (used by CLI flag).
    #[must_use]
    pub fn with_display_name(mut self, name: DisplayName) -> Self {
        self.display_name = name;
        self
    }

    /// Path to the persisted Server Registry. See
    /// [`docs/DAEMON.md`](../../../docs/DAEMON.md) §7.1.
    #[must_use]
    pub fn registry_path(&self) -> PathBuf {
        self.data_dir.join("registry.json")
    }

    /// Endpoint identifier for the IPC server.
    ///
    /// On Unix: a UDS path under `data_dir` (`control.sock`). On Windows:
    /// a fixed named-pipe name (`\\.\pipe\mcp-bridge-control`) — Windows
    /// named pipes are a flat per-machine namespace, so the data dir
    /// doesn't participate. The single-user threat model in
    /// [`docs/THREAT-MODEL.md`](../../../docs/THREAT-MODEL.md) makes this
    /// safe; co-installed daemons aren't supported.
    #[must_use]
    pub fn ipc_socket_path(&self) -> PathBuf {
        #[cfg(unix)]
        {
            self.data_dir.join("control.sock")
        }
        #[cfg(windows)]
        {
            PathBuf::from(r"\\.\pipe\mcp-bridge-control")
        }
    }

    /// Override the bind address (used by CLI `--bind` flag).
    #[must_use]
    pub fn with_bind_addr(mut self, addr: SocketAddr) -> Self {
        self.bind_addr = addr;
        self
    }

    /// Override the loopback listener address (used by CLI flags and
    /// tests that need an ephemeral port).
    #[must_use]
    pub fn with_loopback_addr(mut self, addr: SocketAddr) -> Self {
        self.loopback_addr = addr;
        self
    }

    /// Override the data directory (used by CLI `--data-dir` flag and by
    /// tests that need an isolated tmpdir).
    #[must_use]
    pub fn with_data_dir(mut self, dir: impl AsRef<Path>) -> Self {
        self.data_dir = dir.as_ref().to_path_buf();
        self
    }
}

/// Resolver display name shown to phones during pairing. Picked up from
/// the `MCP_BRIDGE_DISPLAY_NAME` env var if set; otherwise falls back
/// to the system hostname (`HOSTNAME`/`COMPUTERNAME`) or, ultimately,
/// the fixed string "MCP Bridge".
fn default_display_name() -> DisplayName {
    if let Ok(custom) = std::env::var("MCP_BRIDGE_DISPLAY_NAME") {
        if let Ok(name) = DisplayName::new(&custom) {
            return name;
        }
    }
    let host_var = if cfg!(windows) { "COMPUTERNAME" } else { "HOSTNAME" };
    if let Ok(host) = std::env::var(host_var) {
        if let Ok(name) = DisplayName::new(&host) {
            return name;
        }
    }
    DisplayName::new("MCP Bridge").expect("static literal fits DisplayName")
}

/// Failure modes for [`Config::defaults`].
#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("could not determine OS project directories — no home directory available")]
    NoProjectDirs,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_returns_loopback_bind() {
        let c = Config::defaults().expect("defaults should resolve a home dir on test hosts");
        assert_eq!(c.bind_addr.ip().to_string(), "127.0.0.1");
        assert_eq!(c.bind_addr.port(), 8765);
    }

    #[test]
    fn defaults_data_dir_is_non_empty_path() {
        let c = Config::defaults().unwrap();
        assert!(!c.data_dir.as_os_str().is_empty());
    }

    #[test]
    fn registry_path_is_under_data_dir() {
        let c = Config::defaults().unwrap();
        assert!(c.registry_path().starts_with(&c.data_dir));
        assert_eq!(
            c.registry_path().file_name().and_then(|s| s.to_str()),
            Some("registry.json")
        );
    }

    #[test]
    fn with_bind_addr_overrides() {
        let c = Config::defaults()
            .unwrap()
            .with_bind_addr("192.168.1.10:9000".parse().unwrap());
        assert_eq!(c.bind_addr.to_string(), "192.168.1.10:9000");
    }

    #[test]
    fn with_data_dir_overrides() {
        let c = Config::defaults().unwrap().with_data_dir("/tmp/mcp-test");
        assert_eq!(c.data_dir, PathBuf::from("/tmp/mcp-test"));
    }
}

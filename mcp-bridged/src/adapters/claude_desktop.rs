//! Claude Desktop adapter — writes `claude_desktop_config.json` entries.
//!
//! Config file shape (per Anthropic's MCP integration docs):
//!
//! ```json
//! {
//!   "mcpServers": {
//!     "<logical_id>": {
//!       "url": "http://127.0.0.1:8766/<logical_id>?key=...",
//!       "_mcp_bridge_managed": "<sentinel-uuid>"
//!     }
//!   }
//! }
//! ```
//!
//! The `_mcp_bridge_managed` marker is a sentinel UUID per
//! [`docs/CONTRIBUTING.md`](../../../../docs/CONTRIBUTING.md) §3.5; on
//! revoke we touch only entries whose marker matches our own.
//!
//! Default config path follows the OS-native conventions, derived via
//! [`directories::BaseDirs`]:
//! - macOS: `~/Library/Application Support/Claude/claude_desktop_config.json`
//! - Windows: `%APPDATA%\Claude\claude_desktop_config.json`
//! - Linux: `$XDG_CONFIG_HOME/Claude/claude_desktop_config.json`

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use directories::BaseDirs;
use serde_json::{Map, Value};
use tokio::fs;

use crate::pair::logical_id::LogicalId;

use super::adapter::{Adapter, AdapterEntry, AdapterError, Detected};
use super::sentinel::Sentinel;

/// JSON key under which entries are nested in Claude Desktop's config.
pub const MCP_SERVERS_KEY: &str = "mcpServers";

/// JSON key carrying our sentinel inside each managed entry.
pub const SENTINEL_KEY: &str = "_mcp_bridge_managed";

/// Adapter for Claude Desktop's `claude_desktop_config.json`.
#[derive(Debug, Clone)]
pub struct ClaudeDesktopAdapter {
    config_path: PathBuf,
}

impl ClaudeDesktopAdapter {
    /// Default adapter — resolves the config path per the OS
    /// conventions. Falls back to a sensible relative path if the
    /// home directory cannot be determined (rare).
    #[must_use]
    pub fn new() -> Self {
        Self {
            config_path: default_config_path(),
        }
    }

    /// Adapter pointed at a specific config file. Used by tests with
    /// a tempdir and by power users with a non-standard layout.
    #[must_use]
    pub fn with_config_path(config_path: PathBuf) -> Self {
        Self { config_path }
    }

    /// Return the resolved config-file path.
    #[must_use]
    pub fn config_path(&self) -> &Path {
        &self.config_path
    }
}

impl Default for ClaudeDesktopAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Adapter for ClaudeDesktopAdapter {
    fn name(&self) -> &'static str {
        "claude-desktop"
    }

    async fn detect(&self) -> Result<Option<Detected>, AdapterError> {
        // "Detected" if either the config file already exists or its
        // parent directory does — the latter handles the "Claude is
        // installed but has never written config" case.
        let exists = fs::try_exists(&self.config_path).await?;
        let parent_exists = match self.config_path.parent() {
            Some(p) => fs::try_exists(p).await?,
            None => false,
        };
        if exists || parent_exists {
            Ok(Some(Detected {
                config_path: self.config_path.clone(),
            }))
        } else {
            Ok(None)
        }
    }

    async fn write_entry(&self, entry: &AdapterEntry) -> Result<(), AdapterError> {
        let mut doc = read_config(&self.config_path).await?;
        let servers = ensure_servers_map(&mut doc)?;

        let mut server = Map::new();
        server.insert("url".to_owned(), Value::String(entry.url.clone()));
        server.insert(
            SENTINEL_KEY.to_owned(),
            Value::String(entry.sentinel.to_string()),
        );

        servers.insert(entry.logical_id.as_str().to_owned(), Value::Object(server));
        write_config_atomic(&self.config_path, &doc).await
    }

    async fn remove_entry(
        &self,
        logical_id: &LogicalId,
        sentinel: Sentinel,
    ) -> Result<(), AdapterError> {
        let Some(mut doc) = read_config_if_present(&self.config_path).await? else {
            return Ok(());
        };

        let Some(servers) = doc.get_mut(MCP_SERVERS_KEY).and_then(Value::as_object_mut) else {
            return Ok(());
        };

        let should_remove = servers
            .get(logical_id.as_str())
            .is_some_and(|entry| entry_has_sentinel(entry, sentinel));
        if should_remove {
            servers.remove(logical_id.as_str());
            write_config_atomic(&self.config_path, &doc).await?;
        }
        // If the entry exists but is NOT ours (no matching sentinel),
        // leave it alone. The user added it by hand.
        Ok(())
    }
}

/// Read the config file. Returns an empty top-level object when the
/// file does not yet exist — the daemon's first-pair case.
async fn read_config(path: &Path) -> Result<Value, AdapterError> {
    Ok(read_config_if_present(path)
        .await?
        .unwrap_or_else(|| Value::Object(Map::new())))
}

async fn read_config_if_present(path: &Path) -> Result<Option<Value>, AdapterError> {
    match fs::read(path).await {
        Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(AdapterError::Io(e)),
    }
}

fn ensure_servers_map(doc: &mut Value) -> Result<&mut Map<String, Value>, AdapterError> {
    if !doc.is_object() {
        return Err(AdapterError::UnexpectedShape(
            "top-level config is not a JSON object".into(),
        ));
    }
    let obj = doc.as_object_mut().expect("checked is_object above");
    if !obj.contains_key(MCP_SERVERS_KEY) {
        obj.insert(MCP_SERVERS_KEY.to_owned(), Value::Object(Map::new()));
    }
    match obj.get_mut(MCP_SERVERS_KEY).and_then(Value::as_object_mut) {
        Some(servers) => Ok(servers),
        None => Err(AdapterError::UnexpectedShape(format!(
            "`{MCP_SERVERS_KEY}` is not an object"
        ))),
    }
}

fn entry_has_sentinel(entry: &Value, sentinel: Sentinel) -> bool {
    entry
        .as_object()
        .and_then(|obj| obj.get(SENTINEL_KEY))
        .and_then(Value::as_str)
        .and_then(|s| s.parse::<Sentinel>().ok())
        == Some(sentinel)
}

/// Pretty-print and write `doc` atomically (temp file + rename).
async fn write_config_atomic(path: &Path, doc: &Value) -> Result<(), AdapterError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }
    let bytes = serde_json::to_vec_pretty(doc)?;
    let tmp = tmp_path_for(path);
    fs::write(&tmp, &bytes).await?;
    let file = fs::OpenOptions::new().write(true).open(&tmp).await?;
    file.sync_all().await?;
    drop(file);
    fs::rename(&tmp, path).await?;
    Ok(())
}

fn tmp_path_for(path: &Path) -> PathBuf {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    PathBuf::from(tmp)
}

/// OS-native default config path. Independent of [`BaseDirs::config_dir`]
/// only insofar as the Claude app name is hardcoded.
fn default_config_path() -> PathBuf {
    BaseDirs::new().map_or_else(
        || PathBuf::from("claude_desktop_config.json"),
        |d| {
            d.config_dir()
                .join("Claude")
                .join("claude_desktop_config.json")
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::DisplayName;

    fn sample_entry(lid: &str, sentinel: Sentinel) -> AdapterEntry {
        AdapterEntry {
            logical_id: LogicalId::new(lid).unwrap(),
            display_name: DisplayName::new("BodyLog").unwrap(),
            url: format!("http://127.0.0.1:8766/{lid}?key=ABCDEF"),
            sentinel,
        }
    }

    #[tokio::test]
    async fn detect_on_missing_dir_returns_none() {
        let tmp = tempfile::tempdir().unwrap();
        // Point at a deep path whose parents do not yet exist.
        let path = tmp.path().join("does/not/exist/claude_desktop_config.json");
        let adapter = ClaudeDesktopAdapter::with_config_path(path);
        assert!(adapter.detect().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn detect_when_parent_dir_exists_returns_some() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("claude_desktop_config.json");
        let adapter = ClaudeDesktopAdapter::with_config_path(path.clone());
        let detected = adapter.detect().await.unwrap().expect("detected");
        assert_eq!(detected.config_path, path);
    }

    #[tokio::test]
    async fn write_creates_file_with_expected_shape() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("claude_desktop_config.json");
        let adapter = ClaudeDesktopAdapter::with_config_path(path.clone());

        let sentinel = Sentinel::random();
        adapter
            .write_entry(&sample_entry("bodylog", sentinel))
            .await
            .unwrap();

        let json: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(
            json["mcpServers"]["bodylog"]["url"],
            "http://127.0.0.1:8766/bodylog?key=ABCDEF"
        );
        assert_eq!(
            json["mcpServers"]["bodylog"]["_mcp_bridge_managed"],
            sentinel.to_string()
        );
    }

    #[tokio::test]
    async fn write_preserves_unrelated_top_level_keys() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("claude_desktop_config.json");
        std::fs::write(
            &path,
            br#"{"someUserSetting": "hello", "mcpServers": {"userServer": {"command": "node"}}}"#,
        )
        .unwrap();

        let adapter = ClaudeDesktopAdapter::with_config_path(path.clone());
        let sentinel = Sentinel::random();
        adapter
            .write_entry(&sample_entry("bodylog", sentinel))
            .await
            .unwrap();

        let json: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(json["someUserSetting"], "hello");
        assert_eq!(json["mcpServers"]["userServer"]["command"], "node");
        assert_eq!(
            json["mcpServers"]["bodylog"]["_mcp_bridge_managed"],
            sentinel.to_string()
        );
    }

    #[tokio::test]
    async fn write_overwrites_existing_managed_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("claude_desktop_config.json");
        let adapter = ClaudeDesktopAdapter::with_config_path(path.clone());
        let sentinel = Sentinel::random();
        adapter
            .write_entry(&sample_entry("bodylog", sentinel))
            .await
            .unwrap();

        let mut updated = sample_entry("bodylog", sentinel);
        updated.url = "http://127.0.0.1:9999/bodylog?key=XYZ".to_owned();
        adapter.write_entry(&updated).await.unwrap();

        let json: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(
            json["mcpServers"]["bodylog"]["url"],
            "http://127.0.0.1:9999/bodylog?key=XYZ"
        );
    }

    #[tokio::test]
    async fn remove_only_touches_matching_sentinel() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("claude_desktop_config.json");
        let adapter = ClaudeDesktopAdapter::with_config_path(path.clone());

        let our = Sentinel::random();
        adapter
            .write_entry(&sample_entry("bodylog", our))
            .await
            .unwrap();

        // User adds an entry with the SAME logical_id name but no sentinel.
        // (Pretend the user revoked the pair then wrote a manual entry
        // happening to reuse the name.) We must NOT delete it.
        let mut doc: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        doc["mcpServers"]["userOnly"] = serde_json::json!({"command": "node"});
        std::fs::write(&path, serde_json::to_vec_pretty(&doc).unwrap()).unwrap();

        // Remove our entry — userOnly survives.
        adapter
            .remove_entry(&LogicalId::new("bodylog").unwrap(), our)
            .await
            .unwrap();
        let json: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert!(json["mcpServers"].get("bodylog").is_none());
        assert_eq!(json["mcpServers"]["userOnly"]["command"], "node");
    }

    #[tokio::test]
    async fn remove_with_wrong_sentinel_leaves_entry_alone() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("claude_desktop_config.json");
        let adapter = ClaudeDesktopAdapter::with_config_path(path.clone());

        let our = Sentinel::random();
        let stranger = Sentinel::random();
        adapter
            .write_entry(&sample_entry("bodylog", our))
            .await
            .unwrap();

        // Attempt to remove with a different install's sentinel.
        adapter
            .remove_entry(&LogicalId::new("bodylog").unwrap(), stranger)
            .await
            .unwrap();

        let json: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(
            json["mcpServers"]["bodylog"]["_mcp_bridge_managed"],
            our.to_string()
        );
    }

    #[tokio::test]
    async fn remove_on_missing_file_is_ok() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("never-existed.json");
        let adapter = ClaudeDesktopAdapter::with_config_path(path);
        adapter
            .remove_entry(&LogicalId::new("anything").unwrap(), Sentinel::random())
            .await
            .unwrap();
    }
}

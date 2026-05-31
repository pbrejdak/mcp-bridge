//! Per-platform launch-unit installation: register the daemon with
//! the OS so it starts at user login and respawns on crash.
//!
//! macOS: a launchd plist under `~/Library/LaunchAgents/`.
//! Linux: systemd user unit (deferred to a follow-up commit).
//! Windows: Scheduled Task (deferred to a follow-up commit).
//!
//! Each platform implementation is responsible for two operations —
//! `install` and `uninstall` — and must be idempotent on both. Install
//! over an existing install replaces the unit; uninstall when nothing
//! is registered is a silent no-op.

use std::path::PathBuf;

use thiserror::Error;

use crate::config::Config;

/// Install or refresh the daemon's launch unit so it starts at user
/// login.
pub fn install(config: &Config) -> Result<InstallReport, InstallError> {
    #[cfg(target_os = "macos")]
    {
        macos::install(config)
    }
    #[cfg(target_os = "linux")]
    {
        linux::install(config)
    }
    #[cfg(target_os = "windows")]
    {
        windows::install(config)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        let _ = config;
        Err(InstallError::Unsupported)
    }
}

/// Reverse [`install`]. Idempotent — succeeds even if no unit is
/// currently registered.
pub fn uninstall(config: &Config) -> Result<UninstallReport, InstallError> {
    #[cfg(target_os = "macos")]
    {
        macos::uninstall(config)
    }
    #[cfg(target_os = "linux")]
    {
        linux::uninstall(config)
    }
    #[cfg(target_os = "windows")]
    {
        windows::uninstall(config)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        let _ = config;
        Err(InstallError::Unsupported)
    }
}

/// Summary of what changed on a successful install.
#[derive(Debug, Clone)]
pub struct InstallReport {
    /// Filesystem path to the launch unit definition.
    pub unit_path: PathBuf,
    /// `true` when the unit file already existed and was overwritten
    /// (reinstall); `false` for first-time install.
    pub replaced_existing: bool,
}

/// Summary of what changed on a successful uninstall.
#[derive(Debug, Clone)]
pub struct UninstallReport {
    /// Filesystem path to the launch unit that was removed (or that
    /// would have been removed if present).
    pub unit_path: PathBuf,
    /// `true` when the unit file existed and was deleted; `false` when
    /// nothing was registered (silent no-op).
    pub removed: bool,
}

/// Failure modes for [`install`] / [`uninstall`].
#[derive(Debug, Error)]
pub enum InstallError {
    #[error("install/uninstall is not yet implemented on this platform")]
    Unsupported,
    #[error("could not resolve the running executable path: {0}")]
    CurrentExe(std::io::Error),
    #[error("could not resolve the user's home directory")]
    NoHomeDir,
    #[error("could not write launch unit at {path:?}: {source}")]
    WriteUnit {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not remove launch unit at {path:?}: {source}")]
    RemoveUnit {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not invoke `{cmd}`: {source}")]
    SpawnLaunchTool {
        cmd: String,
        #[source]
        source: std::io::Error,
    },
    #[error("`{cmd}` exited with status {status}: {stderr}")]
    LaunchToolFailed {
        cmd: String,
        status: i32,
        stderr: String,
    },
}

// ---------------------------------------------------------------------
// macOS — launchd plist under ~/Library/LaunchAgents/
// ---------------------------------------------------------------------

#[cfg(target_os = "macos")]
mod macos {
    use std::path::PathBuf;
    use std::process::Command;

    use directories::BaseDirs;

    use crate::config::Config;

    use super::{InstallError, InstallReport, UninstallReport};

    /// Launchd label / bundle-ID. Matches the keychain service name.
    const LABEL: &str = "dev.mcpbridge.mcp-bridged";

    pub(super) fn install(config: &Config) -> Result<InstallReport, InstallError> {
        let plist_path = plist_path()?;
        let binary = std::env::current_exe().map_err(InstallError::CurrentExe)?;
        let log_dir = config.data_dir.join("logs");

        // Make sure the log directory exists so launchd can redirect
        // stdout/stderr into it.
        if let Err(e) = std::fs::create_dir_all(&log_dir) {
            return Err(InstallError::WriteUnit {
                path: log_dir,
                source: e,
            });
        }

        let plist = render_plist(
            &binary,
            &log_dir.join("mcp-bridged.out.log"),
            &log_dir.join("mcp-bridged.err.log"),
        );

        // Ensure ~/Library/LaunchAgents exists. On a fresh user account
        // it sometimes doesn't.
        if let Some(parent) = plist_path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                return Err(InstallError::WriteUnit {
                    path: parent.to_path_buf(),
                    source: e,
                });
            }
        }

        let replaced_existing = plist_path.exists();

        // Reload-safety: unload the existing unit first (ignore errors —
        // it may not be loaded). Then write the file, then load.
        if replaced_existing {
            let _ = run_launchctl(&["unload", &plist_path.to_string_lossy()]);
        }
        std::fs::write(&plist_path, plist).map_err(|e| InstallError::WriteUnit {
            path: plist_path.clone(),
            source: e,
        })?;
        run_launchctl(&["load", "-w", &plist_path.to_string_lossy()])?;

        Ok(InstallReport {
            unit_path: plist_path,
            replaced_existing,
        })
    }

    pub(super) fn uninstall(_config: &Config) -> Result<UninstallReport, InstallError> {
        let plist_path = plist_path()?;
        if !plist_path.exists() {
            return Ok(UninstallReport {
                unit_path: plist_path,
                removed: false,
            });
        }

        // Best-effort unload; ignore errors because the agent may not
        // currently be loaded (manual unload, prior crash, etc.).
        let _ = run_launchctl(&["unload", "-w", &plist_path.to_string_lossy()]);
        std::fs::remove_file(&plist_path).map_err(|e| InstallError::RemoveUnit {
            path: plist_path.clone(),
            source: e,
        })?;

        Ok(UninstallReport {
            unit_path: plist_path,
            removed: true,
        })
    }

    fn plist_path() -> Result<PathBuf, InstallError> {
        let dirs = BaseDirs::new().ok_or(InstallError::NoHomeDir)?;
        Ok(dirs
            .home_dir()
            .join("Library")
            .join("LaunchAgents")
            .join(format!("{LABEL}.plist")))
    }

    fn run_launchctl(args: &[&str]) -> Result<(), InstallError> {
        let output = Command::new("launchctl")
            .args(args)
            .output()
            .map_err(|e| InstallError::SpawnLaunchTool {
                cmd: format!("launchctl {}", args.join(" ")),
                source: e,
            })?;
        if !output.status.success() {
            return Err(InstallError::LaunchToolFailed {
                cmd: format!("launchctl {}", args.join(" ")),
                status: output.status.code().unwrap_or(-1),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            });
        }
        Ok(())
    }

    pub(super) fn render_plist(
        binary: &std::path::Path,
        stdout_path: &std::path::Path,
        stderr_path: &std::path::Path,
    ) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{LABEL}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{}</string>
        <string>daemon</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>ProcessType</key>
    <string>Background</string>
    <key>StandardOutPath</key>
    <string>{}</string>
    <key>StandardErrorPath</key>
    <string>{}</string>
</dict>
</plist>
"#,
            xml_escape(&binary.to_string_lossy()),
            xml_escape(&stdout_path.to_string_lossy()),
            xml_escape(&stderr_path.to_string_lossy()),
        )
    }

    /// XML-escape the five reserved characters. Plist files are XML, so
    /// any binary or path containing `<`, `>`, `&`, `'`, `"` would
    /// produce a malformed plist if dropped in raw.
    fn xml_escape(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        for c in s.chars() {
            match c {
                '<' => out.push_str("&lt;"),
                '>' => out.push_str("&gt;"),
                '&' => out.push_str("&amp;"),
                '\'' => out.push_str("&apos;"),
                '"' => out.push_str("&quot;"),
                _ => out.push(c),
            }
        }
        out
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::path::PathBuf;

        #[test]
        fn rendered_plist_has_expected_structure() {
            let plist = render_plist(
                &PathBuf::from("/usr/local/bin/mcp-bridge"),
                &PathBuf::from("/Users/me/Library/Application Support/mcp-bridged/logs/out.log"),
                &PathBuf::from("/Users/me/Library/Application Support/mcp-bridged/logs/err.log"),
            );
            assert!(plist.starts_with("<?xml version=\"1.0\""));
            assert!(plist.contains("<key>Label</key>"));
            assert!(plist.contains("<string>dev.mcpbridge.mcp-bridged</string>"));
            assert!(plist.contains("<string>/usr/local/bin/mcp-bridge</string>"));
            assert!(plist.contains("<string>daemon</string>"));
            assert!(plist.contains("<key>RunAtLoad</key>"));
            assert!(plist.contains("<key>KeepAlive</key>"));
            assert!(plist.ends_with("</plist>\n"));
        }

        #[test]
        fn xml_escape_handles_all_five_reserved_chars() {
            assert_eq!(xml_escape("<&>'\""), "&lt;&amp;&gt;&apos;&quot;");
        }

        #[test]
        fn rendered_plist_escapes_xml_reserved_chars_in_paths() {
            // Some macOS users have apostrophes or ampersands in their
            // home directory name; a naive renderer would produce
            // malformed plists.
            let plist = render_plist(
                &PathBuf::from("/Users/Patryk's Mac/bin/mcp-bridge"),
                &PathBuf::from("/tmp/out & err.log"),
                &PathBuf::from("/tmp/err.log"),
            );
            assert!(plist.contains("Patryk&apos;s Mac"));
            assert!(plist.contains("out &amp; err.log"));
        }
    }
}

// ---------------------------------------------------------------------
// Linux — systemd user unit under ~/.config/systemd/user/
// ---------------------------------------------------------------------

#[cfg(target_os = "linux")]
mod linux {
    use std::path::PathBuf;
    use std::process::Command;

    use directories::BaseDirs;

    use crate::config::Config;

    use super::{InstallError, InstallReport, UninstallReport};

    /// Systemd unit name. Must match the keychain service name minus
    /// the bundle-ID dotted form; systemd is happy with the dot-form
    /// directly as the unit name.
    const UNIT_NAME: &str = "mcp-bridged";

    pub(super) fn install(config: &Config) -> Result<InstallReport, InstallError> {
        let unit_path = unit_path()?;
        let binary = std::env::current_exe().map_err(InstallError::CurrentExe)?;
        let log_dir = config.data_dir.join("logs");

        if let Err(e) = std::fs::create_dir_all(&log_dir) {
            return Err(InstallError::WriteUnit {
                path: log_dir,
                source: e,
            });
        }
        if let Some(parent) = unit_path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                return Err(InstallError::WriteUnit {
                    path: parent.to_path_buf(),
                    source: e,
                });
            }
        }

        let unit_contents = render_unit(
            &binary,
            &log_dir.join("mcp-bridged.out.log"),
            &log_dir.join("mcp-bridged.err.log"),
        );

        let replaced_existing = unit_path.exists();

        std::fs::write(&unit_path, unit_contents).map_err(|e| InstallError::WriteUnit {
            path: unit_path.clone(),
            source: e,
        })?;

        // systemd needs daemon-reload after a unit file change.
        run_systemctl(&["--user", "daemon-reload"])?;
        // enable --now persists at login AND starts the unit immediately.
        run_systemctl(&["--user", "enable", "--now", &service_name()])?;

        Ok(InstallReport {
            unit_path,
            replaced_existing,
        })
    }

    pub(super) fn uninstall(_config: &Config) -> Result<UninstallReport, InstallError> {
        let unit_path = unit_path()?;
        if !unit_path.exists() {
            return Ok(UninstallReport {
                unit_path,
                removed: false,
            });
        }

        // Best-effort disable + stop. Both can fail if the unit isn't
        // currently active or enabled; ignore those errors.
        let _ = run_systemctl(&["--user", "disable", "--now", &service_name()]);

        std::fs::remove_file(&unit_path).map_err(|e| InstallError::RemoveUnit {
            path: unit_path.clone(),
            source: e,
        })?;

        // Reload so systemd forgets the gone unit.
        let _ = run_systemctl(&["--user", "daemon-reload"]);

        Ok(UninstallReport {
            unit_path,
            removed: true,
        })
    }

    fn unit_path() -> Result<PathBuf, InstallError> {
        let dirs = BaseDirs::new().ok_or(InstallError::NoHomeDir)?;
        // ~/.config/systemd/user is the conventional location;
        // XDG_CONFIG_HOME overrides when set, which BaseDirs honors.
        Ok(dirs
            .config_dir()
            .join("systemd")
            .join("user")
            .join(service_name()))
    }

    fn service_name() -> String {
        format!("{UNIT_NAME}.service")
    }

    fn run_systemctl(args: &[&str]) -> Result<(), InstallError> {
        let output = Command::new("systemctl")
            .args(args)
            .output()
            .map_err(|e| InstallError::SpawnLaunchTool {
                cmd: format!("systemctl {}", args.join(" ")),
                source: e,
            })?;
        if !output.status.success() {
            return Err(InstallError::LaunchToolFailed {
                cmd: format!("systemctl {}", args.join(" ")),
                status: output.status.code().unwrap_or(-1),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            });
        }
        Ok(())
    }

    pub(super) fn render_unit(
        binary: &std::path::Path,
        stdout_path: &std::path::Path,
        stderr_path: &std::path::Path,
    ) -> String {
        // systemd unit syntax is INI-flavoured with backslash-escape
        // semantics in [Service] string values. Escape backslashes,
        // newlines, and quote chars that would break shell parsing.
        format!(
            "[Unit]\n\
             Description=MCP Bridge daemon (Stable-Loopback Bridge)\n\
             Documentation=https://github.com/mcp-bridge/mcp-bridge\n\
             After=network.target\n\
             \n\
             [Service]\n\
             Type=simple\n\
             ExecStart={binary} daemon\n\
             Restart=on-failure\n\
             RestartSec=5\n\
             StandardOutput=append:{stdout}\n\
             StandardError=append:{stderr}\n\
             \n\
             [Install]\n\
             WantedBy=default.target\n",
            binary = systemd_escape(&binary.to_string_lossy()),
            stdout = systemd_escape(&stdout_path.to_string_lossy()),
            stderr = systemd_escape(&stderr_path.to_string_lossy()),
        )
    }

    /// Escape a path for use in a `Key=Value` line. systemd treats `\`
    /// as an escape introducer in unit-file string fields; literal `\`
    /// must be doubled. Paths with whitespace or `=` work without
    /// quoting in systemd unit files (no shell parsing applied to
    /// Exec lines beyond systemd's own splitter, and the splitter
    /// respects quoting). Newlines would terminate the value, so we
    /// reject them implicitly by escaping.
    fn systemd_escape(s: &str) -> String {
        s.replace('\\', "\\\\").replace('\n', "\\n")
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::path::PathBuf;

        #[test]
        fn rendered_unit_has_expected_sections() {
            let unit = render_unit(
                &PathBuf::from("/usr/local/bin/mcp-bridge"),
                &PathBuf::from("/var/log/mcp-bridged.out.log"),
                &PathBuf::from("/var/log/mcp-bridged.err.log"),
            );
            assert!(unit.contains("[Unit]"));
            assert!(unit.contains("[Service]"));
            assert!(unit.contains("[Install]"));
            assert!(unit.contains("ExecStart=/usr/local/bin/mcp-bridge daemon"));
            assert!(unit.contains("Restart=on-failure"));
            assert!(unit.contains("WantedBy=default.target"));
            assert!(unit.contains(
                "StandardOutput=append:/var/log/mcp-bridged.out.log"
            ));
        }

        #[test]
        fn systemd_escape_doubles_backslashes() {
            assert_eq!(systemd_escape("/tmp/a\\b"), "/tmp/a\\\\b");
        }

        #[test]
        fn service_name_matches_unit_name_constant() {
            assert_eq!(service_name(), "mcp-bridged.service");
        }
    }
}

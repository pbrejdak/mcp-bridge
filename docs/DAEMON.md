# `mcp-bridged` — Native Daemon

Status: exploratory, current revision 2026-05-23. Companion to [`ARCHITECTURE.md`](ARCHITECTURE.md) (cross-cutting design) and [`UI.md`](UI.md) (Bridge Console). This document covers the Rust daemon's internals: process model, module layout, IPC surface, request flow, persistence, observability.

The Resolver from [`ARCHITECTURE.md §3`](ARCHITECTURE.md) runs as two cooperating processes. `mcp-bridged` is the always-on, no-UI half — everything that the user's clients depend on continuously, sized to live in the background unobtrusively. Bridge Console is the on-demand UI half ([`UI.md`](UI.md)).

---

## 1. Position and responsibilities

`mcp-bridged` owns everything in [`ARCHITECTURE.md §3.1`](ARCHITECTURE.md) except Bridge Console. Concretely:

- Loopback Listener (Consumer-facing HTTP)
- Origin Connector(s) (backend-facing TLS-pinned MCP clients)
- Server Registry (persistent state)
- Discovery Agent (Bonjour + pair endpoint)
- Client Adapters (config writers)
- Identity Keystore (OS keychain bridge)
- IPC surface for UI and CLI
- Update checker

It runs as the logged-in user. No root, no admin, no SYSTEM. Loopback and pair-endpoint binds are unprivileged ports.

---

## 2. Process model per platform

| Platform | Mechanism | Path |
|---|---|---|
| **macOS** | LaunchAgent (user-scoped) | `~/Library/LaunchAgents/dev.mcpbridge.daemon.plist` |
| **Linux** | systemd user unit | `~/.config/systemd/user/mcp-bridged.service` |
| **Windows** | Scheduled Task at user logon | `\MCP Bridge\mcp-bridged` |

Why not LaunchDaemon / Windows Service: those run as root / SYSTEM and pre-date login. The daemon needs the user's OS keychain (login keychain on macOS is the only sane place for the Resolver's private key), so it must run *as* the user, *after* login.

**Lifecycle**:

- **Install** — Bridge Console first-launch wizard registers the platform unit and starts the daemon. Asks for Local Network permission grant (macOS 15+) before the daemon's first Bonjour bind.
- **Auto-start** — at every login.
- **Auto-restart** — on crash, via the platform's supervision (`KeepAlive` on launchd, `Restart=on-failure` on systemd, "Restart on failure" on Scheduled Task).
- **Stop** — tray-quit signals SIGTERM via IPC; graceful shutdown sequence (§11).
- **Uninstall** — see §2.1 below for the two-mode uninstall.

### 2.1 Uninstall completeness

Two uninstall modes. Both preserve the daemon binary so reinstall does not require redownload.

**Soft uninstall** — `mcp-bridge daemon --uninstall`:

- Unregisters the platform unit (LaunchAgent / systemd unit / Scheduled Task)
- Stops the daemon
- Leaves the registry, keychain entries, logs, and adapter-written Consumer config entries in place

Use case: temporary uninstall; reinstall picks up where it left off.

**Hard uninstall** — `mcp-bridge uninstall --purge`:

Removes every artefact attributable to MCP Bridge. The complete list (auditable claim — anything missing here is a bug):

- Platform unit (LaunchAgent / systemd unit / Scheduled Task)
- Registry file + backups + settings file
- All rolling log files
- All keychain entries with prefix `dev.mcpbridge.*` (identity keypair, per-pin tokens, per-Consumer loopback keys, pinned Origin pubkeys)
- All adapter-written entries in Consumer configs, matched by `_mcp_bridge_managed` UUID tag (Claude Desktop, Cursor, Continue — every Adapter contributes its own removal step)
- Tauri webview persistent state if any (`~/Library/Application Support/MCPBridge/WebView*`)
- Tauri Single-Instance lock files
- Auto-update staging directory
- Cached SF Symbol renders (macOS Tauri side)
- Persisted UI preferences if `tauri-plugin-store` is used

After `--purge` and binary deletion, no data attributable to MCP Bridge remains on the user's machine. This is the auditable "right to erasure" — see [`PRIVACY.md`](PRIVACY.md) §7. The user can verify by filesystem scan, keychain inspection (`security dump-keychain | grep mcpbridge`), and Consumer config inspection.

---

## 3. Module layout

```
mcp-bridged/
├── src/
│   ├── main.rs                  — entry, CLI arg parse, supervisor
│   ├── supervisor.rs            — task tree + restart policy
│   ├── config.rs                — runtime config
│   ├── ipc/
│   │   ├── server.rs            — UDS / named-pipe listener
│   │   ├── methods.rs           — JSON-RPC method handlers (see §5)
│   │   └── events.rs            — pub/sub event broadcaster
│   ├── pair/
│   │   ├── invite.rs            — Resolver invite (QR payload, nonce lifecycle)
│   │   ├── payload.rs           — sealed payload acceptance + validation
│   │   └── sas.rs               — SAS phrase derivation from pubkey + nonce
│   ├── announce/
│   │   ├── bonjour.rs           — mDNS subscriber + sealed-TXT decoder
│   │   ├── unicast.rs           — HTTP POST announce endpoint
│   │   └── verify.rs            — sig + seq + exp + rate-limit
│   ├── registry/
│   │   ├── store.rs             — atomic JSON read/write + migrations
│   │   └── pin.rs               — Server Pin types
│   ├── keystore/                — OS keychain wrapper
│   ├── identity/                — Resolver Ed25519 keypair lifecycle
│   ├── proxy/
│   │   ├── listener.rs          — HTTP server on 127.0.0.1
│   │   ├── connector.rs         — outbound TLS-pinned MCP client
│   │   ├── route.rs             — path → pin lookup, key + Host validation
│   │   └── stream.rs            — SSE pass-through
│   ├── adapters/
│   │   ├── claude_desktop.rs
│   │   ├── cursor.rs
│   │   ├── continue_dev.rs
│   │   └── common.rs            — atomic write, tagged entries, schema probe
│   ├── update/                  — manifest fetch + signature verify
│   ├── observability/           — tracing layers, redaction, rotation
│   └── lib.rs                   — public crate API (for integration tests + future embed)
├── tests/                       — integration tests
├── test-vectors/                — mcp-pair / mcp-announce conformance JSON
└── Cargo.toml
```

Each subsystem is testable in isolation. `lib.rs` exposes the daemon as a library so integration tests can spawn an in-process instance with a mock Origin.

---

## 4. Concurrency model

Single tokio multi-threaded runtime. Each subsystem runs as one or more tasks under a **supervisor** that handles restart policy:

```
supervisor (root)
├── ipc::server               — accepts UDS / pipe clients
├── proxy::listener           — Loopback Listener (one accept loop)
├── pair::endpoint            — LAN pair-POST HTTPS listener
├── announce::bonjour         — mDNS subscriber
├── announce::unicast         — pair-port shared with pair::endpoint
├── update::checker           — daily timer
└── per-pin Origin Connector  — spawned on first request, idle-closed after 5 min
```

**Restart policy**: critical subsystems (`proxy::listener`, `ipc::server`) restart with exponential backoff on panic. Non-critical (`update::checker`, `announce::bonjour`) restart with a longer backoff. Three consecutive failures triggers a structured error event to Bridge Console and a "Restart Bridge" UI affordance — we don't want to loop forever masking a real bug.

**Cancellation**: every long-lived task takes a `tokio_util::sync::CancellationToken`. Graceful shutdown signals the root token, which propagates.

**Backpressure**: the Loopback Listener uses `tower::limit::ConcurrencyLimit` to cap inflight requests per pin (default 32). Activity events to Bridge Console use a bounded `tokio::sync::broadcast` channel — slow UI clients drop events rather than blocking the request path.

---

## 5. IPC surface

UDS on macOS / Linux at `~/.local/share/mcp-bridge/control.sock` (mode 0600, user-owned). Named pipe on Windows at `\\.\pipe\MCPBridge-<user-sid>` with ACL restricting to the owning user.

Wire format: **JSON-RPC 2.0** over a length-prefixed framing (4-byte big-endian payload length + JSON bytes). Bi-directional — daemon can push events as JSON-RPC notifications.

Authentication: OS file permission / pipe ACL is the boundary. Only the owning user can connect; no application-level auth needed.

### 5.1 Methods (caller → daemon)

```
# Pairing
pair.invite_start()                         → { token, qr_payload, sas, display_name, expires_at }
pair.invite_cancel(token)                   → {}
pair.confirm(token, consumers: [{name, allowed_tools}])
                                            → { pin_id }

# Servers
servers.list()                              → [{ pin_id, name, state, consumers, last_activity_at }]
servers.detail(pin_id)                      → { ...full pin sans secrets }
servers.rename(pin_id, name)                → {}
servers.revoke(pin_id, consumer?)           → {}
servers.update_acl(pin_id, consumer, allowed_tools)
                                            → {}

# Adapters
adapters.scan()                             → [{ name, detected, version, config_path }]
adapters.repair(pin_id, consumer)           → {}    # rewrite if config drifted

# Settings
settings.get()                              → { verbose_logging, update_channel, ... }
settings.set(patch)                         → {}

# Identity
identity.rotate()                           → {}    # wipes Resolver keypair; existing pins invalidated
identity.display_name()                     → { name }
identity.set_display_name(name)             → {}

# Diagnostics
diagnostics.bundle()                        → { redacted_text }
log.set_verbose(enabled, reason, duration_sec=3600)
                                            → { expires_at }   # auto-reverts after duration
log.subscribe()                             → ()    # opens streaming events

# Update
update.check()                              → { current, latest, available }
update.apply()                              → {}    # stages, applies on next launch

# Daemon control
daemon.status()                             → { uptime, version, tasks: [...] }
daemon.shutdown()                           → {}
```

### 5.2 Events (daemon → caller)

```
pair.invite_displayed       { token, sas, display_name, expires_at }
pair.payload_received       { token, origin_name, origin_pubkey_fp }
pair.installed              { pin_id }
pair.failed                 { token, reason }

server.state_changed        { pin_id, state }
server.activity             { pin_id, consumer, method, status, duration_ms }
                              # body redacted unless verbose

announce.received           { pin_id, fp_changed, auth_rotated }
announce.rejected           { reason, source_ip }       # rate-limit hits

adapter.config_drift        { pin_id, consumer, suggestion }
update.available            { version, notes_url }
log.entry                   { level, target, message }  # streaming
daemon.shutting_down        { reason }
```

The CLI uses the same surface — `mcp-bridge list` calls `servers.list`, `mcp-bridge pair <token>` drives the pair flow via events. CLI parity from [`ARCHITECTURE.md §10`](ARCHITECTURE.md) resolved for free.

---

## 6. MCP proxy request flow

```
Consumer
   │  POST http://127.0.0.1:8765/bodylog?key=<token>
   │  Host: 127.0.0.1:8765
   │  body: JSON-RPC tools/call
   ▼
┌──────────────────────────────────────────────────┐
│ proxy::listener                                  │
│ 1. validate Host: matches loopback               │ → else 421
│ 2. parse path → logical_id "bodylog"              │ → else 404
│ 3. constant-time compare ?key against pin's      │
│    Consumer keys, identify which Consumer        │ → else 401
│ 4. if request is tools/call, check allowed_tools │ → else 403
│ 5. open per-request span (logical_id, consumer)  │
└────────┬─────────────────────────────────────────┘
         │
         ▼
┌──────────────────────────────────────────────────┐
│ proxy::route → Server Pin "bodylog"               │
│ 6. fetch backend, fp, auth from in-memory cache  │
│ 7. select or spawn Origin Connector for this pin │
└────────┬─────────────────────────────────────────┘
         │
         ▼
┌──────────────────────────────────────────────────┐
│ proxy::connector  (per-pin, keep-warm)           │
│ 8. if no warm conn: rustls dial to backend.url   │
│    with cert verifier pinning backend.fp         │
│ 9. attach Authorization: Bearer <token-from-     │
│    keychain>                                     │
│ 10. forward request                              │
│ 11. stream response (HTTP + SSE) back            │
│ 12. on backend fail: 503 with                    │
│     X-MCP-Bridge-Reason header                   │
└────────┬─────────────────────────────────────────┘
         │
         ▼
┌──────────────────────────────────────────────────┐
│ observability                                    │
│ 13. log request (path, method, status, duration) │
│ 14. broadcast server.activity event              │
└──────────────────────────────────────────────────┘
```

**SSE pass-through**: the connector and listener both speak streaming. The connector returns a `Stream<Item = Bytes>` of the backend's SSE chunks; the listener writes them straight to the Consumer with no buffering. Backend sends `event: tool_progress`, Consumer sees it within milliseconds.

**Connection pooling**: per-pin `hyper::Client` with `pool_idle_timeout(Duration::from_secs(300))` and `pool_max_idle_per_host(4)`. Closed and rebuilt on `auth_rotated_at` or `cert_rotated_at` change.

---

## 7. Persistence

### 7.1 Server Registry

`registry.json` at the OS-appropriate data dir:

- macOS: `~/Library/Application Support/MCPBridge/registry.json`
- Linux: `~/.local/share/mcp-bridge/registry.json`
- Windows: `%LOCALAPPDATA%\MCPBridge\registry.json`

File mode 0600 (POSIX) / per-user ACL (Windows).

Schema:

```jsonc
{
  "v": 1,
  "resolver": {
    "display_name": "Patryk's MacBook Pro",
    "pubkey_id": "kc://bridge.identity.pubkey"   // pointer into keychain
  },
  "pins": [
    { /* Server Pin per ARCHITECTURE.md §3.1 */ }
  ],
  "settings": {
    "verbose_logging": false,
    "update_channel": "stable",
    "loopback_port": 8765
  }
}
```

**Atomic writes**: open `registry.json.tmp`, write, `fsync`, `rename` over the target. Rotate `registry.json.bak.{1,2,3}` on every successful write. Migration: on read, if `v` is older, apply migration functions in sequence and rewrite.

### 7.2 Identity Keystore

Wrapper over the `keyring` crate with platform-specific key names:

| Key | Contents |
|---|---|
| `dev.mcpbridge.identity.privkey` | Resolver's Ed25519 private key (32 bytes) |
| `dev.mcpbridge.identity.pubkey` | Resolver's pubkey (32 bytes) — duplicated for fast read |
| `dev.mcpbridge.pin.<pin_id>.token` | Bearer token for that Origin |
| `dev.mcpbridge.pin.<pin_id>.consumer.<name>.key` | Loopback `?key=` for that Consumer |
| `dev.mcpbridge.pin.<pin_id>.origin_pubkey` | Pinned Origin pubkey |

On macOS the daemon adds itself to the keychain ACL on first write so subsequent reads don't prompt. First-write triggers the standard "MCPBridge wants to access the keychain" prompt — surfaced clearly during install.

### 7.3 Memory hygiene

Sensitive material in process memory must not survive past need:

- Every type carrying a private key, bearer token, loopback key, or pubkey handle derives `Zeroize` and `ZeroizeOnDrop` (from the `zeroize` and `zeroize-derive` crates). On `Drop` the memory is overwritten before deallocation.
- **Core dumps disabled at daemon startup**: `setrlimit(RLIMIT_CORE, 0, 0)` on Unix, `SetErrorMode(SEM_NOGPFAULTERRORBOX)` on Windows. A panicking or crashing daemon will not write its address space to disk for later forensic recovery.
- Tokio task locals are flushed on cancellation.
- The Resolver private key never appears in the registry on disk — only its keychain reference (see §7.1).
- The Identity Keystore returns short-lived `Secret<T>` handles via the `secrecy` crate so callers can't accidentally `Clone` them around the codebase.

### 7.4 Backup and indexing exclusions

The data directory is configured to opt out of common backup and indexing tools so that the registry and logs do not silently appear in cloud-sync, Time Machine snapshots, or system search indexes:

- **macOS** — extended attribute `com.apple.metadata:com_apple_backup_excludeItem` set on the data directory (Time Machine skips); `.metadata_never_index` file present (Spotlight skips).
- **Linux** — `nodump` chattr where supported; `.directory` flag honored by Tracker / Baloo.
- **Windows** — `FILE_ATTRIBUTE_NOT_CONTENT_INDEXED` on the data folder.

The user can override these if they want their pins synced across machines, but the default is "don't leak."

---

## 8. Observability

`tracing` + `tracing-subscriber` layers:

```
RootSubscriber
├── JSON file writer  → ~/Library/Logs/MCPBridge/mcp-bridged.log.{N}
│   ├── level filter (default INFO, verbose DEBUG)
│   ├── redaction layer (see §8.2 for full field list)
│   └── rolling appender (5 files × 2 MB)
├── IPC events       → forwarded to subscribers of log.subscribe()
└── stderr (dev only)
```

**Per-request span**:

```
INFO request{pin=bodylog consumer=claude-desktop method=tools/call}: 200 in 47ms
```

When verbose is on, request and response bodies join the span as structured fields, still redacted for `Authorization`.

**Activity event** (separate from log): emitted to Bridge Console for the activity panel. Independent path — even if file logging is off, the UI activity feed still works.

**Diagnostics bundle**: `diagnostics.bundle()` produces a redacted text snapshot:

- Last 100 log lines
- Daemon version + commit
- OS + arch
- List of pins (names + states only, no secrets)
- Adapter scan output
- Last update check result

Re-redacted at bundle time (defense in depth) before returning to the UI for copy-to-clipboard.

### 8.2 Redaction policy

Applied at the log layer for routine logging and at bundle-creation time for diagnostics export. The same redactor implementation runs in both places.

| Field | Default logs | Diagnostic bundle |
|---|---|---|
| `Authorization` headers | redacted | redacted |
| Loopback `?key=` parameter (URL + headers) | redacted | redacted |
| Request bodies | not captured | not captured (verbose-only) |
| Response bodies | not captured | not captured (verbose-only) |
| Home-directory paths (`/Users/<name>`, `/home/<name>`, `C:\Users\<name>`) | replaced with `~` | replaced with `~` |
| Hostnames (`<machine>.local`) | redacted | replaced with `[host]` |
| IP addresses (LAN + WAN) | logged | hashed with per-bundle salt (correlatable within the bundle, not across) |
| MAC addresses | not logged | redacted if present |
| Email addresses | not logged | redacted if present |
| Server display names | logged | optionally redacted (one-tap toggle in export UI) |
| Origin pubkey fingerprints | logged | hashed with per-bundle salt |
| Service-type HMACs | logged | hashed with per-bundle salt |

Each diagnostic bundle starts with a header listing what was redacted and what was preserved so the user can audit it before sharing.

---

## 9. Auto-update

```
       (timer: 24h since last check)
              │
              ▼
       GET updates.mcpbridge.me/manifest.json
       (no query params, no client identifiers)
              │
              ▼
       parse + verify Ed25519 sig against embedded pubkey
              │
              ▼
   manifest.latest > current ?
        │             │
       no            yes
        │             ▼
        │       emit update.available event
        │             │
        │             ▼ (user clicks "Update" in UI)
        │       update.apply():
        │         - download bundle to staging dir
        │         - verify signature against same pubkey
        │         - mark "apply on next launch"
        │         - send daemon.shutting_down event
        │         - graceful shutdown (§11)
        │             │
        │             ▼
        │       launchd / systemd / Task restarts daemon
        │             │
        │             ▼
        │       supervisor sees staged update, swaps binary, restarts
        ▼
       no-op until next timer
```

Signing key rotation: the manifest can carry a new pubkey signed by both old and new private keys. Verified daemons accept either signature for one release cycle; older daemons accept only the old key and must update via the previous channel.

**Privacy properties** of the update channel (see [`PRIVACY.md`](PRIVACY.md) §4):

- **No query parameters, no User-Agent, no client identifiers.** The request to `updates.mcpbridge.me/manifest.json` carries only what HTTPS necessarily carries (TLS SNI, IP).
- **Randomized check time** within the 24h window with ±6h jitter. A million users running on different schedules do not stampede the endpoint at the same minute, and no install can be fingerprinted by its check pattern.
- **TLS cert pinning** to the manifest endpoint via a separate pin from the bundle-signing key. Even the daemon's own update operator cannot quietly MITM the channel.
- **Settings → Updates → "Check manually only"** disables the daily timer entirely; the user clicks "Check now" when they want.

The endpoint itself is hosted on a no-log CDN (see [`PRIVACY.md`](PRIVACY.md) §4), so no access logs accumulate on the operator side.

---

## 10. CLI parity

Single binary, multiple modes:

```
mcp-bridge daemon                # the long-running mode (used by launchd/systemd)
mcp-bridge daemon --install      # register platform unit + start
mcp-bridge daemon --uninstall    # reverse

mcp-bridge pair <token>          # connect to running daemon, drive pair via token
mcp-bridge list                  # servers.list
mcp-bridge show <pin>            # servers.detail
mcp-bridge revoke <pin> [consumer]
mcp-bridge status                # daemon.status
mcp-bridge logs [--follow]       # log.subscribe (follows) or read on-disk
mcp-bridge diagnostics           # diagnostics.bundle to stdout
mcp-bridge update                # update.check + apply
mcp-bridge identity rotate
mcp-bridge identity show
```

Read-only commands (`list`, `status`, `show`) fall back to reading the on-disk registry directly if no daemon is running. Write commands require the daemon and error out clearly: `error: daemon not running — start with 'mcp-bridge daemon --install' or open Bridge Console.`

`clap` derive for the argument parsing. Output format: human-readable by default, `--json` flag for machine consumption (matches `gh`'s convention).

---

## 11. Graceful shutdown

```
1. Receive SIGTERM / IPC daemon.shutdown / launchd stop
2. supervisor signals root CancellationToken
3. ipc::server stops accepting new connections, drains existing
4. proxy::listener stops accepting; inflight requests get 503 with
   X-MCP-Bridge-Reason: shutdown after a 5s grace
5. pair::endpoint stops accepting
6. announce::bonjour deregisters service-type subscriptions
7. Origin Connectors flush + close pooled connections
8. observability flushes log appender
9. registry::store does a final fsync (defense in depth — nothing
   should be dirty by this point)
10. keystore releases handles
11. exit(0)
```

Total budget: ≤8 s. Beyond that, supervisor force-exits and platform manager logs the abnormal termination.

---

## 12. Cargo dependencies (frozen for v1)

```toml
[package]
name = "mcp-bridged"
version = "0.1.0"
edition = "2024"
rust-version = "1.85"

[dependencies]
tokio = { version = "1", features = ["full"] }
tokio-util = { version = "0.7", features = ["codec"] }
tower = { version = "0.5", features = ["limit", "timeout"] }
axum = "0.8"
hyper = { version = "1", features = ["full"] }
hyper-util = "0.1"

rustls = { version = "0.23", default-features = false, features = ["ring"] }
rustls-pki-types = "1"

ed25519-dalek = { version = "2", features = ["rand_core"] }
x25519-dalek = { version = "2", features = ["static_secrets"] }
crypto_box = "0.9"
hkdf = "0.12"
sha2 = "0.10"
hmac = "0.12"
constant_time_eq = "0.3"
rand = "0.8"

zeroconf = "0.15"            # mDNS cross-platform (alt: astro-dnssd on macOS)
keyring = "3"
directories = "5"

serde = { version = "1", features = ["derive"] }
serde_json = "1"
serde_jcs = "0.1"            # RFC 8785 canonical JSON
jsonrpsee-core = "0.24"      # JSON-RPC 2.0 types
uuid = { version = "1", features = ["v4", "serde"] }

tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "json"] }
tracing-appender = "0.2"

anyhow = "1"
thiserror = "2"
parking_lot = "0.12"
once_cell = "1"

zeroize = { version = "1.8", features = ["zeroize_derive"] }
secrecy = "0.10"

clap = { version = "4", features = ["derive", "env"] }

[target.'cfg(unix)'.dependencies]
nix = { version = "0.29", features = ["signal", "fs"] }

[target.'cfg(windows)'.dependencies]
windows = { version = "0.59", features = [
  "Win32_System_Pipes",
  "Win32_Security",
] }

[dev-dependencies]
tokio-test = "0.4"
wiremock = "0.6"
tempfile = "3"
```

---

## 13. What's not in v1

Explicitly deferred to avoid scope creep:

- **stdio MCP transport** — only HTTP + SSE in v1.
- **WebSocket MCP transport** — track upstream spec, add when stable.
- **Multi-host pair coordination** — phone holds N Resolver pins; no Resolver ↔ Resolver sync.
- **Embedded MCP capabilities** (bridge as its own MCP server exposing things like "list paired servers") — interesting later, distracting now.
- ~~Network monitor view~~ — promoted to v1 ([`PRIVACY.md`](PRIVACY.md) §4); the daemon emits per-connection records, the UI renders them at Settings → Privacy → Outbound connections.
- **Per-tool consent intercept** — pass-through only, matches [`ARCHITECTURE.md §4.3`](ARCHITECTURE.md).
- **Crash reporting** — local logs only, no upload channel.

---

## 14. Open daemon-side decisions

1. **`zeroconf` vs `astro-dnssd` for mDNS.** `zeroconf` is cross-platform but lighter on Windows than mac / Linux quality. `astro-dnssd` is macOS-native and best-in-class there but needs platform branching. Likely: `astro-dnssd` on macOS, `zeroconf` elsewhere, behind a trait abstraction.
2. **`jsonrpsee` vs hand-rolled JSON-RPC.** `jsonrpsee` is the obvious pick but pulls a lot. For ~30 methods, a hand-rolled dispatcher might be cleaner and ~30 KB lighter.
3. **In-process integration tests vs subprocess.** Library API allows in-process which is faster, but subprocess catches more real bugs (signal handling, file locking). Probably: both, with in-process for the common path.
4. **Process supervision on Linux when systemd-user isn't available.** Some minimal distros / containers ship without user systemd. Fallback to a foreground process the user manually launches? Provide a `--no-supervisor` mode?

---

## 15. Status

Not committed. Companion to [`ARCHITECTURE.md §13`](ARCHITECTURE.md). Build phase plan and effort estimates live in [`ARCHITECTURE.md §11`](ARCHITECTURE.md).

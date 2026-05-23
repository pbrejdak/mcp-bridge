# MCP Bridge — Privacy Charter

Status: exploratory, current revision 2026-05-23. Companion to [`ARCHITECTURE.md`](ARCHITECTURE.md), [`DAEMON.md`](DAEMON.md), [`UI.md`](UI.md), [`UX.md`](UX.md), and [`LEGAL.md`](LEGAL.md).

This document is the privacy charter — a single page that a journalist, auditor, or skeptical user can read to verify MCP Bridge's privacy-first claim, with pointers to the implementation parts that back it up.

---

## 1. The promise

MCP Bridge is a privacy-first tool. Concretely:

- **Your data stays on your devices.** Your phone, your computer, your AI apps. Nothing about your tool calls, server activity, or content leaves your machine.
- **No telemetry. No analytics. No third-party SDKs.** Not now, not later, not "anonymized."
- **One narrow outside connection**, by default: a once-a-day check for Bridge's own software updates. One click to disable.
- **Everything is auditable.** Open source. Reproducible builds. Documented egress allowlist. In-product outbound-connection monitor.

Each of these is implemented and verifiable. The rest of this document spells out how, and where to look in the source to check.

---

## 2. Privacy threat model

Distinct from the [security threat model](ARCHITECTURE.md) — different adversaries, different mitigations.

| Adversary | Asset they want | How Bridge resists |
|---|---|---|
| The bridge's own operator (us) | Build analytics, profile usage, monetize | We don't collect; verifiable through open source + reproducible builds |
| Update-channel operator | Track install base, geolocate | Randomized check time; no query params; TLS pinning; manual-only opt-out |
| User's backup software | Mirror sensitive state to cloud | Backup-exclusion xattrs; Spotlight `.metadata_never_index`; `NOT_CONTENT_INDEXED` on Windows |
| User's OS telemetry / crash reporter | Capture process memory containing secrets | Core dumps disabled; `zeroize` on every sensitive type |
| Co-located LAN observers | Inventory what the user runs | Sealed-body Bonjour announces; randomized service-type names; "Pause discovery" toggle |
| Webview telemetry channels (WebView2, WKWebView) | Phone home about navigation / state | Explicit hardening per platform — see [`UI.md`](UI.md) §5.4 |
| Forensic recovery from disposed laptop | Read recoverable Resolver state | At-rest encryption assumption; clean `--purge` uninstall |
| Future-us in monetization panic | Quietly add "small" analytics events | Self-imposed constraints in the docs + reproducible builds means quiet additions cannot stay quiet |

Privacy assets in priority order:

1. **Tool-call activity** — what tools the user invoked, when, with what arguments. Most sensitive, paired with user behavior.
2. **Pinned server identities** — pubkeys, fingerprints, logical IDs. Correlatable across machines and contexts.
3. **Machine identifiers** — paths revealing username, hostnames, display name.
4. **Existence of MCP Bridge on this machine** — even mere installation is information.

---

## 3. Data lifetimes

Every artefact stored by Bridge has a stated lifetime. Nothing accumulates indefinitely.

| Artefact | Default lifetime | Configurable? | Notes |
|---|---|---|---|
| Server Pins (paired servers) | until user revokes | n/a | revocation triggers 30-day soft grace, then full purge of all related data |
| Bearer tokens, loopback keys | same as pin | n/a | zeroed in memory on Drop; keychain entry deleted on revoke |
| Activity feed (in-memory) | 500 entries OR 24h | size in Settings | not persisted by default |
| Activity feed (persisted) | opt-in only, default off | yes | 7-day or 10 MB cap when enabled |
| Log files | rolling 5 × 2 MB OR 7 days | size in Settings | redacted per [`DAEMON.md`](DAEMON.md) §8.2; rotated; auto-purged |
| Diagnostic bundles | never auto-generated | n/a | ephemeral, only on user export |
| Verbose-logging session | 1 hour auto-revert | duration in Settings (15min / 1h / 4h) | persistent UI banner while on |
| Update check timestamp | 24h | n/a | randomized ±6h jitter |
| Sentinel UUIDs in adapter configs | until pin revoke | n/a | per-install random; never re-derived |
| Webview cache | non-persistent | n/a | cleared on window close |
| Tauri persisted prefs | until purge | n/a | UI-only (window position, last tab) |

When the user invokes `mcp-bridge uninstall --purge` ([`DAEMON.md`](DAEMON.md) §2.1), every artefact above is removed, along with webview state, single-instance locks, staging directories, and OS keychain entries.

---

## 4. Egress allowlist

The Resolver's complete network behavior is documented in [`ARCHITECTURE.md`](ARCHITECTURE.md) §6.1. Summary:

**Listens on** (inbound):

- `127.0.0.1:<port>` for Consumer requests
- LAN address `:<pair-port>` for Direction-B pair POSTs (sealed bodies)

**Connects outbound to** (this list is the complete allowlist — anything else the daemon does is a bug to report):

- Paired Origin backends (`backend.url` per Server Pin) — typically same LAN; in principle could be WAN if the user has configured their phone as such
- `updates.mcpbridge.me/manifest.json` — daily, anonymous, no query params, no identifiers, can be disabled

**Multicast**:

- `224.0.0.251:5353` — passive Bonjour subscription

**Configuration of the update endpoint operator**:

- No-log CDN — no access logs retained server-side.
- TLS cert pinning — the daemon refuses to talk to anyone but the genuine endpoint.

The Bridge Console exposes an **Outbound connections** view (Settings → Privacy → Outbound connections) showing every connection the daemon has made in the last 24 hours, with destination, purpose, byte counts. This is the in-product way the user verifies the allowlist matches reality.

---

## 5. Webview privacy hardening

System webviews have their own telemetry. The privacy-first claim does not survive a default Tauri configuration on Windows. The full hardening is documented in [`UI.md`](UI.md) §5.4. Summary:

- **WebView2 (Windows)** — telemetry, domain reliability, optimization-guide downloads, pings all disabled via launch flags.
- **WKWebView (macOS)** — non-persistent data store; no state sharing with Safari; fresh process pool.
- **WebKitGTK (Linux)** — page cache + offline-app cache + crash reports off; data store cleared on quit.
- **Tauri CSP** — strict `default-src 'self'`; no external `connect-src`; `withGlobalTauri: false`; frozen prototype.

Tight CSP plus `withGlobalTauri: false` means injected JS in any window cannot reach the IPC bridge or fetch external resources. No third-party fonts, no external images, no analytics scripts can load — even accidentally via copy-pasted snippets during development.

---

## 6. Crash and memory hygiene

Sensitive material in process memory must not survive past need. See [`DAEMON.md`](DAEMON.md) §7.3:

- `Zeroize + ZeroizeOnDrop` on every type carrying private keys, tokens, or loopback keys. Memory is overwritten before deallocation.
- **Core dumps disabled at daemon startup** (`setrlimit(RLIMIT_CORE, 0, 0)` on Unix, `SetErrorMode(SEM_NOGPFAULTERRORBOX)` on Windows). A panicking or crashing daemon will not write its address space to disk for later forensic recovery.
- Resolver private key never appears in the registry on disk — only its keychain reference.
- Identity Keystore returns short-lived `Secret<T>` handles via the `secrecy` crate so callers cannot accidentally clone secrets around the codebase.

---

## 7. Uninstall completeness — right to erasure

`mcp-bridge uninstall --purge` removes every artefact attributable to Bridge. The complete list is documented in [`DAEMON.md`](DAEMON.md) §2.1. After purge plus binary deletion, no data attributable to MCP Bridge remains on the user's machine.

This is the auditable "right to erasure":

- Filesystem scan finds no `mcp-bridge` / `MCPBridge` paths.
- `security dump-keychain | grep mcpbridge` (macOS) finds nothing.
- Consumer config inspection finds no `_mcp_bridge_managed` UUID tags.

If the user can detect any residue after purge, that is a bug.

---

## 8. Per-platform privacy nuances

### 8.1 macOS

- **Gatekeeper notarization is stapled** (`xcrun stapler staple`) — see [`ARCHITECTURE.md`](ARCHITECTURE.md) §11. First launch validates offline; no phone-home to `ocsp.apple.com` for the user's IP.
- **Time Machine**: data directory has `com.apple.metadata:com_apple_backup_excludeItem` set.
- **Spotlight**: `.metadata_never_index` file in the data dir.
- **iCloud Drive Desktop+Documents**: data lives at `~/Library/Application Support/MCPBridge/`, outside iCloud sync paths by default.
- **Local Network privacy prompt** (macOS 15+): triggered intentionally; pre-flight UX explains why ([`UX.md`](UX.md) §13.2).
- **Keychain access**: macOS prompts on first write; daemon adds itself to the ACL so subsequent reads do not prompt.

### 8.2 Windows

- **SmartScreen**: freshly-signed binaries trigger reputation warnings on first launch; reputation accrues over time.
- **Indexing**: `FILE_ATTRIBUTE_NOT_CONTENT_INDEXED` set on the data directory.
- **WebView2 telemetry**: explicitly disabled via launch flags.
- **OneDrive Personal Vault / Backup**: data path is outside default sync locations.
- **Windows Credential Manager**: per-user; same isolation model as macOS keychain.

### 8.3 Linux

- **GNOME Tracker / Baloo**: `.metadata_never_index` honored by Tracker; `.directory` flag for Baloo.
- **systemd journal**: Bridge writes its own logs at `~/.local/state/mcp-bridge/log/`, not via journal. User can verify with `systemctl --user status mcp-bridged`.
- **Secret Service**: unlocked once per session; covers credential storage.

---

## 9. Dependency privacy posture

Every third-party component, audited for network behavior. New dependencies are reviewed for this property before adoption.

| Dependency | Phones home? | Notes |
|---|---|---|
| Tauri 2.x | no | auto-update is off; we own the update channel |
| Svelte 5 / Vite | no | compile-time + runtime, no network |
| Bits UI / shadcn-svelte | no | pure UI |
| Tailwind v4 | no | compile-time CSS |
| `@lucide/svelte` | no | bundled SVG |
| `@fluentui/svg-icons` | no | bundled SVG |
| SF Symbols (macOS only) | no | rendered via local AppKit `NSImage` |
| `mode-watcher` | no | reads `prefers-color-scheme` |
| `svelte-sonner`, `tinykeys` | no | DOM only |
| `qrcode` (vanilla npm) | no | local computation |
| Rust crates (`tokio`, `hyper`, `rustls`, `axum`, `tower`) | no | networked crates contact only endpoints the daemon explicitly directs them at |
| `ed25519-dalek`, `x25519-dalek`, `crypto_box`, `zeroize`, `secrecy` | no | local crypto |
| `keyring` | no | OS-API wrapper |
| `zeroconf` / `astro-dnssd` | no | local mDNS |

---

## 10. At-rest encryption assumption

Bridge does not implement application-level encryption-at-rest. The decision: relying on the OS's full-disk encryption is the right level of abstraction. Adding our own layer would require either a master password (user friction) or storing a derived key elsewhere (no real gain).

Bridge assumes:

- macOS: **FileVault** enabled (default on modern Macs).
- Windows: **BitLocker** enabled (default on Win 11 Pro/Enterprise; recommended on Home).
- Linux: **LUKS** or equivalent FDE.

Without full-disk encryption, anyone with file-system access to the user's machine can read paired-server details. This trade-off is surfaced in the in-app About → Privacy view.

---

## 11. Right to data portability

`mcp-bridge identity export <path>` produces an encrypted bundle (passphrase-derived, libsodium `secretbox`) containing the Resolver keypair and all Server Pins.

`mcp-bridge identity import <path>` on another machine accepts the bundle. The user re-installs into the new machine's AI apps but does not need to re-pair their phones.

This is the user's GDPR-style right to data portability, satisfied without operator involvement.

---

## 12. How to verify these claims

The privacy-first claim is only as good as its verifiability. Concrete actions a skeptical user (or auditor) can take:

1. **Network capture** — run `tcpdump` / Wireshark while using Bridge. The captured outbound destinations should match exactly §4 above. Anything else is a bug to report via [`LEGAL.md`](LEGAL.md) §11.
2. **In-product egress monitor** — Settings → Privacy → Outbound connections shows the daemon's own view of its connections. Should match the packet capture.
3. **Read the source** — the daemon is open source. Audit `src/proxy/connector.rs`, `src/update/`, `src/announce/`. Nothing else makes outbound connections.
4. **Reproducible builds** — distributed binaries match the source via SLSA provenance attestation. Anyone can rebuild the source and verify the binary hash matches what we shipped.
5. **Filesystem inspection after `--purge`** — scan the filesystem for any `mcp-bridge` / `MCPBridge` artefact. None should remain.

If any of these verification steps fail, [report it](LEGAL.md) §11.

---

## 13. Residual limits

Privacy-first does not mean privacy-perfect. Bridge does not claim to defeat:

- **LAN-presence detection** — even with sealed-body announces and randomized service types, the existence of `_mcp-bridge-<hmac>` traffic is observable on the LAN. A determined LAN observer can detect "this household runs Bridge." Mitigation: Settings → Privacy → Pause discovery + manual-SAS pairing for fresh pairs.
- **OS-level network metadata** — DHCP records, router logs, ISP records. Outside Bridge's scope.
- **User-configured backup software that respects xattrs but is then manually overridden** — that's the user's choice.
- **Keychain access by other apps as the same user** — macOS prompts on first cross-app access; Linux Secret Service is unlocked once per session. Bridge relies on the OS keychain isolation model, which is best-in-class but not perfect.
- **Compromised user account on the laptop** — once an attacker has root or the user's session, no userspace tool can defend against them.

These limits are acknowledged, not waved away. They inform the threshold at which Bridge stops being the right tool. Users facing adversaries that defeat these limits should layer additional measures (Tor, Tails, dedicated hardware, air-gapped pairing).

---

## 14. Privacy contact

Privacy and security disclosures route through the same channel. See [`LEGAL.md`](LEGAL.md) §11. 90-day responsible-disclosure window. PGP key published at the same source.

---

## 15. Status

Not committed. Companion to [`ARCHITECTURE.md`](ARCHITECTURE.md) §13. Privacy claims in this document are aspirational until the implementation lands. As code ships, each section should be linked to the actual files that enforce the claim.

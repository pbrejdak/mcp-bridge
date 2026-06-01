# Changelog

All notable changes to MCP Bridge will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Wire-protocol versions ([`mcp-pair`](docs/SPEC.md#4-mcp-pairv01--pairing-protocol),
[`mcp-announce`](docs/SPEC.md#5-mcp-announcev01--identity-refresh-protocol))
are versioned independently of the project and are called out explicitly when
they change.

## [Unreleased]

### Added

Initial Rust daemon implementation (`mcp-bridged` crate) covering the
Phase 1 end-to-end pair-and-proxy flow:

- **Identity & TLS** — Ed25519 Resolver keypair (zeroizing, persisted in
  the OS keychain via `keyring`), self-signed cert generation for the
  pair endpoint, and a `PinningVerifier` for outbound HTTPS calls.
- **`mcp-pair/v0.1` (SPEC §4)** — typed `PairPayload`, libsodium-style
  sealed-box construction, the SAS phrase derivation, and an
  `InviteRegister` actor enforcing the 60-second lifetime. All 10
  acceptance rules from SPEC §4.5 enforced.
- **HTTPS pair endpoint** — axum + axum-server + rustls. 204 on accept,
  bare 400 on every failure (no error detail leaks to the network per
  SPEC §5.3).
- **Server Registry** — atomic JSON read/write under `data_dir`,
  mode-0600 perms on Unix, single `RwLock<Registry>` shared between the
  pair endpoint, announce handler, and proxy listener.
- **Loopback listener** — `127.0.0.1:8766` HTTP proxy that validates
  the Host header, parses path → logical_id, checks the per-Pin
  loopback key in constant time, and forwards through a per-Pin
  `OriginConnector` to the backend with the pinned bearer token.
- **SSE streaming** — the connector returns a body stream instead of a
  buffered Vec; tools/call responses ride end-to-end without buffering.
- **`mcp-announce/v0.1` (SPEC §5)** — typed `AnnouncePayload`, the
  `/announce` HTTPS endpoint, all 7 acceptance rules (sig, seq, exp,
  fp/auth ratchets), and SPEC §5.6 pre-signature rate limits (8/s/IP +
  1/s/LID). mDNS subscriber is deferred to a follow-up commit.
- **Client Adapters** — `Adapter` trait + `Sentinel` per-install UUID
  + a `ClaudeDesktopAdapter` that writes `mcpServers/<lid>` entries
  atomically and removes only entries it owns on revoke.
- **IPC (UDS on Unix, named-pipe on Windows)** — JSON-RPC 2.0 over a
  length-prefixed frame protocol. Methods:
  - `daemon.status`
  - `servers.list`, `servers.detail`, `servers.revoke`
  - `pair.invite_start`, `pair.invite_cancel`
  - `identity.show`
- **CLI surface** (`mcp-bridge`):
  - `daemon` (long-running mode), `daemon --install` / `--uninstall`
    (macOS launchd plist today; Linux systemd-user and Windows
    Scheduled Task pending).
  - `pair` — generates an invite, renders a terminal QR code, polls
    until a phone completes the POST or Ctrl-C cancels it.
  - `list`, `show <pin>`, `revoke <pin>`, `status`, `identity show`.
- **Cross-platform support** — Unix UDS and Windows named-pipe IPC;
  macOS launchd installer; verified compile against
  `x86_64-pc-windows-gnu` via cross-check.
- **CLAUDE.md** at `mcp-bridged/CLAUDE.md` documenting Rust + cross-
  platform conventions for contributors and AI assistants.

Phase 2 in progress (`bridge-console` crate + npm package):

- **Bridge Console scaffold** — Tauri 2 + Svelte 5 + Vite + TypeScript
  under [`bridge-console/`](bridge-console/). One Console window today,
  showing daemon status + identity + the paired-servers table. Tray
  icon, pair-window (QR + SAS), and activity feed are tracked for
  follow-up commits.
- **`daemon_call` Tauri command** wraps `mcp_bridged::ipc::call_local`
  so the renderer talks to the same UDS / Windows named-pipe the CLI
  uses. Renderer-side TypeScript hits it via a typed
  `daemonCall<T>(method, params)` helper with `DaemonError` and
  `TransportError` thrown on the two failure modes.

Existing design/policy documentation set:

- Documentation set under [`docs/`](docs/):
  - [`ARCHITECTURE.md`](docs/ARCHITECTURE.md) — Stable-Loopback Bridge pattern, trust model, sequence diagrams.
  - [`SPEC.md`](docs/SPEC.md) — normative wire protocol specification (`mcp-pair/v0.1`, `mcp-announce/v0.1`).
  - [`DAEMON.md`](docs/DAEMON.md) — Rust daemon internals.
  - [`UI.md`](docs/UI.md), [`UX.md`](docs/UX.md) — Bridge Console design.
  - [`MOBILE.md`](docs/MOBILE.md) — Origin-side mobile SDK.
  - [`PRIVACY.md`](docs/PRIVACY.md) — privacy charter and threat model.
  - [`SECURITY.md`](docs/SECURITY.md) — responsible disclosure policy.
  - [`LEGAL.md`](docs/LEGAL.md) — licensing, ToS, trademark, export classification.
  - [`CONTRIBUTING.md`](docs/CONTRIBUTING.md) — DCO sign-off, privacy-first review checklist.
  - [`GLOSSARY.md`](docs/GLOSSARY.md) — terminology reference.
- [`README.md`](README.md) at repo root.
- [`LICENSE`](LICENSE) — Apache License 2.0.
- [`docs/NOTICE`](docs/NOTICE) — Apache 2.0 attribution.

### Changed

- (none)

### Deprecated

- (none)

### Removed

- (none)

### Fixed

- (none)

### Security

- (none)

### Wire protocol

- (none — `mcp-pair/v0.1` and `mcp-announce/v0.1` are the inaugural versions.)

[Unreleased]: https://github.com/mcp-bridge/mcp-bridge/compare/HEAD

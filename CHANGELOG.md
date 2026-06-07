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
  fp/auth ratchets), and SPEC §5.6 pre-signature rate limits (8/s/IP
  for HTTP, 4/s/IP for mDNS, 1/s/LID for both).
- **mDNS carrier (SPEC §5.2)** — daily-rotated `_mcp-bridge-<HMAC>.
  _tcp.local` service type, `MdnsSubscriber` trait + pure-Rust
  `mdns-sd` backend, async bridge task that pipes resolved TXT
  records into the same accept pipeline as the HTTP carrier. Outer
  loop re-subscribes at UTC midnight when the daily HMAC rolls.
- **Bearer-token rotation (SPEC §5.7)** — when an accepted announce
  asserts `auth_rotated_at` strictly greater than the recorded value,
  the daemon fetches a fresh bearer via the
  `BearerTokenRefresher` trait, persists it, and invalidates the
  pooled `OriginConnector`. Default impl follows the well-known
  convention `GET <backend>/.well-known/mcp-bridge/refresh-token`
  (the spec leaves the control-call shape out of scope).
- **Client Adapters** — `Adapter` trait + `Sentinel` per-install UUID
  + a `ClaudeDesktopAdapter` that writes `mcpServers/<lid>` entries
  atomically and removes only entries it owns on revoke.
- **IPC (UDS on Unix, named-pipe on Windows)** — JSON-RPC 2.0 over a
  length-prefixed frame protocol. Methods:
  - `daemon.status`
  - `servers.list`, `servers.detail`, `servers.revoke` (with optional
    per-Consumer scope: when `consumer` is set, only the named
    adapter's entry is removed and the pin stays alive)
  - `pair.invite_start`, `pair.invite_cancel`
  - `identity.show`, `identity.rotate` (hot-swaps the in-memory
    keypair; clears the proxy connector cache and signals mDNS to
    re-derive its service-type HMAC immediately)
  - `log.recent`, `diagnostics.bundle`, `update.check`
- **CLI surface** (`mcp-bridge`):
  - `daemon` (long-running mode), `daemon --install` / `--uninstall`
    (launchd plist on macOS, systemd-user unit on Linux, Scheduled
    Task on Windows).
  - `pair` — generates an invite, renders a terminal QR code, polls
    until a phone completes the POST or Ctrl-C cancels it.
  - `list`, `show <pin>`, `revoke <pin> [<consumer>]`, `status`,
    `identity show`, `identity rotate`, `logs [--follow]`,
    `diagnostics`, `update`.
- **Cross-platform support** — Unix UDS and Windows named-pipe IPC;
  macOS launchd installer; verified compile against
  `x86_64-pc-windows-gnu` via cross-check.
- **CLAUDE.md** at `mcp-bridged/CLAUDE.md` documenting Rust + cross-
  platform conventions for contributors and AI assistants.

Phase 2 (`bridge-console` crate + npm package — Tauri 2 + Svelte 5 +
Vite + TypeScript under [`bridge-console/`](bridge-console/)):

- **Console window** — header status grid (daemon version / uptime /
  pair-endpoint / Resolver name), three tabs:
  - **Servers** — paired-pin table with per-row Revoke action
    (confirmation modal + adapter cleanup).
  - **Activity** — polls `log.recent` every 1s with a seq cursor;
    Pause/Resume + Clear controls.
  - **Settings** — Identity card (display name, full pubkey, Copy
    button, destructive Rotate identity… modal with typed "rotate"
    confirmation), Updates card (calls `update.check`), Diagnostics
    card (wide modal showing the bundle from `diagnostics.bundle`
    with a Copy-to-clipboard button).
- **Pair flow** — separate view with QR + SAS + 60s countdown.
  Polls `servers.list` for a new pin; success / expired / cancel
  states. Hits `pair.invite_cancel` on user abort and on timeout so
  the daemon's invite register releases the nonce immediately.
- **System tray** — `tauri-plugin-tray` (built-in) icon with
  Open / Pair new server / Quit menu. Close-button hides instead of
  exits so the tray stays alive; left-click brings the window back.
- **Deep-link handler** — `tauri-plugin-deep-link` registers the
  `mcp-bridge://pair[/…]` URI scheme; incoming URLs switch to the
  pair view. Path content is reserved for future semantics.
- **Single-instance plugin** — second launch focuses the existing
  window instead of spawning a duplicate.
- **Window effects** — `window-vibrancy` integration: NSVisualEffect
  Sidebar material on macOS, Mica on Windows 11, no-op on Linux.
- **`daemon_call` Tauri command** wraps `mcp_bridged::ipc::call_local`
  so the renderer talks to the same UDS / Windows named-pipe the CLI
  uses. Renderer-side TypeScript hits it via a typed
  `daemonCall<T>(method, params)` helper with `DaemonError` and
  `TransportError` thrown on the two failure modes.
- **Reusable Svelte components** under `src/lib/components/` (Modal
  with backdrop+Escape) and shared `app.css` with design tokens +
  button variants.

Phase 4 (`mcp-bridge-mobile/` — Kotlin Multiplatform protocol core
under [`mcp-bridge-mobile/core/`](mcp-bridge-mobile/core/)):

- **KMP Gradle scaffold** — `settings.gradle.kts` + `build.gradle.kts`
  + version catalog at `gradle/libs.versions.toml`. Targets enabled:
  `jvm()`, `androidTarget()`, `iosArm64()`, `iosSimulatorArm64()`,
  `iosX64()`. Gradle 8.14.3 (wrapper checked in), Kotlin 2.1.21,
  Android Gradle Plugin 8.7.3. compileSdk 34, minSdk 26 per
  [`docs/MOBILE.md`](docs/MOBILE.md) §7.1.
- **`commonMain` protocol models** — `OriginConfig`, `ServerConfig`,
  `Scope`, `ResolverInvite`, `ResolverPin`. All `@Serializable`.
  No cryptography yet.
- **`commonTest`** with `kotlin.test` runs on JVM + Android Debug +
  Android Release; first two sanity assertions green across all
  three targets.
- **iOS framework link verified** for `iosSimulatorArm64`. Other iOS
  targets wired in but their first link is deferred until local disk
  pressure clears (Kotlin/Native intermediates are ~hundreds of MB
  per target).
- **READMEs** at [`mcp-bridge-mobile/README.md`](mcp-bridge-mobile/README.md)
  (workspace orientation) and
  [`mcp-bridge-mobile/core/README.md`](mcp-bridge-mobile/core/README.md)
  (KMP build + tooling notes).
- **RFC 8785 (JCS) canonical-JSON encoder** in
  `commonMain/canonical/CanonicalJson.kt` — the byte sequence
  signatures are computed over per [`docs/SPEC.md`](docs/SPEC.md) §3.3.
  Integer-only numeric support (the two MCP Bridge wire protocols
  carry no floats); object keys sorted by UTF-16 code unit; only the
  seven JCS shorthand string escapes + `\u00XX` for the remaining
  ASCII controls. 12 unit tests in `commonTest` cover key sorting,
  string escapes, nested structures, Unicode keys/values, UTF-8 byte
  layout, the float-rejection contract, and announce-payload-shaped
  parity with the daemon's `serde_jcs::to_vec` output. All 12 green
  on JVM + Android Debug + Android Release + iOS simulator arm64.
- **Cross-language canonical-JSON conformance fixtures** in
  [`test-vectors/canonical/`](test-vectors/canonical/) (6 cases:
  empty containers, key sorting, nested structures, the seven JCS
  string escapes, Unicode passthrough including a surrogate pair, and
  an announce-payload-shaped object). Two runners walk the same files
  and assert byte-equal output: the Rust daemon's
  [`canonical_conformance.rs`](mcp-bridged/tests/canonical_conformance.rs)
  via `serde_jcs::to_vec`, and the Kotlin core's
  [`CanonicalConformanceTest`](mcp-bridge-mobile/core/src/jvmTest/kotlin/dev/mcpbridge/mobile/canonical/CanonicalConformanceTest.kt)
  via `CanonicalJson.encodeToBytes`. Both green; drift between them
  surfaces at fixture time, not at runtime in someone's living room.
- **ADR 0006 — KMP crypto stack** ([`docs/decisions/0006-kmp-crypto-stack.md`](docs/decisions/0006-kmp-crypto-stack.md)).
  Hybrid stack: Apple CryptoKit for keyed crypto on iOS (Secure Enclave
  where available), BouncyCastle for the libsodium primitives on
  JVM/Android, upstream libsodium framework on iOS for the
  XSalsa20-Poly1305 inside `crypto_box`. Pure-Kotlin in `commonMain`
  for unkeyed hashes (SHA-256) and the Edwards-to-Montgomery map.
  Accepted.
- **Pure-Kotlin SHA-256** in
  `commonMain/crypto/Sha256.kt` (FIPS 180-4 §6.2), with a streaming
  `State` for callers that need to feed multiple buffers. Five tests
  cover the FIPS empty / "abc" / two-block / million-A vectors plus
  the 48-byte all-zeros input that drives the SAS KAT. Green on JVM,
  Android Debug, Android Release, and iOS simulator arm64.
- **SAS derivation** in `commonMain/sas/SasDerivation.kt`
  ([`docs/SPEC.md`](docs/SPEC.md) §4.2). The SAS phrase comparison is
  the user-facing security control during pairing — Kotlin and Rust
  must agree on it byte-for-byte. The 2048-word BIP39 wordlist is
  generated into `SasWordlist.kt` from
  [`test-vectors/sas-wordlist-v1.txt`](test-vectors/sas-wordlist-v1.txt)
  by a Gradle task at build time, so the SDK ships with the same
  source of truth the daemon's `accept-canonical.json` fixture pins.
  KAT verified: all-zero pubkey + all-zero nonce → `voyage-sentence-voyage-deny`
  on every target.

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

- mDNS subscriber: pass fully-qualified service names to `mdns-sd`'s
  `browse()` (which insists on the trailing dot). Without this, the
  daemon kept running with mDNS silently dead — pair endpoint, IPC,
  and loopback listener still came up fine, but mDNS announces were
  rejected at the browse call.

### Security

- (none)

### Wire protocol

- (none — `mcp-pair/v0.1` and `mcp-announce/v0.1` are the inaugural versions.)

[Unreleased]: https://github.com/mcp-bridge/mcp-bridge/compare/HEAD

# MCP Bridge — Glossary

A single-page reference for every term that appears across the MCP Bridge documentation set. Entries link to the docs where each term is used in depth.

If a term you are looking for is missing, please open an issue or a PR — the glossary is meant to stay exhaustive.

---

## At a glance — the three roles

| Role | Lives on | Job |
|---|---|---|
| [Origin](#origin) | Mobile device | Hosts the MCP server and the [Bridge Peer](#bridge-peer) SDK |
| [Resolver](#resolver) | User's computer | Holds a stable localhost URL per Origin; absorbs all Origin-side drift |
| [Consumer](#consumer) | User's computer | A standard MCP client (Claude Desktop, Cursor, …) configured once against the Resolver |

---

## A

### Announce
A recurring `mcp-announce/v0.1` message sent by an [Origin](#origin) to refresh backend identity on an already-paired [Server Pin](#server-pin). Sent on Wi-Fi changes, token rotation, cert renewal, and as a 30-second keepalive. See [SPEC.md §5](SPEC.md), [ARCHITECTURE.md §4.2](ARCHITECTURE.md).

### Adapter
See [Client Adapter](#client-adapter).

---

## B

### Backend
The Origin-side MCP server endpoint (URL, certificate fingerprint, optional CA) that the [Resolver](#resolver) connects to on behalf of [Consumers](#consumer). The backend can move freely — the [Server Pin](#server-pin) ratchets follow it. See [SPEC.md §5.8](SPEC.md).

### Backup-exclude
The set of OS-specific extended attributes and metadata flags the Resolver sets on its persisted files so that user backup software (Time Machine, OneDrive, iCloud, etc.) does not mirror sensitive state to the cloud. See [DAEMON.md §7.4](DAEMON.md), [PRIVACY.md §2](PRIVACY.md).

### Bonjour
Apple's implementation of multicast DNS / DNS-SD ([RFC 6762](https://www.rfc-editor.org/info/rfc6762), [RFC 6763](https://www.rfc-editor.org/info/rfc6763)). One of the two carriers for the [announce](#announce) protocol. Used interchangeably with **mDNS** in this documentation.

### Bridge Console
The user-facing UI of MCP Bridge. A Tauri + Svelte 5 desktop app that lives in the menu bar / system tray and provides the pair flow, the activity feed, and Settings. See [UI.md](UI.md), [UX.md](UX.md).

### Bridge Peer
The mobile-side runtime object provided by [`@mcp-bridge/mobile`](MOBILE.md). One per host app. Owns QR scanning, payload signing and sealing, per-Resolver pin storage, and the [announce](#announce) lifecycle. See [MOBILE.md §3](MOBILE.md).

---

## C

### Canonical JSON
The deterministic JSON encoding defined by [RFC 8785](https://www.rfc-editor.org/info/rfc8785) (JCS). Used by both wire protocols so that signatures verify byte-identically regardless of which implementation produced them. See [SPEC.md §3.3](SPEC.md).

### Client Adapter
A Resolver module that knows how to detect, write, and remove entries in one specific [Consumer](#consumer)'s config file (e.g., Claude Desktop, Cursor, Continue). Each entry it writes is tagged with the [sentinel UUID](#sentinel-uuid) so the Resolver can find and remove only its own entries. See [ARCHITECTURE.md §3.1](ARCHITECTURE.md), [CONTRIBUTING.md §3.5](CONTRIBUTING.md).

### Conformance test vectors
The fixture set under `test-vectors/` that any conforming [Origin](#origin) or [Resolver](#resolver) implementation must pass. Changes to wire behavior must include updated vectors. See [SPEC.md §9](SPEC.md).

### Consumer
A standard MCP client — Claude Desktop, Cursor, Continue, Zed (planned), or any future MCP client. Consumers are configured *exactly once* against the Resolver's loopback URL and never need re-configuration after that, regardless of how the [Origin](#origin) drifts. See [ARCHITECTURE.md §2](ARCHITECTURE.md).

### `crypto_box`
The libsodium public-key authenticated encryption primitive (X25519 + XSalsa20-Poly1305). Used to seal [pair payloads](#pair-payload) and sealed-body [announces](#announce) to the Resolver's public key. See [SPEC.md §3.1](SPEC.md).

---

## D

### DCO (Developer Certificate of Origin)
The contributor sign-off mechanism used in lieu of a CLA. Every commit must carry a `Signed-off-by:` trailer. See [CONTRIBUTING.md §4](CONTRIBUTING.md), [LEGAL.md §10](LEGAL.md).

### Diagnostic bundle
A user-triggered, redacted export of recent log files and registry state, copyable from Bridge Console for support purposes. Redacts `Authorization` headers and loopback `?key=` parameters before producing the bundle. See [ARCHITECTURE.md §8.1](ARCHITECTURE.md), [PRIVACY.md §3](PRIVACY.md).

### Direction A
The pairing flow in which the **Origin** displays a QR and the **Resolver** scans it via the computer's webcam. Used when phone and computer cannot reach each other on the LAN. See [SPEC.md §4.1](SPEC.md), [ARCHITECTURE.md §5.5](ARCHITECTURE.md).

### Direction B
The default pairing flow in which the **Resolver** displays a QR and the **phone** scans it with its camera, then POSTs a sealed [pair payload](#pair-payload) over the LAN. See [SPEC.md §4.2](SPEC.md), [ARCHITECTURE.md §5.1](ARCHITECTURE.md).

### Discovery Agent
The Resolver sub-component that subscribes to [Bonjour](#bonjour) for [announces](#announce), handles incoming pair POSTs, and reads QR codes from the webcam. See [ARCHITECTURE.md §3.1](ARCHITECTURE.md).

### Drift
The collective term for Origin-side identity changes — new IP (Wi-Fi switch), new port (app restart), new bearer token (rotation), new certificate fingerprint (renewal). The fundamental problem MCP Bridge exists to solve. See [ARCHITECTURE.md §1](ARCHITECTURE.md).

---

## E

### Egress allowlist
The exhaustive list of outbound destinations the Resolver is permitted to contact. Anything outside the allowlist is a bug to report. See [ARCHITECTURE.md §6.1](ARCHITECTURE.md), [PRIVACY.md §4](PRIVACY.md).

### `exp`
The freshness window timestamp in an [announce](#announce) payload. Must lie within ±60 seconds of the Resolver's wall clock to be accepted. See [SPEC.md §5.4](SPEC.md).

---

## F

### Fingerprint (cert fingerprint, `fp`)
SHA-256 digest of the DER-encoded leaf certificate the Origin's backend currently presents. Pinned at first pair; subsequent rotation requires the [`cert_rotated_at` ratchet](#ratchet). See [SPEC.md §5.8](SPEC.md).

---

## I

### Identity Keystore
The Resolver-side OS keychain wrapper that holds the Resolver keypair, per-Origin pinned public keys, per-`(Origin, Consumer)` loopback keys, and bearer tokens. See [ARCHITECTURE.md §3.1](ARCHITECTURE.md), [DAEMON.md §7.3](DAEMON.md).

### Invite
The `mcp-pair/v0.1` message a Resolver displays as a QR in [Direction B](#direction-b). Carries `resolver.pubkey`, `resolver.lan_addr`, the [SAS](#sas), and the [nonce](#nonce). See [SPEC.md §4.2](SPEC.md).

### IPC surface
The JSON-RPC interface the Resolver daemon exposes to [Bridge Console](#bridge-console) and the [`mcp-bridge` CLI](#mcp-bridge-cli). Treated as a versioned contract subject to the wire-protocol change process. See [DAEMON.md §5](DAEMON.md), [CONTRIBUTING.md §3.4](CONTRIBUTING.md).

---

## J

### JCS
JSON Canonicalization Scheme — [RFC 8785](https://www.rfc-editor.org/info/rfc8785). See [Canonical JSON](#canonical-json).

---

## K

### `?key=` (loopback key)
The 256-bit per-`(Server Pin, Consumer)` random secret embedded in the loopback URL as a query parameter, written into the Consumer's config alongside the URL. Constant-time compared on every request. See [SPEC.md §6.1](SPEC.md), [ARCHITECTURE.md §4.3](ARCHITECTURE.md).

### KMP (Kotlin Multiplatform)
The shared-codebase technology used to build the mobile SDK's protocol core. Compiles to an iOS framework, an Android AAR, and a JS bundle from one source. See [MOBILE.md §2](MOBILE.md), [decisions/0001-kmp-for-mobile-core](decisions/0001-kmp-for-mobile-core.md).

---

## L

### LID (Logical ID)
An Origin-chosen identifier that remains stable across IP, port, token, and certificate rotation. Distinct from the backend URL precisely because the URL is not stable. The path segment in the loopback URL (`/<logical_id>`) maps 1:1 to the LID. See [SPEC.md §4.4](SPEC.md).

### Loopback face / Loopback Listener
The Resolver-side HTTP server bound to `127.0.0.1` that Consumers connect to. Enforces the [Host-header check](#host-header-check) and the [loopback key](#key-loopback-key) before forwarding to the pinned backend. See [SPEC.md §6](SPEC.md).

---

## M

### `mcp-announce/v0.1`
The wire protocol for recurring identity refresh from [Origin](#origin) to [Resolver](#resolver). See [SPEC.md §5](SPEC.md).

### `mcp-bridged`
The Resolver process — the always-on background daemon. See [DAEMON.md](DAEMON.md).

### `mcp-bridge` CLI
The command-line interface that drives the same [IPC surface](#ipc-surface) as Bridge Console. Used for headless setup and power-user automation. See [DAEMON.md §10](DAEMON.md).

### `mcp-pair/v0.1`
The wire protocol for one-shot pairing between [Origin](#origin) and [Resolver](#resolver). See [SPEC.md §4](SPEC.md).

### mDNS
Multicast DNS ([RFC 6762](https://www.rfc-editor.org/info/rfc6762)). See [Bonjour](#bonjour).

---

## N

### Nonce
A 16-byte cryptographically random value generated by the Resolver for each [invite](#invite). Single-use, 60-second lifetime. Defeats replay of captured invites and sealed [pair payloads](#pair-payload). See [SPEC.md §4.2-§4.3](SPEC.md).

---

## O

### Origin
A mobile MCP server — the source of truth for tools and resources. Rotates its own identity (Wi-Fi, port, token, certificate) freely. Paired exactly once with each [Resolver](#resolver) via [`mcp-pair/v0.1`](#mcp-pairv01). See [ARCHITECTURE.md §2](ARCHITECTURE.md).

### Origin Connector
The Resolver-side outbound MCP client (one per Server Pin) that holds the TLS-pinned connection to a Backend and attaches the bearer token. See [ARCHITECTURE.md §3.1](ARCHITECTURE.md), [DAEMON.md §6](DAEMON.md).

### `origin_offered`
See [Direction A](#direction-a).

---

## P

### Pair / Pairing
The one-shot exchange that establishes a [Server Pin](#server-pin). Implemented by [`mcp-pair/v0.1`](#mcp-pairv01). See [SPEC.md §4](SPEC.md).

### Pair payload
The signed (and, in [Direction B](#direction-b), sealed) `mcp-pair/v0.1` message the Origin sends to the Resolver. Carries the Origin's long-lived public key, the backend identity, the bearer token, and the requested scopes. See [SPEC.md §4.4](SPEC.md).

### Pin
Short for [Server Pin](#server-pin). The Resolver-side record of one paired Origin.

### Pinning (certificate)
The practice of binding to the exact SHA-256 fingerprint of the Origin backend's leaf certificate. Defeats network-attacker certificate swaps. See [SPEC.md §5.8](SPEC.md).

### Pinning (public key)
The practice of binding the Pin to the Origin's long-lived Ed25519 public key at pair time, so all subsequent announces must verify against that exact key. See [SPEC.md §5.5](SPEC.md).

---

## Q

### QR (QR code)
The encoded form of an [invite](#invite) (Direction B) or a [pair payload](#pair-payload) (Direction A). The QR is the out-of-band channel that anchors the trust model — what is shown on one screen must be scanned by the other device. See [ARCHITECTURE.md §5.5](ARCHITECTURE.md).

---

## R

### Ratchet
The strictly-monotonic timestamp rule that governs sensitive transitions. `seq` ratchets per-Pin to defeat announce replay; `cert_rotated_at` ratchets to authorize a new backend certificate fingerprint; `auth_rotated_at` ratchets to trigger a token re-fetch. See [SPEC.md §5.4-§5.8](SPEC.md).

### Resolver
The always-on process on the user's computer (`mcp-bridged`) that holds a stable localhost URL per [Origin](#origin). Re-discovers the Origin on every drift. Implemented in Rust and packaged as a Tauri-driven desktop app. See [ARCHITECTURE.md §3.1](ARCHITECTURE.md), [DAEMON.md](DAEMON.md).

### `resolver_offered`
See [Direction B](#direction-b).

### Revoke
The user-driven action that removes a [Server Pin](#server-pin) and (via the [Client Adapter](#client-adapter)) the corresponding entries from every Consumer's config. Tag-based — the Resolver only touches lines it added. See [ARCHITECTURE.md §5.4](ARCHITECTURE.md).

---

## S

### SAS (Short Authentication String)
A four-word phrase derived from `H(resolver.pubkey || nonce)`, drawn from the [SAS wordlist](#sas-wordlist). Displayed simultaneously on Bridge Console and on the phone during pairing; the user confirms they match before any secret leaves the phone. Defeats MITM substitution of the [invite](#invite) QR. See [SPEC.md §4.2](SPEC.md).

### SAS wordlist
The 2048-word lowercase-ASCII wordlist used to render the [SAS](#sas). Canonical fixture at `test-vectors/sas-wordlist-v1.txt`; locked at v0.1. See [SPEC.md §8.4](SPEC.md).

### Sealed body
An [announce](#announce) carried inside a libsodium [`crypto_box`](#crypto_box)-sealed envelope addressed to `resolver.pubkey`. Hides even the [LID](#lid) from on-LAN observers. See [SPEC.md §5.2](SPEC.md).

### Sentinel UUID
The per-install random UUID that the Resolver writes alongside every entry a [Client Adapter](#client-adapter) adds to a Consumer config file. Allows the Resolver to find and remove only its own entries during revoke or uninstall, without touching anything else the user has configured. See [CONTRIBUTING.md §3.5](CONTRIBUTING.md).

### `seq`
The strictly-increasing per-LID counter in an [announce](#announce) payload. Replay defense — once the Resolver has accepted `seq = N`, any later announce with `seq ≤ N` is rejected. See [SPEC.md §5.4](SPEC.md).

### Server Pin
The Resolver-side record binding an Origin's long-lived public key to a backend URL, certificate fingerprint, bearer token, list of paired Consumers, and per-Consumer access keys. The unit of revocation. Persisted in the [Server Registry](#server-registry). See [ARCHITECTURE.md §3.1](ARCHITECTURE.md).

### Server Registry
The Resolver's persistent store of [Server Pins](#server-pin) (`~/Library/Application Support/MCPBridge/registry.json` or platform equivalent). File mode 0600. See [DAEMON.md §7.1](DAEMON.md).

### SLSA
Supply-chain Levels for Software Artifacts. The provenance attestation framework used for every signed Bridge release. See [ARCHITECTURE.md §11](ARCHITECTURE.md), [SECURITY.md §8](SECURITY.md).

### Stable-Loopback Bridge
The working title for the architectural pattern MCP Bridge implements: a small Resolver process pins one stable localhost URL per registered server; Consumers are configured exactly once; all drift is absorbed by the Resolver. See [ARCHITECTURE.md §2](ARCHITECTURE.md).

---

## T

### Tauri
The cross-platform desktop-app framework (Rust core + native WebView UI) on which [Bridge Console](#bridge-console) is built. Chosen over Electron for binary size and security posture. See [UI.md](UI.md), [decisions/0002-tauri-over-electron.md](decisions/0002-tauri-over-electron.md).

### `target_resolver_pubkey`
The field inside a [Direction B](#direction-b) pair payload that binds the payload to one specific Resolver. The Resolver rejects payloads whose `target_resolver_pubkey` does not match its own public key — defeats re-targeting attacks. See [SPEC.md §4.4-§4.5](SPEC.md).

### Path B (icons)
The icon-strategy convention adopted across the UI: per-platform native icon sets (SF Symbols on macOS, Fluent on Windows, Lucide on Linux) rather than a single cross-platform set. See [UI.md §8](UI.md), [decisions/0005-path-b-per-platform-icons.md](decisions/0005-path-b-per-platform-icons.md).

---

## U

### Universal link
The `mcp-pair://<token>` URI scheme handled by the host app on the phone. Optional handoff from a QR scan into the host app's pair-confirmation UI. See [SPEC.md §4.2](SPEC.md).

---

## V

### Verbose mode
The user-opt-in logging mode that captures request and response bodies (which are never logged by default). Auto-reverts after a configurable duration; persistent UI banner while enabled so the user is never unknowingly in verbose mode. See [ARCHITECTURE.md §8.1](ARCHITECTURE.md), [PRIVACY.md §3](PRIVACY.md).

---

## See also

- [SPEC.md](SPEC.md) — normative wire-protocol grammar.
- [ARCHITECTURE.md](ARCHITECTURE.md) — cross-cutting design and rationale.
- [PRIVACY.md](PRIVACY.md) — what data Bridge collects (none) and how that's verifiable.
- [decisions/](decisions/) — Architecture Decision Records for the load-bearing choices.

# MCP Bridge — Architecture

Status: **exploratory** — not a roadmap commitment. Drafted 2026-05-22, current revision 2026-05-23. Supersedes [`BRIDGE_ARCHITECTURE_OUTDATED.md`](BRIDGE_ARCHITECTURE_OUTDATED.md). Lives in this repo so it doesn't get lost if it grows into a separate OSS project.

Working title for the pattern: **Stable-Loopback Bridge**.

---

## 1. Problem

Mobile MCP servers — fitness trackers, home-automation hubs, on-device journals, future apps — share two friction points that today get re-solved badly by every author:

1. **First-time configuration**: every MCP client (Claude Desktop, Cursor, Continue, Zed, future) has its own config file location, schema, and reload semantics. Each new server forces N hand-edits across N clients.
2. **Session-to-session drift**: a phone-resident server changes identity constantly. New Wi-Fi → new IP. App restart → new ephemeral port. Token rotation → new auth header. Cert renewal → new fingerprint. Every drift either breaks the client until manually re-configured, or forces the server author to rewrite N config files on N clients with N different reload models.

The second problem is the load-bearing one. First-time configuration is a one-off; drift recurs forever. A bridge that only handles first-time pairing is an installer; a bridge that handles drift is what actually keeps the user's clients working.

**Hypothesis**: a small Resolver process on the user's computer can pin one stable localhost URL per registered server. Consumers are configured exactly once against that URL. All drift on the server side is invisible to the Consumers, absorbed by the Resolver re-discovering the backend.

This document sketches the architecture. It is intentionally not a commitment to build.

---

## 2. The pattern: Stable-Loopback Bridge

```
┌──────────────────────────────┐       ┌──────────────────────────────────────────┐
│         Phone (Origin)       │       │             User's computer              │
│                              │       │                                          │
│ ┌──────────────────────────┐ │       │ ┌──────────────────────────────────────┐ │
│ │   Host app (BodyLog, …)   │ │       │ │           mcp-bridged                │ │
│ │                          │ │       │ │  (background process + menu-bar UI)  │ │
│ │ ┌──────────────────────┐ │ │       │ │                                      │ │
│ │ │  MCP server (HTTPS)  │◄┼─┼───────┼─┤  Origin Connector (per Server Pin)   │ │
│ │ └──────────────────────┘ │ │  TLS  │ │            ▲                         │ │
│ │ ┌──────────────────────┐ │ │ +pin  │ │            │                         │ │
│ │ │  Bridge Peer (SDK)   │◄┼─┼───────┼─►  Server Registry  ◄──── Bridge       │ │
│ │ └──────────────────────┘ │ │mDNS + │ │            │              Console    │ │
│ └──────────────────────────┘ │announce│ │            ▼              (UI)       │ │
│                              │       │ │     Loopback Listener                │ │
└──────────────────────────────┘       │ │     http://127.0.0.1:PORT/<name>     │ │
                                       │ └──────────────┬───────────────────────┘ │
                                       │                │                          │
                                       │   ┌────────────┼──────────────┐           │
                                       │   ▼            ▼              ▼           │
                                       │ Claude       Cursor       Continue  …    │
                                       │ Desktop                                   │
                                       │ (Consumers — each has a single, stable   │
                                       │  config entry pointing at the loopback)  │
                                       └──────────────────────────────────────────┘
```

Three roles, named consistently for the rest of the document:

| Role | Who | Responsibility |
|---|---|---|
| **Origin** | Phone-hosted MCP server | Source of truth for tools/resources; rotates its own identity freely |
| **Resolver** | `mcp-bridged` on the user's computer | Holds a stable localhost URL per Origin; re-discovers the Origin on every drift |
| **Consumer** | Claude Desktop, Cursor, Continue, … | Standard MCP clients; configured exactly once against the Resolver |

The pattern's central guarantee: **a Consumer's configuration never changes after first install, regardless of how the Origin drifts.**

---

## 3. Components

The Resolver runs as two cooperating processes — a long-running native daemon and an on-demand UI. Their internals are documented in companion files:

- [`DAEMON.md`](DAEMON.md) — `mcp-bridged` internals: process model, module layout, IPC surface, request flow, persistence, observability, CLI parity.
- [`UI.md`](UI.md) — Bridge Console: Tauri + Svelte 5 stack, window architecture, IPC consumer, styling for OS-native feel, icon strategy.
- [`UX.md`](UX.md) — Bridge Console: user-facing vocabulary, flows, screen mockups, empty/error states, copy, onboarding, accessibility.
- [`MOBILE.md`](MOBILE.md) — Origin-side: `@mcp-bridge/mobile` SDK surface, iOS / Android implementation, mobile-side privacy, host-app integration contract.
- [`PRIVACY.md`](PRIVACY.md) — privacy charter: threat model, data lifetimes, egress allowlist, verification paths, residual limits.
- [`LEGAL.md`](LEGAL.md) — source-of-truth index for licensing, privacy policy, ToS, trademark, export classification, distribution agreements.

This section gives the overview; the companion docs go deep.

### 3.1 Resolver (`mcp-bridged`)

Always-on background process. No UI of its own — exposes a JSON-RPC surface that Bridge Console and the `mcp-bridge` CLI both consume.

```
mcp-bridged
├── Bridge Console        — menu-bar / tray UI, also the installer's first-launch surface
├── Loopback Listener     — http://127.0.0.1:<PORT>/<name>, speaks MCP-over-HTTP to Consumers
├── Origin Connector(×N)  — one outbound MCP client per registered Origin, TLS-pinned
├── Server Registry       — persistent store of Server Pins (~/Library/Application Support/MCPBridge/registry.json)
├── Discovery Agent       — mDNS subscriber + QR webcam reader + deeplink handler
├── Client Adapter(×M)    — one per Consumer family (claude-desktop, cursor, continue, …)
└── Identity Keystore     — Resolver keypair + per-Origin pinned pubkeys + per-(Origin, Consumer) loopback keys + bearer tokens (OS keychain backed)
```

Each Server Pin in the Registry has the shape:

```jsonc
{
  "logical_id": "bodylog-7f3a...",
  "origin_pubkey": "ed25519:<base64url>",
  "display_name": "BodyLog",
  "backend": { "url": "https://10.0.0.42:54321/", "fp": "sha256:<hex>", "ca": "..." },
  "auth": { "type": "bearer", "rotated_at": "<unix-ts>" },   // token itself lives in Identity Keystore
  "scope": ["tools", "resources"],
  "consumers": [
    { "name": "claude-desktop", "key_id": "<uuid>", "allowed_tools": ["*"] },
    { "name": "cursor",         "key_id": "<uuid>", "allowed_tools": ["read_*"] }
  ],
  "last_seen_seq": 42,
  "cert_rotated_at": "<unix-ts>",
  "state": "Reachable"
}
```

`key_id` references the OS keychain entry holding the loopback `?key=` value for that Consumer; per-Consumer revoke flips `state` on that entry without disturbing the rest of the pin.

Stack candidate: Tauri (Rust core, small bundle ~3-5 MB, native UI). Electron is the obvious alternative but quintuples binary size. Platforms: macOS, Windows, Linux; signed builds per platform.

### 3.2 Mobile SDK (`@mcp-bridge/mobile`)

A Capacitor / React Native / pure-JS library any mobile MCP-server author drops in. Single runtime object, **Bridge Peer**, with three jobs: scan a Resolver invite QR, build and sign the `mcp-pair/v0.1` payload, and announce identity on the LAN.

```ts
import { BridgePeer } from "@mcp-bridge/mobile";

const peer = await BridgePeer.scanResolverInvite();   // camera-scanned QR from Resolver
await peer.pair({
  name: "BodyLog",
  logicalId: "bodylog-7f3a...",
  backend: { url, fp, ca },
  auth: { type: "bearer", value: "..." },
  scope: ["tools", "resources"],
});

peer.onStatus((s) => console.log(s)); // 'paired' | 'installed' | 'announced' | 'error'
```

---

## 4. Wire protocols

Two protocols, versioned independently. The `mcp-` prefix leaves room for upstream adoption later.

### 4.1 `mcp-pair/v0.1` — one-shot pairing (Origin → Resolver, out-of-band)

The protocol carries two messages in Direction B (`resolver_offered`): an **invite** the Resolver displays as a QR, and a **pair payload** the phone sends back, sealed to the Resolver's pubkey.

**Resolver invite** (encoded as the QR shown by Bridge Console):

```jsonc
{
  "spec": "mcp-pair/v0.1",
  "direction": "resolver_offered",
  "resolver": {
    "pubkey": "ed25519:<base64url>",          // pinned by phone after confirmation
    "display_name": "Patryk's MacBook Pro",   // shown on phone before any secret leaves
    "sas": "tiger-river-marble-clay",         // 4-word SAS = words(H(resolver.pubkey || nonce))
    "lan_addr": "https://10.0.0.5:8765/pair"  // where the phone POSTs the sealed payload
  },
  "nonce": "<base64url-128bit>",              // single-use, 60s lifetime, anti-replay
  "uri": "mcp-pair://..."                     // universal-link variant for handoff
}
```

Before sending any secrets, the phone displays `display_name` and `sas` and asks the user to confirm against the same `sas` printed below the QR on Bridge Console (audit C-3). On mismatch the user cancels; nothing leaves the phone.

**Pair payload** (sealed to `resolver.pubkey` via libsodium `crypto_box`, POSTed by phone):

```jsonc
{
  "spec": "mcp-pair/v0.1",
  "direction": "resolver_offered",
  "origin": {
    "name": "BodyLog",
    "pubkey": "ed25519:<base64url>",          // Origin's long-lived identity — pinned by Resolver
    "logical_id": "bodylog-7f3a..."            // stable across IP/port changes
  },
  "backend": {
    "url": "https://10.0.0.42:54321/",
    "fp": "sha256:<hex>",
    "ca": "-----BEGIN CERTIFICATE-----\n…"
  },
  "auth": {
    "type": "bearer",
    "value": "<base64url>"                    // current token; rotated by Origin, refreshed via announce
  },
  "scope": ["tools", "resources"],
  "nonce": "<base64url-128bit>",              // echoes invite nonce
  "target_resolver_pubkey": "ed25519:...",   // binds payload to this Resolver (audit C-2)
  "sig": "<ed25519 by origin.pubkey over canonical-JSON of all the above>"
}
```

Canonicalization for signing: RFC 8785 (JCS). The Resolver accepts the payload only when all of:

1. The outer seal opens with the Resolver's private key (proves the payload was addressed to *this* Resolver and the body is confidential)
2. `target_resolver_pubkey` matches the Resolver's own pubkey
3. `nonce` matches the currently active invite nonce, which has not yet expired (60s window) and has not been previously consumed
4. `sig` validates against `origin.pubkey`

In `origin_offered` direction (webcam scan fallback, see §5.5), the payload travels as-is inside the QR — no seal, no `target_resolver_pubkey`, no separate invite. The QR is the OOB channel.

### 4.2 `mcp-announce/v0.1` — recurring identity update (Origin → Resolver, on LAN)

Sent by Bridge Peer whenever the phone wakes, joins a network, rotates a token, renews a cert, or every 30s while connected (keepalive).

**Confidentiality on the LAN.** Bonjour TXT broadcasts are visible to anyone within multicast range. Two mitigations against inventory leakage (audit H-1) — implementations should ship the second by default:

1. **Randomized service type** — service name is `_mcp-bridge-<HMAC(resolver_pubkey, daily_salt)>._tcp.local` rather than a static well-known name. Resolver subscribes only to its derived service-types.
2. **Sealed body** (preferred) — the TXT body is sealed to `resolver.pubkey` via libsodium `crypto_box` with the phone's ephemeral keypair as sender. ~120 bytes overhead — fits within Bonjour TXT limits for a single record. Even `logical_id` is hidden from observers.

**Inner payload** (after unseal, or sent in plaintext via HTTP POST when carrier is unicast):

```jsonc
{
  "spec": "mcp-announce/v0.1",
  "lid": "bodylog-7f3a...",
  "backend": { "url": "https://10.0.0.42:54321/", "fp": "sha256:<hex>" },
  "auth_rotated_at": "<unix-ts>",           // optional; signals Resolver to re-fetch token (audit M-1)
  "cert_rotated_at": "<unix-ts>",           // optional; new fp accepted only when this increases (audit M-6)
  "seq": 42,                                 // strictly increasing per logical_id (audit H-3)
  "exp": "<unix-ts>",                        // freshness window
  "sig": "<ed25519 by origin.pubkey over canonical-JSON of the rest>"
}
```

**Acceptance rules.** The Resolver accepts an announce only when all of:

1. `origin.pubkey` is already pinned in the Server Registry for that `lid` (unknown LIDs ignored until paired via `mcp-pair`)
2. `seq > last_seen_seq` for that pin (defeats replay within the freshness window)
3. `exp` lies within `[now - 60s, now + 60s]` (clock-skew tolerance)
4. `sig` validates against the pinned `origin.pubkey`

**Rate limiting** (audit H-4). Cheap pre-filters drop traffic before any signature work:

- mDNS carrier: at most 4 verifications/sec per source IP, 1/sec per `lid`
- HTTP POST carrier: at most 8 verifications/sec per source IP

Records exceeding budget are dropped without logging.

**Rotation channels.**

- **Token rotation** (`auth_rotated_at` increases): Resolver opens a control call over the existing pinned backend TLS connection to fetch the new bearer token; pooled Origin Connector connections are closed and re-created with the new credential (audit M-7).
- **Backend cert rotation** (`cert_rotated_at` increases): Resolver accepts the new `fp` only when the announce is sig-valid and `cert_rotated_at` exceeds the pin's last recorded value.

That single pinning-plus-`seq` rule makes the LAN side hostile-network-safe.

### 4.3 Loopback face

Plain MCP over HTTP with per-(install, Consumer) access control. Consumers configure once:

```jsonc
{
  "mcpServers": {
    "bodylog": { "url": "http://127.0.0.1:8765/bodylog?key=<256-bit-base64url>" }
  }
}
```

- The path segment (`/bodylog`) maps 1:1 to a Server Pin's `logical_id`.
- The `?key=` parameter is a per-(Server Pin, Consumer) random token generated at pair time, stored in the OS keychain, and written into the Consumer's config alongside the URL. Constant-time comparison; missing or mismatched key → `401 Unauthorized` with no body (audit C-1).
- The Loopback Listener validates `Host:` is `127.0.0.1:<port>` or `localhost:<port>`. Anything else returns `421 Misdirected Request` before the key check fires — this defeats DNS-rebinding attacks from browser tabs (audit C-1).
- MCP protocol messages, including tool-consent flows, pass through transparently. The Bridge does not add or remove protocol-level interactions (audit M-3).

The Server Pin's `consumers` array holds per-Consumer key references; Bridge Console UI shows access per Consumer and supports per-Consumer revoke without revoking the pin itself (audit M-2).

---

## 5. Sequence diagrams

### 5.1 First install + first pair (Direction B — Resolver shows QR, phone scans)

```
User    Bridge Console     Resolver           Bridge Peer (phone)     Host app (BodyLog)
 │           │                │                       │                      │
 │ click "Pair new server"    │                       │                      │
 ├──────────►│                │                       │                      │
 │           │ generate nonce │                       │                      │
 │           ├───────────────►│                       │                      │
 │           │◄──── QR ───────┤  contains:            │                      │
 │           │                │   resolver.pubkey     │                      │
 │           │                │   resolver.lan_addr   │                      │
 │           │                │   nonce               │                      │
 │           │                │   spec="mcp-pair/v0.1"│                      │
 │           │                │   uri="mcp-pair://..."│                      │
 │           │                │                       │                      │
 │ open BodyLog, tap "Connect", point at QR            │                      │
 ├───────────┼────────────────┼──────────────────────►│                      │
 │           │                │                       │ universal-link opens │
 │           │                │                       ├─────────────────────►│
 │           │                │                       │                      │
 │           │                │                       │ phone shows confirm: │
 │           │                │                       │ "Pair BodyLog with    │
 │           │                │                       │  <display_name>?     │
 │           │                │                       │  SAS: <4-word>"      │
 │ verify SAS matches Bridge Console, tap Confirm     │                      │
 ├───────────┼────────────────┼──────────────────────►│                      │
 │           │                │                       │                      │
 │           │                │                       │ build pair payload   │
 │           │                │                       │ incl.                │
 │           │                │                       │ target_resolver_pub, │
 │           │                │                       │ sign, seal via       │
 │           │                │                       │ crypto_box to        │
 │           │                │                       │ resolver.pubkey      │
 │           │                │                       │◄─────────────────────┤
 │           │                │                       │                      │
 │           │                │                       │ POST sealed body to  │
 │           │                │  (no TLS required —   │ resolver.lan_addr    │
 │           │                │   seal authenticates) │ with nonce           │
 │           │                │◄──────────────────────┤                      │
 │           │                │                       │                      │
 │           │                │ unseal; verify        │                      │
 │           │                │ target_resolver_pub,  │                      │
 │           │                │ origin.sig, nonce;    │                      │
 │           │                │ pin origin.pubkey     │                      │
 │           │                │                       │                      │
 │ "BodyLog wants to install in Claude Desktop, Cursor" sheet                 │
 │           │                │                       │                      │
 │ confirm ─►│                │                       │                      │
 │           ├ Client Adapters write loopback URL ───►(Consumer configs)     │
 │           │                │ Origin Connector up   │                      │
 │           │                ├──────────────────────►│ status: 'installed'  │
 │           │                │                       ├─────────────────────►│
 │◄──────────┴────────────────┴───────────────────────┴──────────────────────┤
```

User-visible actions: open BodyLog and aim at the QR; glance at the SAS phrase and tap Confirm; tap Install on the bridge; restart the Consumer once. Four taps, one glance, no decisions that require understanding.

### 5.2 Steady-state request

```
Consumer                Loopback Listener           Origin Connector         Origin
   │                            │                          │                    │
   │ HTTP POST /bodylog          │                          │                    │
   │ {jsonrpc tools/call …}     │                          │                    │
   ├───────────────────────────►│                          │                    │
   │                            │ lookup pin "bodylog"      │                    │
   │                            │ attach Authorization      │                    │
   │                            ├─────────────────────────►│                    │
   │                            │                          │ TLS (cert pinned)  │
   │                            │                          ├───────────────────►│
   │                            │                          │                    │
   │                            │                          │◄───────────────────┤
   │                            │◄─────────────────────────┤                    │
   │◄───────────────────────────┤                          │                    │
```

Hot path: two hops, both localhost or LAN. Latency budget: <2ms loopback + LAN RTT.

### 5.3 Reconciliation — backend identity drift

Phone joins new Wi-Fi (new IP) — or rotates token — or renews cert.

```
Bridge Peer                   Discovery Agent          Origin Connector       Consumer
    │                               │                         │                  │
    │ mDNS TXT announce             │                         │                  │
    │ (signed by origin.pubkey)     │                         │                  │
    ├──────────────────────────────►│                         │                  │
    │                               │ verify sig against pin  │                  │
    │                               │ update Server Registry  │                  │
    │                               ├────────────────────────►│                  │
    │                               │                         │ re-resolve next  │
    │                               │                         │ inbound request  │
    │                               │                         │                  │
    │                               │                         │◄─── /bodylog ─────┤
    │                               │                         │ uses new backend │
```

**Consumer never notices.** No config rewrite. No restart. This is the architectural win that pays for the proxy.

### 5.4 Revoke

```
User                    Bridge Console        Server Registry      Consumer config files
 │ "Remove BodyLog"          │                       │                       │
 ├────────────────────────►│                       │                       │
 │                          │ mark pin Revoked      │                       │
 │                          ├──────────────────────►│                       │
 │                          │ Loopback returns 410   │                       │
 │                          │ for /bodylog thereafter │                       │
 │                          │                       │                       │
 │                          │ remove entries tagged  │                       │
 │                          │ _mcp_bridge_managed=<id>                       │
 │                          ├───────────────────────┼──────────────────────►│
```

Tag-based removal: the Resolver only touches lines it added.

### 5.5 Pairing direction fallbacks

Three fallbacks under the canonical Direction-B "Resolver shows QR" flow:

1. **Direction A — Origin shows QR, Resolver scans via webcam.** Whole `mcp-pair/v0.1` payload travels in the QR. For users with a webcam who can't get phone and computer on the same LAN.
2. **Short authentication string (SAS).** Resolver displays a 6-word phrase derived from `resolver.pubkey`; user types it into the host app. Slowest, but works without a camera on either device.
3. **File import.** Resolver exports `bridge-identity.json`; user transfers it to the phone (any channel — AirDrop, email, USB) and the host app imports. The escape hatch for the most hostile networks.

All four pathways converge on the same `mcp-pair/v0.1` payload. The branching is purely about *how the resolver's pubkey reaches the phone* via an out-of-band channel.

---

## 6. Trust model

Layered. Every channel after pairing is authenticated by something pinned during pairing. The QR scan plus the phone-side SAS confirmation are the only ceremonies that have to be right.

| Layer | Mechanism | What it defeats |
|---|---|---|
| **Pair (OOB)** | QR scanned via phone camera; phone-side SAS confirmation against Bridge Console display; pair payload sealed (`crypto_box`) to `resolver.pubkey` and signed by `origin.pubkey` over canonical fields including `target_resolver_pubkey`; nonce single-use, 60s lifetime | Active MITM on LAN, malicious mDNS responders, QR substitution, payload re-targeting |
| **Announce (LAN)** | TXT body sealed to `resolver.pubkey` or carried on a randomized service type; signed by pinned `origin.pubkey`; strictly increasing `seq`; `exp` ±60s; verification rate-limited | Spoofed announcers, replay within freshness window, inventory enumeration on the LAN, DoS via signature flood |
| **Backend TLS** | Cert pinning to `backend.fp`; rotation accepted only via sig-valid announce whose `cert_rotated_at` increases | DNS / IP hijack, cert swap by network attacker, downgrade |
| **Loopback** | `127.0.0.1` bind only; per-(Origin, Consumer) random key in URL parameter (constant-time compare); `Host:` header validated to `127.0.0.1`/`localhost` | Other local processes, DNS-rebinding from browser tabs, lateral movement |
| **Storage** | Resolver keypair, pinned origin pubkeys, bearer tokens, loopback keys all in OS keychain; registry file mode 0600 | Casual local access |

No upstream service. No cloud, no telemetry, no analytics. The Resolver talks only over local network + loopback.

**Residual presence leak**: even with sealed-body announces and randomized service-type names (§4.2), the *fact* that traffic exists on `_mcp-bridge-<hmac>._tcp.local` is observable to anyone on the LAN. We cannot close this without giving up Bonjour entirely. Users on hostile networks should use Settings → Privacy → "Pause discovery" together with manual-SAS pairing for fresh pairs. Documented in [`PRIVACY.md`](PRIVACY.md) §13.

### 6.1 Egress allowlist

Auditable network behaviour. The Resolver:

**Listens on**:
- `127.0.0.1:<port>` for Consumer requests (TCP HTTP)
- LAN address `:<pair-port>` for Direction-B pair POSTs (TCP HTTPS, sealed bodies)

**Connects outbound to**:
- Paired Origin backends — `backend.url` of each Server Pin, on tool calls and announce-driven control messages
- `updates.mcpbridge.me/manifest.json` — daily update manifest fetch, no query parameters, no identifiers

**Multicast**:
- `224.0.0.251:5353` — passive Bonjour subscription for `mcp-announce` records

That is the complete list. No analytics endpoints. No crash-report telemetry. No third-party SDKs. The Resolver source is open and reproducibly built; binary egress can be verified against this list via packet capture and provenance attestation (see §11).

---

## 7. State machines

### 7.1 Server Pin lifecycle

```
                 (mcp-pair payload validated)
   ── Unpaired ──────────────────────► Paired
                                          │
                                          │ (announce or fresh pair)
                                          ▼
                                    ┌─ Reachable ──────┐
        (announce stale > exp)      │                  │ (request → backend fail)
                                    │                  ▼
                                    │              Unreachable ──► retry/backoff
                                    │                  │
                                    │                  │ (next valid announce)
                                    └◄─────────────────┘
                                          │
                                          │ (user revokes)
                                          ▼
                                       Revoked
                                          │
                                          │ (purged after grace period)
                                          ▼
                                       (gone)
```

### 7.2 `mcp-bridged` runtime

```
   Installed ──► FirstLaunch ──► Idle ◄────────────────┐
                                  │                     │
                                  │ (pair trigger:      │
                                  │  webcam / drop /    │
                                  │  deeplink)          │
                                  ▼                     │
                              Pairing ─── confirm ──► Running ── tray quit ──► Stopped
                                  │                     ▲
                                  │ cancel              │ (always returns here)
                                  └─────────────────────┘
```

---

## 8. Failure modes and their telegraph

| Failure | Detection | Surfaced as |
|---|---|---|
| Origin unreachable (Wi-Fi off, phone asleep) | Origin Connector timeout | Loopback 503 with `X-MCP-Bridge-Reason: origin-unreachable`; Bridge Console badge |
| Announce signature fails | Discovery Agent | Logged, dropped silently (hostile traffic by definition) |
| Cert fp mismatch on backend | Origin Connector | Loopback 502; Bridge Console raises "BodyLog's certificate changed unexpectedly — open to review" |
| Consumer config file moved / schema changed | Client Adapter probe | Bridge Console marks Consumer "needs reattach"; per-Adapter doctor button |
| Resolver crashed | launchd / systemd restarts | Brief Loopback outage; Consumer surfaces its own "unreachable" until restart completes (≤2s) |
| Loopback port collision | Listener bind | Auto-rebind to next free; Adapters rewrite Consumer entries; only situation in which configs are rewritten in steady state |

Observability without telemetry: rolling local logs at `~/Library/Logs/mcp-bridge/`, one-click "copy diagnostics" button in Bridge Console.

### 8.1 Logging scope

Local-only, no telemetry. Defaults are deliberately narrow (audit H-6):

| Field | Default | Verbose mode (opt-in) |
|---|---|---|
| Method, path, status, `logical_id`, duration | logged | logged |
| Request bodies (tool arguments) | not logged | logged |
| Response bodies (tool results) | not logged | logged |
| Headers (excluding `Authorization`) | not logged | logged |
| `Authorization` headers, loopback `?key=` parameter | never logged | never logged |

Verbose mode is gated behind a Bridge Console toggle that shows a persistent banner in the UI while enabled — the user is never unknowingly in verbose mode. Rotation: 5 files × 2 MB; auto-purge on pin revoke. The "Copy diagnostics" button redacts `Authorization` headers and `?key=` parameters from the bundle before producing it, including in URLs that appear in error messages.

---

## 9. Distribution and first install

The bridge is the only component the user installs. Everything else is delivered by the host app. The first-install path has to be a single continuous flow from "I want to use BodyLog in Claude" to "BodyLog works in Claude," with no manual steps in between.

### 9.1 The primitive: short URL with embedded pairing token

The phone displays `mcpbridge.me/p/<token>` (also rendered as a QR). The URL carries intent — the token ties install and first-pair into one user action.

**Token lifetime is 5 minutes** from generation (audit H-5). If the user takes longer to reach the install + pair step, the token expires and the bridge politely fails with "this pairing link expired — reopen BodyLog to generate a new one." Short lifetime bounds the damage if the token leaks via filename, cloud sync (Dropbox / iCloud Drive), or accidental sharing.

Behaviour by environment:

1. **Desktop browser, Resolver already installed**: page-load probe of `mcp-bridge://pair/<token>` triggers the OS URI handler. Resolver opens with pair sheet pre-populated for that token. Browser tab never finishes loading.
2. **Desktop browser, Resolver not installed**: probe times out (~1.5s); page falls back to a UA-detected installer. The token survives install primarily via the post-install URI-scheme handoff — the landing page re-invokes `mcp-bridge://pair/<token>` automatically once the installer registers the scheme. A filename suffix on the download (`MCP-Bridge-Setup-<token>.dmg`) is the fallback if the scheme handoff fails; the installer parses the filename, registers the scheme, scrubs the token from disk, and opens the pair sheet.
3. **Phone browser**: interstitial "this link is for your computer," with the channels in §9.2 listed underneath.

### 9.2 Cross-device delivery paths

Ranked by friction. Phone UI shows URL + QR as the primary display, with "Other ways to send" as a tap-to-expand row.

| Path | Action count | Coverage |
|---|---|---|
| **AirDrop the URL** | One tap on phone share sheet, pick Mac; Safari opens automatically on Mac | iPhone ↔ Mac |
| **Universal Clipboard** | Copy on phone (one tap), ⌘V on Mac | iPhone ↔ Mac, same iCloud |
| **Handoff (web)** | Open URL in Safari on phone; Mac Dock shows Handoff icon | iPhone ↔ Mac, same iCloud |
| **Scan QR with computer webcam** | One scan | Most laptops |
| **Type URL on computer** | ~6 keystrokes (`mcpbridge.me/p/` + token) | Universal |
| **Email link to self** | Tap, switch to Mail on computer, click | Universal, slow |
| **SMS link to self** | Tap, open Messages / Phone Link on computer, click | Universal, slow |
| **Package manager** (`brew install mcp-bridge && mcp-bridge pair <token>`) | Copy-paste one line | Power users |

### 9.3 The cross-device install question — what Apple does and doesn't allow

There is **no Apple API that lets an iPhone remotely trigger installation of a developer-distributed app on a Mac.** The platform's security model treats that as a malware vector. What is available:

- **Mac App Store + Handoff** — if the app is on the Mac App Store, opening its page on phone triggers a Handoff icon on the Mac's Dock; one click lands on the install page. **But MAS sandboxing forbids cross-app config writes** without per-path user grants via `NSOpenPanel`, which defeats the zero-friction goal. Keep MAS as a possible "thin edition" much later; main channel is direct download.
- **AirDrop the installer file** — transfers a DMG/PKG to the Mac's Downloads. User still double-clicks and passes Gatekeeper. This isn't really "install transfer," it's just file delivery.
- **AirDrop the URL** — transfers a URL to the Mac's browser. The smart landing page in §9.1 then does the rest. Functionally the closest thing to "install transfer" Apple's stack permits, and it costs one phone tap.

The pragmatic answer is therefore: **transfer the URL, not the binary.** AirDrop the short URL into the Mac; the landing page either hands off to an already-installed Resolver or pulls the right installer with the token preserved. Two phone taps, zero typing, zero scanning.

### 9.4 Smart landing page details

Three behaviours stack:

1. **Probe before pitch**: try the URI scheme on load, render installer UI only on timeout.
2. **UA-detected installer**: single big button per platform; do not make the user choose.
3. **Token survives install**: post-install URI-scheme handoff is the primary path; download-filename suffix is the fallback. Installer scrubs the token from disk on first launch.

**Hosting and privacy** (audit H-7). The landing page is served as a static asset from a CDN configured with **access logging disabled** — no IP, no User-Agent, no `Referer`, no token captured server-side. The page source is open-published so users (and auditors) can verify behaviour matches the published claim. A tokenless URL — `mcpbridge.me/get` — is available for users who want to install without binding a pairing token. The landing page itself carries a brief privacy notice linking to the source repository:

> "This page is served from a no-log CDN. The token in your URL is read only by the JavaScript on this page and the installer it offers; nothing about you is recorded here. Source: [link]."

### 9.5 Branding copy

The phone screen should not say "Install MCP Bridge." That's jargon and reads as a chore. Lead with the user's mental model — for example, "Connect BodyLog to apps on your computer" — and let the bridge brand surface only on the landing page and in the installer chrome.

---

## 10. Open architectural decisions

Worth pinning down before phase-1 code, in priority order:

1. **Loopback port: fixed or dynamic?** Fixed (8765) means zero rewrites ever but risks collision. Dynamic with auto-rebind means writing-once-and-occasionally. Likely: try fixed, fall back to dynamic with rewrite on collision only.
2. **Origin Connector lifetime.** Cold-start on first request (saves memory) vs. keep-warm with idle TCP to backend (saves first-request latency). Likely keep-warm with 5-min idle close.
3. **MCP transport coverage.** Spec supports HTTP+SSE, stdio, and emerging WebSocket. Phase 1 should cover HTTP+SSE only — what mobile MCP servers use.
4. **Consumer restart UX.** Resolver can detect Consumer process and prompt to restart, or rely on the Consumer's own config-reload. Per-Adapter capability flag.
5. **Multi-host pairing.** Two laptops, one phone — both want BodyLog. The pin is per (Origin, Resolver) pair; phone holds N Resolver pins. Cheap to support, just say so in the spec.
6. **Auto-update channel privacy.** Update check should be a GET to a static manifest on a CDN with no query parameters and no client identifiers — IP is unavoidable, everything else stays absent. Update bundle signature verified against a key embedded in the Resolver binary. Cadence: probably daily, with a manual "check for updates" affordance in Bridge Console.
7. **Supply-chain attestation.** GitHub Actions matrix builds with SLSA provenance attestation published alongside each release. Reproducible builds where the toolchain allows. Signing-key rotation policy documented before the first signed release.

---

## 11. Phased plan

| Phase | Deliverable | Effort |
|---|---|---|
| 0 | This doc | done |
| 1 | `mcp-pair/v0.1` + `mcp-announce/v0.1` spec markdown + a CLI proof-of-concept that runs the Loopback Listener and writes Claude Desktop config. Plus a 200-line iOS test harness that does QR-anchored pairing end-to-end. | 1 week |
| 2 | Tauri menu-bar Resolver with Claude Desktop writer only, signed + notarized-and-stapled + SLSA-provenance macOS build, smart landing page (no-log CDN) + token-aware installer | 1-2 weeks |
| 3 | Cursor + Continue writers, Windows + Linux signed + provenance-attested builds, package manager publishing | 1 week |
| 4 | Build out `@mcp-bridge/mobile` — Capacitor iOS plugin, Android plugin, JS SDK wrapping both. AirDrop-URL share affordance on iOS. | 1-2 weeks |
| 5 | File RFC at [modelcontextprotocol/spec](https://github.com/modelcontextprotocol) with reference implementation already in production | days of writing, months of consensus |

Phase 1 is the architecture verdict. Stop after that if it doesn't hold up end-to-end on a real phone.

---

## 12. Why not …

- **Original config-rewriter design** — see [`BRIDGE_ARCHITECTURE_OUTDATED.md`](BRIDGE_ARCHITECTURE_OUTDATED.md). Rewrites Consumer configs on every Origin drift; multiplies adapter code by N clients × M schema versions; requires system-keychain cert trust. Replaced by this document.
- **mDNS hostname only** (server publishes `bodylog.local`) — solves IP drift but not port, token, or cert rotation. `.local` TLS certs are awkward. Doesn't reach 70% of the drift problem.
- **Cloud sync (iCloud / Dropbox)** — adds a third-party trust relationship and an upstream provider for a payload that has perfectly good local transports.
- **`mcp+pair://` deeplink only** — relies on every Consumer implementing the handler natively, which is the gap the Resolver exists to paper over. Useful as a *transport* into the Resolver, not as a replacement.
- **OS Share Sheet only** — useful as a fallback (iOS AirDrop → Resolver), but a Share-Sheet-only design forces every mobile MCP server to know how to render a file payload, which is exactly the per-app friction the SDK avoids.
- **Mac App Store as primary distribution** — sandbox forbids the cross-app config writes that are the Resolver's reason to exist. Keep MAS as a possible thin edition much later.

---

## 13. Status

Not committed. Mobile apps with built-in MCP servers can already ship working integrations via manual QR + Copy-MCP-config + AirDrop-share flows. If a second such app appears that would benefit from a shared SDK + Resolver, revisit phase 1.

Sustained maintenance — Client Adapter updates per Consumer release, signing certs, security responses — is the real cost. Build only if there's a person prepared to own it.

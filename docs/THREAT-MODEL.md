# MCP Bridge — Threat Model

Last revised: 2026-05-24.

This document is the **consolidated threat model** for MCP Bridge. It joins the three threat-model views that previously lived only in their respective documents:

- The cryptographic / network trust model in [`ARCHITECTURE.md`](ARCHITECTURE.md) §6.
- The privacy threat model in [`PRIVACY.md`](PRIVACY.md) §2.
- The mobile-side delta in [`MOBILE.md`](MOBILE.md) §8.

Each of those documents remains authoritative for its scope. This page is the single landing surface for security researchers, auditors, and anyone evaluating whether Bridge is the right tool for a given threat environment.

> **Status**: pre-1.0. The claims in this document describe the *intended* behavior of the system as specified. As code lands, each row of every table should link to the file that enforces the row.

---

## 1. Scope

In scope: every component MCP Bridge ships or specifies.

- The Rust daemon (`mcp-bridged`) and its modules ([`DAEMON.md`](DAEMON.md)).
- [Bridge Console](GLOSSARY.md#bridge-console) — the Tauri + Svelte UI process ([`UI.md`](UI.md)).
- The official mobile SDK (`@mcp-bridge/mobile`) and every packaging in [`MOBILE.md`](MOBILE.md) §2.
- The wire protocols `mcp-pair/v0.1` and `mcp-announce/v0.1` ([`SPEC.md`](SPEC.md)).
- Signed installers and the update channel for any platform we distribute.
- The smart landing page on `mcpbridge.me`.
- SLSA provenance attestations.

Out of scope: the AI clients themselves (Claude Desktop, Cursor, Continue, …), the user's operating system, the host app's own MCP server implementation, and physical access to an unlocked machine. Out-of-scope items are called out explicitly in [`SECURITY.md`](SECURITY.md) §3.2 and §5.

---

## 2. Assets

Combined security + privacy view. Ranked by sensitivity.

| Rank | Asset | Why it matters |
|---|---|---|
| 1 | **Tool-call activity** — which tools, when, with what arguments | The most sensitive payload class; paired with user behavior |
| 2 | **Origin private key** (per host app, on the phone) | A pinned Ed25519 identity an attacker could use to mint valid announces or pair payloads |
| 3 | **Origin bearer token** (per Pin) | Direct backend access if extracted |
| 4 | **Loopback per-`(Pin, Consumer)` key** | Local-process barrier to the Pin's loopback URL |
| 5 | **Resolver private key** | Pinned by phones at pair time; rotation requires re-pairing every phone |
| 6 | **Pinned identities** (Origin pubkeys, cert fingerprints, LIDs) | Correlatable across machines and contexts |
| 7 | **Machine identifiers** (paths revealing username, hostnames, display name) | Lower-sensitivity but easily leaked through logs |
| 8 | **Existence of MCP Bridge on this machine** | Even mere installation is information |

---

## 3. Adversaries

The full union of adversaries from the three source documents. Each adversary has a primary mitigation; specifics live in §5–§7.

### 3.1 Network-positioned

| Adversary | What they want | Primary mitigation |
|---|---|---|
| Active MITM on the LAN | Substitute a malicious QR, impersonate the Resolver | Phone-side [SAS](GLOSSARY.md#sas) confirmation; pair payload sealed to `resolver.pubkey` and signed by `origin.pubkey` |
| Malicious mDNS responder | Inject forged announces | Announces accepted only against the pinned `origin.pubkey`; unknown LIDs ignored ([SPEC.md §5.5](SPEC.md)) |
| Co-located LAN observer | Inventory what the user runs | [Sealed-body](GLOSSARY.md#sealed-body) announces; randomized service-type names ([SPEC.md §5.2](SPEC.md)); "Pause discovery" toggle |
| DNS / IP hijack on the backend | Re-route the Origin Connector | Certificate fingerprint pinning; rotation requires the [`cert_rotated_at` ratchet](GLOSSARY.md#ratchet) |
| Browser tab from another origin | Pivot through the loopback face | `127.0.0.1`-only bind; `Host:` header check returns `421` before the key check ([SPEC.md §6.2](SPEC.md)) |
| Network observer of update traffic | Track install base, geolocate | TLS pinning; no query parameters; randomized check time; manual opt-out |

### 3.2 Local

| Adversary | What they want | Primary mitigation |
|---|---|---|
| Other local processes / browser tabs | Reach the Pin's backend via loopback | Per-`(Pin, Consumer)` `?key=`; constant-time compare; [Host-header check](GLOSSARY.md#host-header-check) |
| Casual local access (unlocked machine) | Read registry, tokens | Registry file mode `0600`; tokens and keys in OS keychain |
| Forensic recovery from a disposed laptop | Read recoverable Resolver state | At-rest encryption assumption; clean `--purge` uninstall ([`DAEMON.md`](DAEMON.md) §2.1) |
| User's backup software | Mirror sensitive state to cloud | Backup-exclusion xattrs (macOS), `.metadata_never_index`, `NOT_CONTENT_INDEXED` (Windows) |
| OS telemetry / crash reporter | Capture process memory containing secrets | Core dumps disabled; `zeroize` on every sensitive type on desktop ([`DAEMON.md`](DAEMON.md) §7.3) |
| Webview telemetry (WebView2, WKWebView, WebKitGTK) | Phone home about navigation / state | Explicit per-platform hardening ([`UI.md`](UI.md) §5.4) |

### 3.3 Mobile-specific

| Adversary | What they want | Primary mitigation |
|---|---|---|
| Host-app process memory dumped via debugger / iOS sysdiagnose | Extract Origin private key | Hardware-backed Keychain / Keystore; sign operations never expose private keys ([`MOBILE.md`](MOBILE.md) §8.1) |
| Screenshot in app switcher leaking SAS | See the verification phrase | Blur / `FLAG_SECURE` during SAS confirmation ([`MOBILE.md`](MOBILE.md) §8.3) |
| App switcher snapshot uploaded to iCloud Backup | Recover SAS or pair state | `kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly`; backup-exclusion ([`MOBILE.md`](MOBILE.md) §8.5) |
| Same-device sibling apps (sandboxed) | Reach the host's MCP server | OS sandbox; SDK refuses App Group keychain attribute on the TLS key by default ([`MOBILE.md`](MOBILE.md) §8.8) |
| `adb` debugging on Android (developer mode left on) | Extract SDK state | Hardware-backed Keystore prevents key extraction; `EncryptedSharedPreferences` mitigates non-Keystore state |

### 3.4 Supply chain & operator

| Adversary | What they want | Primary mitigation |
|---|---|---|
| Update-channel compromise | Sign and ship a backdoored binary | Manifest signature verified against a key embedded in the Resolver binary; SLSA provenance per release ([`ARCHITECTURE.md`](ARCHITECTURE.md) §11) |
| Compromised CDN or DNS for `updates.mcpbridge.me` | Serve a poisoned manifest | TLS pinning; manifest-signature check rejects unsigned or wrong-signed manifests |
| The bridge's own operator (us) | Build analytics, profile usage, monetize | We don't collect; verifiable through open source + reproducible builds + reproducible-build verification by anyone ([`PRIVACY.md`](PRIVACY.md) §12) |
| "Future-us" in monetization panic | Quietly add "small" analytics events | Self-imposed constraints in the docs + reproducible builds means quiet additions cannot stay quiet |
| Malicious dependency phoning home | Side-channel exfiltration | [`CONTRIBUTING.md`](CONTRIBUTING.md) §6.1 dependency intake checklist; egress allowlist verifiable via packet capture |

---

## 4. Trust boundaries

```
┌─────────────────────────────────────────────────────────────────────┐
│  Phone (Origin)                                                     │
│  ┌─────────────────┐                                                │
│  │ Host app sandbox│  ── boundary A ──► OS Keychain/Keystore       │
│  │ ┌──────────────┐│                                                │
│  │ │ MCP server   ││                                                │
│  │ │ + Bridge Peer││                                                │
│  │ └──────────────┘│                                                │
│  └────────┬────────┘                                                │
└───────────┼─────────────────────────────────────────────────────────┘
            │
            │ boundary B: LAN (mDNS + HTTPS to Resolver)
            ▼
┌───────────┴─────────────────────────────────────────────────────────┐
│  User's computer                                                    │
│  ┌──────────────────────────────┐                                   │
│  │ mcp-bridged process          │  ── boundary C ──► OS Keychain    │
│  │ ┌──────────────────────────┐ │                                   │
│  │ │ Loopback Listener        │ │                                   │
│  │ └────────┬─────────────────┘ │                                   │
│  └──────────┼───────────────────┘                                   │
│             │ boundary D: 127.0.0.1                                 │
│             ▼                                                       │
│  Consumers (Claude Desktop, Cursor, …)                              │
│                                                                     │
│  Bridge Console (Tauri) ── boundary E ──► IPC to mcp-bridged       │
└─────────────────────────────────────────────────────────────────────┘

Boundary F: outbound to updates.mcpbridge.me (signed manifest only)
```

| Boundary | Who/what is on each side | What crosses |
|---|---|---|
| **A** — Host app ↔ OS keychain | Application code ↔ hardware-backed storage | Sign-operation requests; bearer-token reads. Private keys never leave. |
| **B** — Phone ↔ LAN ↔ Resolver | Origin ↔ network ↔ Resolver | Sealed pair payloads (Direction B); sealed-body announces; signed pair payloads (Direction A) |
| **C** — Resolver ↔ OS keychain | Daemon process ↔ hardware-backed storage | Sign-operation requests; bearer-token reads; per-Consumer loopback-key reads |
| **D** — Resolver ↔ Consumer | `mcp-bridged` ↔ AI client | MCP-over-HTTP traffic, gated on `Host:` header + `?key=` |
| **E** — Bridge Console ↔ daemon | UI process ↔ daemon process | Local JSON-RPC; daemon owns all security-critical state |
| **F** — Resolver ↔ update endpoint | Daemon ↔ `updates.mcpbridge.me` | Signed manifest GET; no identifiers, no query parameters |

Every boundary is enforced by something pinned during pairing (B, D), by the OS (A, C), or by signature verification (F).

---

## 5. Threats by component

### 5.1 Pairing (`mcp-pair/v0.1`)

| Threat | Defense | Where specified |
|---|---|---|
| QR substitution | Phone-side SAS confirmation; user cancels on mismatch | [`SPEC.md`](SPEC.md) §4.2 |
| Pair-payload re-targeting (attacker re-sends a captured payload to a different Resolver) | `target_resolver_pubkey` matched against the Resolver's own pubkey | [`SPEC.md`](SPEC.md) §4.5 |
| Replay within the invite window | Single-use 16-byte nonce; 60-second lifetime; consumed-nonce record | [`SPEC.md`](SPEC.md) §4.3 |
| Eavesdropping on the LAN pair POST | `crypto_box` seal to `resolver.pubkey`; the body is confidential regardless of TLS | [`SPEC.md`](SPEC.md) §4.4 |
| Forged Origin identity at pair time | `sig` verified against `origin.pubkey`; the pubkey is pinned on acceptance | [`SPEC.md`](SPEC.md) §4.5 |
| First-time Origin-pubkey impersonation | OOB SAS confirms the *Resolver*; the Origin is trusted on first use bounded by the human who tapped Confirm. Re-pair with the same LID and a different pubkey requires explicit user confirmation, surfacing the previous fingerprint | [`SPEC.md`](SPEC.md) §4.6 |
| Token leak via QR screenshot | 5-minute lifetime on the install-flow token ([`ARCHITECTURE.md`](ARCHITECTURE.md) §9.1, audit H-5) | [`ARCHITECTURE.md`](ARCHITECTURE.md) §9 |

### 5.2 Announce (`mcp-announce/v0.1`)

| Threat | Defense | Where specified |
|---|---|---|
| Spoofed announcer | Signature verified against pinned `origin.pubkey`; unknown LIDs dropped | [`SPEC.md`](SPEC.md) §5.5 |
| Replay within the freshness window | Strictly-increasing per-LID `seq`; `exp` ±60s | [`SPEC.md`](SPEC.md) §5.4 |
| Signature-flood DoS | Pre-signature rate limits: mDNS ≤4/sec per source IP, ≤1/sec per LID; HTTP ≤8/sec per source IP | [`SPEC.md`](SPEC.md) §5.6 (audit H-4) |
| Inventory leak on the LAN | Sealed-body announces; randomized service-type names with daily salt | [`SPEC.md`](SPEC.md) §5.2 (audit H-1) |
| Cert-rotation forgery (push a new fingerprint) | New `fp` accepted only when `cert_rotated_at` strictly increases under a sig-valid announce | [`SPEC.md`](SPEC.md) §5.8 (audit M-6) |
| Token-rotation forgery (force token re-fetch) | `auth_rotated_at` must strictly increase; new token fetched over the existing pinned backend TLS connection | [`SPEC.md`](SPEC.md) §5.7 (audit M-1, M-7) |

### 5.3 Backend (Origin Connector ↔ Origin)

| Threat | Defense | Where specified |
|---|---|---|
| Network-level cert swap | Certificate-fingerprint pinning at the connector | [`ARCHITECTURE.md`](ARCHITECTURE.md) §6 |
| Downgrade to non-TLS | `backend.url` is `https://`-only; `http://` is rejected at pair time | [`SPEC.md`](SPEC.md) §4.4 |
| Token re-use after rotation | All pooled connections closed on `auth_rotated_at` ratchet | [`SPEC.md`](SPEC.md) §5.7 (audit M-7) |

### 5.4 Loopback face (Resolver ↔ Consumer)

| Threat | Defense | Where specified |
|---|---|---|
| DNS-rebinding from a browser tab | `Host:` header check returns `421 Misdirected Request` before any key check | [`SPEC.md`](SPEC.md) §6.2 (audit C-1) |
| Other local processes connecting via guessed URL | Per-`(Pin, Consumer)` random `?key=`; constant-time compare; mismatch → `401` with no body | [`SPEC.md`](SPEC.md) §6.2 (audit C-1) |
| Bind to non-loopback interface | Listener bound to `127.0.0.1` (and optionally `::1`) only | [`SPEC.md`](SPEC.md) §6.1 |
| Tool-consent flow interception | Resolver forwards MCP messages transparently; no synthesis or modification | [`SPEC.md`](SPEC.md) §6.3 (audit M-3) |
| Per-Consumer compromise → all-Pins compromise | Per-Consumer `key_id` references; per-Consumer revoke flips state without disturbing other Consumers | [`ARCHITECTURE.md`](ARCHITECTURE.md) §3.1 (audit M-2) |

### 5.5 Storage (Resolver side)

| Threat | Defense | Where specified |
|---|---|---|
| Casual local read of registry | Registry file mode `0600`; per-user ACL on Windows | [`DAEMON.md`](DAEMON.md) §7.1 |
| Secrets in memory longer than necessary | `Zeroize` + `ZeroizeOnDrop` on every sensitive type | [`DAEMON.md`](DAEMON.md) §7.3 |
| Memory disclosure via core dump | Core dumps disabled on the daemon process | [`PRIVACY.md`](PRIVACY.md) §2 |
| Secrets in cloud backups | Backup-exclude xattrs, `.metadata_never_index`, `NOT_CONTENT_INDEXED` | [`DAEMON.md`](DAEMON.md) §7.4 |
| Spotlight / Windows Search indexing leaks | `.metadata_never_index` (macOS); `NOT_CONTENT_INDEXED` (Windows) | [`DAEMON.md`](DAEMON.md) §7.4 |
| Token leak in logs | `Authorization` headers and `?key=` parameters **never** logged, in any verbosity mode | [`ARCHITECTURE.md`](ARCHITECTURE.md) §8.1 |
| Sensitive content in default logs | Request and response bodies not logged at default verbosity; Verbose mode is opt-in with persistent UI banner and auto-revert | [`ARCHITECTURE.md`](ARCHITECTURE.md) §8.1 (audit H-6) |

### 5.6 Storage (Origin side, mobile)

| Threat | Defense | Where specified |
|---|---|---|
| Process memory dump of Origin private key | Native sign operations via Keychain (`SecKeyCreateSignature`) / Keystore (`Signature.sign`); key never leaves the secure boundary | [`MOBILE.md`](MOBILE.md) §8.1 |
| String interning preserves bearer in memory | `CharArray` over `String` on Android; `memset_s` on iOS for sensitive buffers | [`MOBILE.md`](MOBILE.md) §8.2 |
| Cloud backup copies SDK state to iCloud / Google | `kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly` (iOS); `android:allowBackup=false` (Android); Keystore entries are not exportable regardless of backup settings | [`MOBILE.md`](MOBILE.md) §8.5 |
| Uninstall residue on the phone | iOS 11+ deletes app keychain entries on uninstall; Android deletes `EncryptedSharedPreferences` + Keystore entries scoped to the app's UID | [`MOBILE.md`](MOBILE.md) §8.4 |

### 5.7 Update channel

| Threat | Defense | Where specified |
|---|---|---|
| Manifest poisoning | Manifest signature verified against a key embedded in the Resolver binary | [`ARCHITECTURE.md`](ARCHITECTURE.md) §10 |
| Downgrade attack via stale manifest | Manifest carries a strictly-monotonic version; older versions rejected | [`ARCHITECTURE.md`](ARCHITECTURE.md) §10 |
| Update-channel tracking | No query parameters, no client identifiers, no `User-Agent` distinguishing field; randomized check time | [`ARCHITECTURE.md`](ARCHITECTURE.md) §9.4 (audit H-7) |
| Compromised release-signing key | Documented key-rotation policy before first signed release; SLSA provenance per release | [`ARCHITECTURE.md`](ARCHITECTURE.md) §11 |
| Backdoored binary that "looks legitimate" | Reproducible builds where the toolchain allows; anyone can rebuild and verify the binary hash | [`PRIVACY.md`](PRIVACY.md) §12 |

### 5.8 Smart landing page (`mcpbridge.me/p/<token>`)

| Threat | Defense | Where specified |
|---|---|---|
| CDN access logs reveal install-time identifiers | CDN configured with access logging **disabled**; no IP, no UA, no Referer, no token captured server-side | [`ARCHITECTURE.md`](ARCHITECTURE.md) §9.4 (audit H-7) |
| Token leak via cloud-sync of downloaded installer filename | 5-minute token lifetime bounds the damage; installer scrubs the token from disk on first launch | [`ARCHITECTURE.md`](ARCHITECTURE.md) §9.1 (audit H-5) |
| Phishing variant of the landing page | Page source is open-published; users (and auditors) can verify behaviour matches the published claim | [`ARCHITECTURE.md`](ARCHITECTURE.md) §9.4 |

---

## 6. Mobile-side deltas vs desktop

Mobile resists adversaries desktop does not face, and faces ones the desktop does not have direct exposure to. See [`MOBILE.md`](MOBILE.md) §8.9 for the canonical view.

**Mobile additional adversaries**: process-memory dumps via debugger, app-switcher screenshots, sibling-app loopback access (iOS App Group misconfiguration), `adb`-debug extraction.

**Desktop adversaries the mobile side does not face directly**:

- DNS-rebinding from browser tabs — there is no Loopback Listener on the phone exposed to other local processes.
- Other local processes reading the daemon's loopback — same reason.
- Webview telemetry — no webview is involved in the SDK.

---

## 7. Residual risks (acknowledged, not waved away)

Bridge does **not** claim to defeat the following. Users facing adversaries in this class should layer additional measures (Tor, Tails, dedicated hardware, air-gapped pairing).

| Residual risk | Why | Mitigation available |
|---|---|---|
| **LAN-presence detection** — the existence of `_mcp-bridge-<hmac>._tcp.local` traffic is observable on the LAN, even with sealed bodies and randomized service-type names | We cannot close this without giving up Bonjour entirely | Settings → Privacy → "Pause discovery" + manual-SAS pairing for fresh pairs ([`ARCHITECTURE.md`](ARCHITECTURE.md) §6, [`PRIVACY.md`](PRIVACY.md) §13) |
| **OS-level network metadata** — DHCP records, router logs, ISP records | Outside Bridge's scope | None within Bridge; user can use VPN |
| **User-configured backup software that respects xattrs but is then manually overridden** | User's choice | Documentation in Settings about what xattrs mean |
| **Keychain access by other apps as the same user** | macOS prompts on first cross-app access; Linux Secret Service is unlocked once per session | Best-in-class OS isolation; defense-in-depth via per-Pin loopback keys |
| **Compromised user account on the laptop** | Once an attacker has root or the user's session, no userspace tool can defend against them | Out of scope; documented in [`SECURITY.md`](SECURITY.md) §3.2 |
| **Theoretical break of widely-deployed primitives** (Ed25519, X25519, ChaCha20-Poly1305, TLS 1.3) | We'd rotate; the report belongs upstream | Out of scope; documented in [`SECURITY.md`](SECURITY.md) §3.2 |
| **Memory residue beyond zeroize windows on mobile** | JVMs and ARC make zero residue impossible without OS support | Minimum dwell time + OS secure storage for everything with a longer-than-instant lifetime ([`MOBILE.md`](MOBILE.md) §8.2) |

---

## 8. How to verify

The model above is only as good as its verifiability. A skeptical user, auditor, or researcher can:

1. **Read the source.** The daemon and SDK are open. Audit `src/proxy/connector.rs`, `src/update/`, `src/announce/` for outbound behavior; `src/listener/` for the loopback face; the SDK's `commonMain` for the protocol core.
2. **Network capture.** Run `tcpdump` / Wireshark while using Bridge. Captured outbound destinations should match the [egress allowlist](ARCHITECTURE.md#61-egress-allowlist) exactly.
3. **In-product egress monitor.** Settings → Privacy → Outbound connections shows the daemon's own view of its connections. Should match the packet capture.
4. **Reproducible builds.** Distributed binaries match the source via SLSA provenance attestation. Anyone can rebuild and verify the binary hash matches what we shipped.
5. **Filesystem inspection after `--purge`.** Scan the filesystem for any `mcp-bridge` / `MCPBridge` artefact. None should remain.
6. **Conformance tests.** Run `cargo test --features conformance` against the JSON test vectors in `test-vectors/`. Mismatches between an Origin implementation and the Resolver are caught here, including SAS derivation, signature canonicalization, and the rate-limit envelope.

If any of these verification steps fails, [report it](SECURITY.md).

---

## 9. Change log

| Date | Change |
|---|---|
| 2026-05-24 | Initial consolidation from [`ARCHITECTURE.md`](ARCHITECTURE.md) §6, [`PRIVACY.md`](PRIVACY.md) §2, [`MOBILE.md`](MOBILE.md) §8. |

---

## 10. Related documents

- [`SECURITY.md`](SECURITY.md) — disclosure policy, response SLA, safe harbor, severity classification.
- [`SPEC.md`](SPEC.md) — normative wire-protocol grammar that the threats above are evaluated against.
- [`ARCHITECTURE.md`](ARCHITECTURE.md) — trust model overview, egress allowlist, sequence diagrams.
- [`PRIVACY.md`](PRIVACY.md) — privacy charter, data lifetimes, verification paths, residual limits.
- [`MOBILE.md`](MOBILE.md) — mobile-side privacy and security details, packaging-specific concerns.
- [`DAEMON.md`](DAEMON.md) — implementation details for the desktop daemon, including persistence, logging, and zeroization.
- [`UI.md`](UI.md) — webview hardening, IPC posture between Bridge Console and the daemon.

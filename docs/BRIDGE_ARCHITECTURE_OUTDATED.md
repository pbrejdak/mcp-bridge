# MCP Bridge — Universal Pairing App (OUTDATED — original config-rewriter design)

> **OUTDATED — superseded by [`ARCHITECTURE.md`](ARCHITECTURE.md) on 2026-05-23.**
>
> This document captures the original config-rewriter design. After review it was replaced
> by the loopback-proxy architecture in [`ARCHITECTURE.md`](ARCHITECTURE.md). Kept for historical
> context only — do not implement from this file.
>
> Why it was replaced:
> - The design optimised for one-time pairing but treated session-to-session drift (new IP,
>   rotated token, renewed cert) as an afterthought. In practice drift is the dominant case
>   for phone-hosted MCP servers, and rewriting four client configs in four schemas on every
>   drift is fragile.
> - The new design moves Consumers to a stable localhost URL behind a thin MCP proxy. Configs
>   are written exactly once and never touched again; drift is absorbed Resolver-side and
>   invisible to clients.
> - System-keychain cert trust is no longer required — TLS to the backend is held privately
>   by the Resolver and the Consumers see plain HTTP loopback.
> - First-pair direction flips: Resolver shows the QR, phone scans it. Removes the webcam
>   requirement, shrinks the QR payload, and matches the "phone reaches out to the bridge"
>   mental model.

Status: **exploratory, OUTDATED** — original drafted 2026-05-22 while shipping a host app's Local-MCP toggle. Superseded 2026-05-23.

---

## 1. Problem

Every MCP server today reinvents the same dance: produce a `{url, headers, ca}` blob and hope the user pastes it into the right file. Every MCP client implements the receive half differently:

- Claude Desktop: edit `~/Library/Application Support/Claude/claude_desktop_config.json` and restart.
- Cursor: Settings → Features → MCP → Add → manual headers + cert trust.
- Continue: edit `~/.continue/config.json`.
- Zed / VS Code MCP extension / future clients: each their own.

For a phone-resident server (a fitness tracker, hypothetical home-automation hub, on-device journals, etc.) the gap is wider — the payload has to cross devices first, then a config file, then a TLS trust prompt. We currently mask it with a QR + "Copy MCP config" + a help carousel that walks through each client. That gets the user to a working state but the per-client friction is real.

**Hypothesis**: a small, server-agnostic pairing helper — a phone-side SDK + a desktop receiver + a tiny wire-format spec — eliminates the friction for every mobile MCP server, not just one specific app. The desktop helper is the only piece each user installs once.

This doc sketches the architecture. It is intentionally not a commitment to build.

---

## 2. Three-layer architecture

```
┌─────────────────────────────────────────────────────────────┐
│  Mobile app A (BodyLog)  ──┐                                 │
│  Mobile app B (some MCP)──┼── via mcp-bridge-mobile SDK     │
│  Mobile app C  ───────────┘                                 │
│                              │ Bonjour / BLE / QR fallback  │
│                              ▼                              │
│                  ┌─ MCP Bridge (desktop) ─┐                 │
│                  │  detects clients,      │                 │
│                  │  prompts once,         │                 │
│                  │  writes configs        │                 │
│                  └──┬───────┬───────┬─────┘                 │
│                     ▼       ▼       ▼                       │
│           Claude Desktop  Cursor  Continue  …               │
└─────────────────────────────────────────────────────────────┘
```

### 2.1 `mcp-bridge-mobile` SDK

A Capacitor / React Native / pure-JS library any mobile MCP-server author drops in. Surface (≤ 10 methods):

```ts
import { McpBridge } from "@mcp-bridge/mobile";

const peer = await McpBridge.discover({
  // Discovery transports the SDK should try, in order:
  transports: ["bonjour", "ble", "qr"],
  timeoutMs: 5000,
});

await peer.send({
  url: "https://10.0.0.42:54321/",
  token: "...base64url...",
  ca: "-----BEGIN CERTIFICATE-----\n...",
  name: "BodyLog",
  fp: "sha256:abcdef...",
});

peer.onStatus((s) => console.log(s)); // 'discovered' | 'connected' | 'sent' | 'installed' | 'error'
```

Responsibilities:

- **Discovery**: prefer Bonjour over the same Wi-Fi; fall back to BLE peripheral mode when the desktop bridge advertises a beacon; final fallback is a QR shown on the phone for the desktop's webcam.
- **Mutual cert pinning**: the bridge's discovery advert carries its own cert fingerprint; the SDK refuses to send to a fingerprint it hasn't seen before without the user confirming on the phone.
- **Status streaming**: lets the calling app show "Sending…", "Installed in 3 clients", or "User declined" without bespoke wire decoding.

### 2.2 MCP Bridge (desktop app)

A small native app that runs in the menu bar / system tray.

- **Stack candidate**: Tauri (Rust core, small bundle ~3-5 MB, native UI). Electron is the obvious alternative but quintuples the binary size.
- **Platforms**: macOS, Windows, Linux. Signed builds per platform.
- **Responsibilities**:
  1. Advertise `_mcp-bridge._tcp.local` via Bonjour at user-opt-in.
  2. Accept BLE pairing offers (peripheral discovery, central role on the desktop side).
  3. Accept manual QR-paste / file-import fallbacks.
  4. Auto-detect which MCP clients are installed by probing known config paths.
  5. Show a confirmation sheet with the incoming server's name + cert fingerprint + the list of clients it will install into.
  6. On user confirm: write the right config block to each client; trust the cert in the OS keychain; surface success per-client.
  7. On revoke: remove the entries it added (it tags them so it doesn't strip unrelated ones).

### 2.3 Wire format (mcp-pair v0.1)

```jsonc
{
  "spec": "mcp-pair/v0.1",
  "name": "BodyLog", // shown in the confirmation sheet
  "url": "https://10.0.0.42:54321/", // the server endpoint the clients will connect to
  "fp": "sha256:abcd...", // server's TLS cert fingerprint
  "ca": "-----BEGIN CERTIFICATE-----\n...", // PEM, inline
  "auth": {
    "type": "bearer",
    "header": "Authorization",
    "value": "Bearer ...base64url...",
  },
  "scope": ["tools"], // MCP capabilities advertised
  "rotate": "https://10.0.0.42:54321/pair/rotate", // optional URL for cert rotation
}
```

Carried over a single HTTP POST (TLS) from phone to bridge once they've pinned each other.

---

## 3. Trust model

The hard problem. A naïve implementation lets any LAN attacker advertise `_mcp-bridge._tcp.local` and silently steal pairing payloads.

Design:

1. **First-pair handshake**: phone and bridge each generate a long-lived ECDSA keypair on first run. Bonjour advertises _only_ the cert fingerprint. Phone refuses to send to an unrecognised fingerprint until the user confirms it matches what the bridge UI is showing.
2. **Subsequent pairings**: pinned fingerprint → silent. User does nothing.
3. **Bridge identity rotation**: user can rotate the bridge's keypair from settings; all mobile SDKs lose their pin and re-confirm on next pair.
4. **Server identity**: included in the payload (`fp`) and shown in the confirmation sheet so the user can see "BodyLog @ fingerprint X is being installed".
5. **No upstream service**: the bridge talks only over local network + BLE. No cloud, no telemetry, no analytics.

---

## 4. Distribution + code-signing reality

- **macOS**: Apple Developer Program ($99/yr) for notarisation. Unsigned builds work but produce a Gatekeeper warning on every launch and a "verify in System Settings" hoop. Sustainable only with the cert.
- **Windows**: code-signing cert (~$200-400/yr from a CA). Without it, SmartScreen flags every install.
- **Linux**: Flatpak / Snap / AppImage. PPA + AUR for distros. Lower friction; reproducible builds via Nix or Bazel a nice-to-have.

Cross-platform builds: GitHub Actions matrix is the obvious starter. ~30 minutes per release after the first one.

---

## 5. Open questions

- **Does the desktop bridge need to be a permanent background process**, or can it be one-shot ("run `mcp-pair listen` for 60 seconds when you want to add a server")? The one-shot model is simpler and matches the rare-event nature of pairing, but loses the "BLE always-on" magic.
- **How do we handle clients we don't know about yet?** A `clients/` directory of YAML adapters where contributors PR new clients. Bridge ships with a built-in set and refreshes from a signed manifest URL at user opt-in.
- **Re-pairing UX when the server's IP changes** (phone joins a new Wi-Fi). Either the bridge subscribes to mDNS updates and rewrites the config silently, or the SDK pushes a new payload on every connect attempt.
- **Conflict resolution** when a config file already has a `bodylog` server entry. Replace, merge, or prompt?
- **CLI parity**: every UI affordance the desktop app exposes should also work as `mcp-pair <cmd>` for headless / CI use. Helps adoption among the kind of users who'd build their own MCP server.

---

## 6. Phased plan if we commit

| Phase | Deliverable                                                                                                                                     | Effort                               |
| ----- | ----------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------ |
| 0     | This doc                                                                                                                                        | done                                 |
| 1     | `mcp-pair/v0.1` spec (markdown) + a 100-line CLI proof-of-concept that listens on Bonjour + writes Claude Desktop config                        | 1-2 days                             |
| 2     | Tauri menu-bar app, Claude Desktop + Cursor writers, signed macOS build                                                                         | 1-2 weeks                            |
| 3     | Windows + Linux signed builds                                                                                                                   | 3-4 days                             |
| 4     | Build out `mcp-bridge-mobile` — Capacitor iOS plugin, Android plugin, JS SDK wrapping both                                                       | 1-2 weeks                            |
| 5     | File RFC at [modelcontextprotocol/spec](https://github.com/modelcontextprotocol) with "here's a reference implementation already in production" | days of writing, months of consensus |

Phase 1 is what tells us whether the architecture is sound. Stop after that if the answer is "no".

---

## 7. Why not just …

- **Cloud sync (iCloud Drive / Dropbox)** — adds a third-party trust relationship and an upstream provider for a payload that already has perfectly good local transports.
- **`mcp+pair://` deeplink only** — relies on every MCP client implementing the handler natively, which is the very gap the bridge exists to paper over. The deeplink form is fine as a _transport_ into the bridge, not as a replacement for it.
- **Bridge-as-MCP-proxy** (one local server that multiplexes everything) — interesting architecture but adds latency, makes per-server scope harder, and is harder to retire if it falls behind on protocol updates. Defer.
- **OS Share Sheet target only** — useful as a fallback (iOS AirDrop → bridge), but a Share-Sheet-only design forces every mobile MCP server to know how to render a file payload, which is exactly the per-app friction the SDK avoids.

---

## 8. Status

Not committed. Mobile apps with built-in MCP servers can already ship working integrations via manual QR + Copy-MCP-config + AirDrop-share flows. If a second such app appears that would benefit from a shared SDK, revisit phase 1.

Sustained maintenance — adapter updates per client release, signing certs, security responses — is the real cost. Build only if there's a person prepared to own it.

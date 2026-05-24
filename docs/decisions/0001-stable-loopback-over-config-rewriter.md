# 0001 — Stable-Loopback Bridge over config-rewriter design

- **Status**: Accepted
- **Date**: 2026-05-23
- **Deciders**: Project founders
- **Supersedes**: —
- **Related**: [`ARCHITECTURE.md`](../ARCHITECTURE.md), [`BRIDGE_ARCHITECTURE_OUTDATED.md`](../BRIDGE_ARCHITECTURE_OUTDATED.md)

## Context

Mobile-hosted MCP servers ([Origins](../GLOSSARY.md#origin)) suffer two friction points: first-time configuration across multiple MCP clients ([Consumers](../GLOSSARY.md#consumer)) and session-to-session **drift** (IP, port, bearer token, certificate fingerprint) that breaks the configuration until it is fixed by hand.

The earlier design (preserved verbatim in [`BRIDGE_ARCHITECTURE_OUTDATED.md`](../BRIDGE_ARCHITECTURE_OUTDATED.md)) attacked this with a **config-rewriter**: a daemon would watch for Origin changes and rewrite each Consumer's config file in-place every time anything drifted. The user installed once; everything else was bookkeeping in JSON.

In practice that bookkeeping multiplies quickly:

- Each Consumer has its own config schema, file location, and reload semantics. Each Origin drift requires `N` writes across `N` Consumers — and each new Consumer release can change the schema again.
- Some Consumers re-read the config live; some require a process restart; some restart but lose state. The user's experience of "the bridge updated my server" varies by Consumer.
- The daemon must hold a CA trust path good enough for browsers/clients to validate ephemeral phone certificates — escalating into the system keychain on macOS, the Windows certificate store, NSS db on Linux. That is a security-sensitive surface to maintain.
- Edge cases proliferate: user-edited fields, comments in JSONC, merge conflicts with the user's own changes.

## Decision

Adopt the **Stable-Loopback Bridge** pattern: a small Resolver process pins **one stable loopback URL per registered server** (`http://127.0.0.1:<port>/<logical_id>?key=…`); Consumers are configured against that URL exactly once; the Resolver absorbs all Origin-side drift transparently by re-discovering the backend via signed [announces](../GLOSSARY.md#announce).

Each Consumer's config is touched exactly twice in its lifecycle: once at pair time (insert the loopback URL + per-Consumer key) and once at revoke time (remove the entry tagged by the [sentinel UUID](../GLOSSARY.md#sentinel-uuid)).

## Alternatives considered

- **Config-rewriter (the predecessor design)** — see Context. Rejected because the surface area and security responsibilities scale linearly with both Consumer count and Consumer release cadence. The proxy approach collapses that to one component the bridge owns end-to-end.
- **mDNS hostname only** (Origin publishes `bodylog.local`) — solves IP drift but not port, token, or cert rotation. `.local` TLS is awkward. Doesn't reach 70% of the drift problem.
- **Per-Consumer SDKs (push pairing into each Consumer)** — requires every Consumer vendor to ship matching SDK changes on coordinated timelines. That coordination doesn't exist; the bridge exists precisely because it doesn't.
- **Cloud sync** (iCloud / Dropbox shuttle the config) — adds a third-party trust relationship and an upstream provider for a payload that has perfectly good local transports. Fails the privacy charter.

## Consequences

What this enables:

- A Consumer's configuration **never changes** after first install, regardless of how the Origin drifts. This is the architectural win that pays for the proxy.
- Per-`(Origin, Consumer)` revoke and audit, because the loopback key is per-pair.
- One place to add observability, rate limits, and the [egress allowlist](../GLOSSARY.md#egress-allowlist), instead of scattering them across `N` adapters.
- No system-keychain trust escalation — the Resolver pins backend fingerprints itself instead of asking the OS to trust them.

Costs we accept:

- One always-on background process (`mcp-bridged`). Memory budget single-digit MB; CPU ~0% at idle.
- A loopback hop per request (≤2ms locally) on top of the LAN RTT to the Origin.
- Signed/notarized desktop builds across macOS, Windows, Linux — installer infrastructure we own.
- A new wire-protocol contract ([`mcp-pair`](../SPEC.md#4-mcp-pairv01--pairing-protocol), [`mcp-announce`](../SPEC.md#5-mcp-announcev01--identity-refresh-protocol)) that we must version and keep stable.

What would force a revisit:

- If a single Consumer ecosystem grows a native pairing primitive that all major Consumers adopt, the bridge becomes less load-bearing.
- If sub-millisecond first-byte latency becomes a hard requirement (it isn't today), the loopback hop is the first place to look.

## Notes

The predecessor design is preserved in [`BRIDGE_ARCHITECTURE_OUTDATED.md`](../BRIDGE_ARCHITECTURE_OUTDATED.md) so the comparison is verifiable from source, not from memory.

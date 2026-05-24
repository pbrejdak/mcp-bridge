# MCP Bridge — Roadmap

Last revised: 2026-05-24.

This is the single page that answers "what's done, what's next, what's deferred." It pulls together the phased plan from [`ARCHITECTURE.md`](ARCHITECTURE.md) §11, the open architectural decisions from §10, the deferred-platforms tracker from [`MOBILE.md`](MOBILE.md) §13, and the open mobile-side decisions from [`MOBILE.md`](MOBILE.md) §14.

> **Status**: pre-1.0, exploratory. Phase 0 (documentation) is complete. Phase 1 (proof-of-concept) has not started. Nothing on this page is a calendar commitment — items move when the work happens.

---

## 1. How we version

Two version streams that move on different cadences:

- **Project SemVer** (`mcp-bridged`, the SDK packages, Bridge Console) — tracks features and user-visible behavior. Documented in [`CHANGELOG.md`](../CHANGELOG.md).
- **Wire protocol** (`mcp-pair/v0.1`, `mcp-announce/v0.1`) — tracks the interoperability contract between Origins and Resolvers. Documented in [`SPEC.md`](SPEC.md) §7. The SDK SemVer is pinned to the protocol version it implements (see [`MOBILE.md`](MOBILE.md) §10.2).

Pre-1.0 we will not maintain cross-version compatibility between Resolver and SDK. Post-1.0 we will.

---

## 2. Phased plan

The plan from [`ARCHITECTURE.md`](ARCHITECTURE.md) §11, restated as a user-facing roadmap. Phases are sequential — each is a stopping point at which the project's viability can be re-evaluated.

| Phase | Deliverable | Status |
|---|---|---|
| **0** | Design and policy documentation set ([`ARCHITECTURE.md`](ARCHITECTURE.md), [`SPEC.md`](SPEC.md), [`DAEMON.md`](DAEMON.md), [`UI.md`](UI.md), [`UX.md`](UX.md), [`MOBILE.md`](MOBILE.md), [`PRIVACY.md`](PRIVACY.md), [`LEGAL.md`](LEGAL.md), [`SECURITY.md`](SECURITY.md), [`CONTRIBUTING.md`](CONTRIBUTING.md), [`USER-GUIDE.md`](USER-GUIDE.md), [`GLOSSARY.md`](GLOSSARY.md), ADRs) | **Done** |
| **1** | `mcp-pair/v0.1` + `mcp-announce/v0.1` spec exercised by a CLI proof-of-concept that runs the Loopback Listener and writes Claude Desktop config. Plus a small iOS test harness that does QR-anchored pairing end-to-end. | Not started |
| **2** | Tauri menu-bar Resolver with Claude Desktop writer only, signed + notarized macOS build with SLSA provenance, smart landing page on a no-log CDN, token-aware installer. | Not started |
| **3** | Cursor + Continue writers, Windows + Linux signed + provenance-attested builds, package-manager publishing. | Not started |
| **4** | Full mobile SDK rollout — Capacitor iOS plugin, Android plugin, JS SDK wrapping both. AirDrop-URL share affordance on iOS. | Not started |
| **5** | RFC at [`modelcontextprotocol/spec`](https://github.com/modelcontextprotocol) with reference implementation already in production. | Not started |

**Phase 1 is the architecture verdict.** It is the smallest amount of code that exercises the load-bearing claims end-to-end on a real phone. If those claims do not hold up there, the rest of the plan does not start.

---

## 3. Right now

The project is in **Phase 0 → Phase 1** transition. The documentation set is committed; no production code has been written.

Before Phase 1 code can begin, the pre-release action items in §6 must reach a state where opening the repo to outside contributors is workable.

---

## 4. Beyond Phase 5

Nothing is planned beyond Phase 5 today. The plan deliberately ends at "RFC filed with reference implementation in production" because everything beyond that depends on what the upstream MCP community wants to standardize.

Future directions we have noted (informative, not commitments):

- A second-maintainer-onboarding flow once code lands and the project opens to contributors.
- Per-Consumer ergonomic improvements (live-reload integrations where the Consumer supports it).
- Optional cloud-relay for users whose phone and computer are never on the same LAN — only after the local-LAN story is solid and only as an opt-in service users can self-host.

---

## 5. Deferred platforms (mobile SDK)

From [`MOBILE.md`](MOBILE.md) §13. Each row has an explicit "what would unlock this" trigger so we are not guessing at priorities.

| Platform | Status | What would unlock it |
|---|---|---|
| **.NET MAUI** | Planned for v0.2 | Largest deferred-platform audience. NuGet wrapper over the KMP iOS xcframework + Android AAR via .NET binding libraries. |
| **Tauri Mobile** | Planned for v0.2 or v0.3 | Rust crate over UniFFI bindings to the KMP-built native binaries; natural alignment with the Rust daemon. |
| **visionOS** | Expected to fall out of the iOS xcframework | Confirm during the v0.2 build pipeline work. |
| **NativeScript** | On demand | Ship when a real user asks. Architecture is straightforward (npm package as NativeScript plugin wrapping the same native binaries as Capacitor and React Native). |
| **Unity** | v0.3+ if demand | Interesting for AR / spatial / sensor-heavy non-game apps. Defer unless specific demand. |
| **watchOS / Wear OS** | Deferred | Wearables as Origins is intriguing for health/fitness data but the SAS confirmation UX and constrained Bonjour stack make this awkward. Revisit when the desktop side has matured. |
| **Xamarin Classic** | Will not support | Deprecated upstream. Migrate to MAUI. |
| **Embedded (ESP32, Arduino, RTOS)** | Out of scope | Different design space. Would require a stripped-trust-model "bridge-microcontroller" SDK as a separate project. |
| **iOS push wake-up** for background MCP serving | Out of v1 | Architecturally interesting — requires the AI client to wake the daemon to wake the phone. Not v1. |
| **Cross-device Origin migration** | Will not support | User re-pairs on the new phone. Bridge does not migrate keys between phones. |

---

## 6. Pre-release action items

From [`LEGAL.md`](LEGAL.md) §14 and the documentation-set status lines. These gate the transition from "exploratory" to "openable to outside contributors."

| Item | Priority | Trigger |
|---|---|---|
| GitHub repo set up with CI configured for the [`CONTRIBUTING.md`](CONTRIBUTING.md) §10 checks | High | Before first external PR |
| DCO bot installed | High | Before first external PR |
| `.github/PULL_REQUEST_TEMPLATE.md` and issue templates committed | High | Before first external PR |
| `CODE_OF_CONDUCT.md` vendored at repo root | Medium | Before opening community channels |
| `SECURITY.md` PGP key generated and published | High | Before first public binary release |
| Domain (`mcpbridge.me`) registration | High | Before first public binary release |
| Security inbox provisioning with PGP-aware tooling | High | Before first public binary release |
| Marketing website privacy policy | Medium | Before website goes live |
| Marketing website terms of service | Medium | Before website goes live |
| BIS / NSA encryption notification email | High | Before first public binary release |
| Trademark clearance search for "MCP Bridge" | High | Before brand finalization |
| Trademark registration filing | Low | After adoption confirmed |
| `DISTRIBUTION.md` (per-channel legal obligations) | Medium | When first channel is committed to |

---

## 7. Open architectural decisions

These are the technical choices that the documentation flags as "not yet pinned down." Each is non-blocking for Phase 0; each will need a call before its implementation work begins.

From [`ARCHITECTURE.md`](ARCHITECTURE.md) §10:

1. **Loopback port** — fixed (`8765`) or dynamic with auto-rebind. Likely: try fixed, fall back to dynamic with rewrite on collision only.
2. **Origin Connector lifetime** — cold-start on first request vs. keep-warm with idle TCP. Likely: keep-warm with 5-minute idle close.
3. **MCP transport coverage** — HTTP+SSE only in Phase 1; WebSocket / stdio later.
4. **Consumer restart UX** — auto-detect-and-prompt vs. rely on Consumer config-reload. Per-Adapter capability flag.
5. **Multi-host pairing** — phone holds N Resolver pins. Cheap to support; document it explicitly.
6. **Auto-update channel privacy** — GET to a static manifest on a CDN; daily cadence; signed bundle; manual override.
7. **Supply-chain attestation** — SLSA provenance per release; reproducible builds where toolchain allows; documented signing-key rotation policy.

From [`MOBILE.md`](MOBILE.md) §14:

1. Bonjour vs jmDNS fallback on Android.
2. iOS multicast entitlement justification text — published canonical version.
3. Camera-permission "pre-warm" method.
4. `auth_rotation_requested` flow shape (SDK-emits-event vs SDK-calls-`authProvider`).
5. Capacitor plugin packaging — unified or split.
6. Origin keypair scope — per host-app install vs per host-app user.
7. React Native old-architecture support (recommended: no v1 support).
8. KMP iOS distribution: xcframework only (recommended).
9. KMP common module Coroutines binary compatibility range.
10. Cross-target test matrix on CI (per Kotlin/Native target).
11. Flutter federated vs monolithic plugin (recommended: federated).
12. Flutter `permission_handler` (recommended: do not depend).
13. Flutter desktop targets (recommended: not v1).

When a decision is made, capture it as an [Architecture Decision Record](decisions/).

---

## 8. How to influence the roadmap

- **Open a discussion** for anything user-visible — new feature, new platform, new Consumer adapter. Reference the design principle from [`UX.md`](UX.md) §1 it satisfies, or explain why an existing principle should change.
- **Open an RFC** (a discussion in the wire-protocol-changes format from [`CONTRIBUTING.md`](CONTRIBUTING.md) §3.4) for anything that affects the protocol, the trust model, or [`PRIVACY.md`](PRIVACY.md).
- **Send a security or privacy report** through [`SECURITY.md`](SECURITY.md) for anything in those domains. Do not open public issues.
- **Contribute a Client Adapter** for a Consumer not yet supported — see [`CONTRIBUTING.md`](CONTRIBUTING.md) §3.5.

We are slow to accept new optional features that grow the maintenance surface; we are quick to accept bug fixes, security improvements, and documentation. The full intake posture is in [`CONTRIBUTING.md`](CONTRIBUTING.md) §3.

---

## 9. Revision cadence

This document is reviewed and updated **at least quarterly**, and any time a phase transitions or a deferred item moves.

| Date | Change |
|---|---|
| 2026-05-24 | Initial extraction from [`ARCHITECTURE.md`](ARCHITECTURE.md) §10–§11 and [`MOBILE.md`](MOBILE.md) §13–§14. |

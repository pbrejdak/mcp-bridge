# MCP Bridge — Legal

Status: exploratory, current revision 2026-05-23. This document is the index and source-of-truth for the legal documents MCP Bridge needs as it moves toward release. Each section either contains the document inline (small things like contact channels) or links to a separate file (large things like the full Privacy Policy).

> **Not legal advice.** This is the working draft of how the project intends to handle each legal concern, for review by qualified counsel before any public release of MCP Bridge.

---

## 1. License (codebase)

**Recommendation: Apache License 2.0** for the bridge codebase.

Reasoning:

- Permissive, allowing commercial and proprietary forks (matches the intent of being adoptable by AI app vendors).
- Includes an **express patent grant** — important for cryptographic software where patent ambiguity is common.
- More legal teams accept it than GPL, reducing adoption friction with enterprise users.
- Compatible with all third-party dependencies currently in the stack (most are MIT, BSD-3, or Apache-2.0).

Alternative: **MIT** if absolute-minimum friction is wanted; loses the explicit patent grant.

**Avoid**: GPL variants — would force the AI clients (Claude Desktop, Cursor) to incorporate copyleft code into their consumer flow, which they will not do.

**Files**:

- `LICENSE` at repository root — full Apache 2.0 text.
- `NOTICE` at repository root — copyright holders + required-attribution list.

---

## 2. Privacy Policy

The canonical privacy charter lives in [`PRIVACY.md`](PRIVACY.md). A shorter, regulator-friendly **Privacy Policy** lives on the marketing website (`mcpbridge.me/privacy`) for users / regulators who want a single page without source links.

Required content (whether on the website or in `PRIVACY.md`):

- **What data Bridge collects**: none, except its own update-check requests (timestamp, IP, User-Agent received by `updates.mcpbridge.me`, no logging by configuration).
- **What data is stored locally**: documented in `PRIVACY.md` §3.
- **Who data is shared with**: no one.
- **Data subject rights** (GDPR Articles 15–22): exercisable by the user against their own machine via `mcp-bridge identity export`, `mcp-bridge uninstall --purge`, and Settings UI.
- **Data retention**: documented in `PRIVACY.md` §3.
- **Contact for privacy questions**: see §12.
- **Effective date and version**: tracked below.

Version history:

| Version | Date | Change |
|---|---|---|
| 0.1.0 | 2026-05-23 | Initial draft. |

---

## 3. Terms of Service

**For the desktop app**: minimal. Bridge is software the user downloads and runs locally; the operator does not host services for them beyond the update channel.

ToS coverage:

- Disclaimer of warranty (Apache 2.0 already includes this).
- Limitation of liability.
- Acknowledgement that Bridge is pre-1.0 / beta until 1.0.
- Acceptable use clause — not a meaningful constraint since the operator does not observe behavior, but included for clarity (no malware redistribution, etc.).

**For the update channel** (`updates.mcpbridge.me`): a brief service ToS:

- Operator does not log requests beyond what the CDN's no-log mode produces (which is "no logs").
- Operator does not modify update bundles other than to publish signed releases.
- Reasonable best-effort uptime; no SLA.

**Hosting**: short page on the marketing website (`mcpbridge.me/terms`), linked from the in-app About panel. Effective date tracked in this document's version history.

---

## 4. Third-party notices / attribution

All bundled third-party software requires attribution per its license. Generated at build time from `cargo deny` and `pnpm licenses ls`. Surfaced in:

- `NOTICE` file at repository root (Apache-2.0 attribution requirement).
- In-app About → Credits view (Bridge Console).
- `mcpbridge.me/credits` mirror of the in-app credits.

Components to enumerate:

| Component | License |
|---|---|
| Tauri 2.x | MIT / Apache-2.0 |
| Svelte 5 | MIT |
| Bits UI | MIT |
| shadcn-svelte (components copied into repo, original attribution preserved) | MIT |
| Tailwind v4 | MIT |
| `@lucide/svelte` | ISC |
| `@fluentui/svg-icons` | MIT |
| `qrcode` (npm) | MIT |
| `tinykeys` | MIT |
| `mode-watcher` | MIT |
| `svelte-sonner` | MIT |
| Rust crates: `tokio`, `hyper`, `rustls`, `axum`, `tower`, `serde`, `serde_json`, `serde_jcs` | MIT / Apache-2.0 |
| `ed25519-dalek`, `x25519-dalek`, `crypto_box` | BSD-3 / MIT-equivalent |
| `zeroize`, `secrecy` | Apache-2.0 / MIT |
| `keyring`, `zeroconf` | MIT / Apache-2.0 |

Special cases:

- **SF Symbols (Apple)**: Apple's font license restricts use to *software running on Apple platforms*. The macOS Tauri build uses SF Symbols only via the `render_sf_symbol` Tauri command, which executes only on macOS. Documented in NOTICE and in [`UI.md`](UI.md) §8.1.
- **Fluent System Icons (Microsoft)**: MIT, usable on all platforms; default for Windows.
- **Lucide**: ISC, no constraints.

---

## 5. Trademark

**"MCP Bridge"** — unregistered word mark. Common-law rights only until / unless registered.

**"MCP"** — "Model Context Protocol" is a name controlled by Anthropic. Pre-release legal review must answer:

- Is "MCP" alone trademarked by Anthropic?
- Does "MCP Bridge" require a license from Anthropic or sit in a "compatibility" naming convention (e.g., "OAuth Client", "OpenAPI Generator", which use the protocol name nominatively)?
- Does the descriptive use ("a bridge for MCP servers") qualify for **nominative fair use** under US/EU trademark doctrine?

**Action item before public release**: trademark search and counsel review of the name. Possible alternatives if "MCP Bridge" is constrained: "Patch", "Wire", "Relay" (each requires its own clearance).

**Logo / icon**: design pending. Once finalized, register for trademark if the project achieves meaningful adoption.

---

## 6. Cryptography export classification

Bridge uses cryptography (Ed25519, X25519, ChaCha20-Poly1305, TLS via rustls) and is therefore subject to U.S. Export Administration Regulations.

- **ECCN**: **5D002** ("information security" software).
- **License exception**: **TSU (Technology and Software — Unrestricted)** under EAR §740.13(e), as publicly available cryptographic source code. Conditions met:
  - Source publicly available on the project repository.
  - Source published in a manner sufficient to allow public knowledge (open-source).
  - Standard algorithms via well-known libraries.
- **Notification requirement** (per §742.15(b)): a one-time email to **`crypt@bis.doc.gov`** and **`enc@nsa.gov`** with the source URL at the time of first public release. No approval required — notice only.

**Action item before first public release**:

1. Confirm the TSU exception still applies as written.
2. Send the BIS + NSA notification email.

**Distribution constraints**:

- Sale or distribution of binaries to **embargoed jurisdictions** (currently Cuba, Iran, North Korea, Syria, certain regions of Ukraine and Russia per OFAC) is prohibited regardless of EAR exception. The marketing website's download page should geo-restrict where required.

---

## 7. GDPR and EU data protection

Because Bridge does not collect or process user data at the operator level (excepting incidental update-check requests covered by [`PRIVACY.md`](PRIVACY.md) §4), the operator is **not a controller or processor** of user data under GDPR.

The user is both the data subject and the data controller of their own data on their own machine.

Practical consequences:

- **No DPO appointment required** (operator processes no personal data).
- **No Standard Contractual Clauses** needed because no transfer to third parties occurs.
- **DSARs satisfied trivially** — the user inspects their own files, runs `mcp-bridge identity export`, etc.
- **Right to erasure** satisfied by `mcp-bridge uninstall --purge` ([`DAEMON.md`](DAEMON.md) §2.1).
- **Right to rectification** satisfied by user editing their own settings.
- **Right to data portability** satisfied by the encrypted-export feature ([`PRIVACY.md`](PRIVACY.md) §11).

The update channel (`updates.mcpbridge.me`) does receive incidental data (IP, User-Agent, timestamp). Per [`PRIVACY.md`](PRIVACY.md) §4, the CDN is configured for **no access logging**, so no personal data is retained. The operator should retain the CDN configuration as evidence of this commitment.

---

## 8. CCPA / California consumer privacy

Bridge does not "sell" personal information (CCPA §1798.140) because Bridge does not collect personal information at the operator level. CCPA data-subject rights are satisfied identically to GDPR (§7 above).

The Bridge website (`mcpbridge.me`) carries a "Do Not Sell My Personal Information" link per §1798.135. The linked page explains that Bridge collects no personal information to sell.

---

## 9. Distribution agreements

Each distribution channel has its own legal terms binding the operator.

| Channel | Agreement |
|---|---|
| Apple Notarization Service | Apple Developer Program License Agreement |
| Microsoft Authenticode signing | CA-specific contract (DigiCert, Sectigo, etc.) |
| Mac App Store (future) | Apple Mac Developer Program + Mac App Store Review Guidelines |
| Microsoft Store (future) | Microsoft Store Policies |
| Homebrew | Formula acceptance criteria + maintainer guidelines |
| Debian / Ubuntu / Fedora | Distribution-specific packager agreements |
| Linux distros' security advisories | CVE assignment via MITRE or distro CNAs |

**Action item**: review each agreement before signing on to that channel; track per-platform legal obligations in a separate `DISTRIBUTION.md` once any channel is committed to.

---

## 10. Contributor License Agreement

**Recommendation**: **no CLA**. Use a **Developer Certificate of Origin** (DCO) — every commit signed off (`Signed-off-by:` line). DCO is sufficient to establish that contributors had the right to contribute their work under the project's license, without the operator owning contributions outright.

This is the model used by the Linux kernel, Git, Docker, and many other major open-source projects. Lower friction than a CLA, equally protective for the project's license compatibility.

Documented in `CONTRIBUTING.md` once the project opens to contributors.

---

## 11. Responsible disclosure / security policy

`SECURITY.md` at repository root, containing:

- **How to report a vulnerability**: `security@<project-domain>` (email).
- **PGP key** for encrypted disclosure (key ID + fingerprint published in `SECURITY.md` and at `mcpbridge.me/security`).
- **Response SLA**:
  - Acknowledgment within 72 hours.
  - Initial assessment within 7 days.
  - Fix or mitigation within 90 days.
- **Coordinated disclosure**: 90 days from reporter's confirmation, **or** until a fix is shipped, whichever first. We coordinate with the researcher on the announcement.
- **No bug bounty** initially. Hall of fame recognition for confirmed reports.

**Security and privacy reports route through the same channel.** Privacy concerns (e.g., "I found a way for Bridge to leak X") are handled as security issues.

---

## 12. Contact

Canonical contacts. These may forward to the same inbox in practice, but the topical splits help triage.

| Topic | Address |
|---|---|
| Privacy questions / data-subject requests | `privacy@<project-domain>` |
| Security vulnerabilities | `security@<project-domain>` (PGP-encrypted preferred) |
| Legal / press / partnerships | `legal@<project-domain>` |

An HTTPS reporting form on the website is provided for users who prefer not to email.

---

## 13. Trademark / brand assets

Once designed and finalized, document at `mcpbridge.me/brand`:

- Word mark spelling and capitalization ("MCP Bridge", not "MCPBridge" / "mcp-bridge" in marketing).
- Approved logo variants (dark, light, monochrome, simplified).
- Approved color palette.
- Permitted uses (compatibility statements, "works with MCP Bridge" — yes, with conditions).
- Prohibited uses (implying endorsement, modified marks, confusingly similar variants).

---

## 14. Documents we have not yet drafted

Tracked here so they do not get forgotten as the project matures.

| Document | Priority | Trigger |
|---|---|---|
| `LICENSE` (full Apache-2.0 text) | high | before first public commit |
| `NOTICE` (attribution list) | done | drafted at [`NOTICE`](NOTICE); production location is repository root |
| `SECURITY.md` | done | drafted at [`SECURITY.md`](SECURITY.md); production location is repository root |
| `CONTRIBUTING.md` (with DCO) | done | drafted at [`CONTRIBUTING.md`](CONTRIBUTING.md); production location is repository root |
| `CODE_OF_CONDUCT.md` | medium | before opening community channels |
| Marketing website privacy policy | medium | before website goes live |
| Marketing website terms of service | medium | before website goes live |
| BIS / NSA encryption notification email | high | before first public binary release |
| Trademark clearance search for "MCP Bridge" | high | before brand finalization |
| Trademark registration filing | low | after adoption confirmed |
| `DISTRIBUTION.md` (per-channel legal obligations) | medium | when first channel is committed to |
| EULA (if needed beyond ToS) | low | only if a specific platform requires it |
| Cookie / consent banner (likely none needed) | low | only if the website introduces tracking — it should not |

---

## 15. Status

Not committed. Each section above is a working plan, not a finalized legal document. All content requires review by qualified counsel before any public release.

Companion to [`PRIVACY.md`](PRIVACY.md) and [`ARCHITECTURE.md`](ARCHITECTURE.md) §13.

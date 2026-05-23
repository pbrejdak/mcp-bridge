# Security Policy

MCP Bridge is a privacy-first tool that handles cryptographic identities, bearer tokens, and the pairing between mobile MCP servers and desktop AI clients. We take security and privacy seriously and welcome responsible disclosure.

> **Working location**: this file currently sits at `docs/SECURITY.md` for working purposes. Before public release it will be moved to **repository root** as `SECURITY.md` (GitHub auto-detection convention).

---

## Reporting a vulnerability

**Do not open public GitHub issues, pull requests, or discussion threads for security vulnerabilities.**

Send reports to **`security@<project-domain>`** using PGP-encrypted email where possible.

**PGP key**:

- Fingerprint: `<TBD — generated and published before first public release>`
- Key publication: `mcpbridge.me/security.asc`
- Also distributed via `keys.openpgp.org` and the project repository root

What to include in your report:

- Description of the vulnerability and its impact.
- Steps to reproduce (proof-of-concept code preferred).
- Your assessment of severity (see §4 below).
- Whether you'd like recognition in the security acknowledgements.
- Whether you have a CVE preference (we can assign via MITRE or coordinate with your own CNA).

If you cannot use PGP, send the report unencrypted and we will switch to encrypted channels immediately on acknowledgement.

---

## Response SLA

| Stage | Time |
|---|---|
| Acknowledgement that your report was received | within **72 hours** |
| Initial triage and severity assessment | within **7 days** |
| Fix or documented mitigation | within **90 days** of confirmation |
| Coordinated public disclosure | at fix release, OR 90 days from confirmation, whichever first |

For actively-exploited vulnerabilities we move faster (same-day where possible). For long-tail / low-severity reports we may take longer than 90 days with your agreement.

---

## Scope

### In scope

The following components accept security reports:

- The Rust daemon (`mcp-bridged`) and all its modules.
- The Bridge Console (Tauri + Svelte UI process).
- The official mobile SDK (`@mcp-bridge/mobile`) once published.
- The wire protocols `mcp-pair/v0.1` and `mcp-announce/v0.1`.
- Signed installers for any platform we distribute.
- The update channel and its signing infrastructure (`updates.mcpbridge.me`).
- The landing page at `mcpbridge.me/p/<token>` and any other operator-controlled hosting.
- Reproducible-build artefacts and SLSA provenance attestation.

**Issue categories we especially want to hear about**:

- Cryptographic flaws — signature forgery, replay, downgrade, MITM.
- Trust-model violations — Pair payload re-targeting, SAS bypass, Origin-pubkey impersonation, Resolver-pubkey spoofing.
- Loopback auth bypass — DNS rebinding, Host-header spoofing, key extraction, side-channel timing.
- Adapter config injection or template escape into Consumer config files.
- Update-channel compromise — signature bypass, manifest poisoning, downgrade attacks.
- Privacy leaks beyond those documented in [`PRIVACY.md`](PRIVACY.md) §13.
- Memory disclosure via crash dumps, panic traces, or core files.
- Race conditions in pairing or revocation paths.
- Sandboxing or entitlement escapes on Tauri / WebView2 / WKWebView / WebKitGTK.

### Out of scope

- Vulnerabilities in third-party dependencies that affect Bridge only theoretically — please report to the upstream project; we'll coordinate.
- Issues requiring physical access to an unlocked machine (acknowledged explicitly in [`PRIVACY.md`](PRIVACY.md) §10 and §13).
- DoS via resource exhaustion bounded by the documented rate limits ([`ARCHITECTURE.md`](ARCHITECTURE.md) §4.2 announce rate-limit, audit H-4).
- Social engineering of end users (out of model).
- Theoretical attacks requiring a break in widely-deployed primitives (Ed25519, X25519, ChaCha20-Poly1305, TLS 1.3) — we'd rotate, but the report belongs upstream.
- Bugs in the user's AI client (Claude Desktop, Cursor, Continue) — please report to the respective vendor.
- Network-level eavesdropping on traffic the protocols explicitly do not protect (e.g., raw mDNS traffic patterns — see [`ARCHITECTURE.md`](ARCHITECTURE.md) §6 residual presence leak).

If you are unsure whether something is in scope, send it anyway. We would rather triage and decline than miss something.

---

## Severity classification

Simplified CVSS-aligned scale:

| Severity | Examples |
|---|---|
| **Critical** | Remote code execution; identity-key extraction; silent cross-host installation; update-channel sig bypass; full Origin-credential capture during pairing |
| **High** | Trust-model bypass on a single pairing; persistent privacy leak (logs / network); loopback auth bypass leading to backend access; cert-pinning bypass |
| **Medium** | DoS escaping the documented rate-limit envelope; redaction failure exposing tokens in default logs; signature-verification timing leak; replay window exploitation |
| **Low** | Disclosure of non-sensitive metadata; timing side-channels below practical thresholds; minor UX flaws that mislead the user about a security property |

Critical and High are handled with priority. Medium and Low are fixed in the next regular release.

---

## Disclosure policy

We follow **coordinated disclosure**:

1. You report privately.
2. We acknowledge within 72 hours and assign a severity.
3. We develop a fix or mitigation.
4. We schedule a release.
5. We coordinate the public announcement with you — security advisory + CVE (where applicable) + release notes.
6. We credit you in the advisory (unless you prefer to remain anonymous).

If we have not shipped a fix within 90 days of your confirmation, you may publish at your discretion. We will provide a status update with reasoning before that deadline if a fix is taking longer.

For actively-exploited vulnerabilities we may expedite public disclosure to protect users; we will tell you immediately if that becomes the path.

---

## Safe harbor

If you make a good-faith effort to comply with this policy during security research, we will:

- Not pursue legal action against you for your research.
- Work with you to understand and resolve the issue quickly.
- Recognise your contribution publicly (with your consent).

**"Good faith" means**:

- You report privately and give us reasonable time to fix before public disclosure.
- You do not exfiltrate user data, modify production systems, or degrade service for other users.
- You do not violate any other laws in conducting your research.
- Your research targets your own installation of Bridge, not attempting to attack other users' installations.

This safe harbor is operator-specific and does not bind third parties (Apple, Microsoft, distro packagers, etc.).

---

## Recognition

We maintain a **Security Acknowledgements** list at `mcpbridge.me/security#acknowledgements` and in this file's revision history (see §10).

We do not currently offer a bug bounty. We may add one if adoption justifies it.

---

## Past advisories

| ID | Severity | Date | Component | Summary |
|---|---|---|---|---|
| — | — | — | — | No advisories yet. |

Future advisories will be linked here and published at `mcpbridge.me/security/advisories/<id>`.

---

## Reproducing builds

For verification, the build pipeline produces:

- SLSA provenance attestations for every release ([`ARCHITECTURE.md`](ARCHITECTURE.md) §11).
- Source archives matching the released git tag.
- Per-platform binary checksums signed by the release key.

Any security claim ("Bridge does not phone home", "the manifest signature is checked") is verifiable from source. If a verification step fails for you, that is itself a security finding.

---

## Related documents

- [`PRIVACY.md`](PRIVACY.md) — privacy charter. Privacy reports route through the same channel as security.
- [`LEGAL.md`](LEGAL.md) §11 — broader responsible-disclosure framing and legal positioning.
- [`ARCHITECTURE.md`](ARCHITECTURE.md) §6 — trust model overview.

---

## Revision history

| Date | Change |
|---|---|
| 2026-05-23 | Initial draft. |

---

## Status

Working draft. Action items before public release ([`LEGAL.md`](LEGAL.md) §14):

- PGP key generation and publication.
- Domain (`mcpbridge.me`) registration.
- Security inbox provisioning with PGP-aware tooling.
- This file moved to repository root.

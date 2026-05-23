# Contributing to MCP Bridge

Thanks for considering a contribution. This document tells you how to work with the project — what we accept, how to sign off your commits, how to set up locally, and what review will look like.

> **Working location**: this file currently sits at `docs/CONTRIBUTING.md` for drafting purposes. Before public release it will be moved to **repository root** as `CONTRIBUTING.md`, where GitHub auto-detects it and links from the new-issue / new-PR pages.

---

## Code of Conduct

We follow the [Contributor Covenant](https://www.contributor-covenant.org/) (canonical text to be vendored as `CODE_OF_CONDUCT.md` at repo root before opening community channels — tracked in [`LEGAL.md`](LEGAL.md) §14).

In short: be civil, assume good faith, focus criticism on the work rather than the person. Reports of unacceptable behavior route through the same contact as security ([`SECURITY.md`](SECURITY.md) §1).

---

## Reporting security and privacy issues

**Do not open public GitHub issues, pull requests, or discussion threads for security vulnerabilities or privacy leaks.**

See [`SECURITY.md`](SECURITY.md) for the disclosure channel, response SLA, and safe harbor terms. Privacy reports route through the same channel.

---

## What we accept

### Bug reports

- Reproduction steps, expected vs. actual, OS/version, Bridge version.
- A diagnostic bundle helps (Settings → Privacy → Copy diagnostics) — review it first; redaction is best-effort but not perfect.
- One report per bug. If you find two, file two.

### Feature requests

- Open a discussion before a PR for anything user-facing.
- Reference the design principle it satisfies ([`UX.md`](UX.md) §1) or explains why an existing principle should change.
- Out-of-scope: features that require Bridge to talk to a new outside endpoint, store new data, or change the trust model. These need an RFC (see "Wire protocol changes" below).

### Code contributions

We welcome:

- Bug fixes (with regression tests).
- New Client Adapters (Zed, future MCP clients) — see "Contributing a Client Adapter."
- Performance improvements (with benchmarks).
- Accessibility improvements ([`UX.md`](UX.md) §16).
- Translations once i18n scaffolding lands (deferred from v1 — [`UX.md`](UX.md) §15).
- Documentation improvements at any time.

We are slow to accept:

- New optional features that increase the maintenance surface.
- Refactors without a concrete bug or measurable gain.
- Cosmetic changes that fight platform-native conventions.

We do not accept:

- Telemetry, analytics, or "anonymous metrics" in any form.
- New outside endpoints without an RFC and a corresponding update to [`PRIVACY.md`](PRIVACY.md) §4.
- Changes that weaken any of the [`PRIVACY.md`](PRIVACY.md) §2 threat-model defenses.
- Dependencies that phone home, regardless of how convenient they are.

### Wire protocol changes

`mcp-pair/v0.1`, `mcp-announce/v0.1`, and the IPC surface ([`DAEMON.md`](DAEMON.md) §5) are versioned contracts that mobile SDKs and third-party implementations will build against.

Changes require:

1. An RFC opened as a GitHub discussion. Format: motivation, current behavior, proposed change, compatibility story, security/privacy implications.
2. Two-week comment window minimum.
3. Maintainer approval before any code lands.
4. Updated conformance test vectors so other implementations can verify against the same fixtures.

Pre-1.0 we will tolerate breaking changes; post-1.0 we won't.

### Contributing a Client Adapter

To add support for a new MCP client (e.g., Zed):

1. Implement the `Adapter` trait in `src/adapters/<name>.rs` — three methods: `detect()`, `write(pin, consumer_key)`, `remove(pin_id)`.
2. The adapter tags every written entry with the `_mcp_bridge_managed` sentinel UUID so `--purge` can find and remove its entries unambiguously.
3. Include integration tests with a mock client config file at `tests/adapters/<name>/`.
4. Update [`DAEMON.md`](DAEMON.md) §3 module layout.
5. Update [`UX.md`](UX.md) §7 pair-flow checkbox list.

Adapters that need privileged access (writing outside `~/Library/Application Support/<client>/` or its platform equivalent) need a security-review discussion before implementation.

---

## Developer Certificate of Origin (DCO)

This project uses the **Developer Certificate of Origin** instead of a Contributor License Agreement. Every commit must be signed off by the author certifying they have the right to contribute the work under the project's Apache 2.0 license.

See [`LEGAL.md`](LEGAL.md) §10 for the rationale.

### How to sign off

Add `-s` to your `git commit` invocation:

```bash
git commit -s -m "fix(proxy): close pooled connection on auth rotation"
```

This appends a `Signed-off-by: Your Name <your-email@example.com>` line to your commit message. The name and email must match your `git config user.name` and `git config user.email`.

If you forget to sign off, you can fix the most recent commit:

```bash
git commit --amend -s --no-edit
```

For older commits in your PR branch:

```bash
git rebase --signoff main
```

CI rejects PRs that contain unsigned commits.

### What sign-off means

By signing off, you certify the full text of the Developer Certificate of Origin 1.1:

> By making a contribution to this project, I certify that:
>
> (a) The contribution was created in whole or in part by me and I have
>     the right to submit it under the open source license indicated in
>     the file; or
>
> (b) The contribution is based upon previous work that, to the best of
>     my knowledge, is covered under an appropriate open source license
>     and I have the right under that license to submit that work with
>     modifications, whether created in whole or in part by me, under
>     the same open source license (unless I am permitted to submit
>     under a different license), as indicated in the file; or
>
> (c) The contribution was provided directly to me by some other person
>     who certified (a), (b) or (c) and I have not modified it.
>
> (d) I understand and agree that this project and the contribution are
>     public and that a record of the contribution (including all
>     personal information I submit with it, including my sign-off) is
>     maintained indefinitely and may be redistributed consistent with
>     this project and the open source license(s) involved.

Verbatim text from [developercertificate.org](https://developercertificate.org).

---

## Privacy and security review for contributions

Every PR is reviewed against the privacy-first principles in [`PRIVACY.md`](PRIVACY.md). Some PRs trigger additional review steps.

### When you add a new dependency

Check this list yourself before opening the PR, and include the answers in the PR description:

- [ ] Does the dependency make network connections? Verify by reading its source or its security audit.
- [ ] If yes, can those connections be disabled at configuration time? If they cannot, the dependency is rejected.
- [ ] What's the license? Must be MIT, BSD, ISC, Apache-2.0, MPL, or another OSI-approved permissive license. GPL-flavored is not acceptable for this project.
- [ ] Is there a lighter alternative that achieves the same goal? Mention what you considered.
- [ ] Is the dependency actively maintained? (Last commit < 12 months for a critical-path crate.)
- [ ] Add it to [`NOTICE`](NOTICE) with copyright, URL, and license.

### When you add a new outbound network connection

If the daemon will ever connect to a new endpoint, you must:

1. Open a discussion explaining why it's necessary.
2. Update [`ARCHITECTURE.md`](ARCHITECTURE.md) §6.1 egress allowlist.
3. Update [`PRIVACY.md`](PRIVACY.md) §4.
4. Surface the new endpoint in the in-product Outbound connections view ([`UX.md`](UX.md) §10).
5. Add the connection to the conformance test that verifies the egress allowlist matches reality.

If the answer to "is this truly necessary" is anything less than "yes," the PR will be rejected.

### When you log a new field

Add the field to the redaction policy table in [`DAEMON.md`](DAEMON.md) §8.2 with an explicit decision: logged / not logged by default / never logged. If the field could contain user data (paths, hostnames, IPs, content), it must be redacted at default verbosity.

### When you add a new persisted file

The file must:

- Live in the OS-appropriate data directory ([`DAEMON.md`](DAEMON.md) §7.1).
- Be mode 0600 / per-user ACL.
- Be marked for backup-exclude and noindex per platform ([`DAEMON.md`](DAEMON.md) §7.4).
- Be removed by `mcp-bridge uninstall --purge` ([`DAEMON.md`](DAEMON.md) §2.1).

### When you touch the cryptographic surface

Pair, announce, and trust-model code paths get extra scrutiny:

- A second maintainer reviews; one reviewer is not sufficient.
- Changes that affect signed material need updated test vectors.
- Changes to key derivation, sealing, or nonce handling require a written rationale in the PR description.

---

## Development setup

### Prerequisites

- **Rust**: stable >= 1.85 ([rustup.rs](https://rustup.rs)).
- **Node.js**: >= 20 LTS.
- **pnpm**: >= 9 (`npm install -g pnpm`).
- **Tauri prerequisites** per platform — see [tauri.app/start/prerequisites](https://tauri.app/start/prerequisites).

Optional but recommended:

- `cargo-watch` for fast Rust iteration.
- `cargo-deny` for license / advisory checks (CI runs this).
- `rust-analyzer` for editor support.

### Clone and build

```bash
git clone https://github.com/<org>/mcp-bridge.git
cd mcp-bridge

# Daemon (Rust)
cd mcp-bridged
cargo build
cargo test

# UI (Tauri + Svelte)
cd ../bridge-console
pnpm install
pnpm tauri dev
```

`pnpm tauri dev` starts the daemon in-process for development; production launches the standalone `mcp-bridged` separately ([`DAEMON.md`](DAEMON.md) §2).

### Running locally

A first-time pair against a mock origin:

```bash
# In one terminal — start a mock MCP server
cargo run -p mock-origin --example fitness-tracker

# In another — run the daemon
cd mcp-bridged
cargo run -- daemon

# In a third — open the Console UI
cd bridge-console
pnpm tauri dev
```

Mock origins for testing live under `test/origins/`.

### Testing

Three layers, all required to pass for a PR to merge:

1. **Unit tests** — `cargo test` (daemon), `pnpm test` (UI).
2. **Integration tests** — `cargo test --features integration` runs the in-process daemon end-to-end with mocks.
3. **Conformance tests** — `cargo test --features conformance` validates against the JSON test vectors in `test-vectors/`.

A PR that changes wire-protocol behavior must include updated test vectors and a justification.

---

## Project layout

```
mcp-bridge/
├── mcp-bridged/        — Rust daemon (DAEMON.md)
├── bridge-console/     — Tauri + Svelte UI (UI.md, UX.md)
├── mcp-bridge-mobile/  — mobile SDK (forthcoming MOBILE.md)
├── test-vectors/       — mcp-pair / mcp-announce conformance fixtures
├── docs/               — architecture, design, legal, privacy
└── .github/            — CI workflows, issue and PR templates
```

For where specific things live within each subproject, see the layout sections in [`DAEMON.md`](DAEMON.md) §3 and [`UI.md`](UI.md) §11.

---

## Commit messages

We use **Conventional Commits** plus DCO sign-off.

Format:

```
<type>(<scope>): <subject>

<body — optional, wrap at 72 cols>

<footer — optional, refs/closes issues, BREAKING CHANGE>

Signed-off-by: Your Name <your-email@example.com>
```

**Types**: `feat`, `fix`, `docs`, `chore`, `refactor`, `perf`, `test`, `build`, `ci`.

**Scopes** (commonly used): `daemon`, `proxy`, `pair`, `announce`, `ui`, `console`, `tray`, `pair-window`, `adapter-claude`, `adapter-cursor`, `mobile`, `docs`, `legal`.

Examples:

```
fix(proxy): close pooled connection on auth rotation

The Origin Connector was reusing pooled connections after the bearer
token was rotated, leading to 401s once the old token was no longer
accepted by the backend. Force-close the pool on auth_rotated_at
change.

Refs: #142
Signed-off-by: Patryk Brejdak <patryk@example.com>
```

```
feat(adapter-zed): add Zed editor MCP config writer

Implements the Adapter trait for Zed's settings.json schema.
Tested against Zed 0.140+.

Signed-off-by: Jane Doe <jane@example.com>
```

```
docs(privacy): expand redaction policy table with hostnames

Signed-off-by: Patryk Brejdak <patryk@example.com>
```

Breaking changes go in the footer:

```
feat(announce): require seq field in v0.2 announce records

BREAKING CHANGE: announce records without `seq` are rejected.
Existing v0.1 implementations need to upgrade their SDK to a 0.2-aware
release before the next bridge daemon update.
```

Breaking changes pre-1.0 are allowed but must be flagged.

---

## Pull request process

### Before opening a PR

- Branch from `main`: `git checkout -b feat/zed-adapter`.
- Branch naming: `<type>/<short-summary>` (matches commit type).
- Build and test locally — CI will run the same checks.
- For non-trivial changes, open a discussion or draft PR first to align on approach.

### Opening the PR

The PR template (`.github/PULL_REQUEST_TEMPLATE.md`) prompts for:

- **What** — concise summary.
- **Why** — motivation (link to issue / discussion if applicable).
- **How** — the approach you took, with notable trade-offs.
- **Tests** — what you added; how to reproduce.
- **Docs** — which docs you updated.
- **Privacy review** — answers to the §6 checklist if applicable.
- **DCO sign-off** — confirmed in the commits themselves; the CI bot checks.

Keep PRs focused. If you have two unrelated changes, open two PRs.

### Review

- Maintainers review on a best-effort basis. Pre-1.0 we are slow; expect 1–2 weeks for non-urgent PRs.
- Security-adjacent PRs get two reviewers; one is not sufficient.
- Reviewers may request: changes, more tests, doc updates, a different approach, or rejection with reasoning.
- We squash-merge by default. Your individual commits don't need to be perfect, but the final squashed message must follow the commit-message format.

### Required checks (CI)

- `cargo fmt --all -- --check` passes.
- `cargo clippy --all-targets -- -D warnings` passes.
- `cargo test --workspace` passes.
- `cargo deny check` passes (license + advisory).
- `pnpm check` (svelte-check + tsc) passes.
- `pnpm test` passes.
- DCO bot finds sign-off on every commit.
- SLSA provenance attestation builds successfully.

PRs with failing checks are not eligible for review until the checks pass.

---

## Coding standards

### Rust

- `rustfmt` formatting — no manual layout.
- `clippy` clean — `-D warnings` in CI.
- No `unwrap()` or `expect()` in production code paths. They are acceptable in tests and in `main.rs` startup where panic is the right behavior.
- No `unsafe` without a `// SAFETY:` comment explaining the invariant.
- Error types use `thiserror::Error`; error propagation uses `?` and `anyhow::Result` only at binary boundaries.
- Async by default for I/O. Use `tokio::task::spawn_blocking` for CPU-heavy work.
- New sensitive types derive `Zeroize` + `ZeroizeOnDrop` ([`DAEMON.md`](DAEMON.md) §7.3).

### TypeScript

- `tsc --noEmit` must pass with the strict config in [`UI.md`](UI.md) §5.2.
- `any` is forbidden without a `// eslint-disable-line` and a justification comment.
- Prefer `interface` for object shapes, `type` for unions and primitives.
- Imports are organized: `node:` → external → internal-aliased (`$lib/`) → relative.
- No default exports for components.

### Svelte 5

- Runes-based reactivity: `$state`, `$derived`, `$effect`, `$props`. No legacy `$:` syntax.
- Shared state lives in `src/lib/state/*.svelte.ts` modules using runes, not in `writable` stores.
- Component file size cap: 250 lines. Split larger.
- Tailwind utility classes preferred over scoped `<style>`; use `<style>` only when CSS variables or platform-attribute logic is genuinely needed.

### Comments

- Default: no comments. Well-named identifiers explain *what*.
- Add comments only where *why* is non-obvious — invariants, workarounds for specific bugs, surprising behavior, hidden constraints.
- Never reference the current PR or issue ("added for #142", "fix for the cursor-config bug"). That metadata belongs in the commit message and PR description, where it stays accurate.
- One-line max per comment in code (multi-line block comments only at module / type level when documenting a published contract).

---

## Testing requirements

| Change type | Tests required |
|---|---|
| Bug fix | regression test demonstrating the bug; CI fails before the fix, passes after |
| Feature (UI) | component tests for the new component(s); integration test for the user flow |
| Feature (daemon) | unit tests on the new module; integration test against a mock origin |
| Adapter | mock Consumer config files; write + remove tested via the `_mcp_bridge_managed` UUID tag |
| Wire protocol | updated conformance test vectors in `test-vectors/`; both sides (daemon, mock SDK) verified |
| Performance | benchmark before/after; results in PR description |

Coverage targets: pragmatic, not numeric. We don't enforce a percentage. We do enforce "every public function has at least one test" for daemon modules.

---

## Documentation

If your change is user-visible, you must update:

- [`UX.md`](UX.md) if the UI or copy changes.
- [`UI.md`](UI.md) if the UI tech stack or window structure changes.
- [`DAEMON.md`](DAEMON.md) if the daemon's behavior, IPC surface, or persistence changes.
- [`ARCHITECTURE.md`](ARCHITECTURE.md) if the cross-cutting pattern, trust model, or wire protocols change.
- [`PRIVACY.md`](PRIVACY.md) if data lifetimes, egress, or threat-model coverage changes.
- [`SECURITY.md`](SECURITY.md) if the scope or disclosure process changes.
- [`NOTICE`](NOTICE) if dependencies change.

A PR that changes behavior without updating the docs will be sent back for the docs.

---

## Recognition

We maintain a `CONTRIBUTORS.md` at the repo root listing everyone who has contributed code, docs, design, or substantive issue reports. Recognition is opt-in (some contributors prefer to stay off lists); the DCO sign-off is the legal record.

Security acknowledgements are tracked separately at `mcpbridge.me/security#acknowledgements` per [`SECURITY.md`](SECURITY.md) §6.

---

## Maintainers

Initial maintainer: `<TBD before public release>`.

Maintainership becomes meaningful only with multiple contributors; for now the founding maintainer reviews everything. As the project grows, additional maintainers will be added through a documented nomination process.

---

## Trademark notes

"MCP Bridge" is an unregistered word mark ([`LEGAL.md`](LEGAL.md) §5). Contributors who fork the project for unrelated purposes should pick a different name. Compatibility statements ("works with MCP Bridge", "MCP Bridge compatible") are permitted under nominative fair use.

---

## Status

Working draft. Action items before opening the project to external contributions ([`LEGAL.md`](LEGAL.md) §14):

- GitHub repo set up with CI configured for the checks listed in §10.
- DCO bot installed.
- `.github/PULL_REQUEST_TEMPLATE.md` and issue templates committed.
- `CODE_OF_CONDUCT.md` vendored.
- This file moved to repository root.

# 0004 — Developer Certificate of Origin over a Contributor License Agreement

- **Status**: Accepted
- **Date**: 2026-05-23
- **Deciders**: Project founders
- **Supersedes**: —
- **Related**: [`CONTRIBUTING.md`](../CONTRIBUTING.md), [`LEGAL.md`](../LEGAL.md) §10

## Context

MCP Bridge is **Apache 2.0**-licensed. Once the project opens to contributors, every code contribution must come with some legal record establishing that the contributor had the right to contribute the work under the project's license, and that future relicensing or enforcement is workable.

Two mechanisms are mainstream:

- **Contributor License Agreement (CLA)** — a separate agreement (often a click-through CLA bot like CLA Assistant). The contributor grants the project operator a broad license, sometimes with copyright assignment. Examples: Google projects, Apache Software Foundation, many corporate-stewarded OSS projects.
- **Developer Certificate of Origin (DCO)** — a per-commit attestation. The contributor adds `Signed-off-by: Name <email>` to each commit, certifying the four-clause DCO 1.1 text. Examples: Linux kernel, Git, Docker, GitLab, Chef, the broader CNCF.

The MCP Bridge project posture, summarized:

- **Single-maintainer at the start, growing slowly.** A heavyweight CLA process is friction the project cannot bear pre-1.0.
- **No intent to relicense under a proprietary or commercial license.** A CLA's classic benefit — the project operator being free to relicense — is not something we want or claim.
- **Apache 2.0's outbound patent grant is already broad.** The DCO supplies the inbound chain-of-title certification we need.
- **Contributor friction matters disproportionately for a small project**: each contributor who has to sign a CLA before their first PR is a contributor who might just walk away.

## Decision

Use the **Developer Certificate of Origin 1.1**, verbatim from [developercertificate.org](https://developercertificate.org). Every commit must carry a `Signed-off-by:` trailer that matches `git config user.name` and `git config user.email`. CI rejects PRs with unsigned commits via the DCO bot.

We do **not** require a separate CLA. We do **not** require copyright assignment.

## Alternatives considered

- **Click-through CLA (CLA Assistant, EasyCLA)** — would give the operator broader rights at the cost of every new contributor having to read and accept a separate agreement. Rejected because (a) the operator doesn't need those rights given that relicensing is not a goal, (b) the friction is real for a small project, and (c) DCO is the industry default for projects with similar posture.
- **Copyright assignment (FSF / Apache-style)** — even higher friction; assumes a legal entity capable of holding assignments. We don't have one and don't want to require contributors to deal with one.
- **Nothing** (no formal attestation, just the Apache 2.0 license file) — relies entirely on the implicit "contributor licenses inbound on the same terms as outbound" doctrine. Workable for small projects, but a per-commit sign-off costs almost nothing and gives us a clean record if a chain-of-title question ever arises.

## Consequences

What this enables:

- **Low contributor friction**: one `-s` flag on `git commit`. No external service to sign up for. No agreement to read. The certification text is in [`CONTRIBUTING.md`](../CONTRIBUTING.md) §4 verbatim for anyone who wants to know what they are attesting to.
- **Per-commit audit trail**: every commit carries its own sign-off, not a one-time consent gate. If a contributor's authority changes mid-project, only their later commits are in question; earlier ones are independently certified.
- **No special legal entity required** to receive contributions.

Costs we accept:

- **Relicensing the project as a whole becomes effectively impossible** without re-collecting consent from every contributor. We are committing to Apache 2.0 as a one-way door.
- **The operator does not get a broader inbound patent grant** than Apache 2.0 already gives. We believe Apache 2.0's grant is sufficient; reasonable people disagree.
- **A small DCO-enforcement surface**: a CI bot must verify sign-off on every PR. We accept the maintenance.

What would force a revisit:

- A legitimate need to dual-license (e.g., a commercial edition) — extremely unlikely given the project's posture.
- An institutional steward (foundation, ASF, CNCF) requiring a different model as a condition of stewardship.

## Notes

The DCO 1.1 text is reproduced verbatim in [`CONTRIBUTING.md`](../CONTRIBUTING.md) §4 so contributors do not have to chase a link to know what they are certifying.

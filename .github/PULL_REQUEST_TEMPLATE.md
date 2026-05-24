<!--
Thanks for contributing to MCP Bridge.

Before opening this PR, please skim docs/CONTRIBUTING.md — in particular:
 - §3.4 wire-protocol-change RFC process (if you're touching mcp-pair/announce
   or the IPC surface)
 - §4 DCO sign-off (every commit must be -s)
 - §6 privacy and security review checklist (if applicable)

CI will not run review-required reviewers until the checks in
docs/CONTRIBUTING.md §10 are green.
-->

## What

<!-- One concise paragraph: what does this PR do? -->

## Why

<!-- Motivation. Link the issue / discussion / RFC if applicable. -->

## How

<!-- The approach you took, with notable trade-offs. Call out anything a
reviewer would otherwise have to ask about. -->

## Tests

<!-- What you added; how to reproduce. Per docs/CONTRIBUTING.md §11 table. -->

## Docs

<!-- Which docs you updated, or N/A. Reminder: behaviour changes without doc
updates get sent back. -->

## Privacy / security review

<!-- Required if any of the following apply (per docs/CONTRIBUTING.md §6):
  - new dependency
  - new outbound network connection
  - new persisted file
  - new logged field
  - any touch on cryptographic surface
Strip this whole section if none apply. -->

- [ ] Dependency review (license, network, alternatives, lifetime, added to docs/NOTICE)
- [ ] Outbound-connection review (discussion opened, allowlist updated, surfaced in UI)
- [ ] Persisted-file review (location, mode 0600, backup-exclude, removable via --purge)
- [ ] Logged-field review (redaction policy table updated)
- [ ] Cryptographic-surface review (second reviewer, test vectors, written rationale)

## DCO

<!-- Confirm each commit on this branch carries a Signed-off-by: line matching
your git config. CI rejects unsigned commits. -->

- [ ] Every commit is signed off (`git commit -s`).

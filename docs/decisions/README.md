# Architecture Decision Records

This directory holds **Architecture Decision Records** (ADRs) for MCP Bridge — short, append-only notes that document the load-bearing technical choices, why they were made, what was considered, and what the trade-offs are.

The goal is twofold:

- **For new contributors**: surface the *why* behind choices that would otherwise look arbitrary or be relitigated every six months.
- **For future-us**: keep a durable record of the constraints in play when each decision was made, so that revisiting a decision is informed by the original context.

## Format

We use a lightly customized [MADR](https://adr.github.io/madr/) ("Markdown Architecture Decision Records") template — see [`0000-template.md`](0000-template.md). Each ADR is a single file at this level, numbered sequentially. The number is permanent; the title slug is descriptive.

## Lifecycle

- **Proposed** — opened as a draft for discussion.
- **Accepted** — the decision is in force.
- **Superseded** — replaced by a later ADR. The newer ADR links back; this one is left in place for history. ADRs are never deleted.
- **Deprecated** — the decision is no longer in force but no replacement was needed.

A change to an "Accepted" ADR is a new ADR that supersedes it, not an edit to the original. Typos and broken links are exceptions.

## When to write one

Write an ADR when:

- A choice affects the project's shape across multiple subsystems.
- A choice trades off two reasonable alternatives, and the reasoning is non-obvious from the code.
- A choice would be expensive to reverse later.

Do not write one for routine library choices (logger, formatter, helper crate) — those go in a PR description.

## Index

| # | Status | Title |
|---|---|---|
| [0001](0001-stable-loopback-over-config-rewriter.md) | Accepted | Stable-Loopback Bridge over config-rewriter design |
| [0002](0002-tauri-over-electron.md) | Accepted | Tauri over Electron for Bridge Console |
| [0003](0003-kmp-for-mobile-core.md) | Accepted | Kotlin Multiplatform for the mobile SDK core |
| [0004](0004-dco-over-cla.md) | Accepted | Developer Certificate of Origin over a Contributor License Agreement |
| [0005](0005-path-b-per-platform-icons.md) | Accepted | Path B — per-platform native icon sets |

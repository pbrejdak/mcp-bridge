# Changelog

All notable changes to MCP Bridge will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Wire-protocol versions ([`mcp-pair`](docs/SPEC.md#4-mcp-pairv01--pairing-protocol),
[`mcp-announce`](docs/SPEC.md#5-mcp-announcev01--identity-refresh-protocol))
are versioned independently of the project and are called out explicitly when
they change.

## [Unreleased]

### Added

- Initial design and policy documentation set ([`docs/`](docs/)):
  - [`ARCHITECTURE.md`](docs/ARCHITECTURE.md) — Stable-Loopback Bridge pattern, trust model, sequence diagrams.
  - [`SPEC.md`](docs/SPEC.md) — normative wire protocol specification (`mcp-pair/v0.1`, `mcp-announce/v0.1`).
  - [`DAEMON.md`](docs/DAEMON.md) — Rust daemon internals.
  - [`UI.md`](docs/UI.md), [`UX.md`](docs/UX.md) — Bridge Console design.
  - [`MOBILE.md`](docs/MOBILE.md) — Origin-side mobile SDK.
  - [`PRIVACY.md`](docs/PRIVACY.md) — privacy charter and threat model.
  - [`SECURITY.md`](docs/SECURITY.md) — responsible disclosure policy.
  - [`LEGAL.md`](docs/LEGAL.md) — licensing, ToS, trademark, export classification.
  - [`CONTRIBUTING.md`](docs/CONTRIBUTING.md) — DCO sign-off, privacy-first review checklist.
  - [`GLOSSARY.md`](docs/GLOSSARY.md) — terminology reference.
- [`README.md`](README.md) at repo root.
- [`LICENSE`](LICENSE) — Apache License 2.0.
- [`docs/NOTICE`](docs/NOTICE) — Apache 2.0 attribution.

### Changed

- (none)

### Deprecated

- (none)

### Removed

- (none)

### Fixed

- (none)

### Security

- (none)

### Wire protocol

- (none — `mcp-pair/v0.1` and `mcp-announce/v0.1` are the inaugural versions.)

[Unreleased]: https://github.com/mcp-bridge/mcp-bridge/compare/HEAD

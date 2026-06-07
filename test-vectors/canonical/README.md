# `canonical/` — RFC 8785 (JCS) canonical-JSON fixtures

Cross-language conformance fixtures for the canonical-JSON encoding
mandated by [`docs/SPEC.md`](../../docs/SPEC.md) §3.3. Every signed
payload in the MCP Bridge wire protocols (`mcp-pair/v0.1`,
`mcp-announce/v0.1`) is signed over the byte sequence produced by
canonicalizing the payload — implementations must agree on that
sequence or `Ed25519_Verify` is meaningless.

Both the Rust daemon (via `serde_jcs::to_vec`) and the Kotlin
Multiplatform Bridge Peer core (via
`dev.mcpbridge.mobile.canonical.CanonicalJson.encodeToBytes`) run these
fixtures in CI. Any drift between the two surfaces a wire-protocol
divergence at fixture time, not at runtime in someone's living room.

## Fixture format

```jsonc
{
  "name": "kebab-case-id",
  "description": "Plain prose explaining what this case tests and why.",
  "input": <any JSON value>,
  "canonical": "the exact UTF-8 canonical string the input must canonicalize to"
}
```

`input` is parsed as a generic JSON value, fed through the
implementation under test, and the resulting bytes are compared with
`canonical.encodeToByteArray()` (or `canonical.as_bytes()` on the Rust
side). Byte-equality is the only acceptance criterion.

Adding a fixture is one step: drop a JSON file in this directory. The
Rust harness in [`mcp-bridged/tests/canonical_conformance.rs`](../../mcp-bridged/tests/canonical_conformance.rs)
and the Kotlin harness in
[`mcp-bridge-mobile/core/src/jvmTest/kotlin/dev/mcpbridge/mobile/canonical/CanonicalConformanceTest.kt`](../../mcp-bridge-mobile/core/src/jvmTest/kotlin/dev/mcpbridge/mobile/canonical/CanonicalConformanceTest.kt)
will both pick it up automatically.

## Scope

These fixtures cover the canonical-JSON encoding **only** — not
signature derivation, not payload semantics. Signed-payload fixtures
live in sibling directories (`invite/`, future `announce/`, etc.) and
have their own wrapper format.

The KMP encoder rejects floating-point numbers (the wire protocols
carry only integer numerics; see the doc on
`CanonicalJson` for the trade-off). Fixtures here therefore do not
include floats; that is a deliberate constraint, not an oversight.

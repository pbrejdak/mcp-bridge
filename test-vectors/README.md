# Conformance test vectors

This directory holds the normative fixtures that every conforming Origin and Resolver implementation must agree with. See [`docs/SPEC.md`](../docs/SPEC.md) §9 for the conformance contract.

## Files

| Path | Purpose |
|---|---|
| [`sas-wordlist-v1.txt`](sas-wordlist-v1.txt) | The 2048-word lowercase ASCII wordlist used by the SAS derivation in [`docs/SPEC.md`](../docs/SPEC.md) §4.2. **Locked at v0.1** — any change requires a new minor version of `mcp-pair`. |
| [`invite/`](invite/) | Direction-B `mcp-pair/v0.1` invite acceptance / rejection fixtures ([SPEC §4.2](../docs/SPEC.md)). |

Fixture suites for sealed pair-payload acceptance, announce sequences, replay/stale-clock rejection, and RFC 8785 canonicalization will land here as Phase 1 progresses.

## Fixture file format

Every JSON fixture under a per-subprotocol directory (e.g. [`invite/`](invite/)) is a self-documenting wrapper:

```jsonc
{
  "name": "kebab-case-id",
  "description": "Plain prose explaining what this case tests and why.",
  "expect": "accept" | "reject",
  "payload": { /* the actual mcp-pair/v0.1 payload under test */ }
}
```

Conformance harnesses iterate every `*.json` file in the directory, deserialize the wrapper, and run `payload` through the implementation under test. The result MUST agree with `expect`. Accept-cases additionally MUST survive a re-serialize / re-deserialize round-trip without loss.

The Rust reference harness lives in [`mcp-bridged/tests/invite_conformance.rs`](../mcp-bridged/tests/invite_conformance.rs); other-language SDKs are expected to ship their own walker over the same fixture tree.

Adding a fixture is two steps: drop a new JSON file in the appropriate subdirectory and write a clear `description`. The harness picks it up automatically — no code change.

## `sas-wordlist-v1.txt`

The wordlist is the [BIP39 English wordlist](https://github.com/bitcoin/bips/blob/master/bip-0039/english.txt), adopted verbatim. Properties:

- Exactly 2048 entries (so the 11-bit index extracted in [`docs/SPEC.md`](../docs/SPEC.md) §4.2 maps bijectively to a word).
- All entries lowercase ASCII, 3–8 characters.
- Curated by the Bitcoin community for distinguishability (no two words share their first four characters, no homoglyph pairs, distinct phonetic profiles).

**Canonical SHA-256**:

```
2f5eed53a4727b4bf8880d8f3f199efc90e58503646d9ff8eff3a2ed3b24dbda
```

To verify locally:

```bash
shasum -a 256 test-vectors/sas-wordlist-v1.txt
```

If the hash differs from the value above, the file has drifted and any SAS derivation using it will disagree with conforming implementations.

### License and attribution

BIP-0039 ([Mnemonic code for generating deterministic keys](https://github.com/bitcoin/bips/blob/master/bip-0039.mediawiki), Marek Palatinus & Pavol Rusnak, 2013) is published under [BSD-2-Clause](https://github.com/bitcoin/bips/blob/master/LICENSE). The wordlist itself is a list of common English words and is widely treated as effectively unencumbered. We adopt it verbatim with attribution.

### Why BIP39 instead of a custom list

A custom 2048-word list curated for distinguishability is a real artefact that warrants its own review cycle and would delay Phase 1. BIP39 has had a decade of cryptographic-community review specifically for the distinguishability properties we need. Adopting it verbatim:

- Saves the curation pass.
- Inherits an established review record.
- Lets any developer cross-check the list against a well-known reference.

A future major version of `mcp-pair` may adopt a different list (the spec leaves room — see [`docs/SPEC.md`](../docs/SPEC.md) §8.4). For v0.1 this is the right default.

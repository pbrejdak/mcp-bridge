//! SAS (Short Authentication String) phrase derivation.
//!
//! Implements [`docs/SPEC.md`](../../../../docs/SPEC.md) §4.2 — derive a
//! four-word lowercase ASCII phrase from
//! `H = SHA-256(resolver.pubkey || nonce)`, with each word selected by taking
//! a big-endian `u16` from successive byte pairs of `H[0..8]` modulo 2048
//! and indexing into [`WORDLIST_V1`].

use std::sync::OnceLock;

use sha2::{Digest, Sha256};

/// Raw bytes of the SAS wordlist v1 — BIP39 English, locked at v0.1.
///
/// See [`docs/SPEC.md`](../../../../docs/SPEC.md) §8.4 and
/// [`test-vectors/README.md`](../../../../test-vectors/README.md).
pub const WORDLIST_V1: &str = include_str!("../../../test-vectors/sas-wordlist-v1.txt");

/// Number of entries the wordlist must contain.
pub const WORDLIST_LEN: usize = 2048;

/// Words drawn per derived phrase.
pub const WORDS_PER_PHRASE: usize = 4;

static WORDS: OnceLock<Vec<&'static str>> = OnceLock::new();

fn words() -> &'static [&'static str] {
    WORDS
        .get_or_init(|| {
            let parsed: Vec<&'static str> = WORDLIST_V1.lines().collect();
            assert_eq!(
                parsed.len(),
                WORDLIST_LEN,
                "SAS wordlist must have exactly {WORDLIST_LEN} entries"
            );
            parsed
        })
        .as_slice()
}

/// Derive the four-word SAS phrase for a `(resolver_pubkey, nonce)` pair.
///
/// Returns a lowercase ASCII string of the form `word1-word2-word3-word4`,
/// each part drawn from [`WORDLIST_V1`].
pub fn derive(resolver_pubkey: &[u8; 32], nonce: &[u8; 16]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(resolver_pubkey);
    hasher.update(nonce);
    let hash = hasher.finalize();

    let table = words();
    let parts: Vec<&'static str> = hash[..2 * WORDS_PER_PHRASE]
        .chunks_exact(2)
        .map(|pair| {
            let raw = u16::from_be_bytes([pair[0], pair[1]]);
            table[(raw as usize) % WORDLIST_LEN]
        })
        .collect();
    parts.join("-")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wordlist_has_exactly_2048_entries() {
        assert_eq!(words().len(), WORDLIST_LEN);
    }

    #[test]
    fn every_word_is_lowercase_ascii_3_to_8_chars() {
        for (i, w) in words().iter().enumerate() {
            assert!(
                w.chars().all(|c| c.is_ascii_lowercase()),
                "word at index {i} is not lowercase ASCII: {w:?}"
            );
            assert!(
                (3..=8).contains(&w.len()),
                "word at index {i} violates SPEC.md §8.4 3-8 char range: {w:?} (len {})",
                w.len()
            );
        }
    }

    #[test]
    fn derive_returns_four_dash_joined_words_from_the_wordlist() {
        let phrase = derive(&[0u8; 32], &[0u8; 16]);
        let parts: Vec<&str> = phrase.split('-').collect();
        assert_eq!(parts.len(), WORDS_PER_PHRASE);
        for part in parts {
            assert!(words().contains(&part), "{part:?} is not in WORDLIST_V1");
        }
    }

    #[test]
    fn derive_is_deterministic() {
        let key = [0xABu8; 32];
        let nonce = [0xCDu8; 16];
        assert_eq!(derive(&key, &nonce), derive(&key, &nonce));
    }

    #[test]
    fn derive_changes_when_the_nonce_changes() {
        let key = [0u8; 32];
        let nonce_a = [0u8; 16];
        let mut nonce_b = [0u8; 16];
        nonce_b[15] = 1;
        assert_ne!(derive(&key, &nonce_a), derive(&key, &nonce_b));
    }

    #[test]
    fn derive_changes_when_the_pubkey_changes() {
        let key_a = [0u8; 32];
        let mut key_b = [0u8; 32];
        key_b[31] = 1;
        let nonce = [0u8; 16];
        assert_ne!(derive(&key_a, &nonce), derive(&key_b, &nonce));
    }

    /// Known-answer vector A: 48 zero bytes input.
    ///
    /// Verified independently against the BIP39 wordlist using shell:
    ///   `printf '\x00%.0s' $(seq 1 48) | shasum -a 256` →
    ///   `17b0761f87b081d5...`, indices `[1968, 1567, 1968, 469]`.
    #[test]
    fn kat_all_zeros() {
        assert_eq!(
            derive(&[0u8; 32], &[0u8; 16]),
            "voyage-sentence-voyage-deny"
        );
    }

    /// Known-answer vector B: pubkey of `0xAA` bytes, nonce of `0x55` bytes.
    ///
    /// Verified independently via shell:
    ///   `{ printf '\xaa%.0s' $(seq 1 32); printf '\x55%.0s' $(seq 1 16); }`
    ///   `| shasum -a 256` → `37e2be3a250028c7...`,
    ///   indices `[1506, 1594, 1280, 211]`.
    #[test]
    fn kat_aa_pubkey_55_nonce() {
        assert_eq!(
            derive(&[0xAAu8; 32], &[0x55u8; 16]),
            "wisdom-shrug-parade-body"
        );
    }
}

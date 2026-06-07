//! Drives every `test-vectors/canonical/*.json` fixture through
//! `serde_jcs::to_vec` and asserts the result matches each fixture's
//! `canonical` field byte-for-byte.
//!
//! The Kotlin Multiplatform Bridge Peer core runs the same fixture
//! set against its own canonical-JSON encoder. The two surfaces must
//! agree — that is the entire purpose of having the fixtures in
//! `test-vectors/` instead of separate per-implementation unit tests.
//! See `mcp-bridge-mobile/core/src/jvmTest/kotlin/.../CanonicalConformanceTest.kt`
//! and [`test-vectors/canonical/README.md`].

use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Fixture {
    name: String,
    #[allow(dead_code)]
    description: String,
    input: serde_json::Value,
    canonical: String,
}

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crate manifest dir has a parent (the workspace root)")
        .join("test-vectors")
        .join("canonical")
}

fn load_fixtures() -> Vec<(PathBuf, Fixture)> {
    let dir = fixture_dir();
    let mut fixtures: Vec<(PathBuf, Fixture)> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read_dir({dir:?}): {e}"))
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .map(|path| {
            let content =
                fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
            let fixture: Fixture = serde_json::from_str(&content)
                .unwrap_or_else(|e| panic!("parse {path:?} as Fixture: {e}"));
            (path, fixture)
        })
        .collect();
    fixtures.sort_by(|a, b| a.0.file_name().cmp(&b.0.file_name()));
    fixtures
}

#[test]
fn every_fixture_canonicalizes_to_its_expected_bytes() {
    let fixtures = load_fixtures();
    assert!(
        !fixtures.is_empty(),
        "no fixtures found in {:?}",
        fixture_dir()
    );

    let mut failures: Vec<String> = Vec::new();

    for (path, fixture) in &fixtures {
        let actual = serde_jcs::to_vec(&fixture.input).unwrap_or_else(|e| {
            panic!(
                "{}: serde_jcs::to_vec failed: {e}",
                path.file_name().unwrap().to_string_lossy()
            )
        });
        let expected = fixture.canonical.as_bytes();
        if actual.as_slice() != expected {
            failures.push(format!(
                "{} ({}):\n  expect: {}\n  got:    {}",
                fixture.name,
                path.file_name().unwrap().to_string_lossy(),
                bytes_repr(expected),
                bytes_repr(&actual),
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "{} of {} canonical fixtures diverged:\n\n{}",
        failures.len(),
        fixtures.len(),
        failures.join("\n\n"),
    );
}

/// Render bytes with non-printables escaped so divergences are
/// readable in failure output.
fn bytes_repr(b: &[u8]) -> String {
    let mut s = String::from("\"");
    for &c in b {
        match c {
            0x22 => s.push_str("\\\""),
            0x5C => s.push_str("\\\\"),
            0x08 => s.push_str("\\b"),
            0x09 => s.push_str("\\t"),
            0x0A => s.push_str("\\n"),
            0x0C => s.push_str("\\f"),
            0x0D => s.push_str("\\r"),
            0x20..=0x7E => s.push(c as char),
            _ => s.push_str(&format!("\\x{c:02x}")),
        }
    }
    s.push('"');
    s
}

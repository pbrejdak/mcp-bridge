//! Drives every `test-vectors/invite/*.json` fixture against the `Invite`
//! deserializer and asserts the result matches each fixture's `expect`.
//!
//! Adding a fixture file under `test-vectors/invite/` automatically extends
//! this test — no harness change needed.

use std::fs;
use std::path::{Path, PathBuf};

use mcp_bridged::pair::Invite;
use serde::Deserialize;

/// Self-documenting wrapper for every fixture file.
#[derive(Debug, Deserialize)]
struct Fixture {
    name: String,
    #[allow(dead_code)]
    description: String,
    expect: Expect,
    payload: serde_json::Value,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum Expect {
    Accept,
    Reject,
}

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crate manifest dir has a parent (the workspace root)")
        .join("test-vectors")
        .join("invite")
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
    // Stable order for stable failure output.
    fixtures.sort_by(|a, b| a.0.file_name().cmp(&b.0.file_name()));
    fixtures
}

#[test]
fn every_fixture_matches_its_expect() {
    let fixtures = load_fixtures();
    assert!(
        !fixtures.is_empty(),
        "no fixtures found in {:?}",
        fixture_dir()
    );

    let mut failures: Vec<String> = Vec::new();

    for (path, fixture) in &fixtures {
        let parsed: Result<Invite, _> = serde_json::from_value(fixture.payload.clone());
        let actual = if parsed.is_ok() {
            Expect::Accept
        } else {
            Expect::Reject
        };
        if actual != fixture.expect {
            let detail = match (&fixture.expect, parsed) {
                (Expect::Accept, Err(e)) => format!("expected accept, got reject: {e}"),
                (Expect::Reject, Ok(_)) => "expected reject, got accept".to_owned(),
                _ => unreachable!(),
            };
            failures.push(format!(
                "  {} ({}): {detail}",
                fixture.name,
                path.file_name().unwrap().to_string_lossy()
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "{} of {} invite fixture(s) failed:\n{}",
        failures.len(),
        fixtures.len(),
        failures.join("\n")
    );
}

/// Accept-expected fixtures must round-trip through serialization without
/// loss — deserialize, re-serialize, re-deserialize, compare.
#[test]
fn accept_fixtures_round_trip_stably() {
    let mut failures: Vec<String> = Vec::new();

    for (path, fixture) in load_fixtures() {
        if fixture.expect != Expect::Accept {
            continue;
        }
        let first: Invite = match serde_json::from_value(fixture.payload.clone()) {
            Ok(v) => v,
            Err(_) => continue, // already reported by the other test
        };
        let json = serde_json::to_string(&first).expect("serialize");
        let second: Invite = match serde_json::from_str(&json) {
            Ok(v) => v,
            Err(e) => {
                failures.push(format!(
                    "  {}: re-deserialize after to_string failed: {e}",
                    fixture.name
                ));
                continue;
            }
        };
        if first != second {
            failures.push(format!(
                "  {} ({}): round-trip changed the value",
                fixture.name,
                path.file_name().unwrap().to_string_lossy()
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "{} accept-fixture(s) lost data on round-trip:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

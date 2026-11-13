//! The §11.3 Release-PR body renderer (M4.3a): the stable, parseable
//! Release-PR body pinned against the §11.3 skeleton — the plan table,
//! the changelog preview, the override footer and the machine-readable
//! embedded plan (`<!-- halyard:plan v1 … -->`) that lets `release` /
//! `publish` verify I8 and bots read the PR.
//!
//! The renderer is the producer side of the seam; the §11.1 machine
//! ([`halyard_forge`], M4.2a) consumes the rendered body through
//! `PrSpec::body`.

use std::io::Write as _;

use base64::Engine as _;
use flate2::write::DeflateEncoder;
use halyard_changelog::{extract_embedded_plan, render_release_body};
use halyard_core::plan::{Bump, Plan, PlanChangelog, PlanReason, PlanRelease, PlanSkip};

/// A skeleton plan with no releases (the empty-plan edge).
fn plan_fixture() -> Plan {
    Plan {
        schema: "halyard-plan/v1".to_owned(),
        base_branch: "main".to_owned(),
        head: "9f2ca67".to_owned(),
        plan_hash: String::new(),
        releases: Vec::new(),
        edits: Vec::new(),
        order: Vec::new(),
        skipped: vec![PlanSkip {
            package: "docs".to_owned(),
            reason: "no_changes".to_owned(),
        }],
    }
}

/// A one-release plan fixture (the §5.8 spec example shape).
fn single_release_plan() -> Plan {
    let mut plan = plan_fixture();
    plan.releases.push(PlanRelease {
        package: "core".to_owned(),
        ecosystem: "cargo".to_owned(),
        from: "0.4.1".to_owned(),
        to: "0.5.0".to_owned(),
        bump: Bump::Minor,
        tag: "core-v0.5.0".to_owned(),
        reasons: vec![PlanReason::Commit {
            oid: "ab12cd34ef567890ab12cd34ef567890ab12cd34".to_owned(),
            kind: "feat".to_owned(),
            breaking: false,
        }],
        changelog: Some(PlanChangelog {
            file: "CHANGELOG.md".to_owned(),
            entry_md: "## 0.5.0\n\n### ⚠ BREAKING CHANGES\n\n- reshape contract\n\n### Features\n\n- grow surface\n".to_owned(),
        }),
        publish: None,
    });
    plan
}

#[test]
fn body_shape_matches_the_spec_skeleton() -> Result<(), Box<dyn std::error::Error>> {
    let body = render_release_body(&single_release_plan())?;
    // §11.3 skeleton, top to bottom.
    assert!(
        body.starts_with("## 🚀 Release plan\n"),
        "the body opens with the release-plan heading: {body}"
    );
    for expected in [
        "| Package | Current | Next | Bump | Why |",
        "|---|---|---|---|---|",
        "| `core` | 0.4.1 | **0.5.0** | minor |",
        "<details><summary>Changelog preview</summary>",
        "### ⚠ BREAKING CHANGES",
        "> Override: add `Release-As:",
        "<!-- halyard:plan v1 blake3:",
    ] {
        assert!(body.contains(expected), "missing {expected:?} in:\n{body}");
    }
    Ok(())
}

#[test]
fn the_embedded_marker_is_the_last_line() -> Result<(), Box<dyn std::error::Error>> {
    let body = render_release_body(&single_release_plan())?;
    let last = body.lines().last().ok_or("body has at least one line")?;
    assert!(
        last.starts_with("<!-- halyard:plan v1 blake3:") && last.ends_with("-->"),
        "the marker closes the body (stable parseable order): {last}"
    );
    Ok(())
}

#[test]
fn render_is_deterministic_byte_for_byte() -> Result<(), Box<dyn std::error::Error>> {
    let plan = single_release_plan();
    let first = render_release_body(&plan)?;
    let second = render_release_body(&plan)?;
    assert_eq!(first, second, "I3: identical inputs ⇒ byte-identical body");
    Ok(())
}

#[test]
fn why_column_maps_reasons_deterministically() -> Result<(), Box<dyn std::error::Error>> {
    let mut plan = plan_fixture();
    plan.releases = vec![
        PlanRelease {
            package: "a".to_owned(),
            ecosystem: "cargo".to_owned(),
            from: "0.1.0".to_owned(),
            to: "0.1.1".to_owned(),
            bump: Bump::Patch,
            tag: "a-v0.1.1".to_owned(),
            reasons: vec![PlanReason::Commit {
                oid: "deadbeef0000".to_owned(),
                kind: "feat".to_owned(),
                breaking: true,
            }],
            changelog: None,
            publish: None,
        },
        PlanRelease {
            package: "b".to_owned(),
            ecosystem: "cargo".to_owned(),
            from: "1.0.0".to_owned(),
            to: "1.0.1".to_owned(),
            bump: Bump::Patch,
            tag: "b-v1.0.1".to_owned(),
            reasons: vec![PlanReason::Cascade {
                via: "a".to_owned(),
                edge: "normal".to_owned(),
                why: "requirement_unsatisfied".to_owned(),
            }],
            changelog: None,
            publish: None,
        },
        PlanRelease {
            package: "c".to_owned(),
            ecosystem: "cargo".to_owned(),
            from: "2.0.0".to_owned(),
            to: "3.0.0".to_owned(),
            bump: Bump::Major,
            tag: "c-v3.0.0".to_owned(),
            reasons: vec![PlanReason::ReleaseAs {
                oid: "1234567".to_owned(),
                version: "3.0.0".to_owned(),
            }],
            changelog: None,
            publish: None,
        },
        PlanRelease {
            package: "d".to_owned(),
            ecosystem: "npm".to_owned(),
            from: "0.0.0".to_owned(),
            to: "0.1.0".to_owned(),
            bump: Bump::Minor,
            tag: "d-v0.1.0".to_owned(),
            reasons: vec![PlanReason::InitialRelease],
            changelog: None,
            publish: None,
        },
        PlanRelease {
            package: "e".to_owned(),
            ecosystem: "npm".to_owned(),
            from: "0.2.0".to_owned(),
            to: "0.2.1".to_owned(),
            bump: Bump::Patch,
            tag: "e-v0.2.1".to_owned(),
            reasons: vec![PlanReason::Changeset {
                file: ".halyard/changes/fix.md".to_owned(),
            }],
            changelog: None,
            publish: None,
        },
    ];
    let body = render_release_body(&plan)?;
    for (package, why) in [
        ("a", "`feat!` (deadbee)"),
        ("b", "cascade: `a`"),
        ("c", "`Release-As: 3.0.0`"),
        ("d", "initial release"),
        ("e", "changeset: `.halyard/changes/fix.md`"),
    ] {
        let row = body
            .lines()
            .find(|line| line.starts_with(&format!("| `{package}` |")))
            .ok_or_else(|| format!("row for {package}"))?;
        assert!(
            row.contains(why),
            "the Why column renders {why:?} for {package}: {row}"
        );
    }
    Ok(())
}

#[test]
fn embedded_plan_roundtrips_through_the_marker() -> Result<(), Box<dyn std::error::Error>> {
    let mut plan = single_release_plan();
    let body = render_release_body(&plan)?;
    let extracted = extract_embedded_plan(&body)?;
    // The embedded document is the completed plan: the renderer fills
    // `plan_hash` from its own recomputation (it never trusts a carried
    // field), so the roundtrip target is the plan with that field set.
    plan.plan_hash = halyard_core::canonical::plan_hash(&plan);
    assert_eq!(extracted, plan, "the embedded plan is the plan (I8)");
    Ok(())
}

#[test]
fn extract_declines_a_body_without_a_marker() {
    let Err(err) = extract_embedded_plan("## 🚀 Release plan\n\nno marker here") else {
        unreachable!("a body without the marker must be declined");
    };
    assert_eq!(err.code(), "E-CHLOG-PRBODY", "{err}");
}

#[test]
fn extract_declines_corrupted_payloads_fail_closed() {
    // I8: a tampered or truncated body never yields a plan that could
    // be trusted — every corruption mode fails closed with the stable
    // code, never a partial plan.
    let plan = single_release_plan();
    let body = render_release_body(&plan).unwrap_or_else(|e| unreachable!("render: {e}"));
    let marker_start = body
        .rfind("<!-- halyard:plan v1 ")
        .unwrap_or_else(|| unreachable!("rendered body carries the marker"));
    let prefix = &body[..marker_start];

    let cases: [(&str, String); 4] = [
        (
            "not-base64",
            format!("{prefix}<!-- halyard:plan v1 blake3:x **not-base64** -->"),
        ),
        (
            "not-deflate",
            format!("{prefix}<!-- halyard:plan v1 blake3:x aGVsbG8gd29ybGQ= -->"),
        ),
        ("not-json", {
            let blob = encode_fixture(b"definitely not a plan document");
            format!("{prefix}<!-- halyard:plan v1 blake3:x {blob} -->")
        }),
        ("hash-mismatch", {
            let blob = encode_fixture_payload(&plan);
            format!("{prefix}<!-- halyard:plan v1 blake3:deadbeef {blob} -->")
        }),
    ];
    for (name, tampered) in cases {
        let Err(err) = extract_embedded_plan(&tampered) else {
            unreachable!("corrupted payload {name:?} must be declined");
        };
        assert_eq!(
            err.code(),
            "E-CHLOG-PRBODY",
            "corruption mode {name:?} fails closed: {err}"
        );
    }
}

/// base64-encodes a deflate payload of `bytes` (the same encoding the
/// renderer uses, for building tampered fixtures).
fn encode_fixture(bytes: &[u8]) -> String {
    let mut encoder = DeflateEncoder::new(Vec::new(), flate2::Compression::default());
    encoder
        .write_all(bytes)
        .unwrap_or_else(|e| unreachable!("fixture deflate: {e}"));
    let compressed = encoder
        .finish()
        .unwrap_or_else(|e| unreachable!("fixture deflate finish: {e}"));
    base64::engine::general_purpose::STANDARD.encode(compressed)
}

/// Encodes a full plan payload (canonical JSON → deflate → base64).
fn encode_fixture_payload(plan: &Plan) -> String {
    let value =
        serde_json::to_value(plan).unwrap_or_else(|e| unreachable!("fixture plan serializes: {e}"));
    let json = halyard_core::canonical::canonical_json(&value);
    encode_fixture(json.as_bytes())
}

#[test]
fn empty_plan_renders_a_total_body() -> Result<(), Box<dyn std::error::Error>> {
    // The §11.1 machine never upserts a PR under an empty plan (it
    // closes the stale one instead) — the renderer stays total anyway so
    // tooling can preview any plan document.
    let body = render_release_body(&plan_fixture())?;
    assert!(body.contains("## 🚀 Release plan"));
    assert!(body.contains("No pending releases"));
    assert!(body.contains("<!-- halyard:plan v1 blake3:"));
    let extracted = extract_embedded_plan(&body)?;
    assert_eq!(extracted.releases.len(), 0);
    Ok(())
}

#[test]
fn pr_body_matches_the_golden_snapshot() -> Result<(), Box<dyn std::error::Error>> {
    let body = render_release_body(&single_release_plan())?;
    insta::assert_snapshot!(body);
    Ok(())
}

#[test]
fn skipped_packages_do_not_appear_in_the_table() -> Result<(), Box<dyn std::error::Error>> {
    let mut plan = single_release_plan();
    plan.skipped.push(PlanSkip {
        package: "cli".to_owned(),
        reason: "no_changes".to_owned(),
    });
    let body = render_release_body(&plan)?;
    assert!(
        !body.contains("| `cli` |"),
        "skipped packages are not release rows: {body}"
    );
    assert!(
        body.contains("`cli`"),
        "the skip is still visible in the plan"
    );
    Ok(())
}

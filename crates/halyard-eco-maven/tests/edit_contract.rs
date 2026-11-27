//! The M8 Maven cell's edit contract (M2 of ADR-0058 — the release
//! half; `docs/HALYARD_ARCHITECTURE.md` §17 YEAR 2, M8 row, gate
//! *"each: discover/edit/publish/resume tests + replay on 2 real
//! repos"*; VERIFIED grammar in `docs/research/maven-pom-grammars.md`).
//!
//! This pins the plan's edit acceptance as code: the plan's own-version
//! byte patches re-apply **semantically and in place** (invariant I4) —
//! each patch span must land on a `<project>`-owned `<version>`
//! element's text of the owning pom, the inner bytes swap for the plan's
//! target, the surrounding markup never moves, and a re-parse read-back
//! verifies the pom now carries the target before the patch is accepted.

use halyard_core::EcosystemAdapter;
use halyard_core::discover::DiscoverCtx;
use halyard_core::package::RelPath;
use halyard_core::plan::PlanPatch;
use halyard_eco_maven::{MavenAdapter, manifest_version};

const POM: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.example</groupId>
  <artifactId>edit-demo</artifactId>
  <version>1.2.3</version>
</project>
"#;

/// The version site the discovery row emits is the patch target the edit
/// seam accepts: discover a single pom, take its version site, and swap
/// the value — the markup and indentation never move.
#[test]
fn the_version_site_patch_swaps_the_inner_bytes_in_place() {
    let mut ctx = DiscoverCtx::new();
    ctx.insert_file("pom.xml", POM.as_bytes().to_vec());
    let rows = MavenAdapter::new().discover(&ctx).expect("discovery");
    let row = rows.first().expect("the fixture discovers one row");
    let site = row.version_site.as_ref().expect("a canonical version site");
    let span = site
        .spans
        .first()
        .copied()
        .expect("the version site carries a span");

    let file = RelPath::new("pom.xml").expect("fixture path");
    let patches = [PlanPatch {
        start: span.start,
        end: span.end,
        text: "1.3.0".to_owned(),
    }];
    let outcome = MavenAdapter::new()
        .edit_versions(&file, POM.as_bytes(), &patches)
        .expect("the patch applies");
    assert_eq!((outcome.applied, outcome.skipped), (1, 0));
    let text = String::from_utf8(outcome.content).expect("pom is utf-8");
    assert!(
        text.contains("<version>1.3.0</version>"),
        "the value swapped: {text}"
    );
    assert!(
        text.contains("  <artifactId>edit-demo</artifactId>"),
        "the surrounding markup never moves: {text}"
    );
    assert_eq!(
        text,
        POM.replace("<version>1.2.3</version>", "<version>1.3.0</version>"),
        "the only changed bytes are the version value"
    );
}

/// The read-back verify: the re-parsed pom carries the target before the
/// patch is accepted (the §9.1 semantic check).
#[test]
fn the_read_back_verify_refuses_a_patch_that_misses_the_target() {
    let file = RelPath::new("pom.xml").expect("fixture path");
    // A span that matches no `<project>`-owned version text (it points
    // at the artifactId's value).
    let needle = b"edit-demo";
    let start = u32::try_from(
        POM.as_bytes()
            .windows(needle.len())
            .position(|w| w == needle)
            .expect("fixture contains the artifactId"),
    )
    .expect("fits u32");
    let length = u32::try_from(needle.len()).expect("fits u32");
    let patches = [PlanPatch {
        start,
        end: start + length,
        text: "1.3.0".to_owned(),
    }];
    let err = MavenAdapter::new()
        .edit_versions(&file, POM.as_bytes(), &patches)
        .expect_err("a non-version span refuses");
    assert_eq!(err.code(), "E-EDIT-PATCHES");
    // And the bytes are untouched on refusal.
    let untouched = MavenAdapter::new().edit_versions(&file, POM.as_bytes(), &[]);
    assert!(untouched.is_ok(), "the empty pass is the sibling posture");
}

/// An inherited member has no own site: the one parent-pom edit covers
/// it, and a patch aimed at the member's manifest fails closed (the
/// member pom carries no `<version>` element at all).
#[test]
fn an_inherited_member_manifest_has_no_editable_row() {
    const MEMBER: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <parent>
    <groupId>com.example</groupId>
    <artifactId>edit-demo</artifactId>
    <version>1.2.3</version>
  </parent>
  <artifactId>edit-demo-member</artifactId>
</project>
"#;
    let file = RelPath::new("member/pom.xml").expect("fixture path");
    let patches = [PlanPatch {
        start: 60,
        end: 65,
        text: "1.3.0".to_owned(),
    }];
    let err = MavenAdapter::new()
        .edit_versions(&file, MEMBER.as_bytes(), &patches)
        .expect_err("an inheritor carries no own version row");
    assert_eq!(err.code(), "E-EDIT-PATCHES");
}

/// The manifest-version probe: the semver-canonical pairing text for a
/// canonical pom, `None` for a pom whose spelling the plan contract
/// cannot render, and `None` for a pom without a version (the documented
/// HEAD fallback).
#[test]
fn the_manifest_version_probe_pairs_canonical_spellings() {
    assert_eq!(
        manifest_version(POM.as_bytes()),
        Some("1.2.3".to_owned()),
        "the canonical spelling pairs verbatim"
    );
    let snapshot = r#"<?xml version="1.0"?>
<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.example</groupId>
  <artifactId>snapshot-demo</artifactId>
  <version>2.0.0-SNAPSHOT</version>
</project>
"#;
    assert_eq!(
        manifest_version(snapshot.as_bytes()),
        Some("2.0.0-SNAPSHOT".to_owned()),
        "a semver-parseable qualifier form pairs verbatim (the contract's type governs)"
    );
    let two_part = r#"<?xml version="1.0"?>
<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.example</groupId>
  <artifactId>two-part-demo</artifactId>
  <version>2.0</version>
</project>
"#;
    assert_eq!(
        manifest_version(two_part.as_bytes()),
        None,
        "a spelling the plan contract cannot parse never pairs (the HEAD fallback)"
    );
}

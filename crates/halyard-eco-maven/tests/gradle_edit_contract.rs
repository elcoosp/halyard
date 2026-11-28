//! The M8 Maven/Gradle cell's **Gradle half** edit contract (ADR-0058
//! continued — the release half): the plan's own-version byte patches
//! re-apply **semantically and in place** (invariant I4) — each patch
//! span must land on the LAST `version = …` literal assignment's
//! inner-byte span (the site the discovery row emitted — Gradle's
//! Project API: *"The version of this project. Gradle always uses the
//! `toString()` value of the version."*; the last assignment in
//! document order wins, the Project setter's ordinary overwrite), the
//! literal's inner bytes swap and every other byte survives verbatim —
//! the quotes, the `plugins`/`repositories` blocks, the comments, the
//! sibling string literals — and a re-scan read-back verifies the file
//! now carries the target before the patch is accepted.
//!
//! This is the plan's acceptance criterion pinned as code: the M8 row's
//! *"each: discover/edit/publish/resume tests"* — the **edit** half for
//! the Gradle build file.
//!
//! A patch aimed at any other span refuses (`E-EDIT-PATCHES`); a patch
//! whose text already equals the current value is the idempotent re-run
//! (I1): matched, skipped, bytes verbatim. A build file with no
//! canonical literal (absent, non-literal, or non-semver — the
//! fail-closed discipline) has no slot: a patch aimed at its manifest
//! refuses.

use halyard_core::EcosystemAdapter;
use halyard_core::package::RelPath;
use halyard_core::plan::PlanPatch;
use halyard_eco_maven::MavenAdapter;

/// The Groovy build-file fixture: the literal `version = '1.4.2'`
/// assignment inside the realistic instruction mix — the `plugins`
/// block, the line comment, the `repositories` block and the sibling
/// string literal are the bytes the swap must preserve.
const GROOVY_BUILD: &str = concat!(
    "plugins {\n",
    "    id 'java'\n",
    "}\n",
    "\n",
    "// the project version (a literal assignment — the documented form)\n",
    "version = '1.4.2'\n",
    "\n",
    "repositories {\n",
    "    mavenCentral()\n",
    "}\n",
    "\n",
    "description = \"the version = '9.9.9' string is skipped in place\"\n",
);

/// The fixture offset as a plan-contract span coordinate (`None` cannot
/// happen for a constant fixture, but the failure must stay loud and
/// panic-free — the sibling bench posture).
fn usize_to_u32(value: Option<usize>, needle: &str) -> u32 {
    if let Some(value) = value.and_then(|value| u32::try_from(value).ok()) {
        return value;
    }
    eprintln!("the fixture must carry {needle}");
    std::process::exit(101);
}

fn patch(haystack: &str, needle: &str, text: &str) -> PlanPatch {
    let start = usize_to_u32(haystack.find(needle), needle);
    let end = start + usize_to_u32(Some(needle.len()), needle);
    PlanPatch {
        start,
        end,
        text: text.to_owned(),
    }
}

/// The literal value swaps in place and every other byte survives
/// verbatim (I4) — the quotes, the `plugins`/`repositories` blocks, the
/// comment and the sibling string literal.
#[test]
fn swaps_the_literal_value_and_preserves_the_markup() {
    let file = RelPath::new("build.gradle").expect("fixture path");
    let adapter = MavenAdapter::new();
    let out = adapter
        .edit_versions(
            &file,
            GROOVY_BUILD.as_bytes(),
            &[patch(GROOVY_BUILD, "1.4.2", "1.5.0")],
        )
        .expect("the literal patch applies");
    assert_eq!((out.applied, out.skipped), (1, 0));
    let bumped = String::from_utf8(out.content).expect("valid UTF-8");
    assert_eq!(
        bumped,
        GROOVY_BUILD.replacen("1.4.2", "1.5.0", 1),
        "only the literal's inner bytes move"
    );
    assert!(bumped.contains("version = '1.5.0'"));
    assert!(
        bumped.contains("id 'java'")
            && bumped.contains("mavenCentral()")
            && bumped.contains("the version = '9.9.9' string is skipped in place")
            && bumped.contains("// the project version"),
        "the surrounding build file never moves: {bumped}"
    );
}

/// A Kotlin double-quoted literal swaps in place (the documented Kotlin
/// `version = "…"` form).
#[test]
fn swaps_a_kotlin_double_quoted_literal_in_place() {
    let build = "plugins { java() }\nversion = \"2.0.0\"\n";
    let file = RelPath::new("build.gradle.kts").expect("fixture path");
    let adapter = MavenAdapter::new();
    let out = adapter
        .edit_versions(&file, build.as_bytes(), &[patch(build, "2.0.0", "2.1.0")])
        .expect("the kotlin literal patch applies");
    assert_eq!((out.applied, out.skipped), (1, 0));
    let bumped = String::from_utf8(out.content).expect("valid UTF-8");
    assert!(bumped.contains("version = \"2.1.0\""));
    assert!(bumped.contains("plugins { java() }"));
}

/// A patch aimed at any span off the last literal's inner bytes refuses
/// (`E-EDIT-PATCHES`) — the sibling string's `'9.9.9'` is not the slot.
#[test]
fn a_patch_off_the_literal_span_refuses() {
    let file = RelPath::new("build.gradle").expect("fixture path");
    let adapter = MavenAdapter::new();
    let err = adapter
        .edit_versions(
            &file,
            GROOVY_BUILD.as_bytes(),
            &[patch(GROOVY_BUILD, "9.9.9", "9.9.8")],
        )
        .expect_err("a span off the slot refuses");
    assert_eq!(err.code(), "E-EDIT-PATCHES");
}

/// The last literal assignment in document order is the slot — a patch
/// aimed at an earlier literal's value refuses (the Project setter's
/// last-assignment-wins rule makes the last literal the site).
#[test]
fn the_last_literal_assignment_is_the_only_slot() {
    let build = "version = '1.0.0'\nversion = '2.0.0'\n";
    let file = RelPath::new("build.gradle").expect("fixture path");
    let adapter = MavenAdapter::new();
    let err = adapter
        .edit_versions(&file, build.as_bytes(), &[patch(build, "1.0.0", "1.1.0")])
        .expect_err("the earlier literal is not the slot");
    assert_eq!(err.code(), "E-EDIT-PATCHES");
    // The last literal IS the slot — a patch aimed at it applies.
    let out = adapter
        .edit_versions(&file, build.as_bytes(), &[patch(build, "2.0.0", "2.1.0")])
        .expect("the last literal is the slot");
    assert_eq!((out.applied, out.skipped), (1, 0));
}

/// A build file with a non-literal version (a property reference) has no
/// slot — a patch aimed at its manifest refuses (`E-EDIT-PATCHES`).
#[test]
fn a_non_literal_version_has_no_slot() {
    let build = "version = findProperty('appVersion')\n";
    let file = RelPath::new("build.gradle").expect("fixture path");
    let adapter = MavenAdapter::new();
    let err = adapter
        .edit_versions(
            &file,
            build.as_bytes(),
            &[patch(build, "appVersion", "1.0.0")],
        )
        .expect_err("a non-literal row has no slot");
    assert_eq!(err.code(), "E-EDIT-PATCHES");
}

/// A build file with no `version = …` assignment has no slot — a patch
/// aimed at its manifest refuses (`E-EDIT-PATCHES`).
#[test]
fn a_build_file_without_a_version_has_no_slot() {
    let build = "plugins { id 'java' }\n";
    let file = RelPath::new("build.gradle").expect("fixture path");
    let adapter = MavenAdapter::new();
    let err = adapter
        .edit_versions(
            &file,
            build.as_bytes(),
            &[PlanPatch {
                start: 0,
                end: 1,
                text: "1.0.0".to_owned(),
            }],
        )
        .expect_err("a no-version build file has no slot");
    assert_eq!(err.code(), "E-EDIT-PATCHES");
}

/// A patch whose text already equals the current value is the idempotent
/// re-run (I1): matched, counted as skipped, bytes verbatim.
#[test]
fn the_idempotent_rerun_skips_the_matched_value() {
    let file = RelPath::new("build.gradle").expect("fixture path");
    let adapter = MavenAdapter::new();
    let out = adapter
        .edit_versions(
            &file,
            GROOVY_BUILD.as_bytes(),
            &[patch(GROOVY_BUILD, "1.4.2", "1.4.2")],
        )
        .expect("the idempotent pass");
    assert_eq!((out.applied, out.skipped), (0, 1));
    assert_eq!(out.content, GROOVY_BUILD.as_bytes());
}

/// An empty patch set returns the verbatim bytes (nothing to apply,
/// nothing changed).
#[test]
fn empty_passes_return_the_verbatim_bytes() {
    let file = RelPath::new("build.gradle").expect("fixture path");
    let adapter = MavenAdapter::new();
    let out = adapter
        .edit_versions(&file, GROOVY_BUILD.as_bytes(), &[])
        .expect("an empty pass");
    assert_eq!(out.content, GROOVY_BUILD.as_bytes());
    assert_eq!((out.applied, out.skipped), (0, 0));
}

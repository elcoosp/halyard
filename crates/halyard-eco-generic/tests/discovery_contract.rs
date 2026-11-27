//! The M8 `generic` cell's discovery contract (`docs/HALYARD_ARCHITECTURE.md`
//! §17 YEAR 2, M8 row — *"More ecosystems"*, the `generic` cell — *"`generic`
//! (regex-edited version files)"* — the next listed cell after the shipped
//! `RubyGems` cell (ADR-0061), gate *"each: discover/edit/publish/resume
//! tests + replay on 2 real repos"*; VERIFIED grammar in
//! `docs/research/generic-version-files.md`).
//!
//! This is the plan's acceptance criterion pinned as code: a config-declared
//! version file discovers as a generic row — the identity is the declared
//! `name`, the version is the pattern's named `version` capture group
//! (Knope's own requirement: *"The regex pattern must include a named
//! capture group called version … This capture group identifies the version
//! number to replace."*), the capture's byte span is the edit site, and the
//! ADR-0056 discipline governs the captured text: only semver-parseable
//! spellings are canonical — every other capture is warned, unpublishable,
//! no edit site, never rewritten.

use halyard_core::EcosystemAdapter;
use halyard_core::discover::{DiscoverCtx, DiscoverError, GenericFileSpec};
use halyard_eco_generic::GenericAdapter;

/// Builds a context from `(path, body)` fixture files.
fn ctx_with(files: &[(&str, &str)]) -> DiscoverCtx {
    let mut ctx = DiscoverCtx::new();
    for (path, body) in files {
        assert!(ctx.insert_file(path, body.as_bytes().to_vec()), "{path}");
    }
    ctx
}

/// Declares one spec: package `name`, version `file`, locator `pattern`
/// (exactly one named `version` group — the loader's contract).
fn spec(name: &str, file: &str, pattern: &str) -> GenericFileSpec {
    GenericFileSpec {
        name: name.to_owned(),
        file: file.to_owned(),
        pattern: pattern.to_owned(),
    }
}

/// The plain `VERSION` file — the escape hatch's canonical fixture.
const PLAIN_VERSION: &str = "1.2.3\n";

/// A version file whose row sits inside surrounding markup — the capture
/// span must cover the version text only.
const MARKED_UP: &str = "pub const VERSION: &str = \"1.4.2\"; // the release row\n";

/// The declared `VERSION` file discovers as a generic row: the declared
/// `name` is the identity, the captured text is the version, the row is
/// unpublishable (no registry decision exists — the escape hatch carries
/// no registry name to record), and the capture span is the edit site.
#[test]
fn a_declared_version_file_discovers_as_a_generic_row() {
    let ctx = ctx_with(&[("VERSION", PLAIN_VERSION)]);
    let specs = [spec(
        "squid-utils",
        "VERSION",
        "(?<version>\\d+\\.\\d+\\.\\d+)",
    )];
    let rows = GenericAdapter::new(&specs)
        .discover(&ctx)
        .expect("discovers");
    assert_eq!(rows.len(), 1, "one declared package");
    let row = rows.first().expect("the single row");
    assert_eq!(row.name, "squid-utils");
    assert_eq!(row.ecosystem, halyard_core::package::Ecosystem::Generic);
    assert_eq!(row.manifest, "VERSION");
    assert_eq!(row.root, "", "a root-level file claims the repo root");
    assert_eq!(
        row.version.as_ref().map(std::string::ToString::to_string),
        Some("1.2.3".to_owned()),
        "the captured text is the version"
    );
    assert_eq!(
        row.version_source,
        halyard_core::discover::RawVersionSource::Local,
        "the version lives in the declared file"
    );
    assert!(
        !row.publishable,
        "no registry decision exists for the generic ecosystem"
    );
    let site = row
        .version_site
        .as_ref()
        .expect("the capture span is the edit site");
    assert_eq!(site.file.as_str(), "VERSION");
    assert_eq!(
        site.spans.first().map(|s| (s.start, s.end)),
        Some((0, 5)),
        "the capture span covers the version text only"
    );
    assert!(
        row.warnings.is_empty(),
        "a canonical capture carries no warning"
    );
}

/// The capture span covers the version text only — markup around the
/// match (the `const` declaration, the quotes, the trailing comment)
/// never joins the edit site (invariant I4).
#[test]
fn the_capture_span_is_the_version_text_only() {
    let ctx = ctx_with(&[("src/version.rs", MARKED_UP)]);
    let specs = [spec(
        "app",
        "src/version.rs",
        "VERSION: &str = \"(?<version>\\d+\\.\\d+\\.\\d+)\";",
    )];
    let rows = GenericAdapter::new(&specs)
        .discover(&ctx)
        .expect("discovers");
    let row = rows.first().expect("the single row");
    let site = row.version_site.as_ref().expect("the capture span");
    let start = site.spans.first().map(|s| s.start).expect("one span");
    let end = site.spans.first().map(|s| s.end).expect("one span");
    let bytes = ctx.file("src/version.rs").expect("the file bytes");
    assert_eq!(
        bytes.get(start as usize..end as usize),
        Some("1.4.2".as_bytes()),
        "the span slices exactly the capture"
    );
}

/// A capture the plan contract's semver cannot render (`1.2` — the
/// ADR-0056 fail-closed discipline, the posture every M8 cell ships) is
/// recognized-but-non-canonical: warned, unpublishable, no edit site,
/// never rewritten.
#[test]
fn a_non_semver_capture_is_warned_and_unpublishable() {
    let ctx = ctx_with(&[("VERSION", "1.2\n")]);
    let specs = [spec("app", "VERSION", "(?<version>\\d+(?:\\.\\d+)*)")];
    let rows = GenericAdapter::new(&specs)
        .discover(&ctx)
        .expect("discovers");
    let row = rows.first().expect("the single row");
    assert!(
        row.version.is_none(),
        "a non-canonical capture plans no version"
    );
    assert!(
        row.version_site.is_none(),
        "no edit site: a non-canonical spelling is never rewritten"
    );
    assert!(
        !row.publishable,
        "the row is unpublishable like every warned posture"
    );
    let warning = row.warnings.first().expect("the non-canonical warning");
    assert_eq!(warning.code, "non-canonical-version");
    assert_eq!(warning.subject, "app");
}

/// A file that matches nothing has no discoverable version — warned
/// (`no-version`), unpublishable, no edit site (the dart/rubygems
/// no-version posture).
#[test]
fn a_non_matching_file_warns_no_version() {
    let ctx = ctx_with(&[("VERSION", "not a version file at all\n")]);
    let specs = [spec("app", "VERSION", "(?<version>\\d+\\.\\d+\\.\\d+)")];
    let rows = GenericAdapter::new(&specs)
        .discover(&ctx)
        .expect("discovers");
    let row = rows.first().expect("the single row");
    assert!(row.version.is_none());
    assert!(row.version_site.is_none());
    let warning = row.warnings.first().expect("the no-version warning");
    assert_eq!(warning.code, "no-version");
}

/// The leftmost match is the version row; a second match is consumed
/// unmodeled with a warning (A2 — the multiple-match grammar is the
/// recorded-open superset).
#[test]
fn the_leftmost_match_is_the_row_and_a_second_match_is_unmodeled() {
    let ctx = ctx_with(&[("docs/versions.md", "current: 1.0.0\nplanned: 2.0.0\n")]);
    let specs = [spec(
        "app",
        "docs/versions.md",
        "(?<version>\\d+\\.\\d+\\.\\d+)",
    )];
    let rows = GenericAdapter::new(&specs)
        .discover(&ctx)
        .expect("discovers");
    let row = rows.first().expect("the single row");
    assert_eq!(
        row.version.as_ref().map(std::string::ToString::to_string),
        Some("1.0.0".to_owned()),
        "the leftmost match is the version row"
    );
    let warning = row
        .warnings
        .iter()
        .find(|w| w.code == "unmodeled-match")
        .expect("the second match is consumed unmodeled");
    assert_eq!(warning.subject, "app");
}

/// A declared file missing from the committed tree refuses
/// (`E-DISC-GLOB` — the same member-missing-from-tree posture the dart
/// adapter pinned for the pub workspace).
#[test]
fn a_missing_declared_file_refuses() {
    let ctx = ctx_with(&[("OTHER", PLAIN_VERSION)]);
    let specs = [spec("app", "VERSION", "(?<version>\\d+\\.\\d+\\.\\d+)")];
    let error = GenericAdapter::new(&specs)
        .discover(&ctx)
        .expect_err("the missing file refuses");
    assert_eq!(error.code(), "E-DISC-GLOB");
}

/// A declared file that is not UTF-8 refuses fail-closed (`E-DISC-MANIFEST`,
/// the rubygems posture — the `regex` crate matches Unicode haystacks, so
/// the scanner parses text, never raw bytes).
#[test]
fn a_non_utf8_file_refuses() {
    let mut ctx = DiscoverCtx::new();
    assert!(ctx.insert_file("VERSION", vec![0xff, 0xfe, b'1']));
    let specs = [spec("app", "VERSION", "(?<version>\\d+)")];
    let error = GenericAdapter::new(&specs)
        .discover(&ctx)
        .expect_err("the non-UTF-8 file refuses");
    assert_eq!(error.code(), "E-DISC-MANIFEST");
}

/// Multiple declared packages discover in declaration order (I3 — the
/// specs arrive sorted from the config layer, the adapter preserves the
/// order; every row carries the generic ecosystem).
#[test]
fn declared_packages_discover_in_order() {
    let ctx = ctx_with(&[("a/VERSION", "0.1.0\n"), ("b/VERSION", "0.2.0\n")]);
    let specs = [
        spec("a", "a/VERSION", "(?<version>\\d+\\.\\d+\\.\\d+)"),
        spec("b", "b/VERSION", "(?<version>\\d+\\.\\d+\\.\\d+)"),
    ];
    let rows = GenericAdapter::new(&specs)
        .discover(&ctx)
        .expect("discovers");
    assert_eq!(rows.len(), 2);
    assert_eq!(rows.first().map(|r| r.name.as_str()), Some("a"));
    assert_eq!(rows.last().map(|r| r.name.as_str()), Some("b"));
    assert_eq!(
        rows.first().and_then(|r| r.root.is_empty().then_some(())),
        None,
        "a nested file claims its directory"
    );
    assert_eq!(
        rows.last().map(|r| r.root.as_str()),
        Some("b"),
        "the file's directory is the package root"
    );
}

/// A spec whose pattern fails to compile (unreachable through the
/// validated loader, reachable through a hand-built spec) refuses
/// fail-closed at discovery with the manifest error naming the
/// plain-text format — never a guess.
#[test]
fn a_non_compiling_pattern_refuses_fail_closed() {
    let ctx = ctx_with(&[("VERSION", PLAIN_VERSION)]);
    let specs = [spec("app", "VERSION", "(?<version>[unclosed")];
    let error = GenericAdapter::new(&specs)
        .discover(&ctx)
        .expect_err("the broken pattern refuses");
    match &error {
        DiscoverError::Manifest { format, .. } => {
            assert_eq!(
                *format,
                halyard_core::discover::ManifestFormat::Text,
                "the declared file is plain text"
            );
        }
        other => panic!("expected a manifest error, got {other:?}"),
    }
}

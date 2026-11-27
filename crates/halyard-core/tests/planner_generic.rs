//! The `generic` planning contract (M8, the §17 **`generic`** cell —
//! *"`generic` (regex-edited version files)"*, ADR-0062): a config-declared
//! package with a canonical capture plans a normal release row whose
//! publish decision is **absent** — `publish: None`, the ADR-0057 Go shape
//! minus tag-only-ness. The plan names no registry for the generic
//! ecosystem, and inventing a registry name is precisely what the escape
//! hatch must not do: the tag is the artifact, the user's own pipeline owns
//! publishing, and the publish engine's documented `no_publish_contract`
//! skip keeps the row out of every forecast.
//!
//! The fixtures mirror `planner_rubygems.rs`: synthetic snapshots planned
//! through the pure `plan()` — this file pins the planner half of the
//! cell; the adapter grammar lives in `halyard-eco-generic`.

use halyard_core::history::{AuthorRef, FixedHistory, HistoryDigest, ParsedCommit, RelevantCommit};
use halyard_core::package::{
    Ecosystem, GroupId, ManifestSite, Package, PathFilter, RelPath, VersionSource, Workspace,
};
use halyard_core::planner::{PlanConfig, Snapshot, plan};
use halyard_core::{Oid, PkgId, ReleaseRefs};
use semver::Version;

/// `aaaaaaaa…0N`-style hex oid for tests (deterministic per suffix).
/// `None` only if the constructed bytes were not hex, which the fixed
/// 40-byte layout makes impossible.
fn oid(suffix: u8) -> Option<Oid> {
    let mut hex = [b'a'; 40];
    hex[39] = b'0' + (suffix % 10);
    Oid::from_hex(&hex)
}

/// One conventional commit row (attribution rides the digest's
/// `per_pkg`, like the golden fixtures).
fn commit(suffix: u8, kind: &str) -> Option<RelevantCommit> {
    Some(RelevantCommit {
        oid: oid(suffix)?,
        subject: format!("{kind}: a change").into(),
        body_range: 0..0,
        parsed: ParsedCommit {
            kind: kind.into(),
            scope: None,
            breaking: false,
            description: "a change".into(),
        },
        pr: None,
        author: AuthorRef {
            name: "Tester".into(),
            email: "tester@example.com".into(),
        },
        trailers: Vec::new().into(),
        touched: halyard_core::SmallBitset::new(),
    })
}

/// Registers one manifest-versioned generic package (the shape the
/// generic adapter emits for a canonical capture: local source, an edit
/// site on the capture span, `publishable = false` — ADR-0062's
/// no-registry-decision posture).
fn push_generic_pkg(ws: &mut Workspace, name: &str, version: &str) -> Option<PkgId> {
    let pkg = Package {
        id: PkgId::FIRST,
        name: ws.intern(name),
        ecosystem: Ecosystem::Generic,
        root: RelPath::new("")?,
        manifest: RelPath::new("VERSION")?,
        version: Some(Version::parse(version).ok()?),
        version_source: VersionSource::Local,
        publishable: false,
        deps: Vec::new(),
        // The group default (`v{version}` for a single package) — the
        // generic adapter invents no ecosystem-specific tag shape (A4).
        tag_template: halyard_core::package::TagTemplate::independent_default(),
        changelog: None,
        version_site: Some(ManifestSite {
            file: RelPath::new("VERSION")?,
            spans: vec![halyard_core::package::Span { start: 0, end: 5 }],
        }),
        include: PathFilter::new(Vec::new(), Vec::new()),
        group: GroupId::new(0),
        facets: halyard_core::Facets::default(),
    };
    Some(ws.push_package(pkg))
}

/// Registers one manifest-versioned Cargo package (the control row: the
/// generic arm never disturbs the other ecosystems' publish shapes).
fn push_cargo_pkg(ws: &mut Workspace, name: &str, version: &str) -> Option<PkgId> {
    let pkg = Package {
        id: PkgId::FIRST,
        name: ws.intern(name),
        ecosystem: Ecosystem::Cargo,
        root: RelPath::new("crates/app")?,
        manifest: RelPath::new("crates/app/Cargo.toml")?,
        version: Some(Version::parse(version).ok()?),
        version_source: VersionSource::Local,
        publishable: true,
        deps: Vec::new(),
        tag_template: halyard_core::package::TagTemplate::parse("app-v{version}").ok()?,
        changelog: None,
        version_site: Some(ManifestSite {
            file: RelPath::new("crates/app/Cargo.toml")?,
            spans: vec![halyard_core::package::Span { start: 40, end: 45 }],
        }),
        include: PathFilter::new(Vec::new(), Vec::new()),
        group: GroupId::new(0),
        facets: halyard_core::Facets::default(),
    };
    Some(ws.push_package(pkg))
}

/// Assembles a snapshot from a workspace, commits and bases.
fn snapshot<'a>(
    ws: &'a Workspace,
    commits: Vec<RelevantCommit>,
    per_pkg: Vec<Vec<usize>>,
    bases: Vec<Option<halyard_core::history::BaseRef>>,
) -> Result<Snapshot<'a>, Box<dyn std::error::Error>> {
    let digest = HistoryDigest {
        commits,
        per_pkg,
        bases: bases.clone(),
    };
    let refs = ReleaseRefs::from_bases(bases);
    let head = oid(9).ok_or("fixture oid")?;
    let history = FixedHistory::new(Some(head), digest);
    // The snapshot borrows; leak the boxed provider for the test lifetime
    // (each test builds one snapshot).
    let leaked: &'a FixedHistory = Box::leak(Box::new(history));
    let refs_leaked: &'a ReleaseRefs = Box::leak(Box::new(refs));
    let changes_leaked: &'a halyard_core::change_file::ChangeFiles =
        Box::leak(Box::new(halyard_core::change_file::ChangeFiles::new()));
    Ok(Snapshot {
        workspace: ws,
        history: leaked,
        refs: refs_leaked,
        change_files: changes_leaked,
    })
}

fn cfg() -> PlanConfig {
    PlanConfig {
        base_branch: Some("main".to_owned()),
        // The publish gates: default-off in the config (the e2e fixtures
        // set `[publish] enabled = true`); the control pins the shape
        // under an enabled run.
        publish_enabled: true,
        ..PlanConfig::default_values()
    }
}

/// A canonical generic package plans a normal release row with **no
/// publish decision at all** (`publish: None` — there is no registry
/// name to record; the publish engine's documented `no_publish_contract`
/// skip keeps the row out of every forecast), and the edit set aims the
/// version patch at the declared file's capture span.
#[test]
fn a_generic_release_row_carries_no_publish_decision() -> Result<(), Box<dyn std::error::Error>> {
    let mut ws = Workspace::new();
    let app = push_generic_pkg(&mut ws, "squid-utils", "1.0.0").expect("fixture package");
    let commits = vec![commit(1, "feat").expect("fixture commit")];
    let snapshot = snapshot(&ws, commits, vec![vec![0]], vec![None])?;
    let plan = plan(&snapshot, &cfg()).expect("the generic row plans");
    assert_eq!(plan.releases.len(), 1, "the generic package releases");
    let row = &plan.releases.first().expect("a release row");
    assert_eq!(row.ecosystem, "generic");
    assert_eq!(row.from, "1.0.0");
    assert_eq!(row.to, "1.1.0", "a feat bump is minor");
    assert!(
        row.publish.is_none(),
        "no publish decision exists: no registry name to record (ADR-0062 D3)"
    );
    // The edit set: the version patch aims at the declared file's capture
    // span and carries the target text.
    let edit = plan
        .edits
        .iter()
        .find(|edit| edit.file.as_str() == "VERSION")
        .expect("the version-file edit is planned");
    assert_eq!(edit.patches.len(), 1, "one value patch");
    let patch = edit.patches.first().expect("one value patch");
    assert_eq!(patch.start, 0);
    assert_eq!(patch.end, 5);
    assert_eq!(patch.text, "1.1.0");
    let _ = app;
    Ok(())
}

/// The control: a publishable cargo package keeps its enabled shape — the
/// generic arm's no-decision posture changes nothing else (the
/// manifest-versioned ecosystems' shapes are pinned).
#[test]
fn the_cargo_control_keeps_its_enabled_shape() -> Result<(), Box<dyn std::error::Error>> {
    let mut ws = Workspace::new();
    push_cargo_pkg(&mut ws, "app", "1.0.0").expect("fixture control");
    let commits = vec![commit(1, "feat").expect("fixture commit")];
    let snapshot = snapshot(&ws, commits, vec![vec![0]], vec![None])?;
    let plan = plan(&snapshot, &cfg()).expect("the control plans");
    let row = &plan.releases.first().expect("a release row");
    let publish = row.publish.as_ref().expect("the cargo publish decision");
    assert!(
        publish.enabled,
        "the cargo shape stays enabled under the same config"
    );
    Ok(())
}

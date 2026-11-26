//! The Maven planning contract (M8, the §17 **Maven/Gradle** cell —
//! ADR-0058): a maven package with a canonical `<version>` plans a
//! normal release row whose publish decision carries the engine's
//! **disabled shape** — `registry: "maven-central"` (the ecosystem's
//! canonical registry, truthful the day the argv arm lands) with
//! `enabled: false` (the adapter discovers `publishable = false` until a
//! `mvn` argv grammar exists), and the plan's edit set aims the version
//! patch at the pom's own-version site.
//!
//! The fixtures mirror `planner_tag_only.rs`: synthetic snapshots
//! planned through the pure `plan()` — this file pins the planner half
//! of the cell; the adapter grammar lives in `halyard-eco-maven`.

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

/// Registers one manifest-versioned maven package (the shape the maven
/// adapter emits for a canonical `<version>`: local source, an edit site
/// on the value span, `publishable = false` — ADR-0058 D5).
fn push_maven_pkg(ws: &mut Workspace, name: &str, version: &str) -> Option<PkgId> {
    let pkg = Package {
        id: PkgId::FIRST,
        name: ws.intern(name),
        ecosystem: Ecosystem::Maven,
        root: RelPath::new("api")?,
        manifest: RelPath::new("api/pom.xml")?,
        version: Some(Version::parse(version).ok()?),
        version_source: VersionSource::Local,
        publishable: false,
        deps: Vec::new(),
        // The group default (`{name}-v{version}`) — the maven adapter
        // invents no maven-specific tag shape.
        tag_template: halyard_core::package::TagTemplate::independent_default(),
        changelog: None,
        version_site: Some(ManifestSite {
            file: RelPath::new("api/pom.xml")?,
            spans: vec![halyard_core::package::Span { start: 40, end: 45 }],
        }),
        include: PathFilter::new(Vec::new(), Vec::new()),
        group: GroupId::new(0),
        facets: halyard_core::Facets::default(),
    };
    Some(ws.push_package(pkg))
}

/// Registers one manifest-versioned Cargo package (the control row: the
/// maven arm never disturbs the other ecosystems' publish shapes).
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
) -> Snapshot<'a> {
    let digest = HistoryDigest {
        commits,
        per_pkg,
        bases: bases.clone(),
    };
    let refs = ReleaseRefs::from_bases(bases);
    let head = oid(9).expect("fixture oid");
    let history = FixedHistory::new(Some(head), digest);
    // The snapshot borrows; leak the boxed provider for the test lifetime
    // (each test builds one snapshot).
    let leaked: &'a FixedHistory = Box::leak(Box::new(history));
    let refs_leaked: &'a ReleaseRefs = Box::leak(Box::new(refs));
    let changes_leaked: &'a halyard_core::change_file::ChangeFiles =
        Box::leak(Box::new(halyard_core::change_file::ChangeFiles::new()));
    Snapshot {
        workspace: ws,
        history: leaked,
        refs: refs_leaked,
        change_files: changes_leaked,
    }
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

/// A canonical maven package plans a normal release row with the
/// engine's disabled publish shape: `registry` names the canonical
/// maven-central, `enabled` is false (the adapter's `publishable = false`
/// posture — no `mvn` argv grammar exists, ADR-0058 D5), and the edit set
/// aims the version patch at the pom's own-version site.
#[test]
fn a_maven_row_plans_with_the_disabled_publish_shape() {
    let mut ws = Workspace::new();
    let api = push_maven_pkg(&mut ws, "halyard-mvn-api", "1.0.0").expect("fixture package");
    let commits = vec![commit(1, "feat").expect("fixture commit")];
    let snapshot = snapshot(&ws, commits, vec![vec![0]], vec![None]);
    let plan = plan(&snapshot, &cfg()).expect("the maven row plans");
    assert_eq!(plan.releases.len(), 1, "the maven package releases");
    let row = &plan.releases[0];
    assert_eq!(row.ecosystem, "maven");
    assert_eq!(row.from, "1.0.0");
    assert_eq!(row.to, "1.1.0", "a feat bump is minor");
    let publish = row.publish.as_ref().expect("a publish decision exists");
    assert_eq!(publish.registry, "maven-central", "the canonical registry");
    assert!(
        !publish.enabled,
        "the disabled shape: no maven publish grammar exists yet"
    );
    assert!(publish.access.is_none() && publish.provenance.is_none());
    // The edit set: the version patch aims at the pom's own site and
    // carries the target text.
    let edit = plan
        .edits
        .iter()
        .find(|edit| edit.file.as_str() == "api/pom.xml")
        .expect("the pom edit is planned");
    assert_eq!(edit.patches.len(), 1, "one value patch");
    assert_eq!(edit.patches[0].start, 40);
    assert_eq!(edit.patches[0].end, 45);
    assert_eq!(edit.patches[0].text, "1.1.0");
    let _ = api;
}

/// The control: a publishable cargo package keeps its enabled shape — the
/// maven arm's disabled posture changes nothing else (the
/// manifest-versioned ecosystems' shapes are pinned).
#[test]
fn the_cargo_control_keeps_its_enabled_shape() {
    let mut ws = Workspace::new();
    push_cargo_pkg(&mut ws, "app", "1.0.0").expect("fixture control");
    let commits = vec![commit(1, "feat").expect("fixture commit")];
    let snapshot = snapshot(&ws, commits, vec![vec![0]], vec![None]);
    let plan = plan(&snapshot, &cfg()).expect("the control plans");
    let row = plan
        .releases
        .iter()
        .find(|row| row.package == "app")
        .expect("the control releases");
    let publish = row.publish.as_ref().expect("a publish decision exists");
    assert_eq!(publish.registry, "crates.io");
    assert!(publish.enabled, "the publishable control stays enabled");
}

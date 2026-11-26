//! The tag-only planning contract (M8, the §17 M8 **Go modules** cell —
//! *"Go modules (tag-only, `go.work`, subdir tags)"*, ADR-0057): a Go
//! package carries no manifest version, so its `from` is the release
//! base tag's version (§8.2 step 3, parsed through the package's own
//! subdir template), an initial release starts from the zero version,
//! and a `/vN` module path refuses a target below its declared major.
//!
//! The fixtures mirror `planner.rs`'s golden tests: synthetic snapshots
//! planned through the pure `plan()` — this file pins the planner half
//! of the cell; the adapter grammar lives in `halyard-eco-gomod`.

use halyard_core::history::{
    AuthorRef, BaseRef, FixedHistory, HistoryDigest, ParsedCommit, RelevantCommit,
};
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

/// Registers one tag-only Go package (the shape the go adapter emits:
/// no manifest version, a subdir tag template, no version site).
fn push_go_pkg(ws: &mut Workspace, name: &str, root: &str, manifest: &str) -> Option<PkgId> {
    let pkg = Package {
        id: PkgId::FIRST,
        name: ws.intern(name),
        ecosystem: Ecosystem::Go,
        root: RelPath::new(root)?,
        manifest: RelPath::new(manifest)?,
        version: None,
        version_source: VersionSource::Tag,
        publishable: false,
        deps: Vec::new(),
        tag_template: template_for_root(root),
        changelog: None,
        version_site: None,
        include: PathFilter::new(Vec::new(), Vec::new()),
        group: GroupId::new(0),
        facets: halyard_core::Facets::default(),
    };
    Some(ws.push_package(pkg))
}

/// Registers one manifest-versioned Cargo package (the control row).
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
    bases: Vec<Option<BaseRef>>,
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

/// The subdir tag template the go adapter builds (the shape pinned by
/// the adapter's contract tests; constructed here directly — core's
/// tests carry no adapter dependency, §7.2).
fn template_for_root(root: &str) -> halyard_core::package::TagTemplate {
    let raw = if root.is_empty() {
        "v{version}".to_owned()
    } else {
        format!("{root}/v{{version}}")
    };
    halyard_core::package::TagTemplate::parse(&raw).expect("fixture template")
}

fn cfg() -> PlanConfig {
    PlanConfig {
        base_branch: Some("main".to_owned()),
        ..PlanConfig::default_values()
    }
}

/// A tag-only package plans from its release base tag's version: the
/// manifest has no version row, the base tag `api/v0.1.0` parses to
/// 0.1.0 through the package's own subdir template, and a feat commit
/// bumps it to 0.2.0 with the subdir tag `api/v0.2.0`.
#[test]
fn a_tag_only_package_plans_from_its_release_base_tag() {
    let mut ws = Workspace::new();
    let api =
        push_go_pkg(&mut ws, "example.com/repo/api", "api", "api/go.mod").expect("fixture package");
    // A manifest-versioned control (never bumped): its presence defeats
    // the single-package `v{version}` template override (§6.1) so the
    // package's own subdir template rules.
    push_cargo_pkg(&mut ws, "control", "9.9.9").expect("fixture control");
    let commits = vec![commit(1, "feat").expect("fixture commit")];
    let base = BaseRef {
        tag: "api/v0.1.0".into(),
        oid: oid(0).expect("fixture oid"),
    };
    let snapshot = snapshot(
        &ws,
        commits,
        vec![vec![0], Vec::new()],
        vec![Some(base), None],
    );
    let plan = plan(&snapshot, &cfg()).expect("the tag-only row plans");
    assert_eq!(plan.releases.len(), 1, "the go package releases");
    let row = &plan.releases[0];
    assert_eq!(row.package, "example.com/repo/api");
    assert_eq!(row.from, "0.1.0", "the base tag carries the from version");
    assert_eq!(
        row.to, "0.1.1",
        "the cargo-compatible 0.y.z policy: a feat bumps the patch"
    );
    assert_eq!(row.tag, "api/v0.1.1", "the subdir tag shape");
    assert_eq!(row.ecosystem, "go");
    let _ = api;
    // No manifest edits: a go.mod has no version row to rewrite.
    assert!(
        plan.edits.iter().all(|edit| edit.file != "api/go.mod"),
        "tag-only rows carry no manifest edit"
    );
}

/// An initial tag-only release (no base tag) starts from the zero
/// version: a feat commit plans 0.1.0 at the repo-root tag shape.
#[test]
fn an_initial_tag_only_release_starts_from_the_zero_version() {
    let mut ws = Workspace::new();
    push_go_pkg(&mut ws, "example.com/repo", "", "go.mod").expect("fixture package");
    let commits = vec![commit(1, "feat").expect("fixture commit")];
    let snapshot = snapshot(&ws, commits, vec![vec![0]], vec![None]);
    let plan = plan(&snapshot, &cfg()).expect("the initial row plans");
    assert_eq!(plan.releases.len(), 1);
    let row = &plan.releases[0];
    assert_eq!(row.from, "0.0.0", "zero-version start");
    assert_eq!(
        row.to, "0.0.1",
        "the cargo-compatible 0.0.x policy: a feat bumps the patch"
    );
    assert_eq!(row.tag, "v0.0.1", "the repo-root tag shape");
}

/// The `/vN` major-version suffix rule (go.dev/ref/mod): a module path
/// ending in /v2 that would release below 2.0.0 is refused and reported
/// as a recorded skip, never silently re-scoped.
#[test]
fn a_major_suffix_module_refuses_a_target_below_its_major() {
    let mut ws = Workspace::new();
    push_go_pkg(
        &mut ws,
        "example.com/repo/metrics/v2",
        "libs/metrics",
        "libs/metrics/go.mod",
    )
    .expect("fixture package");
    // A Release-As foot forces 1.5.0 on a /v2 module — below the major.
    let mut forced = commit(1, "chore").expect("fixture commit");
    forced.trailers.push(halyard_core::history::Trailer {
        key: "Release-As".into(),
        value: "1.5.0".into(),
    });
    let snapshot = snapshot(&ws, vec![forced], vec![vec![0]], vec![None]);
    let plan = plan(&snapshot, &cfg()).expect("the plan still builds");
    assert!(
        plan.releases.is_empty(),
        "the below-major target never releases"
    );
    assert_eq!(plan.skipped.len(), 1);
    assert_eq!(plan.skipped[0].reason, "major_version_suffix");
}

/// A suffix-honoring target releases at the declared major: a /v2
/// module forced to 2.1.0 plans, tagging `libs/metrics/v2.1.0`.
#[test]
fn a_major_suffix_module_plans_at_its_declared_major() {
    let mut ws = Workspace::new();
    push_go_pkg(
        &mut ws,
        "example.com/repo/metrics/v2",
        "libs/metrics",
        "libs/metrics/go.mod",
    )
    .expect("fixture package");
    push_cargo_pkg(&mut ws, "control", "9.9.9").expect("fixture control");
    let mut forced = commit(1, "chore").expect("fixture commit");
    forced.trailers.push(halyard_core::history::Trailer {
        key: "Release-As".into(),
        value: "2.1.0".into(),
    });
    let snapshot = snapshot(
        &ws,
        vec![forced],
        vec![vec![0], Vec::new()],
        vec![None, None],
    );
    let plan = plan(&snapshot, &cfg()).expect("the plan builds");
    assert_eq!(plan.releases.len(), 1);
    assert_eq!(plan.releases[0].to, "2.1.0");
    assert_eq!(
        plan.releases[0].tag, "libs/metrics/v2.1.0",
        "the subdir tag carries the major-version version"
    );
}

/// The manifest-versioned control still plans exactly as before (the
/// tag-only path is gated on the ecosystem): a cargo feat bumps from the
/// manifest version, and a version-less cargo package stays skipped with
/// its own reason.
#[test]
fn manifest_versioned_packages_keep_their_planning_shape() {
    let mut ws = Workspace::new();
    push_cargo_pkg(&mut ws, "app", "0.3.0").expect("fixture package");
    push_cargo_pkg(&mut ws, "control", "9.9.9").expect("fixture control");
    let commits = vec![commit(1, "feat").expect("fixture commit")];
    let base = BaseRef {
        tag: "app-v0.3.0".into(),
        oid: oid(0).expect("fixture oid"),
    };
    let snapshot = snapshot(
        &ws,
        commits,
        vec![vec![0], Vec::new()],
        vec![Some(base), None],
    );
    let plan = plan(&snapshot, &cfg()).expect("the cargo row plans");
    assert_eq!(plan.releases.len(), 1);
    assert_eq!(plan.releases[0].from, "0.3.0");
    assert_eq!(
        plan.releases[0].to, "0.3.1",
        "the cargo-compatible 0.y.z policy: a feat bumps the patch"
    );
    assert_eq!(plan.releases[0].tag, "app-v0.3.1");
}

/// The release half of the cell (§17 M8 — *tag-only*): a Go release row
/// carries **no** publish decision at all (`publish: None`) — the
/// repository tag is the artifact, no registry exists, and the publish
/// engine's documented skip (`no_publish_contract`) keeps the row out
/// of every forecast.
#[test]
fn a_tag_only_release_row_carries_no_publish_decision() {
    let mut ws = Workspace::new();
    push_go_pkg(&mut ws, "example.com/repo/api", "api", "api/go.mod").expect("fixture package");
    let commits = vec![commit(1, "feat").expect("fixture commit")];
    let base = BaseRef {
        tag: "api/v0.1.0".into(),
        oid: oid(0).expect("fixture oid"),
    };
    let snapshot = snapshot(&ws, commits, vec![vec![0]], vec![Some(base)]);
    let plan = plan(&snapshot, &cfg()).expect("the tag-only row plans");
    assert_eq!(plan.releases.len(), 1);
    assert!(
        plan.releases[0].publish.is_none(),
        "a tag-only row carries no registry publish decision, got {:?}",
        plan.releases[0].publish
    );
}

/// The manifest-versioned control keeps its publish decision: a cargo
/// row still carries `Some(PlanPublish)` (the tag-only shape is gated on
/// the ecosystem, never global).
#[test]
fn a_manifest_versioned_row_keeps_its_publish_decision() {
    let mut ws = Workspace::new();
    push_cargo_pkg(&mut ws, "app", "0.3.0").expect("fixture package");
    let commits = vec![commit(1, "feat").expect("fixture commit")];
    let base = BaseRef {
        tag: "app-v0.3.0".into(),
        oid: oid(0).expect("fixture oid"),
    };
    let snapshot = snapshot(&ws, commits, vec![vec![0]], vec![Some(base)]);
    let plan = plan(&snapshot, &cfg()).expect("the cargo row plans");
    assert_eq!(plan.releases.len(), 1);
    let publish = plan.releases[0]
        .publish
        .as_ref()
        .expect("a cargo row carries its publish decision");
    assert_eq!(
        publish.registry, "crates.io",
        "the manifest-versioned row keeps its registry decision"
    );
}

/// A tag-only package with no commits and no base stays out of the plan
/// with the `no_changes` reason — never `no_version` (the release tag
/// legitimately carries the version).
#[test]
fn a_untouched_tag_only_package_skips_as_no_changes() {
    let mut ws = Workspace::new();
    push_go_pkg(&mut ws, "example.com/repo", "", "go.mod").expect("fixture package");
    let snapshot = snapshot(&ws, Vec::new(), vec![Vec::new()], vec![None]);
    let plan = plan(&snapshot, &cfg()).expect("the plan builds");
    assert!(plan.releases.is_empty());
    assert_eq!(plan.skipped.len(), 1);
    assert_eq!(
        plan.skipped[0].reason, "no_changes",
        "a tag-only package's missing manifest version is not a skip reason"
    );
}

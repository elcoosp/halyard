//! Golden plans (M1.9): synthetic snapshots planned through the pure
//! `plan()` and pinned as insta snapshots. The snapshots double as the
//! `halyard-plan/v1` wire-format fixtures and pin the `plan_hash`
//! stability (I3/I8).

use halyard_core::history::{
    AuthorRef, BaseRef, FixedHistory, HistoryDigest, ParsedCommit, RelevantCommit, Trailer,
};
use halyard_core::package::{
    DepEdge, DepKind, Ecosystem, GroupId, ManifestSite, Package, PathFilter, RelPath, ReqSpec,
    Span, TagTemplate, VersionSource, Workspace,
};
use halyard_core::planner::{PlanConfig, Snapshot, plan};
use halyard_core::{Oid, PkgId, ReleaseRefs};
use semver::{Version, VersionReq};

/// `aaaaaaaa…01`-style hex oid for tests (deterministic per suffix).
/// `None` only if the constructed bytes were not hex, which the fixed
/// 40-byte layout makes impossible.
fn oid(suffix: u8) -> Option<Oid> {
    let mut hex = [b'a'; 40];
    hex[39] = b'0' + (suffix % 10);
    Oid::from_hex(&hex)
}

/// One conventional commit row.
fn commit(
    suffix: u8,
    kind: &str,
    breaking: bool,
    description: &str,
    pr: Option<u64>,
) -> Option<RelevantCommit> {
    Some(RelevantCommit {
        oid: oid(suffix)?,
        subject: format!("{kind}: {description}").into(),
        body_range: 0..0,
        parsed: ParsedCommit {
            kind: kind.into(),
            scope: None,
            breaking,
            description: description.into(),
        },
        pr,
        author: AuthorRef {
            name: "Tester".into(),
            email: "tester@example.com".into(),
        },
        trailers: Vec::new().into(),
        touched: halyard_core::SmallBitset::new(),
    })
}

/// A `Release-As: <version>` commit.
fn release_as_commit(suffix: u8, version: &str) -> Option<RelevantCommit> {
    let mut commit = commit(suffix, "chore", false, "release force", None)?;
    commit.trailers.push(Trailer {
        key: "Release-As".into(),
        value: version.into(),
    });
    Some(commit)
}

/// Registers one package with sane defaults.
fn push_pkg(
    ws: &mut Workspace,
    name: &str,
    version: Option<&str>,
    template: &str,
    manifest: &str,
) -> Option<PkgId> {
    let parsed_version = match version {
        Some(v) => Some(Version::parse(v).ok()?),
        None => None,
    };
    let pkg = Package {
        id: PkgId::FIRST,
        name: ws.intern(name),
        ecosystem: Ecosystem::Cargo,
        root: RelPath::new("crates/core")?,
        manifest: RelPath::new(manifest)?,
        version: parsed_version,
        version_source: VersionSource::Local,
        publishable: true,
        deps: Vec::new(),
        tag_template: TagTemplate::parse(template).ok()?,
        changelog: Some(RelPath::new("CHANGELOG.md")?),
        version_site: Some(ManifestSite {
            file: RelPath::new(manifest)?,
            spans: vec![Span { start: 40, end: 45 }],
        }),
        include: PathFilter::new(Vec::new(), Vec::new()),
        group: GroupId::new(0),
        facets: halyard_core::Facets::default(),
    };
    Some(ws.push_package(pkg))
}

/// Adds one version-req edge to a package. Returns `false` when a fixture
/// string fails to parse, which the fixed literals make impossible.
fn add_dep(
    ws: &mut Workspace,
    from: PkgId,
    to: PkgId,
    req: &str,
    site_file: &str,
    span: (u32, u32),
) -> bool {
    let Some(req_spec) = VersionReq::parse(req).ok().map(ReqSpec::VersionReq) else {
        return false;
    };
    let Some(site) = RelPath::new(site_file) else {
        return false;
    };
    let raw = req.to_owned();
    if let Some(pkg) = ws.package_mut(from) {
        pkg.deps.push(DepEdge {
            to,
            kind: DepKind::Normal,
            req: req_spec,
            site: ManifestSite {
                file: site,
                spans: vec![Span {
                    start: span.0,
                    end: span.1,
                }],
            },
            raw: raw.into(),
        });
        return true;
    }
    false
}

/// Assembles a snapshot from a workspace, commits and bases.
fn snapshot<'a>(
    ws: &'a Workspace,
    commits: Vec<RelevantCommit>,
    per_pkg: Vec<Vec<usize>>,
    bases: Vec<Option<BaseRef>>,
    head: &str,
) -> Snapshot<'a> {
    let digest = HistoryDigest {
        commits,
        per_pkg,
        bases: bases.clone(),
    };
    let refs = ReleaseRefs::from_bases(bases);
    let history = FixedHistory::new(Oid::from_hex(head.as_bytes()), digest);
    // The snapshot borrows; leak the boxed provider for the test lifetime
    // (each test builds one snapshot).
    let leaked: &'a FixedHistory = Box::leak(Box::new(history));
    let refs_leaked: &'a ReleaseRefs = Box::leak(Box::new(refs));
    Snapshot {
        workspace: ws,
        history: leaked,
        refs: refs_leaked,
    }
}

fn cfg() -> PlanConfig {
    PlanConfig {
        base_branch: Some("main".to_owned()),
        ..PlanConfig::default_values()
    }
}

/// Renders the plan as pretty JSON for the snapshot (sorted keys via
/// canonical JSON keeps output stable across runs). Serialization of the
/// wire plan cannot fail; the marker fallback keeps this helper total.
fn plan_json(plan: &halyard_core::Plan) -> String {
    serde_json::to_value(plan).map_or_else(
        |_| "<unserializable plan>".to_owned(),
        |value| halyard_core::canonical_json(&value),
    )
}

#[test]
fn golden_single_package_feat_and_fix() {
    let mut ws = Workspace::new();
    push_pkg(&mut ws, "app", Some("0.1.0"), "v{version}", "Cargo.toml").expect("fixture package");
    let commits = vec![
        commit(1, "fix", false, "mend edge", Some(12)).expect("fixture commit"),
        commit(2, "feat", false, "grow surface", Some(13)).expect("fixture commit"),
    ];
    let snapshot = snapshot(
        &ws,
        commits,
        vec![vec![0, 1]],
        vec![Some(BaseRef {
            tag: "v0.1.0".into(),
            oid: oid(0).expect("fixture oid"),
        })],
        &"b".repeat(40),
    );
    let plan = plan(&snapshot, &cfg()).unwrap();
    insta::assert_snapshot!("plan__single_feat_fix", plan_json(&plan));
}

#[test]
fn golden_workspace_cascade_rewrites_requirement() {
    let mut ws = Workspace::new();
    let core = push_pkg(
        &mut ws,
        "core",
        Some("0.4.1"),
        "{name}-v{version}",
        "crates/core/Cargo.toml",
    )
    .expect("fixture package");
    let cli = push_pkg(
        &mut ws,
        "cli",
        Some("1.0.3"),
        "{name}-v{version}",
        "crates/cli/Cargo.toml",
    )
    .expect("fixture package");
    assert!(add_dep(
        &mut ws,
        cli,
        core,
        "^0.4",
        "crates/cli/Cargo.toml",
        (30, 34),
    ));
    // A breaking commit on a 0.x crate remaps to Minor under the
    // Cargo-compatible pre-major policy (0.4.1 → 0.5.0), which ^0.4 does
    // not admit — the cascade must fire for cli (its only bump source;
    // cli carries no commits of its own).
    let commits =
        vec![commit(1, "feat", true, "core breaks contract", None).expect("fixture commit")];
    let snapshot = snapshot(
        &ws,
        commits,
        vec![vec![0], Vec::new()],
        vec![
            Some(BaseRef {
                tag: "core-v0.4.1".into(),
                oid: oid(0).expect("fixture oid"),
            }),
            Some(BaseRef {
                tag: "cli-v1.0.3".into(),
                oid: oid(0).expect("fixture oid"),
            }),
        ],
        &"c".repeat(40),
    );
    let plan = plan(&snapshot, &cfg()).unwrap();
    // Cascade: core 0.4.1 → 0.5.0 makes cli's ^0.4 unsatisfied → cli is
    // bumped patch and its requirement rewritten to ^0.5.0.
    let cli_release = plan.releases.iter().find(|r| r.package == "cli").unwrap();
    assert_eq!(cli_release.to, "1.0.4");
    assert!(
        cli_release
            .reasons
            .iter()
            .any(|r| matches!(r, halyard_core::PlanReason::Cascade { .. })),
        "reasons = {:?}",
        cli_release.reasons
    );
    let cli_edit = plan
        .edits
        .iter()
        .find(|e| e.file == "crates/cli/Cargo.toml")
        .unwrap();
    assert!(cli_edit.patches.iter().any(|p| p.text == "^0.5"));
    insta::assert_snapshot!("plan__workspace_cascade", plan_json(&plan));
}

#[test]
fn golden_breaking_commit_is_major() {
    let mut ws = Workspace::new();
    push_pkg(&mut ws, "app", Some("1.4.2"), "v{version}", "Cargo.toml").expect("fixture package");
    let commits =
        vec![commit(1, "feat", true, "reshape contract", Some(7)).expect("fixture commit")];
    let snapshot = snapshot(
        &ws,
        commits,
        vec![vec![0]],
        vec![Some(BaseRef {
            tag: "v1.4.2".into(),
            oid: oid(0).expect("fixture oid"),
        })],
        &"d".repeat(40),
    );
    let plan = plan(&snapshot, &cfg()).unwrap();
    let release = plan.releases.first().unwrap();
    assert_eq!(release.to, "2.0.0");
    assert_eq!(release.bump, halyard_core::Bump::Major);
    insta::assert_snapshot!("plan__breaking_major", plan_json(&plan));
}

#[test]
fn golden_release_as_forces_version() {
    let mut ws = Workspace::new();
    push_pkg(&mut ws, "app", Some("1.2.0"), "v{version}", "Cargo.toml").expect("fixture package");
    let commits = vec![release_as_commit(1, "2.0.0").expect("fixture commit")];
    let snapshot = snapshot(
        &ws,
        commits,
        vec![vec![0]],
        vec![Some(BaseRef {
            tag: "v1.2.0".into(),
            oid: oid(0).expect("fixture oid"),
        })],
        &"e".repeat(40),
    );
    let plan = plan(&snapshot, &cfg()).unwrap();
    let release = plan.releases.first().unwrap();
    assert_eq!(release.to, "2.0.0");
    assert!(matches!(
        release.reasons.first(),
        Some(halyard_core::PlanReason::ReleaseAs { .. })
    ));
    insta::assert_snapshot!("plan__release_as", plan_json(&plan));
}

#[test]
fn golden_initial_release_and_skips() {
    let mut ws = Workspace::new();
    let _fresh = push_pkg(
        &mut ws,
        "fresh",
        Some("0.1.0"),
        "{name}-v{version}",
        "crates/fresh/Cargo.toml",
    )
    .expect("fixture package");
    let quiet = push_pkg(
        &mut ws,
        "quiet",
        Some("0.2.0"),
        "{name}-v{version}",
        "crates/quiet/Cargo.toml",
    )
    .expect("fixture package");
    let commits = vec![commit(1, "feat", false, "fresh feature", None).expect("fixture commit")];
    let snapshot = snapshot(
        &ws,
        commits,
        vec![vec![0], Vec::new()],
        vec![
            None,
            Some(BaseRef {
                tag: "quiet-v0.2.0".into(),
                oid: oid(0).expect("fixture oid"),
            }),
        ],
        &"f".repeat(40),
    );
    let mut config = cfg();
    config.release_disabled.insert("quiet".to_owned());
    let plan = plan(&snapshot, &config).unwrap();
    assert!(plan.releases.iter().any(|r| r.package == "fresh"));
    assert!(
        plan.releases
            .iter()
            .find(|r| r.package == "fresh")
            .unwrap()
            .reasons
            .iter()
            .any(|r| matches!(r, halyard_core::PlanReason::InitialRelease))
    );
    assert!(
        plan.skipped
            .iter()
            .any(|s| s.package == "quiet" && s.reason == "release_disabled")
    );
    let _ = quiet;
    insta::assert_snapshot!("plan__initial_and_skips", plan_json(&plan));
}

#[test]
fn plan_hash_is_stable_and_self_excluding() {
    let mut ws = Workspace::new();
    push_pkg(&mut ws, "app", Some("0.1.0"), "v{version}", "Cargo.toml").expect("fixture package");
    let commits = vec![commit(1, "feat", false, "stable feature", None).expect("fixture commit")];
    let snap1 = snapshot(
        &ws,
        commits,
        vec![vec![0]],
        vec![Some(BaseRef {
            tag: "v0.1.0".into(),
            oid: oid(0).expect("fixture oid"),
        })],
        &"9".repeat(40),
    );
    let plan = plan(&snap1, &cfg()).unwrap();
    // blake3 over the canonical JSON with the hash field scrubbed — the
    // digest of THIS fixture document is pinned here (I3/I8).
    assert!(plan.plan_hash.starts_with("blake3:"));
    assert_eq!(plan.plan_hash.len(), 7 + 64);
    let rehashed = halyard_core::plan_hash(&plan);
    assert_eq!(rehashed, plan.plan_hash);
    // Determinism: two identical runs produce byte-identical documents.
    let mut ws2 = Workspace::new();
    push_pkg(&mut ws2, "app", Some("0.1.0"), "v{version}", "Cargo.toml").expect("fixture package");
    let snap2 = snapshot(
        &ws2,
        vec![commit(1, "feat", false, "stable feature", None).expect("fixture commit")],
        vec![vec![0]],
        vec![Some(BaseRef {
            tag: "v0.1.0".into(),
            oid: oid(0).expect("fixture oid"),
        })],
        &"9".repeat(40),
    );
    let plan2 = halyard_core::planner::plan(&snap2, &cfg()).unwrap();
    assert_eq!(plan_json(&plan), plan_json(&plan2));
    assert_eq!(plan.plan_hash, plan2.plan_hash);
}

#[test]
fn changelog_entry_orders_breaking_first_and_hides_types() {
    let mut ws = Workspace::new();
    push_pkg(&mut ws, "app", Some("1.0.0"), "v{version}", "Cargo.toml").expect("fixture package");
    let mut config = cfg();
    config.changelog_hidden_types.clear();
    config.changelog_hidden_types.insert("docs".to_owned());
    let commits = vec![
        commit(1, "feat", true, "big one", Some(1)).expect("fixture commit"),
        commit(2, "feat", false, "small feature", Some(3)).expect("fixture commit"),
        commit(3, "fix", false, "small fix", Some(2)).expect("fixture commit"),
        commit(4, "docs", false, "readme only", None).expect("fixture commit"),
    ];
    let snapshot = snapshot(
        &ws,
        commits,
        vec![vec![0, 1, 2, 3]],
        vec![Some(BaseRef {
            tag: "v1.0.0".into(),
            oid: oid(0).expect("fixture oid"),
        })],
        &"8".repeat(40),
    );
    let plan = plan(&snapshot, &config).unwrap();
    let entry = &plan
        .releases
        .first()
        .unwrap()
        .changelog
        .as_ref()
        .unwrap()
        .entry_md;
    let breaking_pos = entry.find("BREAKING CHANGES");
    let features_pos = entry.find("Features");
    let fixes_pos = entry.find("Bug Fixes");
    assert!(breaking_pos < features_pos && features_pos < fixes_pos);
    assert!(!entry.contains("readme only"), "hidden types dropped");
}

//! Golden plans (M1.9): synthetic snapshots planned through the pure
//! `plan()` and pinned as insta snapshots. The snapshots double as the
//! `halyard-plan/v1` wire-format fixtures and pin the `plan_hash`
//! stability (I3/I8).

use halyard_core::bump::PrereleaseChannel;
use halyard_core::change_file::ChangeFiles;
use halyard_core::history::{
    AuthorRef, BaseRef, FixedHistory, HistoryDigest, ParsedCommit, RelevantCommit, Trailer,
};
use halyard_core::package::{
    DepEdge, DepKind, Ecosystem, GroupId, ManifestSite, Package, PathFilter, RelPath, ReqSpec,
    Span, TagTemplate, VersionSource, Workspace,
};
use halyard_core::planner::{MaintenanceLine, PlanConfig, Snapshot, plan};
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
    let changes_leaked: &'a ChangeFiles = Box::leak(Box::new(ChangeFiles::new()));
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

// ---------------------------------------------------------------------
// Prerelease channels (§10.3, M5.6 step 1): a configured channel turns
// the planner's stable target into `{target}-{channel}.{n}`, the counter
// derived from the release base's own tag (no state file), and a
// prerelease manifest with the channel removed promotes to its own
// stable triple.

/// `PlanConfig` with a prerelease channel configured (§10.3).
fn channel_cfg(channel: &str) -> PlanConfig {
    let mut config = cfg();
    config.prerelease_channel =
        PrereleaseChannel::new(channel).or_else(|| unreachable!("fixture channel"));
    config
}

#[test]
fn channel_first_prerelease_starts_at_one() {
    let mut ws = Workspace::new();
    push_pkg(&mut ws, "app", Some("1.2.3"), "v{version}", "Cargo.toml").expect("fixture package");
    let commits = vec![commit(1, "feat", false, "grow surface", Some(7)).expect("fixture commit")];
    let snapshot = snapshot(
        &ws,
        commits,
        vec![vec![0]],
        vec![Some(BaseRef {
            tag: "v1.2.3".into(),
            oid: oid(0).expect("fixture oid"),
        })],
        &"c".repeat(40),
    );
    let plan = plan(&snapshot, &channel_cfg("rc")).unwrap();
    let release = plan.releases.first().unwrap();
    assert_eq!(release.to, "1.3.0-rc.1");
    assert_eq!(release.tag, "v1.3.0-rc.1");
    assert_eq!(release.bump, halyard_core::Bump::Minor);
    // The manifest edit carries the prerelease, not the stable triple.
    let edit = plan
        .edits
        .iter()
        .find(|edit| edit.file == "Cargo.toml")
        .unwrap_or_else(|| unreachable!("manifest edit present"));
    assert_eq!(edit.patches.first().unwrap().text, "1.3.0-rc.1");
}

#[test]
fn channel_advances_the_counter_from_the_base_tag() {
    // The train already shipped `v1.3.0-rc.1` and the manifest carries
    // it; one more fix commit advances the counter on the same triple.
    let mut ws = Workspace::new();
    push_pkg(
        &mut ws,
        "app",
        Some("1.3.0-rc.1"),
        "v{version}",
        "Cargo.toml",
    )
    .expect("fixture package");
    let commits = vec![commit(1, "fix", false, "mend edge", Some(8)).expect("fixture commit")];
    let snapshot = snapshot(
        &ws,
        commits,
        vec![vec![0]],
        vec![Some(BaseRef {
            tag: "v1.3.0-rc.1".into(),
            oid: oid(0).expect("fixture oid"),
        })],
        &"d".repeat(40),
    );
    let plan = plan(&snapshot, &channel_cfg("rc")).unwrap();
    let release = plan.releases.first().unwrap();
    assert_eq!(release.to, "1.3.0-rc.2");
    assert_eq!(release.tag, "v1.3.0-rc.2");
    // The stable triple did not move, so the row reports the bump the
    // commits derived (patch), never `none` while a release is owed.
    assert_eq!(release.bump, halyard_core::Bump::Patch);
}

#[test]
fn channel_switch_restarts_the_counter() {
    // The rc tags exist; switching the channel to `beta` starts fresh.
    let mut ws = Workspace::new();
    push_pkg(
        &mut ws,
        "app",
        Some("1.3.0-rc.1"),
        "v{version}",
        "Cargo.toml",
    )
    .expect("fixture package");
    let commits = vec![commit(1, "fix", false, "mend edge", None).expect("fixture commit")];
    let snapshot = snapshot(
        &ws,
        commits,
        vec![vec![0]],
        vec![Some(BaseRef {
            tag: "v1.3.0-rc.1".into(),
            oid: oid(0).expect("fixture oid"),
        })],
        &"e".repeat(40),
    );
    let plan = plan(&snapshot, &channel_cfg("beta")).unwrap();
    assert_eq!(plan.releases.first().unwrap().to, "1.3.0-beta.1");
}

#[test]
fn forced_release_as_target_is_exempt_from_the_channel() {
    // `Release-As` is the user's explicit version: the channel never
    // rewrites a forced target.
    let mut ws = Workspace::new();
    push_pkg(&mut ws, "app", Some("1.2.3"), "v{version}", "Cargo.toml").expect("fixture package");
    let commits = vec![release_as_commit(1, "2.0.0").expect("fixture commit")];
    let snapshot = snapshot(
        &ws,
        commits,
        vec![vec![0]],
        vec![Some(BaseRef {
            tag: "v1.2.3".into(),
            oid: oid(0).expect("fixture oid"),
        })],
        &"f".repeat(40),
    );
    let plan = plan(&snapshot, &channel_cfg("rc")).unwrap();
    assert_eq!(plan.releases.first().unwrap().to, "2.0.0");
}

#[test]
fn prerelease_manifest_promotes_when_the_channel_is_removed() {
    // §10.3 promotion: with no channel configured and no new commits,
    // a prerelease manifest releases its own stable triple.
    let mut ws = Workspace::new();
    push_pkg(
        &mut ws,
        "app",
        Some("1.3.0-rc.2"),
        "v{version}",
        "Cargo.toml",
    )
    .expect("fixture package");
    let snapshot = snapshot(
        &ws,
        Vec::new(),
        vec![Vec::new()],
        vec![Some(BaseRef {
            tag: "v1.3.0-rc.2".into(),
            oid: oid(0).expect("fixture oid"),
        })],
        &"a".repeat(40),
    );
    let plan = plan(&snapshot, &cfg()).unwrap();
    let release = plan.releases.first().unwrap();
    assert_eq!(release.to, "1.3.0");
    assert_eq!(release.tag, "v1.3.0");
    let edit = plan
        .edits
        .iter()
        .find(|edit| edit.file == "Cargo.toml")
        .unwrap_or_else(|| unreachable!("manifest edit present"));
    assert_eq!(edit.patches.first().unwrap().text, "1.3.0");
}

#[test]
fn promotion_leaves_a_stable_manifest_untouched() {
    // The promotion rule is a prerelease-manifest rule only: a stable
    // manifest with no commits plans nothing (unchanged default).
    let mut ws = Workspace::new();
    push_pkg(&mut ws, "app", Some("1.3.0"), "v{version}", "Cargo.toml").expect("fixture package");
    let snapshot = snapshot(
        &ws,
        Vec::new(),
        vec![Vec::new()],
        vec![Some(BaseRef {
            tag: "v1.3.0".into(),
            oid: oid(0).expect("fixture oid"),
        })],
        &"b".repeat(40),
    );
    let plan = plan(&snapshot, &cfg()).unwrap();
    assert!(
        plan.releases.is_empty(),
        "no commits, stable manifest ⇒ no release"
    );
}

#[test]
fn channel_applies_to_the_initial_release() {
    // No release tag yet: the counter starts at one.
    let mut ws = Workspace::new();
    push_pkg(&mut ws, "app", Some("1.0.0"), "v{version}", "Cargo.toml").expect("fixture package");
    let commits = vec![commit(1, "feat", false, "first feature", None).expect("fixture commit")];
    let snapshot = snapshot(&ws, commits, vec![vec![0]], vec![None], &"c".repeat(40));
    let plan = plan(&snapshot, &channel_cfg("rc")).unwrap();
    assert_eq!(plan.releases.first().unwrap().to, "1.1.0-rc.1");
}
// ---------------------------------------------------------------------
// Per-package channel overrides (§10.3, M5.6 step 3 / ADR-0026): a
// `[[package]] channel = "rc"` override names that package's train —
// the most explicit policy wins over the repo-wide (branch-resolved)
// default; every other package keeps the default (or stays stable).

/// Applies a `[[package]] channel` override to a plan config — the same
/// shape `ResolvedConfig::plan_config` builds from the resolved map.
fn with_override(mut config: PlanConfig, name: &str, channel: &str) -> PlanConfig {
    let channel =
        PrereleaseChannel::new(channel).unwrap_or_else(|| unreachable!("fixture channel"));
    config.package_channels.insert(name.to_owned(), channel);
    config
}

#[test]
fn package_override_channelizes_only_that_package() {
    // No repo-wide channel: the override plans `app` on its `rc` train
    // while `lib` keeps the stable derivation.
    let mut ws = Workspace::new();
    let app = push_pkg(&mut ws, "app", Some("1.2.3"), "v{version}", "Cargo.toml")
        .expect("fixture package");
    let lib = push_pkg(
        &mut ws,
        "lib",
        Some("0.5.0"),
        "v{version}",
        "lib/Cargo.toml",
    )
    .expect("fixture package");
    assert_ne!(app, lib, "the fixture packages carry distinct ids");
    let commits = vec![
        commit(1, "feat", false, "grow surface", None).expect("fixture commit"),
        commit(2, "fix", false, "repair edge", None).expect("fixture commit"),
    ];
    let snapshot = snapshot(
        &ws,
        commits,
        vec![vec![0], vec![1]],
        vec![
            Some(BaseRef {
                tag: "v1.2.3".into(),
                oid: oid(3).expect("fixture oid"),
            }),
            Some(BaseRef {
                tag: "v0.5.0".into(),
                oid: oid(4).expect("fixture oid"),
            }),
        ],
        &"a".repeat(40),
    );
    let plan = plan(&snapshot, &with_override(cfg(), "app", "rc")).unwrap();
    let app_row = plan
        .releases
        .iter()
        .find(|row| row.package == "app")
        .unwrap_or_else(|| unreachable!("the overridden package plans"));
    assert_eq!(app_row.to, "1.3.0-rc.1", "the override names the train");
    assert_eq!(app_row.tag, "v1.3.0-rc.1");
    let lib_row = plan
        .releases
        .iter()
        .find(|row| row.package == "lib")
        .unwrap_or_else(|| unreachable!("the un-overridden package plans"));
    assert_eq!(lib_row.to, "0.5.1", "the stable derivation is untouched");
}

#[test]
fn package_override_wins_over_the_repo_wide_channel() {
    // Repo-wide `beta`; the override puts `app` on its own `rc` train.
    let config = with_override(channel_cfg("beta"), "app", "rc");
    let mut ws = Workspace::new();
    push_pkg(&mut ws, "app", Some("1.2.3"), "v{version}", "Cargo.toml").expect("fixture package");
    push_pkg(
        &mut ws,
        "lib",
        Some("0.5.0"),
        "v{version}",
        "lib/Cargo.toml",
    )
    .expect("fixture package");
    let commits = vec![
        commit(1, "feat", false, "grow surface", None).expect("fixture commit"),
        commit(2, "fix", false, "repair edge", None).expect("fixture commit"),
    ];
    let snapshot = snapshot(
        &ws,
        commits,
        vec![vec![0], vec![1]],
        vec![
            Some(BaseRef {
                tag: "v1.2.3".into(),
                oid: oid(3).expect("fixture oid"),
            }),
            Some(BaseRef {
                tag: "v0.5.0".into(),
                oid: oid(4).expect("fixture oid"),
            }),
        ],
        &"b".repeat(40),
    );
    let plan = plan(&snapshot, &config).unwrap();
    let app_row = plan
        .releases
        .iter()
        .find(|row| row.package == "app")
        .unwrap_or_else(|| unreachable!("the overridden package plans"));
    assert_eq!(app_row.to, "1.3.0-rc.1", "the override beats the default");
    let lib_row = plan
        .releases
        .iter()
        .find(|row| row.package == "lib")
        .unwrap_or_else(|| unreachable!("the default-channel package plans"));
    assert_eq!(lib_row.to, "0.5.1-beta.1", "the default fills the rest");
}

#[test]
fn override_continues_the_train_while_unnamed_packages_promote() {
    // §10.3 promotion is per package under an override map: `app`
    // carries the override and continues its train (`rc.1` + one fix →
    // `rc.2`), while `lib` — a prerelease manifest with no channel of
    // its own — promotes to its stable triple.
    let mut ws = Workspace::new();
    push_pkg(
        &mut ws,
        "app",
        Some("1.3.0-rc.1"),
        "v{version}",
        "Cargo.toml",
    )
    .expect("fixture package");
    push_pkg(
        &mut ws,
        "lib",
        Some("1.2.0-rc.3"),
        "v{version}",
        "lib/Cargo.toml",
    )
    .expect("fixture package");
    let commits = vec![commit(1, "fix", false, "repair edge", None).expect("fixture commit")];
    let snapshot = snapshot(
        &ws,
        commits,
        vec![vec![0], Vec::new()],
        vec![
            Some(BaseRef {
                tag: "v1.3.0-rc.1".into(),
                oid: oid(2).expect("fixture oid"),
            }),
            Some(BaseRef {
                tag: "v1.2.0-rc.3".into(),
                oid: oid(3).expect("fixture oid"),
            }),
        ],
        &"c".repeat(40),
    );
    let plan = plan(&snapshot, &with_override(cfg(), "app", "rc")).unwrap();
    let app_row = plan
        .releases
        .iter()
        .find(|row| row.package == "app")
        .unwrap_or_else(|| unreachable!("the overridden train continues"));
    assert_eq!(app_row.to, "1.3.0-rc.2", "the train absorbs the fix");
    let lib_row = plan
        .releases
        .iter()
        .find(|row| row.package == "lib")
        .unwrap_or_else(|| unreachable!("the unnamed package promotes"));
    assert_eq!(lib_row.to, "1.2.0", "promotion stays per package");
}

#[test]
fn override_channel_applies_to_the_initial_release() {
    // No release tag for the overridden package: the counter starts at
    // one exactly like the repo-wide rule.
    let mut ws = Workspace::new();
    push_pkg(&mut ws, "app", Some("1.0.0"), "v{version}", "Cargo.toml").expect("fixture package");
    let commits = vec![commit(1, "feat", false, "first feature", None).expect("fixture commit")];
    let snapshot = snapshot(&ws, commits, vec![vec![0]], vec![None], &"d".repeat(40));
    let plan = plan(&snapshot, &with_override(cfg(), "app", "beta")).unwrap();
    assert_eq!(plan.releases.first().unwrap().to, "1.1.0-beta.1");
}

// ---------------------------------------------------------------------
// Maintenance-line release trains (§10.3, M5.6 / ADR-0027): a branch
// that names its line (`1.x`, `1.2.x`) pins every non-forced target to
// that line — a bump that would leave the line (the branch's major, or
// the pinned minor) is refused `E-PLAN-MAINTENANCE`, never guessed
// across it. `Release-As` stays the explicit escape hatch.

/// `PlanConfig` with a branch-resolved maintenance line (§10.3).
fn maintenance_cfg(line: &str) -> PlanConfig {
    let mut config = cfg();
    config.maintenance_line = MaintenanceLine::parse(line);
    config
}

#[test]
fn maintenance_line_parses_the_documented_grammar() {
    // `MAJOR.x` and `MAJOR.MINOR.x` only: the same string names the
    // branch and the line it pins.
    assert_eq!(
        MaintenanceLine::parse("1.x").map(|line| line.to_string()),
        Some("1.x".to_owned())
    );
    assert_eq!(
        MaintenanceLine::parse("1.2.x").map(|line| line.to_string()),
        Some("1.2.x".to_owned())
    );
    assert!(MaintenanceLine::parse("1.y").is_none());
    assert!(MaintenanceLine::parse("12x").is_none());
    assert!(MaintenanceLine::parse("x").is_none());
    assert!(MaintenanceLine::parse("1.x.y").is_none());
}

#[test]
fn maintenance_line_refuses_a_cross_line_major() {
    // A breaking commit on a `1.x` branch must not plan `2.0.0`: the
    // line guard refuses with the package and the offending target.
    let mut ws = Workspace::new();
    push_pkg(&mut ws, "app", Some("1.2.3"), "v{version}", "Cargo.toml").expect("fixture package");
    let commits = vec![commit(1, "feat", true, "drop the api", None).expect("fixture commit")];
    let snapshot = snapshot(
        &ws,
        commits,
        vec![vec![0]],
        vec![Some(BaseRef {
            tag: "v1.2.3".into(),
            oid: oid(2).expect("fixture oid"),
        })],
        &"a".repeat(40),
    );
    let err = plan(&snapshot, &maintenance_cfg("1.x"))
        .err()
        .unwrap_or_else(|| unreachable!("a cross-line major must refuse"));
    assert_eq!(err.code(), "E-PLAN-MAINTENANCE");
    let rendered = err.to_string();
    assert!(rendered.contains("app"), "the package is named: {rendered}");
    assert!(
        rendered.contains("2.0.0"),
        "the offending target is named: {rendered}"
    );
}

#[test]
fn maintenance_line_allows_in_line_bumps() {
    // `fix` on `1.2.x` and on `1.x` both stay in line; `feat` (minor)
    // stays in `1.x` but leaves `1.2.x`.
    let mut ws = Workspace::new();
    push_pkg(&mut ws, "app", Some("1.2.3"), "v{version}", "Cargo.toml").expect("fixture package");
    let commits = vec![commit(1, "fix", false, "backport repair", None).expect("fixture commit")];
    let snapshot_fix = snapshot(
        &ws,
        commits,
        vec![vec![0]],
        vec![Some(BaseRef {
            tag: "v1.2.3".into(),
            oid: oid(2).expect("fixture oid"),
        })],
        &"b".repeat(40),
    );
    for line in ["1.x", "1.2.x"] {
        let plan = plan(&snapshot_fix, &maintenance_cfg(line)).unwrap();
        assert_eq!(
            plan.releases.first().unwrap().to,
            "1.2.4",
            "a patch stays in {line}"
        );
    }
    let feats = vec![commit(1, "feat", false, "backport feature", None).expect("fixture commit")];
    let snapshot_feat = snapshot(
        &ws,
        feats,
        vec![vec![0]],
        vec![Some(BaseRef {
            tag: "v1.2.3".into(),
            oid: oid(2).expect("fixture oid"),
        })],
        &"c".repeat(40),
    );
    let minor_plan = plan(&snapshot_feat, &maintenance_cfg("1.x")).unwrap();
    assert_eq!(
        minor_plan.releases.first().unwrap().to,
        "1.3.0",
        "minor stays in 1.x"
    );
    let err = plan(&snapshot_feat, &maintenance_cfg("1.2.x"))
        .err()
        .unwrap_or_else(|| unreachable!("a cross-line minor must refuse"));
    assert_eq!(err.code(), "E-PLAN-MAINTENANCE");
}

#[test]
fn maintenance_line_refuses_the_pre_major_remap_escape() {
    // The §10.2 pre-major remap applies first; the line guard is the
    // last gate: a breaking commit on `0.1.x` remaps to `minor`
    // (`0.2.0`) — which still leaves the line and refuses.
    let mut ws = Workspace::new();
    push_pkg(&mut ws, "app", Some("0.1.2"), "v{version}", "Cargo.toml").expect("fixture package");
    let commits = vec![commit(1, "feat", true, "drop the api", None).expect("fixture commit")];
    let snapshot = snapshot(
        &ws,
        commits,
        vec![vec![0]],
        vec![Some(BaseRef {
            tag: "v0.1.2".into(),
            oid: oid(2).expect("fixture oid"),
        })],
        &"d".repeat(40),
    );
    let err = plan(&snapshot, &maintenance_cfg("0.1.x"))
        .err()
        .unwrap_or_else(|| unreachable!("the remapped escape must refuse"));
    assert_eq!(err.code(), "E-PLAN-MAINTENANCE");
    // On the major-only `0.x` line the remapped `0.2.0` is in line.
    let plan = plan(&snapshot, &maintenance_cfg("0.x")).unwrap();
    assert_eq!(plan.releases.first().unwrap().to, "0.2.0");
}

#[test]
fn forced_release_as_target_is_exempt_from_the_line() {
    // `Release-As` is the user's explicit version: the line never
    // rewrites or refuses it (the ADR-0025 exemption reasoning).
    let mut ws = Workspace::new();
    push_pkg(&mut ws, "app", Some("1.2.3"), "v{version}", "Cargo.toml").expect("fixture package");
    let commits = vec![release_as_commit(1, "2.0.0").expect("fixture commit")];
    let snapshot = snapshot(
        &ws,
        commits,
        vec![vec![0]],
        vec![Some(BaseRef {
            tag: "v1.2.3".into(),
            oid: oid(2).expect("fixture oid"),
        })],
        &"e".repeat(40),
    );
    let plan = plan(&snapshot, &maintenance_cfg("1.x")).unwrap();
    assert_eq!(plan.releases.first().unwrap().to, "2.0.0");
}

#[test]
fn maintenance_line_guard_sees_the_channelized_target() {
    // The guard runs over the FINAL targets (ADR-0027): a channelized
    // train target carries the prerelease suffix, and its stable triple
    // is what must stay in line.
    let mut ws = Workspace::new();
    push_pkg(&mut ws, "app", Some("1.2.3"), "v{version}", "Cargo.toml").expect("fixture package");
    let in_line = vec![commit(1, "fix", false, "backport repair", None).expect("fixture commit")];
    let snapshot_in = snapshot(
        &ws,
        in_line,
        vec![vec![0]],
        vec![Some(BaseRef {
            tag: "v1.2.3".into(),
            oid: oid(2).expect("fixture oid"),
        })],
        &"f".repeat(40),
    );
    let mut config = maintenance_cfg("1.2.x");
    config.prerelease_channel =
        PrereleaseChannel::new("rc").or_else(|| unreachable!("fixture channel"));
    let in_line_plan = plan(&snapshot_in, &config).unwrap();
    assert_eq!(in_line_plan.releases.first().unwrap().to, "1.2.4-rc.1");
    let out_of_line =
        vec![commit(1, "feat", false, "backport feature", None).expect("fixture commit")];
    let snapshot_out = snapshot(
        &ws,
        out_of_line,
        vec![vec![0]],
        vec![Some(BaseRef {
            tag: "v1.2.3".into(),
            oid: oid(2).expect("fixture oid"),
        })],
        &"0".repeat(40),
    );
    let err = plan(&snapshot_out, &config)
        .err()
        .unwrap_or_else(|| unreachable!("a cross-line train target must refuse"));
    assert_eq!(err.code(), "E-PLAN-MAINTENANCE");
    assert!(
        err.to_string().contains("1.3.0-rc.1"),
        "the channelized target is named: {err}"
    );
}

// ---------------------------------------------------------------------------
// The npm publish contract (ADR-0033): `publish.npm.access`/`provenance`
// ride the plan's per-release `PlanPublish` — publish consumes the plan
// only (I8), so the spawn-side options must be part of the verified
// contract, not re-read from the shell. Written first (red), implemented
// second.
// ---------------------------------------------------------------------------

/// An npm-package variant of [`push_pkg`] (the same fixture shape, the
/// npm ecosystem, `package.json`).
fn push_npm_pkg(
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
        ecosystem: Ecosystem::Npm,
        root: RelPath::new("pkgs/app")?,
        manifest: RelPath::new(manifest)?,
        version: parsed_version,
        version_source: VersionSource::Local,
        publishable: true,
        deps: Vec::new(),
        tag_template: TagTemplate::parse(template).ok()?,
        changelog: None,
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

#[test]
fn npm_releases_carry_the_publish_options() {
    let mut ws = Workspace::new();
    push_npm_pkg(&mut ws, "app", Some("0.1.0"), "v{version}", "package.json")
        .expect("fixture package");
    let commits = vec![commit(1, "feat", false, "grow surface", Some(12)).expect("fixture commit")];
    let head = &"b".repeat(40);
    let snapshot = snapshot(&ws, commits, vec![vec![0]], vec![None], head);
    let cfg = PlanConfig {
        publish_enabled: true,
        publish_npm_access: Some("public".to_owned()),
        publish_npm_provenance: Some(true),
        ..PlanConfig::default_values()
    };
    let plan = plan(&snapshot, &cfg).unwrap();
    let release = plan
        .releases
        .first()
        .unwrap_or_else(|| unreachable!("a release exists"));
    let publish = release
        .publish
        .as_ref()
        .unwrap_or_else(|| unreachable!("the contract is present"));
    assert_eq!(publish.registry, "npm");
    assert!(publish.enabled, "publishable npm package is enabled");
    assert_eq!(publish.access.as_deref(), Some("public"));
    assert_eq!(publish.provenance, Some(true));
}

#[test]
fn unset_npm_options_leave_the_contract_optional() {
    // Without `publish.npm` config the contract carries `None` both
    // ways — the spawned tool's own posture governs, and the plan never
    // invents an option the user did not set.
    let mut ws = Workspace::new();
    push_npm_pkg(&mut ws, "app", Some("0.1.0"), "v{version}", "package.json")
        .expect("fixture package");
    let commits = vec![commit(1, "feat", false, "grow surface", Some(12)).expect("fixture commit")];
    let head = &"b".repeat(40);
    let snapshot = snapshot(&ws, commits, vec![vec![0]], vec![None], head);
    let cfg = PlanConfig {
        publish_enabled: true,
        ..PlanConfig::default_values()
    };
    let plan = plan(&snapshot, &cfg).unwrap();
    let release = plan
        .releases
        .first()
        .unwrap_or_else(|| unreachable!("a release exists"));
    let publish = release
        .publish
        .as_ref()
        .unwrap_or_else(|| unreachable!("the contract is present"));
    assert_eq!(publish.access, None);
    assert_eq!(publish.provenance, None);
}

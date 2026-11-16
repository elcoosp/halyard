//! Change-file planning contract (§9.4, ADR-0028): the optional
//! `.halyard/changes/*.md` files ride the pure planner as one
//! [`Snapshot`] input — an explicit per-package bump that replaces the
//! commit-derived level (the §9.4 precedence: `Release-As` > change
//! file > conventional commits > cascade) and a markdown note merged
//! verbatim into the release's changelog preview.

use halyard_core::change_file::ChangeFiles;
use halyard_core::history::{
    AuthorRef, BaseRef, FixedHistory, HistoryDigest, ParsedCommit, ReleaseRefs, RelevantCommit,
    Trailer,
};
use halyard_core::package::{
    DepEdge, DepKind, Ecosystem, GroupId, ManifestSite, Package, PathFilter, RelPath, ReqSpec,
    Span, TagTemplate, VersionSource, Workspace,
};
use halyard_core::plan::Bump;
use halyard_core::planner::{PlanConfig, PlanError, Snapshot, plan};
use halyard_core::{Oid, PkgId, PlanReason};
use semver::{Version, VersionReq};

/// `aaaaaaaa…01`-style hex oid for tests (deterministic per suffix).
fn oid(suffix: u8) -> Option<Oid> {
    let mut hex = [b'a'; 40];
    hex[39] = b'0' + (suffix % 10);
    Oid::from_hex(&hex)
}

/// One conventional commit row (`kind` may be a non-conventional type —
/// the squash-merge case the change file exists to replace).
fn commit(suffix: u8, kind: &str, breaking: bool, description: &str) -> Option<RelevantCommit> {
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
        pr: None,
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
    let mut commit = commit(suffix, "chore", false, "release force")?;
    commit.trailers.push(Trailer {
        key: "Release-As".into(),
        value: version.into(),
    });
    Some(commit)
}

/// Registers one package with sane defaults (the fixture strings are
/// fixed literals — the parses cannot fail, `unreachable!` witnesses).
fn push_pkg(ws: &mut Workspace, name: &str, version: Option<&str>, manifest: &str) -> PkgId {
    let parsed_version = version.and_then(|v| Version::parse(v).ok());
    let rel =
        |raw: &str| RelPath::new(raw).unwrap_or_else(|| unreachable!("fixture rel path is valid"));
    let pkg = Package {
        id: PkgId::FIRST,
        name: ws.intern(name),
        ecosystem: Ecosystem::Cargo,
        root: rel(&format!("crates/{name}")),
        manifest: rel(manifest),
        version: parsed_version,
        version_source: VersionSource::Local,
        publishable: true,
        deps: Vec::new(),
        tag_template: TagTemplate::parse("v{version}")
            .unwrap_or_else(|e| unreachable!("fixture template is valid: {e}")),
        changelog: Some(rel("CHANGELOG.md")),
        version_site: Some(ManifestSite {
            file: rel(manifest),
            spans: vec![Span { start: 40, end: 45 }],
        }),
        include: PathFilter::new(Vec::new(), Vec::new()),
        group: GroupId::new(0),
        facets: halyard_core::Facets::default(),
    };
    ws.push_package(pkg)
}

/// Adds one version-req edge to a package (the site file is the
/// dependent's manifest, where the requirement text lives). `false`
/// only if a fixture string failed to parse, which the fixed literals
/// make impossible.
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

/// One-package snapshot with per-commit attribution and one base tag.
fn one_pkg_snapshot<'a>(
    ws: &'a Workspace,
    commits: Vec<RelevantCommit>,
    changes: &'a ChangeFiles,
) -> Snapshot<'a> {
    let per_pkg = vec![(0..commits.len()).collect::<Vec<_>>()];
    snapshot_with(
        ws,
        commits,
        per_pkg,
        vec![Some(base())],
        "cccccccccccccccccccccccccccccccccccccccc",
        changes,
    )
}

/// The shared `v1.2.3` base ref.
fn base() -> BaseRef {
    BaseRef {
        tag: "v1.2.3".into(),
        oid: fixture_oid(0),
    }
}

/// A fixture oid that cannot fail (the fixed 40-byte hex layout).
fn fixture_oid(suffix: u8) -> Oid {
    let mut hex = [b'a'; 40];
    hex[39] = b'0' + (suffix % 10);
    Oid::from_hex(&hex).unwrap_or_else(|| unreachable!("fixture hex is valid"))
}

/// Assembles a snapshot (fixture helper; the workspace borrows the test's
/// binding, the history/refs/changes are leaked for the test lifetime).
fn snapshot_with<'a>(
    ws: &'a Workspace,
    commits: Vec<RelevantCommit>,
    per_pkg: Vec<Vec<usize>>,
    bases: Vec<Option<BaseRef>>,
    head: &str,
    changes: &'a ChangeFiles,
) -> Snapshot<'a> {
    let digest = HistoryDigest {
        commits,
        per_pkg,
        bases: bases.clone(),
    };
    let refs = ReleaseRefs::from_bases(bases);
    let history = FixedHistory::new(Oid::from_hex(head.as_bytes()), digest);
    let leaked: &'a FixedHistory = Box::leak(Box::new(history));
    let refs_leaked: &'a ReleaseRefs = Box::leak(Box::new(refs));
    Snapshot {
        workspace: ws,
        history: leaked,
        refs: refs_leaked,
        change_files: changes,
    }
}

fn cfg() -> PlanConfig {
    PlanConfig {
        base_branch: Some("main".to_owned()),
        ..PlanConfig::default_values()
    }
}

fn release_of<'p>(plan: &'p halyard_core::Plan, name: &str) -> &'p halyard_core::plan::PlanRelease {
    plan.releases
        .iter()
        .find(|r| r.package == name)
        .unwrap_or_else(|| unreachable!("the plan carries a `{name}` release"))
}

/// The change file's explicit bump replaces the commit-derived level in
/// BOTH directions: a `feat` commit (minor) under a `patch` file plans
/// the patch — the explicit input wins, not the union.
#[test]
fn change_file_bump_replaces_commit_bump() {
    let mut ws = Workspace::new();
    push_pkg(&mut ws, "app", Some("1.2.3"), "Cargo.toml");
    let commits = vec![commit(1, "feat", false, "grow surface").expect("fixture commit")];
    let mut changes = ChangeFiles::new();
    changes.record(
        ".halyard/changes/patch.md",
        [("app".to_owned(), Bump::Patch)],
        "",
    );
    let snapshot = one_pkg_snapshot(&ws, commits, &changes);
    let plan = plan(&snapshot, &cfg()).expect("plans");
    let release = release_of(&plan, "app");
    assert_eq!(
        release.to, "1.2.4",
        "the explicit patch wins over the minor: {plan:?}"
    );
    assert_eq!(release.bump, Bump::Patch);
    assert!(
        release.reasons.iter().any(
            |r| matches!(r, PlanReason::Changeset { file } if file == ".halyard/changes/patch.md")
        ),
        "the reason chain names the change file: {:?}",
        release.reasons
    );
}

/// The escape hatch (§9.4): a squash-merge repo whose only commit is a
/// non-conventional title still plans when the change file names the
/// bump — and the note carries the description the commits cannot.
#[test]
fn change_file_bump_without_conventional_commits() {
    let mut ws = Workspace::new();
    push_pkg(&mut ws, "app", Some("1.2.3"), "Cargo.toml");
    let commits = vec![commit(1, "update", false, "merge branch 'next'").expect("fixture commit")];
    let mut changes = ChangeFiles::new();
    changes.record(
        ".halyard/changes/next.md",
        [("app".to_owned(), Bump::Minor)],
        "",
    );
    let snapshot = one_pkg_snapshot(&ws, commits, &changes);
    let plan = plan(&snapshot, &cfg()).expect("plans");
    assert_eq!(release_of(&plan, "app").to, "1.3.0");
}

/// `Release-As` outranks the change file (§9.4 precedence): the forced
/// target wins; both reasons stay in the chain (each documents one
/// explicit input).
#[test]
fn release_as_overrides_the_change_file() {
    let mut ws = Workspace::new();
    push_pkg(&mut ws, "app", Some("1.2.3"), "Cargo.toml");
    let commits = vec![release_as_commit(1, "3.0.0").expect("fixture commit")];
    let mut changes = ChangeFiles::new();
    changes.record(
        ".halyard/changes/major.md",
        [("app".to_owned(), Bump::Major)],
        "",
    );
    let snapshot = one_pkg_snapshot(&ws, commits, &changes);
    let plan = plan(&snapshot, &cfg()).expect("plans");
    let release = release_of(&plan, "app");
    assert_eq!(release.to, "3.0.0", "the forced target wins: {plan:?}");
    assert!(
        release
            .reasons
            .iter()
            .any(|r| matches!(r, PlanReason::ReleaseAs { .. }))
    );
    assert!(
        release
            .reasons
            .iter()
            .any(|r| matches!(r, PlanReason::Changeset { .. }))
    );
}

/// A dependent carrying an explicit change-file bump is exempt from the
/// cascade (§9.4 precedence: change file > cascade): `cli` keeps its
/// explicit patch even though `lib`'s breaking step makes `^0.4`
/// unsatisfied. The requirement rewrite still lands — the edit follows
/// the target, not the bump.
#[test]
fn cascade_does_not_raise_an_explicit_package() {
    let mut ws = Workspace::new();
    let lib = push_pkg(&mut ws, "lib", Some("0.4.1"), "crates/lib/Cargo.toml");
    let cli = push_pkg(&mut ws, "cli", Some("1.0.3"), "crates/cli/Cargo.toml");
    assert!(add_dep(
        &mut ws,
        cli,
        lib,
        "^0.4",
        "crates/cli/Cargo.toml",
        (30, 34)
    ));
    let commits = vec![commit(1, "feat", true, "lib breaks contract").expect("fixture commit")];
    let mut changes = ChangeFiles::new();
    changes.record(
        ".halyard/changes/cli.md",
        [("cli".to_owned(), Bump::Patch)],
        "",
    );
    let snapshot = snapshot_with(
        &ws,
        commits,
        vec![vec![0], Vec::new()],
        vec![
            Some(BaseRef {
                tag: "lib-v0.4.1".into(),
                oid: fixture_oid(0),
            }),
            Some(BaseRef {
                tag: "cli-v1.0.3".into(),
                oid: fixture_oid(0),
            }),
        ],
        "cccccccccccccccccccccccccccccccccccccccc",
        &changes,
    );
    let plan = plan(&snapshot, &cfg()).expect("plans");
    let cli_release = release_of(&plan, "cli");
    assert_eq!(
        cli_release.to, "1.0.4",
        "the explicit patch stands: {plan:?}"
    );
    assert!(
        cli_release
            .reasons
            .iter()
            .any(|r| matches!(r, PlanReason::Changeset { .. }))
    );
    assert!(
        !cli_release
            .reasons
            .iter()
            .any(|r| matches!(r, PlanReason::Cascade { .. })),
        "the cascade must not raise an explicit package: {:?}",
        cli_release.reasons
    );
    let lib_release = release_of(&plan, "lib");
    assert_eq!(
        lib_release.to, "0.5.0",
        "the unpinned package plans normally"
    );
    let cli_edit = plan
        .edits
        .iter()
        .find(|e| e.file == "crates/cli/Cargo.toml")
        .expect("the unsatisfied requirement is still rewritten");
    assert!(cli_edit.patches.iter().any(|p| p.text == "^0.5"));
}

/// A change file naming a package the workspace does not know refuses
/// (`E-PLAN-CHANGE-FILE`) — the strict input discipline of
/// `E-CFG-UNKNOWN-KEY`, applied to the change-file layer.
#[test]
fn unknown_change_file_package_refuses() {
    let mut ws = Workspace::new();
    push_pkg(&mut ws, "app", Some("1.2.3"), "Cargo.toml");
    let mut changes = ChangeFiles::new();
    changes.record(
        ".halyard/changes/typo.md",
        [("ap".to_owned(), Bump::Minor)],
        "",
    );
    let snapshot = one_pkg_snapshot(&ws, Vec::new(), &changes);
    let error = plan(&snapshot, &cfg()).expect_err("an unknown package refuses");
    assert!(
        matches!(error, PlanError::ChangeFilePackage { .. }),
        "{error:?}"
    );
    assert_eq!(error.code(), "E-PLAN-CHANGE-FILE");
    assert!(
        error.to_string().contains("ap"),
        "the unknown name is named: {error}"
    );
}

/// Two change files naming the same package: the strongest bump wins
/// (the same monotone rule the planner applies everywhere) and both
/// notes merge in path order — the loader feeds [`ChangeFiles::record`]
/// in path order, and the merge keeps it.
#[test]
fn strongest_change_file_bump_wins_and_notes_merge_in_path_order() {
    let mut ws = Workspace::new();
    push_pkg(&mut ws, "app", Some("1.2.3"), "Cargo.toml");
    let mut changes = ChangeFiles::new();
    // Path order: `a-minor.md` sorts before `b-patch.md` (the loader's
    // order — `load_change_files` sorts, `record` preserves).
    changes.record(
        ".halyard/changes/a-minor.md",
        [("app".to_owned(), Bump::Minor)],
        "a-minor note",
    );
    changes.record(
        ".halyard/changes/b-patch.md",
        [("app".to_owned(), Bump::Patch)],
        "b-patch note",
    );
    let snapshot = one_pkg_snapshot(&ws, Vec::new(), &changes);
    let plan = plan(&snapshot, &cfg()).expect("plans");
    let release = release_of(&plan, "app");
    assert_eq!(release.to, "1.3.0", "the strongest bump wins across files");
    assert_eq!(
        release.changelog.as_ref().expect("preview").entry_md,
        "## 1.3.0\n\na-minor note\n\nb-patch note\n",
        "both notes merge in path order: {:?}",
        release.changelog
    );
}

/// `[[package]] release = false` outlasts the change file: a disabled
/// package stays skipped even when a file names it (the skip wins over
/// every bump source).
#[test]
fn disabled_package_ignores_the_change_file() {
    let mut ws = Workspace::new();
    push_pkg(&mut ws, "app", Some("1.2.3"), "Cargo.toml");
    let commits = vec![commit(1, "feat", false, "grow surface").expect("fixture commit")];
    let mut changes = ChangeFiles::new();
    changes.record(
        ".halyard/changes/app.md",
        [("app".to_owned(), Bump::Minor)],
        "",
    );
    let snapshot = one_pkg_snapshot(&ws, commits, &changes);
    let mut config = cfg();
    config.release_disabled = ["app".to_owned()].into_iter().collect();
    let plan = plan(&snapshot, &config).expect("plans");
    assert!(
        plan.releases.is_empty(),
        "a disabled package never releases: {plan:?}"
    );
    assert!(
        plan.skipped
            .iter()
            .any(|s| s.package == "app" && s.reason == "release_disabled"),
        "{plan:?}"
    );
}

/// The full wire contract: bump + reasons + the merged note are pinned
/// as a golden snapshot (the plan JSON is the §5.8 wire format).
#[test]
fn golden_change_file_plan() {
    let mut ws = Workspace::new();
    push_pkg(&mut ws, "app", Some("1.2.3"), "Cargo.toml");
    let commits = vec![commit(1, "feat", false, "grow surface").expect("fixture commit")];
    let mut changes = ChangeFiles::new();
    changes.record(
        ".halyard/changes/next.md",
        [("app".to_owned(), Bump::Minor)],
        "Upgrade notes:\n\n- run `halyard migrate` first\n",
    );
    let snapshot = one_pkg_snapshot(&ws, commits, &changes);
    let plan = plan(&snapshot, &cfg()).expect("plans");
    let release = release_of(&plan, "app");
    assert_eq!(release.to, "1.3.0");
    let entry = &release.changelog.as_ref().expect("preview").entry_md;
    assert!(
        entry.ends_with("Upgrade notes:\n\n- run `halyard migrate` first\n"),
        "the note rides after the commit sections: {entry:?}"
    );
    insta::assert_snapshot!("plan__change_file", plan_json(&plan));
}

/// Renders the plan as pretty JSON for the snapshot (canonical JSON
/// keeps output stable across runs).
fn plan_json(plan: &halyard_core::Plan) -> String {
    serde_json::to_value(plan).map_or_else(
        |_| "<unserializable plan>".to_owned(),
        |value| halyard_core::canonical_json(&value),
    )
}

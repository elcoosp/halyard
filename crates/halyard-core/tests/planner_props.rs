//! Planner property tests (M1.9, §10.4/§15.1): over random workspaces and
//! commit sets, the pure `plan()` must terminate, keep every release
//! version strictly increasing, produce tags carrying the target version,
//! cascade only patch-level bumps, and be hash-deterministic (I3).

use halyard_core::history::{
    AuthorRef, BaseRef, FixedHistory, HistoryDigest, ParsedCommit, RelevantCommit,
};
use halyard_core::package::{
    DepEdge, DepKind, Ecosystem, GroupId, ManifestSite, Package, PathFilter, RelPath, ReqSpec,
    TagTemplate, VersionSource, Workspace,
};
use halyard_core::planner::{PlanConfig, Snapshot, plan};
use halyard_core::{Oid, PkgId, ReleaseRefs};
use proptest::prelude::*;
use semver::{Version, VersionReq};

/// Hex oid from an index (deterministic, unique per test run). The
/// generated text is always 40 lowercase hex chars, so `from_hex`
/// succeeds; the error only fires on a broken generator.
fn oid(index: u32) -> Result<Oid, TestCaseError> {
    let text = format!("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa{index:08}");
    Oid::from_hex(text.as_bytes()).ok_or_else(|| TestCaseError::fail("invalid generated oid"))
}

/// Repo-relative path from a test string.
fn rel(raw: &str) -> Result<RelPath, TestCaseError> {
    RelPath::new(raw).ok_or_else(|| TestCaseError::fail(format!("invalid rel path {raw}")))
}

/// Tag template from a test string.
fn template(raw: &str) -> Result<TagTemplate, TestCaseError> {
    TagTemplate::parse(raw).map_err(|_| TestCaseError::fail(format!("invalid template {raw}")))
}

/// Random workspace + history: package count, edge set, per-package
/// majors and breaking flags.
#[derive(Clone, Debug)]
struct Fixture {
    majors: Vec<u64>,
    edges: Vec<(usize, usize, &'static str)>,
    breaking: Vec<bool>,
    has_commits: Vec<bool>,
    base_flags: Vec<bool>,
}

fn fixture_strategy() -> impl Strategy<Value = Fixture> {
    (1usize..=6usize, 0u64..64u64)
        .prop_flat_map(move |(pkg_count, seed)| {
            (
                Just(pkg_count),
                Just(seed),
                prop::collection::vec(0u64..3, pkg_count),
                prop::collection::vec(any::<bool>(), pkg_count),
                prop::collection::vec(any::<bool>(), pkg_count),
                prop::collection::vec(0u8..3, 2 * pkg_count),
            )
        })
        .prop_map(
            |(pkg_count, seed, majors, breaking, has_commits, raw_edges)| {
                // The seed fits far inside `usize`, so the checked
                // conversion cannot saturate on any supported target.
                let seed = usize::try_from(seed).unwrap_or(0);
                let mut edges: Vec<(usize, usize, &'static str)> = Vec::new();
                for (slot, req_kind) in raw_edges.into_iter().enumerate() {
                    let from = slot % pkg_count;
                    let to = (slot / pkg_count + seed) % pkg_count;
                    if from == to {
                        continue;
                    }
                    let req = match req_kind {
                        0 => ">=0.0.0",
                        1 => "^1.0.0",
                        _ => "0.1.0",
                    };
                    let candidate = (from, to, req);
                    if !edges.contains(&candidate) {
                        edges.push(candidate);
                    }
                }
                Fixture {
                    majors,
                    edges,
                    breaking,
                    has_commits,
                    base_flags: (0..pkg_count).map(|i| (i + seed) % 2 == 0).collect(),
                }
            },
        )
}

/// Builds the workspace half of the fixture: one package per major plus
/// the deduplicated normal-edge set.
fn build_workspace(f: &Fixture) -> Result<(Workspace, Vec<Version>), TestCaseError> {
    let mut ws = Workspace::new();
    let mut versions: Vec<Version> = Vec::new();
    for (i, major) in f.majors.iter().enumerate() {
        let version = Version::new(*major, 0, 1);
        let pkg = Package {
            id: PkgId::FIRST,
            name: ws.intern(&format!("p{i}")),
            ecosystem: Ecosystem::Cargo,
            root: rel(&format!("crates/p{i}"))?,
            manifest: rel(&format!("crates/p{i}/Cargo.toml"))?,
            version: Some(version.clone()),
            version_source: VersionSource::Local,
            publishable: true,
            deps: Vec::new(),
            tag_template: template("{name}-v{version}")?,
            changelog: None,
            version_site: Some(ManifestSite {
                file: rel(&format!("crates/p{i}/Cargo.toml"))?,
                spans: vec![halyard_core::package::Span { start: 1, end: 6 }],
            }),
            include: PathFilter::new(Vec::new(), Vec::new()),
            group: GroupId::new(0),
            facets: halyard_core::Facets::default(),
        };
        versions.push(version);
        ws.push_package(pkg);
    }
    for (from, to, req) in &f.edges {
        // Package indices are far below u32 range; saturation keeps the
        // helper total.
        let from_id = PkgId::new(u32::try_from(*from).unwrap_or(u32::MAX));
        let to_id = PkgId::new(u32::try_from(*to).unwrap_or(u32::MAX));
        if ws.package(from_id).is_none() || ws.package(to_id).is_none() {
            continue;
        }
        if let Some(pkg) = ws.package_mut(from_id) {
            pkg.deps.push(DepEdge {
                to: to_id,
                kind: DepKind::Normal,
                req: ReqSpec::VersionReq(
                    VersionReq::parse(req)
                        .map_err(|_| TestCaseError::fail(format!("invalid req {req}")))?,
                ),
                site: ManifestSite {
                    file: rel(&format!("crates/p{from}/Cargo.toml"))?,
                    spans: vec![halyard_core::package::Span { start: 3, end: 9 }],
                },
                raw: (*req).into(),
            });
        }
    }
    Ok((ws, versions))
}

/// Builds the history half of the fixture: one commit per flag, per-package
/// commit lists and release bases.
fn build_history(f: &Fixture) -> Result<(FixedHistory, ReleaseRefs), TestCaseError> {
    let mut commits: Vec<RelevantCommit> = Vec::new();
    let mut per_pkg: Vec<Vec<usize>> = vec![Vec::new(); f.majors.len()];
    let mut commit_index = 0usize;
    for (pkg_index, flag) in f.has_commits.iter().enumerate() {
        if !*flag {
            continue;
        }
        let breaking = f.breaking.get(pkg_index).copied().unwrap_or(false);
        let kind = if breaking { "feat" } else { "fix" };
        let subject = if breaking {
            format!("{kind}!: change {commit_index}")
        } else {
            format!("{kind}: change {commit_index}")
        };
        commits.push(RelevantCommit {
            oid: oid(u32::try_from(commit_index).unwrap_or(u32::MAX))?,
            subject: subject.into(),
            body_range: 0..0,
            parsed: ParsedCommit {
                kind: kind.into(),
                scope: None,
                breaking,
                description: format!("change {commit_index}").into(),
            },
            pr: None,
            author: AuthorRef {
                name: "T".into(),
                email: "t@example.com".into(),
            },
            trailers: smallvec::SmallVec::new(),
            touched: halyard_core::SmallBitset::new(),
        });
        // `per_pkg` is sized to the package count, so the slot exists.
        if let Some(slot) = per_pkg.get_mut(pkg_index) {
            slot.push(commit_index);
        }
        commit_index += 1;
    }
    let base_oid = oid(9000)?;
    let bases: Vec<Option<BaseRef>> = f
        .base_flags
        .iter()
        .map(|has_base| {
            has_base.then(|| BaseRef {
                tag: "base-v0.0.1".into(),
                oid: base_oid.clone(),
            })
        })
        .collect();
    let digest = HistoryDigest {
        commits,
        per_pkg,
        bases: bases.clone(),
    };
    let history = FixedHistory::new(Some(oid(9999)?), digest);
    Ok((history, ReleaseRefs::from_bases(bases)))
}

/// One planned fixture with all invariants asserted.
fn check_fixture(f: &Fixture) -> Result<(), TestCaseError> {
    let (ws, _versions) = build_workspace(f)?;
    let (history, refs) = build_history(f)?;
    let snapshot = Snapshot {
        workspace: &ws,
        history: &history,
        refs: &refs,
    };
    let config = PlanConfig::default_values();
    let plan = match plan(&snapshot, &config) {
        // A non-dev cycle is the documented refusal (E-PLAN-CYCLE); the
        // invariants below only bind successful plans.
        Err(halyard_core::PlanError::Cycle { .. }) => return Ok(()),
        Err(e) => return Err(TestCaseError::fail(format!("plan failed: {e}"))),
        Ok(plan) => plan,
    };

    // 1. Every release strictly increases the version.
    for release in &plan.releases {
        let from = Version::parse(&release.from)
            .map_err(|e| TestCaseError::fail(format!("bad from {e}")))?;
        let to =
            Version::parse(&release.to).map_err(|e| TestCaseError::fail(format!("bad to {e}")))?;
        prop_assert!(
            to > from,
            "release {} regressed: {from} -> {to}",
            release.package
        );
        // 2. The rendered tag carries the target version.
        prop_assert!(release.tag.contains(&release.to));
        // 3. A bumped dependent never jumps beyond patch from cascade
        //    alone: reasons may only raise to patch via Cascade entries
        //    when the package has no direct commits.
        let patch_like = matches!(
            release.bump,
            halyard_core::Bump::None | halyard_core::Bump::Patch
        ) || release
            .reasons
            .iter()
            .any(|r| matches!(r, halyard_core::PlanReason::Commit { .. }));
        prop_assert!(patch_like, "unexpected bump shape for {}", release.package);
    }
    // 4. Determinism (I3): re-planning the same snapshot yields the
    //    identical hash.
    let again = halyard_core::planner::plan(&snapshot, &config)
        .map_err(|e| TestCaseError::fail(format!("re-plan failed: {e}")))?;
    prop_assert_eq!(plan.plan_hash, again.plan_hash);
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn plan_invariants_hold(fixture in fixture_strategy()) {
        check_fixture(&fixture)?;
    }
}

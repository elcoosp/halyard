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
use halyard_core::planner::{plan, PlanConfig, Snapshot};
use halyard_core::{Oid, PkgId, ReleaseRefs};
use proptest::prelude::*;
use semver::{Version, VersionReq};

/// Hex oid from an index (deterministic, unique per test run).
fn oid(index: u32) -> Oid {
    let text = format!("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa{index:08}");
    Oid::from_hex(text.as_bytes()).unwrap()
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
    (1usize..=6usize, 0u64..64u64).prop_flat_map(move |(pkg_count, seed)| {
        (
            Just(pkg_count),
            Just(seed),
            prop::collection::vec(0u64..3, pkg_count),
            prop::collection::vec(any::<bool>(), pkg_count),
            prop::collection::vec(any::<bool>(), pkg_count),
            prop::collection::vec(0u8..3, 2 * pkg_count),
        )
    })
    .prop_map(|(pkg_count, seed, majors, breaking, has_commits, raw_edges)| {
        let mut edges: Vec<(usize, usize, &'static str)> = Vec::new();
        for (slot, req_kind) in raw_edges.into_iter().enumerate() {
            let from = slot % pkg_count;
            let to = (slot / pkg_count + seed as usize) % pkg_count;
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
            base_flags: (0..pkg_count).map(|i| (i + seed as usize) % 2 == 0).collect(),
        }
    })
}

/// One planned fixture with all invariants asserted.
fn check_fixture(f: &Fixture) -> Result<(), TestCaseError> {
    let mut ws = Workspace::new();
    let mut versions: Vec<Version> = Vec::new();
    for (i, major) in f.majors.iter().enumerate() {
        let version = Version::new(*major, 0, 1);
        let pkg = Package {
            id: PkgId::FIRST,
            name: ws.intern(&format!("p{i}")),
            ecosystem: Ecosystem::Cargo,
            root: RelPath::new(&format!("crates/p{i}")).unwrap(),
            manifest: RelPath::new(&format!("crates/p{i}/Cargo.toml")).unwrap(),
            version: Some(version.clone()),
            version_source: VersionSource::Local,
            publishable: true,
            deps: Vec::new(),
            tag_template: TagTemplate::parse("{name}-v{version}").unwrap(),
            changelog: None,
            version_site: Some(ManifestSite {
                file: RelPath::new(&format!("crates/p{i}/Cargo.toml")).unwrap(),
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
        let (from_id, to_id) = (PkgId::new(*from as u32), PkgId::new(*to as u32));
        if ws.package(from_id).is_none() || ws.package(to_id).is_none() {
            continue;
        }
        if let Some(pkg) = ws.package_mut(from_id) {
            pkg.deps.push(DepEdge {
                to: to_id,
                kind: DepKind::Normal,
                req: ReqSpec::VersionReq(VersionReq::parse(req).unwrap()),
                site: ManifestSite {
                    file: RelPath::new(&format!("crates/p{from}/Cargo.toml")).unwrap(),
                    spans: vec![halyard_core::package::Span { start: 3, end: 9 }],
                },
                raw: (*req).into(),
            });
        }
    }
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
            oid: oid(commit_index as u32),
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
        per_pkg[pkg_index].push(commit_index);
        commit_index += 1;
    }
    let bases: Vec<Option<BaseRef>> = f
        .base_flags
        .iter()
        .map(|has_base| {
            has_base.then(|| BaseRef {
                tag: "base-v0.0.1".into(),
                oid: oid(9000),
            })
        })
        .collect();
    let digest = HistoryDigest {
        commits,
        per_pkg,
        bases: bases.clone(),
    };
    let history = FixedHistory::new(Some(oid(9999)), digest);
    let refs = ReleaseRefs::from_bases(bases);
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
        let from = Version::parse(&release.from).unwrap();
        let to = Version::parse(&release.to).unwrap();
        prop_assert!(to > from, "release {} regressed: {from} -> {to}", release.package);
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
    let again = halyard_core::planner::plan(&snapshot, &config).unwrap();
    prop_assert_eq!(plan.plan_hash, again.plan_hash);
    let _ = versions;
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn plan_invariants_hold(fixture in fixture_strategy()) {
        check_fixture(&fixture)?;
    }
}

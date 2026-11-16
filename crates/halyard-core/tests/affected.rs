//! The §16.3/M10 affected-package selector (engine contract): the pure
//! transitive-dependent closure over a workspace's reverse edges. The
//! selection is a pure function of the graph and the changed set — no
//! I/O, no clocks (§4) — and deterministic (I3): rows are [`PkgId`]s in
//! ascending id order and the `via` attribution is the nearest changed
//! ancestor in first-discovery order over the sorted CSR in-edges.

use halyard_core::affected::{AffectedReason, affected_selection, changed_from_digest};
use halyard_core::graph::WorkspaceGraph;
use halyard_core::history::{AuthorRef, HistoryDigest, ParsedCommit, RelevantCommit};
use halyard_core::id::PkgId;
use halyard_core::package::{
    DepEdge, DepKind, Ecosystem, GroupId, ManifestSite, Package, PathFilter, RelPath, ReqSpec,
    Span, TagTemplate, VersionSource, Workspace,
};
use halyard_core::{Oid, SmallBitset};
use semver::{Version, VersionReq};

/// Registers one package with sane defaults (the planner-test shape).
fn push_pkg(ws: &mut Workspace, name: &str, root: &str) -> Option<PkgId> {
    let pkg = Package {
        id: PkgId::FIRST,
        name: ws.intern(name),
        ecosystem: Ecosystem::Cargo,
        root: RelPath::new(root)?,
        manifest: RelPath::new(&format!("{root}/Cargo.toml"))?,
        version: Some(Version::parse("0.1.0").ok()?),
        version_source: VersionSource::Local,
        publishable: true,
        deps: Vec::new(),
        tag_template: TagTemplate::parse("{name}-v{version}").ok()?,
        changelog: Some(RelPath::new("CHANGELOG.md")?),
        version_site: Some(ManifestSite {
            file: RelPath::new(&format!("{root}/Cargo.toml"))?,
            spans: vec![Span { start: 40, end: 45 }],
        }),
        include: PathFilter::new(Vec::new(), Vec::new()),
        group: GroupId::new(0),
        facets: halyard_core::Facets::default(),
    };
    Some(ws.push_package(pkg))
}

/// Adds one dependency edge `from → to` (a fixed fixture requirement —
/// the value is never parsed again by the engine under test). A
/// fixture literal that failed to parse would be an unreachable skip:
/// every edge this helper wires is a hardcoded valid value.
fn add_dep(ws: &mut Workspace, from: PkgId, to: PkgId, kind: DepKind) {
    let Some(req_spec) = VersionReq::parse(">=0.1.0").ok().map(ReqSpec::VersionReq) else {
        return;
    };
    let Some(site) = RelPath::new("Cargo.toml") else {
        return;
    };
    let Some(pkg) = ws.package_mut(from) else {
        return;
    };
    pkg.deps.push(DepEdge {
        to,
        kind,
        req: req_spec,
        site: ManifestSite {
            file: site,
            spans: vec![Span { start: 0, end: 1 }],
        },
        raw: ">=0.1.0".into(),
    });
}

/// Three packages wired `lib ← mid ← app` (mid depends on lib, app on
/// mid). Returns the ids in registration order.
fn chain_workspace() -> Option<(Workspace, PkgId, PkgId, PkgId)> {
    let mut ws = Workspace::new();
    let lib = push_pkg(&mut ws, "lib", "crates/lib")?;
    let mid = push_pkg(&mut ws, "mid", "crates/mid")?;
    let app = push_pkg(&mut ws, "app", "crates/app")?;
    add_dep(&mut ws, mid, lib, DepKind::Normal);
    add_dep(&mut ws, app, mid, DepKind::Normal);
    Some((ws, lib, mid, app))
}

/// One attributed commit (deterministic hex per index).
fn commit(idx: usize) -> Option<RelevantCommit> {
    let digit = b'0' + u8::try_from(idx % 10).ok()?;
    let hex: [u8; 40] = core::array::from_fn(|i| if i == 39 { digit } else { b'a' });
    Some(RelevantCommit {
        oid: Oid::from_hex(&hex)?,
        subject: "feat: x".into(),
        body_range: 0..0,
        parsed: ParsedCommit {
            kind: "feat".into(),
            scope: None,
            breaking: false,
            description: "x".into(),
        },
        pr: None,
        author: AuthorRef {
            name: "Tester".into(),
            email: "tester@example.com".into(),
        },
        trailers: Vec::new().into(),
        touched: SmallBitset::new(),
    })
}

/// A digest where package `i` carries `counts[i]` attributed commits.
fn digest_for(counts: &[usize]) -> HistoryDigest {
    let mut digest = HistoryDigest::default();
    for (pkg, count) in counts.iter().enumerate() {
        for idx in 0..*count {
            // A fixture index whose oid failed to render would be an
            // unreachable skip; the 40-char `a`+digit hex always parses.
            let Some(commit) = commit(idx) else {
                continue;
            };
            digest.commits.push(commit);
            while digest.per_pkg.len() <= pkg {
                digest.per_pkg.push(Vec::new());
            }
            if let Some(slots) = digest.per_pkg.get_mut(pkg) {
                slots.push(digest.commits.len() - 1);
            }
        }
    }
    digest
}

#[test]
fn changed_packages_carry_the_changed_reason() {
    let mut ws = Workspace::new();
    let Some(lone) = push_pkg(&mut ws, "lone", "crates/lone") else {
        panic!("fixture packages register");
    };
    let Some(other) = push_pkg(&mut ws, "other", "crates/other") else {
        panic!("fixture packages register");
    };
    let _ = other;
    let graph = WorkspaceGraph::build(&ws);
    let selection = affected_selection(&graph, &[lone]);
    assert_eq!(
        selection.len(),
        1,
        "a lone changed package with no dependents affects nobody else"
    );
    let row = selection
        .rows()
        .first()
        .expect("the lone changed package is a row");
    assert_eq!(row.package, lone);
    assert_eq!(row.reason, AffectedReason::Changed);
}

#[test]
fn dependents_close_transitively_with_nearest_via() {
    let Some((ws, lib, mid, app)) = chain_workspace() else {
        panic!("fixture packages register");
    };
    let graph = WorkspaceGraph::build(&ws);
    let selection = affected_selection(&graph, &[lib]);
    let rows = selection.rows();
    assert_eq!(rows.len(), 3, "the whole chain is affected by lib");
    let [root_row, mid_row, app_row] = rows else {
        panic!("three rows in ascending id order");
    };
    assert_eq!(root_row.package, lib);
    assert_eq!(root_row.reason, AffectedReason::Changed);
    assert_eq!(mid_row.package, mid);
    assert_eq!(mid_row.reason, AffectedReason::Dependent { via: lib });
    assert_eq!(app_row.package, app);
    assert_eq!(
        app_row.reason,
        AffectedReason::Dependent { via: mid },
        "via is the nearest changed ancestor, not the change root"
    );
}

#[test]
fn changed_wins_over_dependent() {
    let Some((ws, lib, mid, _app)) = chain_workspace() else {
        panic!("fixture packages register");
    };
    let graph = WorkspaceGraph::build(&ws);
    let selection = affected_selection(&graph, &[lib, mid]);
    let rows = selection.rows();
    assert_eq!(rows.len(), 3);
    let Some(mid_row) = rows.get(1) else {
        panic!("three rows in ascending id order");
    };
    assert_eq!(mid_row.package, mid);
    assert_eq!(
        mid_row.reason,
        AffectedReason::Changed,
        "a package with its own pending commits is never demoted to a dependent row"
    );
}

#[test]
fn dev_dependents_count_for_ci_selection() {
    let mut ws = Workspace::new();
    let Some(lib) = push_pkg(&mut ws, "lib", "crates/lib") else {
        panic!("fixture packages register");
    };
    let Some(tests) = push_pkg(&mut ws, "tests", "crates/tests") else {
        panic!("fixture packages register");
    };
    add_dep(&mut ws, tests, lib, DepKind::Dev);
    let graph = WorkspaceGraph::build(&ws);
    let selection = affected_selection(&graph, &[lib]);
    assert_eq!(
        selection.len(),
        2,
        "a dev-dependency still runs the dependent's suite against the change (CI selection, \
         not the §10.4 publish order)"
    );
}

#[test]
fn diamond_attribution_is_deterministic() {
    let mut ws = Workspace::new();
    let Some(core) = push_pkg(&mut ws, "core", "crates/core") else {
        panic!("fixture packages register");
    };
    let Some(left) = push_pkg(&mut ws, "left", "crates/left") else {
        panic!("fixture packages register");
    };
    let Some(right) = push_pkg(&mut ws, "right", "crates/right") else {
        panic!("fixture packages register");
    };
    let Some(top) = push_pkg(&mut ws, "top", "crates/top") else {
        panic!("fixture packages register");
    };
    add_dep(&mut ws, left, core, DepKind::Normal);
    add_dep(&mut ws, right, core, DepKind::Normal);
    add_dep(&mut ws, top, left, DepKind::Normal);
    add_dep(&mut ws, top, right, DepKind::Normal);
    let graph = WorkspaceGraph::build(&ws);
    let selection = affected_selection(&graph, &[core]);
    assert_eq!(selection.len(), 4);
    // `top` is discovered through `left` (the lower in-edge target id in
    // the sorted CSR), and re-discovery through `right` is a no-op.
    assert_eq!(
        selection.reason_of(top),
        Some(&AffectedReason::Dependent { via: left }),
        "the first discoverer in sorted-edge order owns the via attribution"
    );
    // Re-running the same inputs yields the same rows (I3).
    let again = affected_selection(&graph, &[core]);
    assert_eq!(selection.rows(), again.rows());
}

#[test]
fn foreign_ids_are_ignored() {
    let Some((ws, lib, _mid, _app)) = chain_workspace() else {
        panic!("fixture packages register");
    };
    let graph = WorkspaceGraph::build(&ws);
    let foreign = PkgId::new(u32::try_from(graph.len()).expect("small fixture"));
    let selection = affected_selection(&graph, &[foreign, lib]);
    assert_eq!(selection.len(), 3, "the foreign id is skipped, not fatal");
    assert!(selection.reason_of(foreign).is_none());
}

#[test]
fn empty_changed_yields_an_empty_selection() {
    let Some((ws, _lib, _mid, _app)) = chain_workspace() else {
        panic!("fixture packages register");
    };
    let graph = WorkspaceGraph::build(&ws);
    let selection = affected_selection(&graph, &[]);
    assert!(selection.is_empty());
    assert_eq!(selection.rows(), &[]);
}

#[test]
fn changed_from_digest_reads_per_package_attribution() {
    let digest = digest_for(&[2, 0, 1]);
    let changed = changed_from_digest(&digest);
    assert_eq!(
        changed,
        vec![PkgId::FIRST, PkgId::new(2)],
        "package order, ids with at least one pending commit"
    );
    let empty = changed_from_digest(&HistoryDigest::default());
    assert!(empty.is_empty(), "an empty digest changes nothing");
}

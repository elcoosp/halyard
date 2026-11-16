//! Demos the affected-package selector ([`halyard_core::affected`], the
//! §16.3 monorepo selector, M10 step 1): a pending-commit set closed
//! over the workspace's reverse dependency edges — the CI question
//! *which packages must rebuild and test?* answered as a pure function
//! over an in-memory workspace and digest (no git, no I/O).
//!
//! Run: `cargo run -p halyard-core --example affected_selection`

use std::error::Error;

use halyard_core::affected::{affected_selection, changed_from_digest};
use halyard_core::graph::WorkspaceGraph;
use halyard_core::history::{AuthorRef, HistoryDigest, ParsedCommit, RelevantCommit};
use halyard_core::package::{
    DepEdge, DepKind, Ecosystem, GroupId, ManifestSite, Package, PathFilter, RelPath, ReqSpec,
    Span, TagTemplate, VersionSource, Workspace,
};
use halyard_core::{Oid, PkgId};

fn main() -> Result<(), Box<dyn Error>> {
    // A three-package workspace on the classic release-train shape:
    // `cli` depends on `planner`, `planner` depends on `core`.
    let mut ws = Workspace::new();
    let mut ids = Vec::new();
    for name in ["core", "planner", "cli"] {
        let pkg = Package {
            id: PkgId::FIRST,
            name: ws.intern(name),
            ecosystem: Ecosystem::Cargo,
            root: RelPath::new(&format!("crates/{name}")).ok_or("invalid root")?,
            manifest: RelPath::new(&format!("crates/{name}/Cargo.toml"))
                .ok_or("invalid manifest")?,
            version: Some(semver::Version::parse("0.4.1")?),
            version_source: VersionSource::Local,
            publishable: true,
            deps: Vec::new(),
            tag_template: TagTemplate::parse("{name}-v{version}")?,
            changelog: None,
            version_site: None,
            include: PathFilter::new(Vec::new(), Vec::new()),
            group: GroupId::new(0),
            facets: halyard_core::discover::Facets::default(),
        };
        ids.push(ws.push_package(pkg));
    }
    let core = ids
        .first()
        .copied()
        .ok_or("the fixture registers core first")?;
    let planner = ids
        .get(1)
        .copied()
        .ok_or("the fixture registers planner second")?;
    let cli = ids
        .get(2)
        .copied()
        .ok_or("the fixture registers cli third")?;
    // The edges the manifests declare (the discovery-filled shape).
    for (from, to) in [(planner, core), (cli, planner)] {
        let req = ReqSpec::VersionReq(semver::VersionReq::parse(">=0.4.1")?);
        let site = RelPath::new("Cargo.toml").ok_or("invalid site")?;
        let pkg = ws.package_mut(from).ok_or("unknown edge source")?;
        pkg.deps.push(DepEdge {
            to,
            kind: DepKind::Normal,
            req,
            site: ManifestSite {
                file: site,
                spans: vec![Span { start: 0, end: 1 }],
            },
            raw: ">=0.4.1".into(),
        });
    }

    // The pending history: one commit attributed to `core`, nothing
    // else (the digest the git engine's walk produces; here in-memory).
    let mut hex = [b'a'; 40];
    hex[39] = b'3';
    let digest = HistoryDigest {
        commits: vec![RelevantCommit {
            oid: Oid::from_hex(&hex).ok_or("fixture oid")?,
            subject: "fix(core): guard the painter window".into(),
            body_range: 0..0,
            parsed: ParsedCommit {
                kind: "fix".into(),
                scope: Some("core".into()),
                breaking: false,
                description: "guard the painter window".into(),
            },
            pr: Some(142),
            author: AuthorRef {
                name: "Tester".into(),
                email: "tester@example.com".into(),
            },
            trailers: Vec::new().into(),
            touched: halyard_core::SmallBitset::new(),
        }],
        per_pkg: vec![vec![0], Vec::new(), Vec::new()],
        bases: vec![None, None, None],
    };

    let graph = WorkspaceGraph::build(&ws);
    let changed = changed_from_digest(&digest);
    let selection = affected_selection(&graph, &changed);

    let name_of = |pkg: PkgId| -> String {
        ws.package(pkg)
            .and_then(|p| ws.interner().get(p.name).map(str::to_owned))
            .unwrap_or_default()
    };
    println!("pending commits per package: core=1, planner=0, cli=0");
    println!("selection (one fix(core) commit, closed over dependents):");
    for row in selection.rows() {
        let why = match row.reason {
            halyard_core::affected::AffectedReason::Changed => "changed".to_owned(),
            halyard_core::affected::AffectedReason::Dependent { via } => {
                format!("dependent (via {})", name_of(via))
            }
        };
        println!("  {:<8} {}", name_of(row.package), why);
    }
    Ok(())
}

//! Demos the §10.3 prerelease-channel planning (ADR-0025): one release
//! train through its whole lifecycle — first prerelease, counter
//! advance, promotion to stable — planned purely through
//! [`halyard_core::planner::plan`] with the `[versioning].channel`
//! policy set and cleared.
//!
//! Run: `cargo run -p halyard-core --example prerelease_channel`

use std::error::Error;

use halyard_core::history::{
    AuthorRef, BaseRef, FixedHistory, HistoryDigest, ParsedCommit, ReleaseRefs, RelevantCommit,
};
use halyard_core::package::{
    Ecosystem, GroupId, Package, PathFilter, RelPath, TagTemplate, VersionSource, Workspace,
};
use halyard_core::planner::{PlanConfig, Snapshot, plan};
use halyard_core::{Oid, PkgId, bump::PrereleaseChannel};

fn main() -> Result<(), Box<dyn Error>> {
    // Pass 1 — the train starts: manifest 1.2.3 (tagged `v1.2.3`), one
    // unreleased `feat`, channel `rc` → `1.3.0-rc.1`.
    let snapshot = fixture("1.2.3", Some("v1.2.3"), vec![commit("feat")])?;
    let mut cfg = PlanConfig::default_values();
    cfg.prerelease_channel = PrereleaseChannel::new("rc");
    print_pass("channel on, first prerelease", &plan(&snapshot, &cfg)?);

    // Pass 2 — the train continues: the manifest carries `1.3.0-rc.1`
    // (tagged), one more `fix` lands. The triple stays the train's own
    // committed progress; the counter advances → `1.3.0-rc.2`.
    let snapshot = fixture("1.3.0-rc.1", Some("v1.3.0-rc.1"), vec![commit("fix")])?;
    print_pass("channel on, counter advance", &plan(&snapshot, &cfg)?);

    // Pass 3 — promotion (§10.3): the channel is removed and nothing
    // new landed; the prerelease manifest releases its own stable
    // triple → `1.3.0`.
    let snapshot = fixture("1.3.0-rc.2", Some("v1.3.0-rc.2"), Vec::new())?;
    let cfg = PlanConfig::default_values();
    print_pass("channel off, promotion", &plan(&snapshot, &cfg)?);

    Ok(())
}

/// Renders the plan's first release row (or the no-release shape).
fn print_pass(label: &str, plan: &halyard_core::Plan) {
    println!("{label}:");
    match plan.releases.first() {
        Some(release) => println!(
            "  {} {} -> {} (bump: {:?}, tag: {})",
            release.package, release.from, release.to, release.bump, release.tag
        ),
        None => println!("  no release owed"),
    }
    for edit in &plan.edits {
        for patch in &edit.patches {
            println!("  edit {}: {} bytes replaced", edit.file, patch.text.len());
        }
    }
}

/// One package at `version` with one attributed conventional commit.
fn fixture(
    version: &str,
    base_tag: Option<&str>,
    commits: Vec<RelevantCommit>,
) -> Result<Snapshot<'static>, Box<dyn Error>> {
    let mut ws = Workspace::new();
    let pkg = Package {
        id: PkgId::FIRST,
        name: ws.intern("app"),
        ecosystem: Ecosystem::Cargo,
        root: RelPath::new("crates/app").ok_or("invalid root")?,
        manifest: RelPath::new("crates/app/Cargo.toml").ok_or("invalid manifest")?,
        version: Some(semver::Version::parse(version)?),
        version_source: VersionSource::Local,
        publishable: true,
        deps: Vec::new(),
        tag_template: TagTemplate::parse("v{version}")?,
        changelog: None,
        version_site: None,
        include: PathFilter::new(Vec::new(), Vec::new()),
        group: GroupId::new(0),
        facets: halyard_core::discover::Facets::default(),
    };
    ws.push_package(pkg);
    let per_pkg = vec![(0..commits.len()).collect::<Vec<_>>()];
    let base_oid = Oid::from_hex(&[b'b'; 40]).ok_or("fixture oid")?;
    let base = base_tag.map(|tag| BaseRef {
        tag: tag.into(),
        oid: base_oid,
    });
    let digest = HistoryDigest {
        commits,
        per_pkg,
        bases: vec![base.clone()],
    };
    let head = Oid::from_hex(&[b'c'; 40]).ok_or("fixture head")?;
    let history = Box::leak(Box::new(FixedHistory::new(Some(head), digest)));
    let refs = Box::leak(Box::new(ReleaseRefs::from_bases(vec![base])));
    let leaked_ws: &'static Workspace = Box::leak(Box::new(ws));
    Ok(Snapshot {
        workspace: leaked_ws,
        history,
        refs,
    })
}

/// One conventional commit row.
fn commit(kind: &str) -> RelevantCommit {
    RelevantCommit {
        oid: Oid::from_hex(&[b'a'; 40]).unwrap_or_else(|| unreachable!("fixture hex is valid")),
        subject: format!("{kind}: fixture").into(),
        body_range: 0..0,
        parsed: ParsedCommit {
            kind: kind.into(),
            scope: None,
            breaking: false,
            description: "fixture".into(),
        },
        pr: None,
        author: AuthorRef {
            name: "Demo".into(),
            email: "demo@example.com".into(),
        },
        trailers: Vec::new().into(),
        touched: halyard_core::SmallBitset::new(),
    }
}

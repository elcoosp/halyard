//! Demos the §10.3 maintenance-line release trains (ADR-0027): a
//! branch that names its line (`1.x`, `1.2.x`) pins every non-forced
//! target to the line — in-line bumps plan through the guard, a bump
//! that would leave the line refuses with `E-PLAN-MAINTENANCE`, and
//! `Release-As` stays the explicit escape hatch.
//!
//! Run: `cargo run -p halyard-core --example maintenance_line`

use halyard_core::history::{
    AuthorRef, BaseRef, FixedHistory, HistoryDigest, ParsedCommit, ReleaseRefs, RelevantCommit,
};
use halyard_core::package::{
    Ecosystem, GroupId, Package, PathFilter, RelPath, TagTemplate, VersionSource, Workspace,
};
use halyard_core::planner::{MaintenanceLine, PlanConfig, Snapshot, plan};
use halyard_core::{Oid, PkgId};

fn main() {
    // Pass 1 — the backport flow: manifest 1.2.3 (tagged `v1.2.3`) on a
    // branch pinned to `1.x`, one backport `fix` → `1.2.4` (in line).
    let snapshot = fixture("1.2.3", Some("v1.2.3"), vec![commit("fix", false)]);
    let cfg = line_cfg("1.x");
    print_pass("backport fix on `1.x`", &plan(&snapshot, &cfg));

    // Pass 2 — the refusal: a breaking commit would plan `2.0.0`, which
    // leaves the line; the guard refuses with the fix named.
    let snapshot = fixture("1.2.3", Some("v1.2.3"), vec![commit("feat", true)]);
    match plan(&snapshot, &cfg) {
        Ok(_) => println!("unexpected: the cross-line major planned"),
        Err(err) => println!("breaking commit on `1.x`:\n  refused: {err}"),
    }

    // Pass 3 — the unpinned branch: the same breaking commit on a
    // branch the config does not pin plans free.
    let snapshot = fixture("1.2.3", Some("v1.2.3"), vec![commit("feat", true)]);
    print_pass(
        "breaking commit on an unpinned branch",
        &plan(&snapshot, &PlanConfig::default_values()),
    );
}

/// `PlanConfig` with a branch-resolved maintenance line (§10.3).
fn line_cfg(line: &str) -> PlanConfig {
    let mut config = PlanConfig::default_values();
    config.maintenance_line = MaintenanceLine::parse(line);
    config
}

/// Renders the plan's first release row (or the no-release shape).
fn print_pass(label: &str, result: &Result<halyard_core::Plan, halyard_core::planner::PlanError>) {
    println!("{label}:");
    match result {
        Ok(plan) => match plan.releases.first() {
            Some(release) => println!(
                "  {} {} -> {} (bump: {:?}, tag: {})",
                release.package, release.from, release.to, release.bump, release.tag
            ),
            None => println!("  (no release owed)"),
        },
        Err(err) => println!("  refused: {err}"),
    }
}

/// One conventional commit row (planner-test fixture shape).
fn commit(kind: &str, breaking: bool) -> RelevantCommit {
    let oid = Oid::from_hex(&[b'a'; 40]).unwrap_or_else(|| unreachable!("fixture hex is valid"));
    RelevantCommit {
        oid,
        subject: format!("{kind}: fixture").into(),
        body_range: 0..0,
        parsed: ParsedCommit {
            kind: kind.into(),
            scope: None,
            breaking,
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

/// One package at `version` with a `v{version}` template and its
/// attributed commits; the snapshot is `'static` through deliberate
/// leaks (each fixture lives for the process).
fn fixture(
    version: &str,
    base_tag: Option<&str>,
    commits: Vec<RelevantCommit>,
) -> Snapshot<'static> {
    let mut ws = Workspace::new();
    let pkg = Package {
        id: PkgId::FIRST,
        name: ws.intern("app"),
        ecosystem: Ecosystem::Cargo,
        root: RelPath::new("crates/app").unwrap_or_else(|| unreachable!("fixture root is valid")),
        manifest: RelPath::new("crates/app/Cargo.toml")
            .unwrap_or_else(|| unreachable!("fixture manifest is valid")),
        version: Some(
            semver::Version::parse(version).unwrap_or_else(|_| unreachable!("fixture version")),
        ),
        version_source: VersionSource::Local,
        publishable: true,
        deps: Vec::new(),
        tag_template: TagTemplate::parse("v{version}")
            .unwrap_or_else(|e| unreachable!("fixture template is valid: {e}")),
        changelog: None,
        version_site: None,
        include: PathFilter::new(Vec::new(), Vec::new()),
        group: GroupId::new(0),
        facets: halyard_core::discover::Facets::default(),
    };
    ws.push_package(pkg);
    let per_pkg = vec![(0..commits.len()).collect::<Vec<_>>()];
    let base = base_tag.map(|tag| BaseRef {
        tag: tag.into(),
        oid: Oid::from_hex(&[b'b'; 40]).unwrap_or_else(|| unreachable!("fixture hex is valid")),
    });
    let digest = HistoryDigest {
        commits,
        per_pkg,
        bases: vec![base.clone()],
    };
    let head = Oid::from_hex(&[b'c'; 40]).unwrap_or_else(|| unreachable!("fixture hex is valid"));
    let history = Box::leak(Box::new(FixedHistory::new(Some(head), digest)));
    let refs = Box::leak(Box::new(ReleaseRefs::from_bases(vec![base])));
    let leaked_ws: &'static Workspace = Box::leak(Box::new(ws));
    let changes: &'static halyard_core::change_file::ChangeFiles =
        Box::leak(Box::new(halyard_core::change_file::ChangeFiles::new()));
    Snapshot {
        workspace: leaked_ws,
        history,
        refs,
        change_files: changes,
    }
}

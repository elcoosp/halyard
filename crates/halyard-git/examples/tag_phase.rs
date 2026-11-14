//! Demos the §11.1 step-2 git-side tag phase end to end: reconcile the
//! fixture workspace's manifest versions against its tags
//! ([`halyard_core::release_state::pending_releases`]), turn the pending
//! queue into directives ([`halyard_git::tag::directives_for_pending`]),
//! create the annotated tags through the [`halyard_git::tag::TagWriter`]
//! seam, then re-run the phase — the re-run reconciles to *nothing
//! pending* (I1) and a stale directive set replays as
//! [`halyard_git::tag::TagOutcome::AlreadyTagged`] (I2 resume shape).
//!
//! Run: `cargo run -p halyard-git --example tag_phase`

use std::error::Error;

use halyard_core::PkgId;
use halyard_core::package::{
    Ecosystem, GroupId, Package, PathFilter, RelPath, TagTemplate, VersionSource, Workspace,
};
use halyard_core::planner::PlanConfig;
use halyard_core::release_state::{pending_releases, reconcile_tags};
use halyard_git::tag::{
    GixTagWriter, TagPhaseRow, TaggerIdentity, directives_for_pending, run_tag_phase,
};
use halyard_git::{GitHistory, GitHistoryOptions};
use halyard_test_support::{CommitSpec, GitFixture};
use tempfile::TempDir;

fn main() -> Result<(), Box<dyn Error>> {
    // A deterministic fixture repo: two packages, no tags yet — both
    // manifest versions are pending (the initial-release state, §8.2).
    // `commit_with` writes, stages and commits in one deterministic step
    // (the returned oid is the commit the tag phase will target).
    let dir = TempDir::new()?;
    let mut fx = GitFixture::init_in(dir.path())?;
    let head = fx.commit_with(&CommitSpec::files(
        vec![
            (
                "crates/core/Cargo.toml".to_owned(),
                "[package]\nname = \"core\"\nversion = \"0.4.0\"\nedition = \"2021\"\n".to_owned(),
            ),
            (
                "crates/core/src/lib.rs".to_owned(),
                "pub const CORE: usize = 1;\n".to_owned(),
            ),
            (
                "crates/cli/Cargo.toml".to_owned(),
                "[package]\nname = \"cli\"\nversion = \"1.0.0\"\nedition = \"2021\"\n".to_owned(),
            ),
            (
                "crates/cli/src/main.rs".to_owned(),
                "fn main() {}\n".to_owned(),
            ),
        ],
        "feat: bootstrap the workspace",
    ))?;
    let head_oid = halyard_core::Oid::from_hex(head.as_bytes())
        .ok_or_else(|| "fixture produced a non-hex oid".to_owned())?;

    // The discovered workspace (the example builds it the way discovery
    // would, so the demo stays a leaf — no halyard-discover dependency).
    let mut ws = Workspace::new();
    let specs = [
        ("core", "crates/core", "0.4.0", "{name}-v{version}"),
        ("cli", "crates/cli", "1.0.0", "{name}-v{version}"),
    ];
    // Intern all names first: `intern` needs `&mut ws`, and the struct
    // literal below borrows `ws` immutably until `push_package` takes it.
    let names: Vec<_> = specs
        .iter()
        .map(|(name, _, _, _)| ws.intern(name))
        .collect();
    for ((_pkg, path, ver, tpl), sym) in specs.iter().zip(names) {
        let manifest = format!("{path}/Cargo.toml");
        ws.push_package(Package {
            id: PkgId::FIRST,
            name: sym,
            ecosystem: Ecosystem::Cargo,
            root: RelPath::new(path).ok_or("bad root")?,
            manifest: RelPath::new(&manifest).ok_or("bad manifest")?,
            version: Some(semver::Version::parse(ver)?),
            version_source: VersionSource::Local,
            publishable: true,
            deps: Vec::new(),
            tag_template: TagTemplate::parse(tpl).map_err(|e| format!("bad template: {e}"))?,
            changelog: None,
            version_site: None,
            include: PathFilter::new(Vec::new(), Vec::new()),
            group: GroupId::new(0),
            facets: halyard_core::Facets::default(),
        });
    }

    // Snapshot the tags through the history engine (the shell's source of
    // tag names — the reconciliation never re-scans refs itself).
    let history = GitHistory::build_with_options(fx.root(), &ws, GitHistoryOptions::default())?;
    let tag_names: Vec<halyard_core::BString> =
        history.tags().iter().map(|tag| tag.name.clone()).collect();
    let cfg = PlanConfig::default_values();

    // Pass 1: reconcile → pending → directives → create.
    let rows = reconcile_tags(&ws, &tag_names, &cfg);
    println!("reconciliation:");
    for row in &rows {
        println!(
            "  {} {:?} (manifest {})",
            row.package, row.state, row.manifest_version
        );
    }
    let pending = pending_releases(&ws, &tag_names, &cfg);
    let directives = directives_for_pending(&pending, &head_oid);
    let tagger = TaggerIdentity::new("halyard", "release@halyard.test", 1_700_000_000);
    let writer = GixTagWriter::open(fx.root(), tagger)?;
    let report = run_tag_phase(&writer, &directives);
    println!(
        "pass 1: {} created, {} already tagged, {} failed",
        report.created, report.already_tagged, report.failed
    );
    for row in &report.rows {
        match row {
            TagPhaseRow::Applied { tag, outcome } => println!("  {tag}: {outcome:?}"),
            TagPhaseRow::Failed { tag, error } => println!("  {tag}: {error}"),
        }
    }

    // Ground truth: git sees the annotated tags.
    println!("git tags: {:?}", fx.ls_tags()?);

    // Pass 2: the reconciliation goes quiet (I1) — and a stale directive
    // set replayed over the tagged repo is a no-op (I2 resume shape).
    let history = GitHistory::build_with_options(fx.root(), &ws, GitHistoryOptions::default())?;
    let tag_names: Vec<halyard_core::BString> =
        history.tags().iter().map(|tag| tag.name.clone()).collect();
    let pending_after = pending_releases(&ws, &tag_names, &cfg);
    println!("pass 2 pending: {} release(s)", pending_after.len());
    let replay = run_tag_phase(&writer, &directives);
    println!(
        "pass 2 replay: {} created, {} already tagged, {} failed",
        replay.created, replay.already_tagged, replay.failed
    );
    Ok(())
}

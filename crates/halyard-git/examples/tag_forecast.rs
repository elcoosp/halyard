//! Demos the offline tag-phase forecast (§11.1 step 2): the same
//! directive set that the live phase would carry out, run through the
//! [`halyard_git::tag::PreviewTagWriter`] — probes against the real ref
//! state (existing tags classify `AlreadyTagged`/conflict exactly like
//! the live run), mutations forecast deterministically, and **nothing is
//! written**. This is the `halyard release --dry` path.
//!
//! Run: `cargo run -p halyard-git --example tag_forecast`

use std::error::Error;

use halyard_core::PkgId;
use halyard_core::package::{
    Ecosystem, GroupId, Package, PathFilter, RelPath, TagTemplate, VersionSource, Workspace,
};
use halyard_core::planner::PlanConfig;
use halyard_core::release_state::{pending_releases, reconcile_tags};
use halyard_git::tag::{PreviewTagWriter, TagPhaseRow, directives_for_pending, run_tag_phase};
use halyard_git::{GitHistory, GitHistoryOptions};
use halyard_test_support::{CommitSpec, GitFixture};
use tempfile::TempDir;

fn main() -> Result<(), Box<dyn Error>> {
    // The post-merge state: `core` is tagged at 0.4.0 and its manifest
    // moved to 0.4.1 — the tag phase owes core-v0.4.1; `cli` is fully
    // tagged (the forecast shows its row as already tagged).
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
        ],
        "feat: bootstrap the workspace",
    ))?;
    fx.tag_annotated("core-v0.4.0", "core 0.4.0")?;
    fx.tag_annotated("cli-v1.0.0", "cli 1.0.0")?;
    fx.commit_with(&CommitSpec::files(
        vec![(
            "crates/core/Cargo.toml".to_owned(),
            "[package]\nname = \"core\"\nversion = \"0.4.1\"\nedition = \"2021\"\n".to_owned(),
        )],
        "chore(core): release v0.4.1",
    ))?;

    // The discovered workspace (built the way discovery would, keeping
    // the demo a leaf — no halyard-discover dependency).
    let mut ws = Workspace::new();
    let specs = [
        ("core", "crates/core", "0.4.1", "{name}-v{version}"),
        ("cli", "crates/cli", "1.0.0", "{name}-v{version}"),
    ];
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

    // Snapshot: tags through the history engine, then the §11.1 step-2
    // reconciliation (the same inputs the live run would use).
    let history = GitHistory::build_with_options(fx.root(), &ws, GitHistoryOptions::default())?;
    let tag_names: Vec<halyard_core::BString> =
        history.tags().iter().map(|tag| tag.name.clone()).collect();
    let cfg = PlanConfig::default_values();
    let rows = reconcile_tags(&ws, &tag_names, &cfg);
    println!("reconciliation:");
    for row in &rows {
        println!(
            "  {} {:?} (manifest {})",
            row.package, row.state, row.manifest_version
        );
    }
    let pending = pending_releases(&ws, &tag_names, &cfg);
    let head_oid = halyard_core::Oid::from_hex(head.as_bytes())
        .ok_or_else(|| "fixture produced a non-hex oid".to_owned())?;
    let directives = directives_for_pending(&pending, &head_oid);
    let writer = PreviewTagWriter::open(fx.root())?;
    let report = run_tag_phase(&writer, &directives);
    println!(
        "forecast: {} owed, {} would create, {} already tagged, {} failed (nothing written)",
        report.rows.len(),
        report.created,
        report.already_tagged,
        report.failed
    );
    for row in &report.rows {
        match row {
            TagPhaseRow::Applied { tag, outcome } => println!("  {tag}: {outcome:?}"),
            TagPhaseRow::Failed { tag, error } => println!("  {tag}: {error}"),
        }
    }
    println!("git tags after the forecast: {:?}", fx.ls_tags()?);
    Ok(())
}

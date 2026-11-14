//! Demos the §11.1 tag-target precision rule end to end: reconcile the
//! fixture workspace's manifest versions against its tags, then resolve
//! where each pending tag should land — the commit that *introduced* the
//! release version ([`halyard_git::tag_target::GixTagTargetResolver`],
//! a bounded first-parent walk on manifest blob-OID change with a
//! caller-supplied version probe) — and what happens when the version
//! was never committed (the documented HEAD fallback).
//!
//! The probe here is a minimal `Cargo.toml` text scan: in the shell the
//! probe dispatches over the ecosystem adapters instead — the git engine
//! never parses manifests itself.
//!
//! Run: `cargo run -p halyard-git --example tag_target`

use std::error::Error;

use halyard_core::HistoryProvider;
use halyard_core::PkgId;
use halyard_core::package::{
    Ecosystem, GroupId, Package, PathFilter, RelPath, TagTemplate, VersionSource, Workspace,
};
use halyard_core::planner::PlanConfig;
use halyard_core::release_state::{pending_releases, reconcile_tags};
use halyard_git::tag_target::{
    GixTagTargetResolver, ManifestVersionProbe, TagTargetSource, directives_for_pending_resolved,
};
use halyard_git::{GitHistory, GitHistoryOptions};
use halyard_test_support::{CommitSpec, GitFixture};
use tempfile::TempDir;

/// The example's probe: the minimal `version = "X"` line scan (the
/// fixture manifests are single-table).
struct CargoTextProbe;

impl ManifestVersionProbe for CargoTextProbe {
    fn manifest_version(&self, _ecosystem: &str, manifest: &[u8]) -> Option<String> {
        let text = std::str::from_utf8(manifest).ok()?;
        for line in text.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("version = \"") {
                let value = rest.strip_suffix('"')?;
                if !value.is_empty() {
                    return Some(value.to_owned());
                }
            }
            if line.starts_with('[') && !line.starts_with("[package]") {
                return None;
            }
        }
        None
    }
}

/// Builds the discovered workspace the reconciliation drives (two Cargo
/// packages: `app` at the repo root, `lib` under `lib/`).
fn discovered_workspace() -> Result<Workspace, Box<dyn Error>> {
    let mut ws = Workspace::new();
    let app_sym = ws.intern("app");
    let lib_sym = ws.intern("lib");
    for (sym, manifest, ver, tpl) in [
        (app_sym, "Cargo.toml", "0.2.0", "v{version}"),
        (lib_sym, "lib/Cargo.toml", "1.3.0", "lib-v{version}"),
    ] {
        let root = RelPath::new("").unwrap_or_else(|| unreachable!("valid RelPath"));
        let manifest = RelPath::new(manifest).unwrap_or_else(|| unreachable!("valid RelPath"));
        let version = semver::Version::parse(ver)?;
        let tag_template = TagTemplate::parse(tpl).map_err(|e| format!("bad template: {e}"))?;
        ws.push_package(Package {
            id: PkgId::FIRST,
            name: sym,
            ecosystem: Ecosystem::Cargo,
            root,
            manifest,
            version: Some(version),
            version_source: VersionSource::Local,
            publishable: true,
            deps: Vec::new(),
            tag_template,
            changelog: None,
            version_site: None,
            include: PathFilter::new(Vec::new(), Vec::new()),
            group: GroupId::new(0),
            facets: halyard_core::Facets::default(),
        });
    }
    Ok(ws)
}

/// Builds the fixture repo: `app` released at v0.1.0, bumped to 0.2.0 one
/// commit later (the pending release whose tag should target the bump
/// commit, not HEAD); `lib`'s manifest never leaves 1.2.0 — its 1.3.0
/// bump is worktree-only, i.e. never committed. Returns the temp dir (the
/// fixture borrows its path), the fixture and the bump commit's hex.
fn fixture() -> Result<(TempDir, GitFixture, String), Box<dyn Error>> {
    let dir = TempDir::new()?;
    let mut fx = GitFixture::init_in(dir.path())?;
    fx.commit_with(&CommitSpec::files(
        vec![
            (
                "Cargo.toml".to_owned(),
                "[package]\nname = \"app\"\nversion = \"0.1.0\"\n".to_owned(),
            ),
            (
                "src/lib.rs".to_owned(),
                "pub const APP: usize = 1;\n".to_owned(),
            ),
            (
                "lib/Cargo.toml".to_owned(),
                "[package]\nname = \"lib\"\nversion = \"1.2.0\"\n".to_owned(),
            ),
        ],
        "feat: scaffold app and lib",
    ))?;
    fx.tag_annotated("v0.1.0", "app 0.1.0")?;
    let bump = fx.commit_with(&CommitSpec::files(
        vec![(
            "Cargo.toml".to_owned(),
            "[package]\nname = \"app\"\nversion = \"0.2.0\"\n".to_owned(),
        )],
        "chore: release app 0.2.0",
    ))?;
    fx.commit_with(&CommitSpec::files(
        vec![(
            "src/lib.rs".to_owned(),
            "pub const APP: usize = 2;\n".to_owned(),
        )],
        "feat: grow app",
    ))?;
    Ok((dir, fx, bump))
}

fn main() -> Result<(), Box<dyn Error>> {
    let (_dir, fx, bump) = fixture()?;
    let ws = discovered_workspace()?;

    // Snapshot the tags through the history engine (the shell's source of
    // tag names and release bases).
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
    let head = history.head_oid().ok_or("fixture without HEAD")?;

    // The tag-target precision rule: where each tag lands.
    let resolver = GixTagTargetResolver::open(fx.root(), Box::new(CargoTextProbe))?;
    let queue = directives_for_pending_resolved(&pending, &ws, history.digest(), &head, &resolver);
    println!("resolved tag targets (HEAD = {head}):");
    for rd in &queue {
        let source = match rd.source {
            TagTargetSource::ManifestPinpoint => "manifest pinpoint",
            TagTargetSource::HeadFallback => "HEAD fallback",
        };
        println!(
            "  {} -> {} ({source})",
            rd.directive.name, rd.directive.target
        );
    }
    let app = queue
        .iter()
        .find(|rd| rd.directive.name == "v0.2.0")
        .ok_or("missing app directive")?;
    let bump_oid = halyard_git::gix::hash::ObjectId::from_hex(bump.as_bytes())
        .map_err(|e| format!("fixture produced a non-hex oid: {e}"))?;
    if app.directive.target != bump_oid {
        return Err("app's tag did not target its bump commit".into());
    }
    println!("the tag targets the commit that introduced 0.2.0: {bump} (not HEAD)");
    if queue
        .iter()
        .any(|rd| rd.directive.name == "lib-v1.3.0" && rd.source == TagTargetSource::HeadFallback)
    {
        println!(
            "lib's never-committed version falls back to HEAD — the phase still runs, honestly"
        );
    } else {
        return Err("lib did not fall back to HEAD".into());
    }
    Ok(())
}

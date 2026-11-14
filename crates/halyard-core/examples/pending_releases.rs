//! Demos the §11.1 step-2 stateless reconciliation
//! ([`halyard_core::release_state`]): the manifest-vs-tags decision
//! behind the forge machine's tag phase, as a pure function over a
//! workspace and a tag table.
//!
//! Run: `cargo run -p halyard-core --example pending_releases`

use std::error::Error;

use halyard_core::package::{
    Ecosystem, GroupId, Package, PathFilter, RelPath, TagTemplate, VersionSource, Workspace,
};
use halyard_core::planner::PlanConfig;
use halyard_core::release_state::{pending_releases, reconcile_tags};
use halyard_core::{BString, PkgId};

fn main() -> Result<(), Box<dyn Error>> {
    // A two-package workspace in the post-merge Release-PR state: `core`
    // already carries v0.4.1 (tagged), `cli` was bumped to 1.0.0 without
    // ever being tagged.
    let mut ws = Workspace::new();
    for (name, version) in [("core", "0.4.1"), ("cli", "1.0.0")] {
        let pkg = Package {
            id: PkgId::FIRST,
            name: ws.intern(name),
            ecosystem: Ecosystem::Cargo,
            root: RelPath::new(&format!("crates/{name}")).ok_or("invalid root")?,
            manifest: RelPath::new(&format!("crates/{name}/Cargo.toml"))
                .ok_or("invalid manifest")?,
            version: Some(semver::Version::parse(version)?),
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
        ws.push_package(pkg);
    }
    // The repository's tag names, as `halyard_git::refs::enumerate`
    // yields them (short, `refs/tags/`-stripped, name-sorted).
    let tags = vec![
        BString::from("cli-v0.9.3"),
        BString::from("core-v0.4.1"),
        BString::from("core-v0.4.0"),
        BString::from("v0.0.1-noise"),
    ];
    let cfg = PlanConfig::default_values();

    println!("manifest-vs-tags reconciliation (§11.1 step 2):");
    for state in reconcile_tags(&ws, &tags, &cfg) {
        let latest = state
            .latest_version
            .as_ref()
            .map_or_else(|| "no tag yet".to_owned(), ToString::to_string);
        println!(
            "  {:<5} manifest {} latest tag {} ⇒ {:?}",
            state.package, state.manifest_version, latest, state.state
        );
    }
    println!("\ntag-phase queue (what the forge machine owes):");
    for release in pending_releases(&ws, &tags, &cfg) {
        println!(
            "  create release {} ({} {})",
            release.tag, release.package, release.version
        );
    }
    println!("\nevery Tagged row is a re-run no-op (I1); a Behind row is");
    println!("reported but never tagged downward (§11.1 step 2).");
    Ok(())
}

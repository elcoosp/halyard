//! The §8.8 smart-minimal-fetch planner, end to end on a scripted
//! remote listing: per-package selection ([`latest_tags_for`]), the
//! deepen boundaries ([`deepen_boundaries_for`]) and the composed fetch
//! argv ([`plan_fetch`] / [`fetch_attempts`]) for both repository
//! shapes (complete = no shallow machinery, shallow = the deepen
//! boundary per package).
//!
//! This example touches no repository and no network (I5) — the live
//! effect lives in [`halyard_git::fetch::GixTagFetcher::open`] and is
//! exercised by `halyard fetch` and the `tests/fetch.rs` suite.

use std::collections::BTreeMap;

use halyard_core::PkgId;
use halyard_core::package::{
    DepEdge, Ecosystem, GroupId, Package, PathFilter, RelPath, TagTemplate, VersionSource,
    Workspace,
};
use halyard_core::planner::PlanConfig;
use halyard_git::fetch::{deepen_boundaries_for, fetch_attempts, latest_tags_for, plan_fetch};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // A two-package workspace: `app` (template `v{version}`) and `lib`
    // (template `lib-v{version}`).
    let mut ws = Workspace::new();
    for (name, template) in [("app", "v{version}"), ("lib", "lib-v{version}")] {
        let name_sym = ws.intern(name);
        ws.push_package(Package {
            id: PkgId::FIRST,
            name: name_sym,
            ecosystem: Ecosystem::Cargo,
            root: RelPath::new("").ok_or("the empty root is a valid RelPath")?,
            manifest: RelPath::new(&format!("{name}/Cargo.toml"))
                .ok_or("the manifest path is a valid RelPath")?,
            version: Some(semver::Version::new(0, 1, 0)),
            version_source: VersionSource::Local,
            publishable: true,
            deps: Vec::<DepEdge>::new(),
            tag_template: TagTemplate::parse(template).map_err(|e| format!("template: {e}"))?,
            changelog: None,
            version_site: None,
            include: PathFilter::new(Vec::new(), Vec::new()),
            group: GroupId::new(0),
            facets: halyard_core::Facets::default(),
        });
    }

    // The `git ls-remote --tags origin` listing (the wire format the
    // live fetcher parses): five releases for `app`, two for `lib`,
    // plus foreign and invalid tags the matcher must ignore.
    let listing: BTreeMap<String, String> = [
        "v0.1.0",
        "v0.2.0",
        "v0.3.0",
        "lib-v1.2.0",
        "lib-v1.3.0",
        "not-a-version",
        "other-v9.9.9",
    ]
    .into_iter()
    .map(|tag| {
        (
            tag.to_owned(),
            "0123456789abcdef0123456789abcdef01234567".to_owned(),
        )
    })
    .collect();

    let selection = latest_tags_for(&ws, &PlanConfig::default_values(), &listing);
    println!("selection (package → requested tag):");
    for (package, tag) in &selection {
        println!("  {package} → {tag}");
    }

    let boundaries = deepen_boundaries_for(&ws, &PlanConfig::default_values(), &listing);
    println!("deepen boundaries (previous release per package): {boundaries:?}");

    let complete = plan_fetch("origin", Some("main"), &selection, &boundaries, false);
    println!("\ncomplete clone — minimal fetch argv:");
    println!("  git {}", complete.argv.join(" "));

    let shallow = plan_fetch("origin", Some("main"), &selection, &boundaries, true);
    println!("\nshallow clone — minimal fetch argv (bounded by the previous releases):");
    println!("  git {}", shallow.argv.join(" "));

    let tags: Vec<String> = selection.values().cloned().collect();
    let attempts = fetch_attempts("origin", Some("main"), &tags, &boundaries, true);
    println!("\nattempt sequence (§8.8 step 3: minimal, then the full-fetch fallback):");
    for attempt in &attempts {
        println!("  [{:?}] git {}", attempt.command, attempt.argv.join(" "));
    }
    println!(
        "\nlive effect: halyard fetch (or GixTagFetcher::open(repo) — probes, selects, \
         classifies, fetches once, falls back once; idempotent on re-run, I1)"
    );
    Ok(())
}

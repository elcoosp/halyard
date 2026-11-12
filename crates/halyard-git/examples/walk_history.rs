//! Painter-walk demo (M2.2, §8.3): opens the repository at HEAD, resolves
//! the release base for every package named on the command line, paints
//! the unreleased history and prints the emitted commits with their
//! per-package bitsets.
//!
//! Run (inside any git repository):
//!
//! ```sh
//! cargo run -p halyard-git --example walk_history -- . app
//! ```

use std::error::Error;

use gix::hash::ObjectId;
use halyard_core::package::{
    Ecosystem, GroupId, Package, PathFilter, RelPath, TagTemplate, VersionSource, Workspace,
};
use halyard_core::PkgId;
use halyard_git::refs::Generations;
use halyard_git::GitHistory;

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let Some(path) = args.next() else {
        return Err("usage: walk_history <repo-path> [package-name…]".into());
    };
    let names: Vec<String> = args.collect();
    let names = if names.is_empty() {
        vec!["app".to_owned()]
    } else {
        names
    };

    // A one-package-per-name workspace with the `{name}-v{version}`
    // default template (appendix A.3).
    let mut ws = Workspace::new();
    for name in &names {
        let pkg = Package {
            id: PkgId::FIRST,
            name: ws.intern(name),
            ecosystem: Ecosystem::Cargo,
            root: RelPath::new("").unwrap(),
            manifest: RelPath::new("Cargo.toml").unwrap(),
            version: None,
            version_source: VersionSource::Local,
            publishable: true,
            deps: Vec::new(),
            tag_template: TagTemplate::parse("{name}-v{version}")?,
            changelog: None,
            version_site: None,
            include: PathFilter::new(Vec::new(), Vec::new()),
            group: GroupId::new(0),
            facets: halyard_core::Facets::default(),
        };
        ws.push_package(pkg);
    }

    let history = GitHistory::build(std::path::Path::new(&path), &ws, 10_000)?;
    println!("head: {}", history.head_oid().map(|o| o.to_string()).unwrap_or_default());
    println!("degraded (no commit-graph): {}", history.degraded);
    println!("truncated (initial bound): {}", history.truncated);
    for pkg in ws.packages() {
        let name = ws.interner().get(pkg.name).unwrap_or_default();
        match history.digest().base_of(pkg.id) {
            Some(base) => println!("base[{name}] = {} ({})", base.tag, base.oid),
            None => println!("base[{name}] = (initial release)"),
        }
        let commits = history.digest().commits_for(pkg.id);
        println!("unreleased commits for {name}: {}", commits.len());
    }

    // Raw paint view: emitted commits with their package bitsets.
    let repo = halyard_git::gix::open(&path)?;
    let head: ObjectId = repo.head_id()?.detach();
    let mut generations = Generations::new(&repo);
    let base_oids: Vec<Option<ObjectId>> = history
        .bases()
        .iter()
        .map(|b| b.as_ref().and_then(|base| ObjectId::from_hex(base.oid.as_bytes()).ok()))
        .collect();
    let painted = halyard_git::walk::paint(&repo, head, &base_oids, 10_000, &mut generations)?;
    println!("painted commits (fresh for some package): {}", painted.commits.len());
    for commit in painted.commits.iter().take(20) {
        let packages: Vec<String> = commit
            .new_for
            .iter()
            .map(|id| id.to_u32().to_string())
            .collect();
        println!("  {} gen={} new_for=[{}]", commit.oid, commit.generation, packages.join(","));
    }
    Ok(())
}

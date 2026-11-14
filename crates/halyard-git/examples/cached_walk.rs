//! Demos the §8.7 persistent per-commit cache through the full
//! [`GitHistory::build_with_options`] pipeline: the first walk populates
//! the cache under the fixture repo's `$GIT_DIR/halyard/cache/v1/`, the
//! second walk consults it (tree-diff attribution comes from records —
//! byte-identical digest, §8.7).
//!
//! Run: `cargo run -p halyard-git --example cached_walk`

use std::error::Error;

use halyard_core::HistoryProvider as _;
use halyard_core::PkgId;
use halyard_core::package::{
    Ecosystem, GroupId, Package, PathFilter, RelPath, TagTemplate, VersionSource, Workspace,
};
use halyard_git::{GitHistory, GitHistoryOptions};
use halyard_test_support::{CommitSpec, GitFixture};
use tempfile::TempDir;

fn main() -> Result<(), Box<dyn Error>> {
    // A deterministic fixture repo: two packages, release tags, then one
    // feat commit per package since the tags.
    let dir = TempDir::new()?;
    let mut fx = GitFixture::init_in(dir.path())?;
    fx.commit_with(&CommitSpec::files(
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
        "feat: scaffold the workspace",
    ))?;
    fx.tag_annotated("core-v0.4.0", "core v0.4.0")?;
    fx.tag_annotated("cli-v1.0.0", "cli v1.0.0")?;
    fx.commit_with(&CommitSpec::files(
        vec![(
            "crates/core/src/lib.rs".to_owned(),
            "pub const CORE: usize = 2;\n".to_owned(),
        )],
        "feat(core): widen the constant",
    ))?;
    fx.commit_with(&CommitSpec::files(
        vec![(
            "crates/cli/src/main.rs".to_owned(),
            "fn main() {\n    println!(\"hi\");\n}\n".to_owned(),
        )],
        "feat(cli): greet the user",
    ))?;

    let ws = fixture_workspace()?;
    // The two §8.7 cache-key components the caller owns: the trie layout
    // hash (package roots + include filters) and the config hash. Any
    // change to either invalidates the cached attributions cleanly.
    let filters: Vec<(String, Vec<String>, Vec<String>)> = ws
        .packages()
        .iter()
        .map(|pkg| {
            (
                pkg.root.as_str().to_owned(),
                pkg.include
                    .include()
                    .iter()
                    .map(ToString::to_string)
                    .collect(),
                pkg.include
                    .exclude()
                    .iter()
                    .map(ToString::to_string)
                    .collect(),
            )
        })
        .collect();
    let layout_hash = halyard_core::trie::layout_hash(&ws, &filters);
    let config_hash = "example-default-config";

    // Cold run: the cache is empty, every commit's attribution comes
    // from the trie-guided tree diff.
    let cold = GitHistory::build_with_options(
        fx.root(),
        &ws,
        GitHistoryOptions {
            initial_max_commits: 10_000,
            cache: Some(cache_open(&fx)?),
            layout_hash: layout_hash.clone(),
            config_hash: config_hash.to_owned(),
            ..GitHistoryOptions::default()
        },
    )?;
    println!(
        "cold run: {} commits; trees read {}, entries read {}, memo hits {}",
        cold.digest().commits.len(),
        cold.tree_diff_stats.trees_read,
        cold.tree_diff_stats.entries_read,
        cold.tree_diff_stats.memo_hits
    );

    // Warm run: same layout + config ⇒ same keys ⇒ the cache answers
    // before the tree diff and the digest is identical (I3).
    let warm = GitHistory::build_with_options(
        fx.root(),
        &ws,
        GitHistoryOptions {
            initial_max_commits: 10_000,
            cache: Some(cache_open(&fx)?),
            layout_hash,
            config_hash: config_hash.to_owned(),
            ..GitHistoryOptions::default()
        },
    )?;
    let same_commits = warm.digest().commits.len() == cold.digest().commits.len();
    println!(
        "warm run: {} commits; digest commit count identical: {same_commits}",
        warm.digest().commits.len()
    );
    println!(
        "cache file: {}",
        fx.git_dir().join("halyard/cache/v1/cache.bin").display()
    );
    Ok(())
}

/// Builds the two-package workspace the fixture commits (the same shapes
/// discovery produces for these manifests).
fn fixture_workspace() -> Result<Workspace, Box<dyn Error>> {
    let mut ws = Workspace::new();
    for (name, root, manifest, version) in [
        ("core", "crates/core", "crates/core/Cargo.toml", "0.4.0"),
        ("cli", "crates/cli", "crates/cli/Cargo.toml", "1.0.0"),
    ] {
        let pkg = Package {
            id: PkgId::FIRST,
            name: ws.intern(name),
            ecosystem: Ecosystem::Cargo,
            root: RelPath::new(root).ok_or("invalid fixture root")?,
            manifest: RelPath::new(manifest).ok_or("invalid fixture manifest")?,
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
    Ok(ws)
}

/// Opens the fixture repo's §8.7 cache (the same location the CLI uses:
/// `$GIT_DIR/halyard/cache/v1/`).
fn cache_open(fx: &GitFixture) -> Result<halyard_cache::Cache, Box<dyn Error>> {
    Ok(halyard_cache::Cache::open(&fx.git_dir())?)
}

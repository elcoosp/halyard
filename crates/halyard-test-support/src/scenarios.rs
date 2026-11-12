//! Named fixture scenarios (M0.5: 20 scenarios incl. shallow/partial).
//!
//! Each function deterministically builds a repository shape that the git
//! engine (`halyard-git`, M2) must handle: linear history, merge-heavy
//! histories, octopus merges, squash-style subjects, reverts, fixups,
//! shallow and partial clones, commit-graph presence, packed refs, linked
//! worktrees, assorted tag-template sets and non-conventional subjects.

use std::fmt::Write as _;

use crate::error::FixtureError;
use crate::fixture::{CommitSpec, GitFixture};

/// `[package]` manifest body for a crate at `<name>/Cargo.toml`.
fn cargo_pkg(name: &str, version: &str) -> String {
    format!("[package]\nname = \"{name}\"\nversion = \"{version}\"\nedition = \"2021\"\n")
}

/// Virtual workspace manifest listing the given member paths.
fn cargo_workspace(members: &[&str]) -> String {
    let mut list = String::new();
    for m in members {
        let _ = writeln!(list, "    \"{m}\",");
    }
    format!("[workspace]\nresolver = \"2\"\nmembers = [\n{list}\n]\n")
}

/// A tiny `src/lib.rs` whose content is unique per line count, so commits are
/// never empty.
fn lib_rs(lines: usize) -> String {
    let mut src = String::new();
    for i in 0..lines {
        let _ = writeln!(src, "pub const L{i}: usize = {i};");
    }
    src
}

/// 1. `git init` only: unborn branch, no commits.
pub fn empty() -> Result<GitFixture, FixtureError> {
    GitFixture::new("empty")
}

/// 2. Single package, exactly one commit.
pub fn single_commit() -> Result<GitFixture, FixtureError> {
    let mut fx = GitFixture::new("single-commit")?;
    fx.commit_with(&CommitSpec::files(
        vec![
            ("Cargo.toml".to_owned(), cargo_pkg("app", "0.1.0")),
            ("src/lib.rs".to_owned(), lib_rs(1)),
        ],
        "feat: initial app",
    ))?;
    Ok(fx)
}

/// 3. Linear history (5 commits) on a single package.
pub fn linear_single_package() -> Result<GitFixture, FixtureError> {
    let mut fx = GitFixture::new("linear-single")?;
    fx.commit_with(&CommitSpec::files(
        vec![("Cargo.toml".to_owned(), cargo_pkg("app", "0.1.0"))],
        "feat: scaffold app",
    ))?;
    for i in 1..5usize {
        let subject = match i % 3 {
            0 => "fix: correct parser edge",
            1 => "feat: extend output",
            _ => "perf: faster scan",
        };
        fx.commit_with(&CommitSpec::files(
            vec![("src/lib.rs".to_owned(), lib_rs(i * 4 + 1))],
            subject,
        ))?;
    }
    Ok(fx)
}

/// Scenario 4: linear history over a two-crate Cargo workspace (virtual
/// root), with a root `Cargo.lock` touched part-way.
pub fn linear_cargo_workspace() -> Result<GitFixture, FixtureError> {
    let mut fx = GitFixture::new("linear-ws")?;
    fx.commit_with(&CommitSpec::files(
        vec![
            (
                "Cargo.toml".to_owned(),
                cargo_workspace(&["crates/a", "crates/b"]),
            ),
            ("crates/a/Cargo.toml".to_owned(), cargo_pkg("a", "0.1.0")),
            ("crates/a/src/lib.rs".to_owned(), lib_rs(1)),
            ("crates/b/Cargo.toml".to_owned(), cargo_pkg("b", "0.2.0")),
            ("crates/b/src/lib.rs".to_owned(), lib_rs(1)),
            ("Cargo.lock".to_owned(), "# generated\n".to_owned()),
        ],
        "feat: scaffold workspace",
    ))?;
    fx.commit_with(&CommitSpec::files(
        vec![("crates/a/src/lib.rs".to_owned(), lib_rs(5))],
        "feat(a): grow a",
    ))?;
    fx.commit_with(&CommitSpec::files(
        vec![("crates/b/src/lib.rs".to_owned(), lib_rs(5))],
        "fix(b): mend b",
    ))?;
    fx.commit_with(&CommitSpec::files(
        vec![
            ("Cargo.lock".to_owned(), "# generated v2\n".to_owned()),
            ("crates/a/src/lib.rs".to_owned(), lib_rs(9)),
        ],
        "feat(a): grow a again",
    ))?;
    fx.commit_with(&CommitSpec::files(
        vec![("docs/README.md".to_owned(), "# docs\n".to_owned())],
        "docs: write readme",
    ))?;
    Ok(fx)
}

/// 5. Two branches with independent commits merged with `--no-ff`.
pub fn merge_two_branches() -> Result<GitFixture, FixtureError> {
    let mut fx = GitFixture::new("merge-two")?;
    fx.commit_with(&CommitSpec::files(
        vec![("Cargo.toml".to_owned(), cargo_pkg("app", "0.1.0"))],
        "feat: base",
    ))?;
    fx.checkout_new("feature", None)?;
    fx.commit_with(&CommitSpec::files(
        vec![("src/feature.rs".to_owned(), lib_rs(1))],
        "feat(feature): first on branch",
    ))?;
    fx.commit_with(&CommitSpec::files(
        vec![("src/feature.rs".to_owned(), lib_rs(2))],
        "feat(feature): second on branch",
    ))?;
    fx.checkout("main")?;
    fx.commit_with(&CommitSpec::files(
        vec![("src/main.rs".to_owned(), lib_rs(1))],
        "fix: parallel main commit",
    ))?;
    fx.merge_no_ff("feature", "Merge branch 'feature'")?;
    Ok(fx)
}

/// 6. Octopus merge: three branches merged in a single commit.
pub fn octopus_merge() -> Result<GitFixture, FixtureError> {
    let mut fx = GitFixture::new("octopus")?;
    fx.commit_with(&CommitSpec::files(
        vec![("Cargo.toml".to_owned(), cargo_pkg("app", "0.1.0"))],
        "feat: base",
    ))?;
    for name in ["b1", "b2", "b3"] {
        fx.checkout_new(name, Some("main"))?;
        fx.commit_with(&CommitSpec::files(
            vec![(format!("src/{name}.rs"), lib_rs(1))],
            format!("feat({name}): branch work"),
        ))?;
    }
    fx.checkout("main")?;
    fx.merge_octopus(&["b1", "b2", "b3"], "Merge branches 'b1', 'b2', 'b3'")?;
    Ok(fx)
}

/// Scenario 7: squash-merge style history. Every change is one commit with a
/// trailing `(#NNN)` PR reference.
pub fn squash_style_history() -> Result<GitFixture, FixtureError> {
    let mut fx = GitFixture::new("squash-style")?;
    fx.commit_with(&CommitSpec::files(
        vec![("Cargo.toml".to_owned(), cargo_pkg("app", "0.1.0"))],
        "feat: initial (#100)",
    ))?;
    fx.commit_with(&CommitSpec::files(
        vec![("src/api.rs".to_owned(), lib_rs(1))],
        "feat(api): add endpoint (#101)",
    ))?;
    fx.commit_with(&CommitSpec::files(
        vec![("src/api.rs".to_owned(), lib_rs(2))],
        "fix(api): handle nil (#102)",
    ))?;
    fx.commit_with(&CommitSpec::files(
        vec![("src/cli.rs".to_owned(), lib_rs(1))],
        "feat(cli): add flag (#103)",
    ))?;
    Ok(fx)
}

/// 8. A feat commit followed by its canonical revert.
pub fn revert_commit() -> Result<GitFixture, FixtureError> {
    let mut fx = GitFixture::new("revert")?;
    fx.commit_with(&CommitSpec::files(
        vec![("Cargo.toml".to_owned(), cargo_pkg("app", "0.1.0"))],
        "feat: base",
    ))?;
    let reverted = fx.commit_with(&CommitSpec::files(
        vec![("src/gone.rs".to_owned(), lib_rs(1))],
        "feat: add thing",
    ))?;
    fx.commit_with(&CommitSpec::files(
        vec![("src/other.rs".to_owned(), lib_rs(1))],
        "fix: unrelated fix",
    ))?;
    fx.revert(&reverted)?;
    Ok(fx)
}

/// 9. `fixup!` / `squash!` autosquash commits interleaved with real ones.
pub fn fixup_commits() -> Result<GitFixture, FixtureError> {
    let mut fx = GitFixture::new("fixup")?;
    fx.commit_with(&CommitSpec::files(
        vec![("Cargo.toml".to_owned(), cargo_pkg("app", "0.1.0"))],
        "feat: base",
    ))?;
    fx.commit_with(&CommitSpec::files(
        vec![("src/lib.rs".to_owned(), lib_rs(1))],
        "feat: add feature",
    ))?;
    fx.commit_with(&CommitSpec::files(
        vec![("src/lib.rs".to_owned(), lib_rs(2))],
        "fixup! feat: add feature",
    ))?;
    fx.commit_with(&CommitSpec::files(
        vec![("src/lib.rs".to_owned(), lib_rs(3))],
        "squash! feat: add feature",
    ))?;
    fx.commit_with(&CommitSpec::files(
        vec![("src/other.rs".to_owned(), lib_rs(1))],
        "fix: other fix",
    ))?;
    Ok(fx)
}

/// 10. Annotated tags (with messages) alongside a lightweight tag.
pub fn annotated_tags() -> Result<GitFixture, FixtureError> {
    let mut fx = GitFixture::new("annotated-tags")?;
    fx.commit_with(&CommitSpec::files(
        vec![("Cargo.toml".to_owned(), cargo_pkg("app", "0.1.0"))],
        "feat: first",
    ))?;
    fx.tag_annotated("v0.1.0", "Release 0.1.0")?;
    fx.commit_with(&CommitSpec::files(
        vec![("src/lib.rs".to_owned(), lib_rs(1))],
        "feat: second",
    ))?;
    fx.tag_annotated("v0.2.0", "Release 0.2.0")?;
    fx.tag_light("nightly-marker")?;
    Ok(fx)
}

/// 11. Refs stored in `packed-refs` (after `git pack-refs --all`).
pub fn packed_refs() -> Result<GitFixture, FixtureError> {
    let mut fx = GitFixture::new("packed-refs")?;
    fx.commit_with(&CommitSpec::files(
        vec![("Cargo.toml".to_owned(), cargo_pkg("app", "0.1.0"))],
        "feat: first",
    ))?;
    fx.tag_light("v0.1.0")?;
    fx.commit_with(&CommitSpec::files(
        vec![("src/lib.rs".to_owned(), lib_rs(1))],
        "feat: second",
    ))?;
    fx.tag_annotated("v0.2.0", "packed too")?;
    fx.pack_refs()?;
    Ok(fx)
}

/// Scenario 12: a release tag that is **not** an ancestor of HEAD (base
/// resolution must reject it).
pub fn tag_on_side_branch() -> Result<GitFixture, FixtureError> {
    let mut fx = GitFixture::new("tag-side-branch")?;
    fx.commit_with(&CommitSpec::files(
        vec![("Cargo.toml".to_owned(), cargo_pkg("app", "0.1.0"))],
        "feat: base",
    ))?;
    fx.checkout_new("side", None)?;
    fx.commit_with(&CommitSpec::files(
        vec![("src/side.rs".to_owned(), lib_rs(1))],
        "feat(side): side work",
    ))?;
    fx.tag_light("v0.1.0")?;
    fx.checkout("main")?;
    fx.commit_with(&CommitSpec::files(
        vec![("src/main.rs".to_owned(), lib_rs(1))],
        "feat: main work",
    ))?;
    Ok(fx)
}

/// 13. Mixed tag states: one crate tagged long ago, one never tagged.
pub fn mixed_tag_states() -> Result<GitFixture, FixtureError> {
    let mut fx = GitFixture::new("mixed-tags")?;
    fx.commit_with(&CommitSpec::files(
        vec![
            (
                "Cargo.toml".to_owned(),
                cargo_workspace(&["crates/a", "crates/b"]),
            ),
            ("crates/a/Cargo.toml".to_owned(), cargo_pkg("a", "0.1.0")),
            ("crates/a/src/lib.rs".to_owned(), lib_rs(1)),
            ("crates/b/Cargo.toml".to_owned(), cargo_pkg("b", "0.1.0")),
            ("crates/b/src/lib.rs".to_owned(), lib_rs(1)),
        ],
        "feat: scaffold a and b",
    ))?;
    fx.tag_annotated("a-v0.1.0", "Release a 0.1.0")?;
    fx.commit_with(&CommitSpec::files(
        vec![("crates/b/src/lib.rs".to_owned(), lib_rs(4))],
        "feat(b): grow b",
    ))?;
    fx.commit_with(&CommitSpec::files(
        vec![("crates/a/src/lib.rs".to_owned(), lib_rs(4))],
        "feat(a): grow a",
    ))?;
    Ok(fx)
}

/// 14. Shallow clone (`--depth=2`) of a linear history.
pub fn shallow_history() -> Result<GitFixture, FixtureError> {
    let src = linear_single_package()?;
    src.shallow_clone(2)
}

/// 15. Partial clone (`--filter=blob:none`) of a linear history.
pub fn partial_blobless_clone() -> Result<GitFixture, FixtureError> {
    let mut src = linear_single_package()?;
    src.enable_filter_serving()?;
    src.partial_clone()
}

/// 16. Commit-graph file present, with Bloom changed-path filters.
pub fn commit_graph_present() -> Result<GitFixture, FixtureError> {
    let mut fx = GitFixture::new("commit-graph")?;
    fx.commit_with(&CommitSpec::files(
        vec![("Cargo.toml".to_owned(), cargo_pkg("app", "0.1.0"))],
        "feat: first",
    ))?;
    for i in 1..6usize {
        fx.commit_with(&CommitSpec::files(
            vec![("src/lib.rs".to_owned(), lib_rs(i * 3 + 1))],
            if i % 2 == 0 {
                "fix: mend"
            } else {
                "feat: grow"
            },
        ))?;
    }
    fx.tag_light("v0.1.0")?;
    fx.commit_graph_write(true)?;
    Ok(fx)
}

/// 17. A linked worktree checked out on a branch.
pub fn linked_worktree() -> Result<GitFixture, FixtureError> {
    let mut fx = GitFixture::new("worktree")?;
    fx.commit_with(&CommitSpec::files(
        vec![("Cargo.toml".to_owned(), cargo_pkg("app", "0.1.0"))],
        "feat: base",
    ))?;
    let wt = fx.worktree_add("release-branch")?;
    let exists = wt.join(".git").exists();
    if !exists {
        return Err(FixtureError::scenario(format!(
            "worktree {} was not created",
            wt.display()
        )));
    }
    Ok(fx)
}

/// Scenario 18: a package set exercising assorted tag templates — crate
/// tags, an npm-scoped tag, and a shared root tag.
pub fn multi_package_tag_set() -> Result<GitFixture, FixtureError> {
    let mut fx = GitFixture::new("tag-set")?;
    fx.commit_with(&CommitSpec::files(
        vec![
            (
                "Cargo.toml".to_owned(),
                cargo_workspace(&["crates/core", "crates/cli"]),
            ),
            (
                "crates/core/Cargo.toml".to_owned(),
                cargo_pkg("core", "0.4.1"),
            ),
            ("crates/core/src/lib.rs".to_owned(), lib_rs(1)),
            (
                "crates/cli/Cargo.toml".to_owned(),
                cargo_pkg("cli", "1.0.3"),
            ),
            ("crates/cli/src/lib.rs".to_owned(), lib_rs(1)),
        ],
        "feat: scaffold core and cli",
    ))?;
    fx.tag_annotated("core-v0.4.1", "core release")?;
    fx.tag_annotated("cli-v1.0.3", "cli release")?;
    fx.tag_light("@scope/web@2.3.4")?;
    fx.commit_with(&CommitSpec::files(
        vec![("crates/core/src/lib.rs".to_owned(), lib_rs(6))],
        "feat(core): grow core",
    ))?;
    Ok(fx)
}

/// 19. Subjects that do not follow conventional-commit form at all.
pub fn non_conventional_subjects() -> Result<GitFixture, FixtureError> {
    let mut fx = GitFixture::new("non-conv")?;
    fx.commit_with(&CommitSpec::files(
        vec![("Cargo.toml".to_owned(), cargo_pkg("app", "0.1.0"))],
        "update stuff",
    ))?;
    fx.commit_with(&CommitSpec::files(
        vec![("src/lib.rs".to_owned(), lib_rs(1))],
        "WIP",
    ))?;
    fx.commit_with(&CommitSpec::files(
        vec![("src/lib.rs".to_owned(), lib_rs(2))],
        "changes",
    ))?;
    Ok(fx)
}

/// Scenario 20: subjects with multibyte content (CJK, emoji) exercising
/// byte-scan paths (`(#N)` extraction, trailer parsing) under UTF-8.
pub fn unicode_subjects() -> Result<GitFixture, FixtureError> {
    let mut fx = GitFixture::new("unicode")?;
    fx.commit_with(&CommitSpec::files(
        vec![("Cargo.toml".to_owned(), cargo_pkg("app", "0.1.0"))],
        "feat: 基础结构 ✨",
    ))?;
    fx.commit_with(&CommitSpec::files(
        vec![("src/解析.rs".to_owned(), lib_rs(1))],
        "feat(解析器): 添加中文支持 (#42)",
    ))?;
    fx.commit_with(&CommitSpec::files(
        vec![("src/lib.rs".to_owned(), lib_rs(1))],
        "fix: 修复崩溃 🐛",
    ))?;
    Ok(fx)
}

/// Builder function for one named scenario.
pub type ScenarioBuilder = fn() -> Result<GitFixture, FixtureError>;

/// All scenarios as `(name, builder)` pairs, in documentation order.
pub fn all() -> Vec<(&'static str, ScenarioBuilder)> {
    vec![
        ("empty", empty),
        ("single_commit", single_commit),
        ("linear_single_package", linear_single_package),
        ("linear_cargo_workspace", linear_cargo_workspace),
        ("merge_two_branches", merge_two_branches),
        ("octopus_merge", octopus_merge),
        ("squash_style_history", squash_style_history),
        ("revert_commit", revert_commit),
        ("fixup_commits", fixup_commits),
        ("annotated_tags", annotated_tags),
        ("packed_refs", packed_refs),
        ("tag_on_side_branch", tag_on_side_branch),
        ("mixed_tag_states", mixed_tag_states),
        ("shallow_history", shallow_history),
        ("partial_blobless_clone", partial_blobless_clone),
        ("commit_graph_present", commit_graph_present),
        ("linked_worktree", linked_worktree),
        ("multi_package_tag_set", multi_package_tag_set),
        ("non_conventional_subjects", non_conventional_subjects),
        ("unicode_subjects", unicode_subjects),
    ]
}

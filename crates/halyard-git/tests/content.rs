//! The §11.1 step-3 content machinery (the M4.2 remainder), pinned
//! against real `git` ground truth — exactly like the other git-engine
//! effect paths:
//!
//! * [`content_tree_oid`] — the applied-EditSet tree hash is
//!   differential-tested against `git add -A` + `git write-tree` over
//!   the same payloads (nested paths, executable entries, fresh files),
//!   is deterministic, and never writes an object (the working tree and
//!   the object database stay untouched);
//! * [`remote_release_tip`] — the release-branch probe over a bare
//!   origin: absent branch → `None`, pushed tip → the commit and its
//!   tree as `git rev-parse` reports them, unconfigured remote → the
//!   honest no-op.

use halyard_git::content::{RemoteTip, content_tree_oid, remote_release_tip};
use halyard_test_support::GitFixture;

type BoxError = Box<dyn std::error::Error>;

const ROOT_MANIFEST: &str = "[package]\nname = \"app\"\nversion = \"0.1.0\"\n";
const LIB_MANIFEST: &str = "[package]\nname = \"lib\"\nversion = \"0.1.0\"\n";
const README: &str = "# fixture\n";
const NEW_MANIFEST: &str = "[package]\nname = \"app\"\nversion = \"0.2.0\"\n";
const NEW_LIB_MANIFEST: &str = "[package]\nname = \"lib\"\nversion = \"0.2.0\"\n";

/// A fixture with a nested workspace shape committed: `Cargo.toml`,
/// `crates/lib/Cargo.toml`, `README.md`, and an executable script.
fn nested_fixture(label: &str) -> Result<GitFixture, BoxError> {
    let mut fx = GitFixture::new(label)?;
    fx.write("Cargo.toml", ROOT_MANIFEST)?;
    fx.write("crates/lib/Cargo.toml", LIB_MANIFEST)?;
    fx.write("README.md", README)?;
    fx.write("scripts/release.sh", "#!/bin/sh\necho hi\n")?;
    fx.stage_all()?;
    fx.commit("feat(app): initial")?;
    Ok(fx)
}

/// The §11.1 payloads the plan's `EditSet` would produce: the two
/// manifests at their new versions plus a brand-new file.
fn payloads() -> Vec<(String, Vec<u8>)> {
    vec![
        ("Cargo.toml".to_owned(), NEW_MANIFEST.as_bytes().to_vec()),
        (
            "crates/lib/Cargo.toml".to_owned(),
            NEW_LIB_MANIFEST.as_bytes().to_vec(),
        ),
        (
            "crates/lib/CHANGELOG.md".to_owned(),
            b"# Changelog\n\n## 0.2.0\n\n- the release\n".to_vec(),
        ),
    ]
}

/// The `git write-tree` ground truth: apply the payloads to the fixture's
/// worktree, stage everything and hash the index — the same semantics the
/// content builder must reproduce (§11.1 "apply `EditSet` in memory → build
/// tree").
fn git_write_tree_ground_truth(
    fx: &GitFixture,
    payloads: &[(String, Vec<u8>)],
) -> Result<String, BoxError> {
    for (path, content) in payloads {
        let target = fx.root().join(path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(target, content)?;
    }
    fx.stage_all()?;
    Ok(fx.run_git(&["write-tree"])?.trim().to_owned())
}

#[test]
fn content_tree_oid_matches_the_git_write_tree_ground_truth() -> Result<(), BoxError> {
    let fx = nested_fixture("content-tree-differential")?;
    let payloads = payloads();
    let oid = content_tree_oid(fx.root(), &payloads)?;
    // The fixture is untouched: no object written, no file changed.
    assert_eq!(
        std::fs::read_to_string(fx.root().join("Cargo.toml"))?,
        ROOT_MANIFEST,
        "the builder never writes the working tree"
    );
    let truth = git_write_tree_ground_truth(&fx, &payloads)?;
    assert_eq!(
        oid.as_str(),
        truth,
        "the content tree equals what git itself hashes for the same payloads"
    );
    Ok(())
}

#[test]
fn content_tree_oid_is_deterministic() -> Result<(), BoxError> {
    let fx = nested_fixture("content-tree-determinism")?;
    let payloads = payloads();
    let first = content_tree_oid(fx.root(), &payloads)?;
    let second = content_tree_oid(fx.root(), &payloads)?;
    assert_eq!(
        first, second,
        "identical base + payloads ⇒ identical tree (I3)"
    );
    Ok(())
}

#[test]
fn content_tree_oid_on_unborn_head_builds_the_payload_tree() -> Result<(), BoxError> {
    // A fresh scaffold: no commits yet — the payloads alone form the tree.
    let fx = GitFixture::new("content-tree-unborn")?;
    let payloads = vec![("Cargo.toml".to_owned(), NEW_MANIFEST.as_bytes().to_vec())];
    let oid = content_tree_oid(fx.root(), &payloads)?;
    let truth = git_write_tree_ground_truth(&fx, &payloads)?;
    assert_eq!(oid.as_str(), truth, "the empty base is the payload tree");
    Ok(())
}

#[test]
fn content_tree_oid_refuses_non_relative_payload_paths() -> Result<(), BoxError> {
    let fx = nested_fixture("content-tree-paths")?;
    for bad in ["/Cargo.toml", "Cargo.toml/", ".", "a/../Cargo.toml", ""] {
        let payloads = vec![(bad.to_owned(), NEW_MANIFEST.as_bytes().to_vec())];
        let Err(err) = content_tree_oid(fx.root(), &payloads) else {
            unreachable!("payload path {bad:?} must be refused");
        };
        assert_eq!(err.code(), "E-GIT-CONTENT", "{bad:?}: {err}");
    }
    Ok(())
}

/// A fixture with a bare `origin` wired as `remote.origin.url`.
fn fixture_with_origin(label: &str) -> Result<GitFixture, BoxError> {
    let fx = nested_fixture(label)?;
    let origin = fx
        .root()
        .parent()
        .ok_or("fixture root has a parent")?
        .join(format!("{label}-origin.git"));
    fx.run_git(&["init", "--bare", origin.to_string_lossy().as_ref()])?;
    fx.run_git(&["remote", "add", "origin", origin.to_string_lossy().as_ref()])?;
    Ok(fx)
}

#[test]
fn remote_release_tip_is_none_for_an_absent_branch() -> Result<(), BoxError> {
    let fx = fixture_with_origin("tip-absent")?;
    let tip = remote_release_tip(fx.root(), "origin", "halyard/release/main")?;
    assert!(
        tip.is_none(),
        "the release branch does not exist remotely yet — absence is a value, not an error"
    );
    Ok(())
}

#[test]
fn remote_release_tip_reads_the_pushed_tip() -> Result<(), BoxError> {
    let fx = fixture_with_origin("tip-pushed")?;
    fx.run_git(&["push", "origin", "HEAD:refs/heads/halyard/release/main"])?;
    let tip = remote_release_tip(fx.root(), "origin", "halyard/release/main")?
        .ok_or("the pushed branch must be found")?;
    let RemoteTip { commit, tree } = tip;
    assert_eq!(
        commit.as_str(),
        fx.resolve("HEAD")?.trim(),
        "the tip commit is the pushed one"
    );
    assert_eq!(
        tree.as_str(),
        fx.resolve("HEAD^{tree}")?.trim(),
        "the tip tree is what git reports"
    );
    Ok(())
}

#[test]
fn remote_release_tip_tracks_a_new_remote_commit() -> Result<(), BoxError> {
    let mut fx = fixture_with_origin("tip-moved")?;
    fx.run_git(&["push", "origin", "HEAD:refs/heads/halyard/release/main"])?;
    fx.write("Cargo.toml", NEW_MANIFEST)?;
    fx.stage_all()?;
    fx.commit("feat(app): move the branch")?;
    fx.run_git(&["push", "origin", "HEAD:refs/heads/halyard/release/main"])?;
    let tip = remote_release_tip(fx.root(), "origin", "halyard/release/main")?
        .ok_or("the pushed branch must be found")?;
    assert_eq!(
        tip.commit.as_str(),
        fx.resolve("HEAD")?.trim(),
        "the probe sees the moved tip, not a stale one"
    );
    Ok(())
}

#[test]
fn remote_release_tip_is_none_without_a_remote() -> Result<(), BoxError> {
    let fx = nested_fixture("tip-no-remote")?;
    let tip = remote_release_tip(fx.root(), "origin", "halyard/release/main")?;
    assert!(
        tip.is_none(),
        "an unconfigured remote is the honest no-op (the push effect's convention)"
    );
    Ok(())
}

use halyard_git::content::{BranchPushRow, stage_release_commit};
use halyard_git::tag::TaggerIdentity;

fn identity() -> TaggerIdentity {
    TaggerIdentity::new("halyard", "release@halyard.test", 1_700_000_000)
}

#[test]
fn stage_release_commit_writes_a_commit_git_can_read() -> Result<(), BoxError> {
    let fx = nested_fixture("stage-commit")?;
    let payloads = payloads();
    let tree = content_tree_oid(fx.root(), &payloads)?;
    let commit = stage_release_commit(fx.root(), "chore: release v0.2.0", &payloads, &identity())?;
    let body = fx.run_git(&["cat-file", "-p", commit.as_str()])?;
    assert!(
        body.contains("chore: release v0.2.0"),
        "the message rides the commit: {body}"
    );
    assert!(
        body.contains(tree.as_str()),
        "the commit's tree is the pure content tree: {body}"
    );
    assert!(
        body.contains("author halyard <release@halyard.test>") && body.contains("committer"),
        "the shell-owned identity rides the commit: {body}"
    );
    // One parent: the base snapshot (HEAD).
    let parents = fx.run_git(&["rev-list", "--parents", "-n", "1", commit.as_str()])?;
    assert_eq!(
        parents.split_whitespace().count(),
        2,
        "the commit carries HEAD as its single parent: {parents}"
    );
    Ok(())
}

#[test]
fn stage_release_commit_refuses_an_incomplete_identity() -> Result<(), BoxError> {
    let fx = nested_fixture("stage-commit-identity")?;
    let payloads = payloads();
    let Err(err) = stage_release_commit(
        fx.root(),
        "chore: release v0.2.0",
        &payloads,
        &TaggerIdentity::new("", "", 0),
    ) else {
        unreachable!("an incomplete identity must be refused");
    };
    assert_eq!(err.code(), "E-GIT-CONTENT", "{err}");
    Ok(())
}

/// A fixture with a bare origin wired by URL (no `insteadOf` — the push
/// tests address the bare path directly, the way the §11.2 CAS test
/// drives it).
fn fixture_with_bare(label: &str) -> Result<(GitFixture, std::path::PathBuf), BoxError> {
    let fx = nested_fixture(label)?;
    let bare = fx
        .root()
        .parent()
        .ok_or("fixture root has a parent")?
        .join(format!("{label}-origin.git"));
    fx.run_git(&["init", "--bare", bare.to_string_lossy().as_ref()])?;
    Ok((fx, bare))
}

const BRANCH: &str = "halyard/release/main";

#[test]
fn push_release_branch_creates_the_branch_under_the_create_only_lease() -> Result<(), BoxError> {
    let (fx, bare) = fixture_with_bare("push-create")?;
    fx.run_git(&["remote", "add", "origin", bare.to_string_lossy().as_ref()])?;
    let payloads = payloads();
    let commit = stage_release_commit(fx.root(), "chore: release v0.2.0", &payloads, &identity())?;
    let report =
        halyard_git::content::push_release_branch(fx.root(), "origin", BRANCH, &commit, None)?;
    assert_eq!(
        report.row,
        Some(BranchPushRow::Pushed {
            branch: BRANCH.to_owned(),
        }),
        "the create-only lease accepts an absent branch: {report:?}"
    );
    let remote_tip = fx.run_git(&["ls-remote", "origin", &format!("refs/heads/{BRANCH}")])?;
    assert!(
        remote_tip.contains(commit.as_str()),
        "the remote branch carries the staged commit: {remote_tip}"
    );
    Ok(())
}

#[test]
fn push_release_branch_advances_with_the_lease() -> Result<(), BoxError> {
    let (mut fx, bare) = fixture_with_bare("push-advance")?;
    fx.run_git(&["remote", "add", "origin", bare.to_string_lossy().as_ref()])?;
    let first_payloads = payloads();
    let first = stage_release_commit(
        fx.root(),
        "chore: release v0.2.0",
        &first_payloads,
        &identity(),
    )?;
    let report =
        halyard_git::content::push_release_branch(fx.root(), "origin", BRANCH, &first, None)?;
    assert!(
        matches!(report.row, Some(BranchPushRow::Pushed { .. })),
        "{report:?}"
    );
    // A second content state (a different tree) advances the branch —
    // parent = HEAD means the push is a non-fast-forward replacement,
    // sanctioned by the §11.2 lease.
    fx.write("Cargo.toml", NEW_MANIFEST)?;
    fx.stage_all()?;
    fx.commit("chore(main): release v0.2.0")?;
    let second_payloads = vec![(
        "Cargo.toml".to_owned(),
        b"[package]\nname = \"app\"\nversion = \"0.3.0\"\n".to_vec(),
    )];
    let second = stage_release_commit(
        fx.root(),
        "chore: release v0.3.0",
        &second_payloads,
        &identity(),
    )?;
    let advance = halyard_git::content::push_release_branch(
        fx.root(),
        "origin",
        BRANCH,
        &second,
        Some(&first),
    )?;
    assert_eq!(
        advance.row,
        Some(BranchPushRow::Pushed {
            branch: BRANCH.to_owned(),
        }),
        "the lease matched the probed tip: {advance:?}"
    );
    let remote_tip = fx.run_git(&["ls-remote", "origin", &format!("refs/heads/{BRANCH}")])?;
    assert!(
        remote_tip.contains(second.as_str()),
        "the remote branch advanced to the new commit: {remote_tip}"
    );
    Ok(())
}

#[test]
fn push_release_branch_rejects_a_stale_lease_fail_closed() -> Result<(), BoxError> {
    let (fx, bare) = fixture_with_bare("push-stale")?;
    fx.run_git(&["remote", "add", "origin", bare.to_string_lossy().as_ref()])?;
    let payloads = payloads();
    let ours = stage_release_commit(fx.root(), "chore: release v0.2.0", &payloads, &identity())?;
    let report =
        halyard_git::content::push_release_branch(fx.root(), "origin", BRANCH, &ours, None)?;
    assert!(matches!(report.row, Some(BranchPushRow::Pushed { .. })));
    // A racing actor (another workspace) moves the branch past our
    // lease. The racer starts from the branch it races (a fast-forward
    // move — an unrelated-history push would be rejected by the remote
    // before the CAS ever sees it), so the remote tip advances past
    // `ours` and our re-run's lease is genuinely stale.
    let mut racer = GitFixture::new("push-stale-racer")?;
    racer.run_git(&[
        "fetch",
        bare.to_string_lossy().as_ref(),
        &format!("refs/heads/{BRANCH}"),
    ])?;
    racer.run_git(&["reset", "--hard", "FETCH_HEAD"])?;
    // The racer's own edit — a file the staged payloads never carry (the
    // branch tip already holds exactly those), so the race is a real
    // change and the fast-forward push lands.
    racer.write("README.md", "# raced past the lease\n")?;
    racer.stage_all()?;
    racer.commit("chore(main): race the branch")?;
    racer.run_git(&[
        "push",
        bare.to_string_lossy().as_ref(),
        "HEAD:refs/heads/halyard/release/main",
    ])?;
    // Our re-run raced past its knowledge of the branch: the lease carries
    // the tip we pushed (`ours`), which the racer has replaced — the CAS
    // rejects it: never forced past the lease (§11.2).
    let stale_tip = ours;
    let second = stage_release_commit(
        fx.root(),
        "chore: release v0.3.0",
        &[(
            "Cargo.toml".to_owned(),
            b"[package]\nname = \"app\"\nversion = \"0.3.0\"\n".to_vec(),
        )],
        &identity(),
    )?;
    let rejected = halyard_git::content::push_release_branch(
        fx.root(),
        "origin",
        BRANCH,
        &second,
        Some(&stale_tip),
    )?;
    assert!(
        matches!(rejected.row, Some(BranchPushRow::Rejected { .. })),
        "the stale lease is rejected, never forced: {rejected:?}"
    );
    let remote_tip = fx.run_git(&["ls-remote", "origin", &format!("refs/heads/{BRANCH}")])?;
    assert!(
        !remote_tip.contains(second.as_str()),
        "the rejected commit never landed: {remote_tip}"
    );
    Ok(())
}

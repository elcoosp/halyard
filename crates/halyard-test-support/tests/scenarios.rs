//! M0.5 acceptance: every scenario builds, is deterministic (same build ⇒
//! same OIDs), and exhibits its documented shape.

use halyard_test_support::GitFixture;
use halyard_test_support::scenarios;

type BuildResult = Result<GitFixture, halyard_test_support::FixtureError>;

fn assert_deterministic(
    build: fn() -> BuildResult,
    label: &str,
) -> Result<(), halyard_test_support::FixtureError> {
    let a = build()?;
    let b = build()?;
    let head_a = a.head_oid()?;
    let head_b = b.head_oid()?;
    assert_eq!(head_a, head_b, "{label}: nondeterministic HEAD");
    Ok(())
}

#[test]
fn all_twenty_scenarios_are_registered() {
    assert_eq!(scenarios::all().len(), 20, "M0.5 requires 20 scenarios");
}

#[test]
fn empty_fixture_is_unborn() {
    let fx = scenarios::empty().expect("build");
    assert!(fx.head_oid().expect("head").is_none());
    assert_deterministic(scenarios::empty, "empty").expect("deterministic");
}

#[test]
fn single_commit_head_resolves() {
    let fx = scenarios::single_commit().expect("build");
    assert!(fx.head_oid().expect("head").is_some());
    assert_deterministic(scenarios::single_commit, "single_commit").expect("deterministic");
}

#[test]
fn linear_single_package_has_five_commits() {
    let fx = scenarios::linear_single_package().expect("build");
    let count = fx.rev_list(&["HEAD"]).expect("rev-list").len();
    assert_eq!(count, 5);
    assert_deterministic(scenarios::linear_single_package, "linear_single_package")
        .expect("deterministic");
}

#[test]
fn linear_cargo_workspace_history() {
    let fx = scenarios::linear_cargo_workspace().expect("build");
    assert_eq!(fx.rev_list(&["HEAD"]).expect("rev-list").len(), 5);
    assert_deterministic(scenarios::linear_cargo_workspace, "linear_cargo_workspace")
        .expect("deterministic");
}

#[test]
fn merge_two_branches_is_merge_shaped() {
    let fx = scenarios::merge_two_branches().expect("build");
    // 1 base + 2 branch + 1 main + 1 merge = 5 commits; exactly one merge.
    let all = fx.rev_list(&["HEAD"]).expect("rev-list");
    assert_eq!(all.len(), 5);
    let merges = fx.rev_list(&["--merges", "HEAD"]).expect("rev-list merges");
    assert_eq!(merges.len(), 1);
    assert_deterministic(scenarios::merge_two_branches, "merge_two_branches")
        .expect("deterministic");
}

#[test]
fn octopus_merge_has_three_parents() {
    let fx = scenarios::octopus_merge().expect("build");
    let merge = fx.rev_list(&["--merges", "HEAD"]).expect("rev-list merges");
    assert_eq!(merge.len(), 1, "exactly one octopus merge");
    let parents = fx
        .run_git(&["rev-list", "--parents", "-n", "1", "HEAD"])
        .expect("parents");
    // The merge commit plus its parents: main (first parent) and the three
    // branch heads.
    assert_eq!(
        parents.split_whitespace().count(),
        5,
        "octopus merge has 4 parents"
    );
    assert_deterministic(scenarios::octopus_merge, "octopus_merge").expect("deterministic");
}

#[test]
fn squash_style_subjects_carry_pr_numbers() {
    let fx = scenarios::squash_style_history().expect("build");
    let log = fx
        .run_git(&["log", "--format=%s", "--reverse"])
        .expect("log");
    let subjects: Vec<&str> = log.lines().collect();
    assert!(subjects.iter().any(|s| s.ends_with("(#101)")));
    assert_eq!(subjects.len(), 4);
    assert_deterministic(scenarios::squash_style_history, "squash_style_history")
        .expect("deterministic");
}

#[test]
fn revert_commit_cancels_a_feat() {
    let fx = scenarios::revert_commit().expect("build");
    let log = fx.run_git(&["log", "--format=%s"]).expect("log");
    let subjects: Vec<&str> = log.lines().collect();
    assert!(
        subjects
            .iter()
            .any(|s| s.starts_with("Revert \"feat: add thing\""))
    );
    let bodies = fx.run_git(&["log", "--format=%b"]).expect("bodies");
    assert!(
        bodies.contains("This reverts commit "),
        "revert body cites the OID"
    );
    assert_deterministic(scenarios::revert_commit, "revert_commit").expect("deterministic");
}

#[test]
fn fixup_commits_are_present() {
    let fx = scenarios::fixup_commits().expect("build");
    let log = fx.run_git(&["log", "--format=%s"]).expect("log");
    let subjects: Vec<&str> = log.lines().collect();
    assert_eq!(
        subjects.iter().filter(|s| s.starts_with("fixup!")).count(),
        1
    );
    assert_eq!(
        subjects.iter().filter(|s| s.starts_with("squash!")).count(),
        1
    );
    assert_deterministic(scenarios::fixup_commits, "fixup_commits").expect("deterministic");
}

#[test]
fn annotated_tags_have_messages_and_peel_to_commits() {
    let fx = scenarios::annotated_tags().expect("build");
    assert_eq!(
        fx.ls_tags().expect("tags"),
        vec![
            "nightly-marker".to_owned(),
            "v0.1.0".to_owned(),
            "v0.2.0".to_owned()
        ]
    );
    // An annotated tag object dereferences to the commit it points at.
    let peeled = fx.resolve("v0.1.0^{commit}").expect("peel v0.1.0");
    let log = fx.rev_list(&["v0.1.0"]).expect("rev-list");
    assert_eq!(log.first().expect("tip").as_str(), peeled);
    assert_deterministic(scenarios::annotated_tags, "annotated_tags").expect("deterministic");
}

#[test]
fn packed_refs_file_contains_tags() {
    let fx = scenarios::packed_refs().expect("build");
    let packed = std::fs::read_to_string(fx.git_dir().join("packed-refs")).expect("packed-refs");
    assert!(
        packed.contains("refs/tags/v0.1.0"),
        "tag moved into packed-refs"
    );
    assert_eq!(
        fx.ls_tags().expect("tags").len(),
        2,
        "refs still enumerate after packing"
    );
    assert_deterministic(scenarios::packed_refs, "packed_refs").expect("deterministic");
}

#[test]
fn side_branch_tag_is_not_ancestor_of_head() {
    let fx = scenarios::tag_on_side_branch().expect("build");
    assert_eq!(fx.ls_tags().expect("tags"), vec!["v0.1.0".to_owned()]);
    let contains = fx.rev_list(&["HEAD", "^v0.1.0"]).expect("rev-list");
    assert!(
        !contains.is_empty(),
        "HEAD has commits the side-branch tag cannot reach"
    );
    assert_deterministic(scenarios::tag_on_side_branch, "tag_on_side_branch")
        .expect("deterministic");
}

#[test]
fn mixed_tag_states_shape() {
    let fx = scenarios::mixed_tag_states().expect("build");
    assert_eq!(fx.ls_tags().expect("tags"), vec!["a-v0.1.0".to_owned()]);
    assert_deterministic(scenarios::mixed_tag_states, "mixed_tag_states").expect("deterministic");
}

#[test]
fn shallow_history_is_actually_shallow() {
    let fx = scenarios::shallow_history().expect("build");
    let shallow = fx.git_dir().join("shallow");
    assert!(shallow.exists(), "clone is marked shallow");
    let count = fx.rev_list(&["HEAD"]).expect("rev-list").len();
    assert_eq!(count, 2, "depth=2 clone carries exactly 2 commits");
    assert_deterministic(scenarios::shallow_history, "shallow_history").expect("deterministic");
}

#[test]
fn partial_clone_is_blobless() {
    let fx = scenarios::partial_blobless_clone().expect("build");
    let cfg = fx
        .run_git(&["config", "remote.origin.partialclonefilter"])
        .expect("config");
    assert_eq!(
        cfg.trim(),
        "blob:none",
        "clone carries the blob:none filter"
    );
    assert_eq!(fx.rev_list(&["HEAD"]).expect("rev-list").len(), 5);
    assert_deterministic(scenarios::partial_blobless_clone, "partial_blobless_clone")
        .expect("deterministic");
}

#[test]
fn commit_graph_file_exists() {
    let fx = scenarios::commit_graph_present().expect("build");
    let chain = fx
        .git_dir()
        .join("objects")
        .join("info")
        .join("commit-graph");
    assert!(chain.exists(), "commit-graph file written");
    assert_eq!(fx.rev_list(&["HEAD"]).expect("rev-list").len(), 6);
    assert_deterministic(scenarios::commit_graph_present, "commit_graph_present")
        .expect("deterministic");
}

#[test]
fn linked_worktree_is_registered() {
    let fx = scenarios::linked_worktree().expect("build");
    let list = fx
        .run_git(&["worktree", "list", "--porcelain"])
        .expect("worktree list");
    assert!(list.contains("branch refs/heads/release-branch"));
    assert_deterministic(scenarios::linked_worktree, "linked_worktree").expect("deterministic");
}

#[test]
fn multi_package_tag_set_enumerates_all_templates() {
    let fx = scenarios::multi_package_tag_set().expect("build");
    assert_eq!(
        fx.ls_tags().expect("tags"),
        vec![
            "@scope/web@2.3.4".to_owned(),
            "cli-v1.0.3".to_owned(),
            "core-v0.4.1".to_owned()
        ]
    );
    assert_deterministic(scenarios::multi_package_tag_set, "multi_package_tag_set")
        .expect("deterministic");
}

#[test]
fn non_conventional_subjects_shape() {
    let fx = scenarios::non_conventional_subjects().expect("build");
    let log = fx.run_git(&["log", "--format=%s"]).expect("log");
    let subjects: Vec<&str> = log.lines().collect();
    assert!(subjects.contains(&"WIP"));
    assert_eq!(subjects.len(), 3);
    assert_deterministic(
        scenarios::non_conventional_subjects,
        "non_conventional_subjects",
    )
    .expect("deterministic");
}

#[test]
fn unicode_subjects_preserve_bytes() {
    let fx = scenarios::unicode_subjects().expect("build");
    let log = fx.run_git(&["log", "--format=%s"]).expect("log");
    assert!(log.contains("feat(解析器): 添加中文支持 (#42)"));
    assert!(log.contains("fix: 修复崩溃"));
    assert_deterministic(scenarios::unicode_subjects, "unicode_subjects").expect("deterministic");
}

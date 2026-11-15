//! The pnpm publish argv table (M5.2 step 2, §13.4): the shell owns the
//! spawn, the adapter owns the grammar — verified against pnpm.io
//! (`docs/research/publish-tooling.md`), pinned here.

use halyard_eco_npm::pnpm_publish_args;

#[test]
fn live_argv_is_bare_the_tool_owns_its_guards() {
    // pnpm's clean-tree/branch checks stay the last line of defense in
    // the run that actually publishes.
    let args = pnpm_publish_args(false);
    assert_eq!(args, vec!["publish"]);
}

#[test]
fn dry_argv_skips_the_git_checks() {
    // pnpm.io: `--dry-run` "Does everything a publish would do except
    // actually publishing"; `--no-git-checks` "Skip the git checks."
    // A forecast must not depend on the working tree's git state (I3).
    let args = pnpm_publish_args(true);
    assert_eq!(args, vec!["publish", "--dry-run", "--no-git-checks"]);
}

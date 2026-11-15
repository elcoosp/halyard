//! The pnpm publish argv table (M5.2 step 2, §13.4): the shell owns the
//! spawn, the adapter owns the grammar. Verified against pnpm.io
//! (`docs/research/publish-tooling.md`): `--dry-run` "Does everything a
//! publish would do except actually publishing to the registry",
//! `--no-git-checks` "Skip the git checks" (by default pnpm verifies a
//! clean tree and the publish branch). No directory flag is documented
//! — the shell roots the process at the package root (the same cwd
//! discipline as the tag-push effect). The rows are argv atoms (I7).

/// The `pnpm` argv for one publish: live `publish` (the tool's own
/// clean-tree/branch checks stay the last line of defense in the run
/// that actually publishes); dry `publish --dry-run --no-git-checks`
/// (a forecast must not depend on the working tree's git state, I3).
#[must_use]
pub fn pnpm_publish_args(dry: bool) -> Vec<String> {
    let mut args = vec!["publish".to_owned()];
    if dry {
        args.push("--dry-run".to_owned());
        args.push("--no-git-checks".to_owned());
    }
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_argv_is_bare() {
        let args = pnpm_publish_args(false);
        assert_eq!(args, vec!["publish"]);
    }

    #[test]
    fn dry_argv_skips_the_git_checks() {
        let args = pnpm_publish_args(true);
        assert_eq!(args, vec!["publish", "--dry-run", "--no-git-checks"]);
    }
}

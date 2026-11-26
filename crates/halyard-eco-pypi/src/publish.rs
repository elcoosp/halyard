//! The `uv` publish argv table (M8 step 2, §13.4, ADR-0056): the shell
//! owns the spawn, the adapter owns the grammar. Verified against the
//! installed tool (`uv 0.13.9`, `uv publish --help`,
//! `docs/research/pypi-uv-grammars.md` §2): `--dry-run` "Perform a dry
//! run without uploading files". No credential/index/trusted-publishing
//! flag is ever invented — the bare argv is the whole contract (the
//! ADR-0033 unset-is-semantic rule at uv's grammar): the tool's own
//! posture governs trusted publishing (`--trusted-publishing
//! {automatic,always,never}` is uv's), the index (`UV_PUBLISH_URL`)
//! and the credentials (`UV_PUBLISH_TOKEN` rides the auth seam's
//! environment pair, not argv — invariant I7). No directory flag — the
//! shell roots the process at the package root (the same cwd
//! discipline as the pnpm effect).
//!
//! The rows are argv atoms (I7).

/// The `uv` argv for one publish: live `publish` (the tool's own
/// posture governs the run that actually publishes); dry
/// `publish --dry-run` (a forecast must not depend on the working
/// tree's git state, I3).
#[must_use]
pub fn uv_publish_args(dry: bool) -> Vec<String> {
    let mut args = vec!["publish".to_owned()];
    if dry {
        args.push("--dry-run".to_owned());
    }
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_argv_is_bare() {
        assert_eq!(uv_publish_args(false), vec!["publish"]);
    }

    #[test]
    fn dry_argv_forecasts_without_uploading() {
        assert_eq!(uv_publish_args(true), vec!["publish", "--dry-run"]);
    }
}

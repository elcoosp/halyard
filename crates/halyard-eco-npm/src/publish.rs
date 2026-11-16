//! The pnpm publish argv table (M5.2 step 2, §13.4): the shell owns the
//! spawn, the adapter owns the grammar. Verified against pnpm.io
//! (`docs/research/publish-tooling.md` + the 2026-11-16 fetch recorded in
//! `docs/research/npm-provenance-oidc.md`): `--dry-run` "Does everything
//! a publish would do except actually publishing to the registry",
//! `--no-git-checks` "Skip the git checks" (by default pnpm verifies a
//! clean tree and the publish branch), `--access <public|restricted>`
//! "Tells the registry whether the published package should be public or
//! restricted", and the provenance pair `--provenance` / `--no-provenance`
//! ("Use --no-provenance (or --provenance=false) to publish without it,
//! for example from a self-hosted runner"; the `provenance` config itself:
//! "Default: undefined" — "When unset, trusted publishing decides").
//! No directory flag is documented — the shell roots the process at the
//! package root (the same cwd discipline as the tag-push effect). The
//! rows are argv atoms (I7).
//!
//! Unset is semantic ([`NpmPublishOptions`]): halyard never invents a
//! flag the user did not set — under trusted publishing the tool attaches
//! provenance on its own, and the registry's own access default stands
//! unless the config asked for a level (ADR-0033).

/// The npm publish options the plan contract resolved from
/// `publish.npm` (ADR-0033): both fields `None` = unset — the spawned
/// tool's own posture governs, and the argv stays bare.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NpmPublishOptions {
    /// `--access <public|restricted>` on an explicit `publish.npm.access`
    /// value; `None` = no flag (the registry's own default stands).
    pub access: Option<String>,
    /// `--provenance` on an explicit `true`, `--no-provenance` on an
    /// explicit `false` (the documented self-hosted opt-out); `None` =
    /// no flag (under trusted publishing the tool decides).
    pub provenance: Option<bool>,
}

/// The `pnpm` argv for one publish: live `publish` (the tool's own
/// clean-tree/branch checks stay the last line of defense in the run
/// that actually publishes); dry `publish --dry-run --no-git-checks`
/// (a forecast must not depend on the working tree's git state, I3).
/// The publish options ride both forms in a fixed order — access before
/// provenance — so the dry forecast shows exactly what the live run
/// would pass (deterministic, I3; the flags are publish-time semantics
/// and `--dry-run` publishes nothing).
#[must_use]
pub fn pnpm_publish_args(dry: bool, options: &NpmPublishOptions) -> Vec<String> {
    let mut args = vec!["publish".to_owned()];
    if dry {
        args.push("--dry-run".to_owned());
        args.push("--no-git-checks".to_owned());
    }
    if let Some(access) = &options.access {
        args.push("--access".to_owned());
        args.push(access.clone());
    }
    match options.provenance {
        Some(true) => args.push("--provenance".to_owned()),
        Some(false) => args.push("--no-provenance".to_owned()),
        None => {}
    }
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_argv_is_bare_without_options() {
        let args = pnpm_publish_args(false, &NpmPublishOptions::default());
        assert_eq!(args, vec!["publish"]);
    }

    #[test]
    fn dry_argv_skips_the_git_checks() {
        let args = pnpm_publish_args(true, &NpmPublishOptions::default());
        assert_eq!(args, vec!["publish", "--dry-run", "--no-git-checks"]);
    }

    #[test]
    fn live_access_rides_the_documented_flag() {
        let args = pnpm_publish_args(
            false,
            &NpmPublishOptions {
                access: Some("public".to_owned()),
                provenance: None,
            },
        );
        assert_eq!(args, vec!["publish", "--access", "public"]);
    }

    #[test]
    fn live_provenance_true_rides_the_documented_flag() {
        let args = pnpm_publish_args(
            false,
            &NpmPublishOptions {
                access: None,
                provenance: Some(true),
            },
        );
        assert_eq!(args, vec!["publish", "--provenance"]);
    }

    #[test]
    fn live_provenance_false_is_the_documented_opt_out() {
        let args = pnpm_publish_args(
            false,
            &NpmPublishOptions {
                access: None,
                provenance: Some(false),
            },
        );
        assert_eq!(args, vec!["publish", "--no-provenance"]);
    }

    #[test]
    fn combined_options_keep_the_documented_order() {
        // Deterministic argv (I3): the access flag precedes the
        // provenance flag in every assembly, live and dry.
        let options = NpmPublishOptions {
            access: Some("restricted".to_owned()),
            provenance: Some(true),
        };
        assert_eq!(
            pnpm_publish_args(false, &options),
            vec!["publish", "--access", "restricted", "--provenance"]
        );
        assert_eq!(
            pnpm_publish_args(true, &options),
            vec![
                "publish",
                "--dry-run",
                "--no-git-checks",
                "--access",
                "restricted",
                "--provenance"
            ]
        );
    }

    #[test]
    fn dry_carries_the_publish_options_verbatim() {
        // The forecast must show the argv the live run would pass: the
        // publish options ride the dry form unchanged (the flags are
        // publish-time semantics; `--dry-run` publishes nothing).
        let args = pnpm_publish_args(
            true,
            &NpmPublishOptions {
                access: Some("public".to_owned()),
                provenance: Some(false),
            },
        );
        assert_eq!(
            args,
            vec![
                "publish",
                "--dry-run",
                "--no-git-checks",
                "--access",
                "public",
                "--no-provenance"
            ]
        );
    }
}

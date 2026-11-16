//! The pnpm publish argv table (M5.2 step 2, §13.4): the shell owns the
//! spawn, the adapter owns the grammar — verified against pnpm.io
//! (`docs/research/publish-tooling.md` + the 2026-11-16 provenance fetch
//! in `docs/research/npm-provenance-oidc.md`), pinned here.

use halyard_eco_npm::{NpmPublishOptions, pnpm_publish_args};

#[test]
fn live_argv_is_bare_the_tool_owns_its_guards() {
    // pnpm's clean-tree/branch checks stay the last line of defense in
    // the run that actually publishes.
    let args = pnpm_publish_args(false, &NpmPublishOptions::default());
    assert_eq!(args, vec!["publish"]);
}

#[test]
fn dry_argv_skips_the_git_checks() {
    // pnpm.io: `--dry-run` "Does everything a publish would do except
    // actually publishing"; `--no-git-checks` "Skip the git checks."
    // A forecast must not depend on the working tree's git state (I3).
    let args = pnpm_publish_args(true, &NpmPublishOptions::default());
    assert_eq!(args, vec!["publish", "--dry-run", "--no-git-checks"]);
}

#[test]
fn explicit_options_ride_the_documented_flags_in_order() {
    // pnpm.io (VERIFIED 2026-11-16): `--access <public|restricted>`
    // "Tells the registry whether the published package should be
    // public or restricted"; `--no-provenance` publishes without
    // attestations "for example from a self-hosted runner". Unset is
    // semantic — under trusted publishing the tool decides — so an
    // unset option never invents a flag (ADR-0033).
    let options = NpmPublishOptions {
        access: Some("public".to_owned()),
        provenance: Some(true),
    };
    assert_eq!(
        pnpm_publish_args(false, &options),
        vec!["publish", "--access", "public", "--provenance"]
    );
    let opt_out = NpmPublishOptions {
        access: None,
        provenance: Some(false),
    };
    assert_eq!(
        pnpm_publish_args(false, &opt_out),
        vec!["publish", "--no-provenance"]
    );
}

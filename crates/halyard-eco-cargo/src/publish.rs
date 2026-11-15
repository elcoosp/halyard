//! The cargo publish argv table (M5.2 step 2, §13.4): the shell owns
//! the spawn, the adapter owns the grammar. Verified against the Cargo
//! Book (`docs/research/publish-tooling.md`): `--dry-run` "Perform all
//! checks without uploading", `--manifest-path path`, `--offline`
//! "Prevents Cargo from accessing the network for any reason" — the
//! book's own example pins `publish --dry-run --manifest-path …
//! --offline`, which is exactly the dry row here. The `--registry`
//! mapping is verified on the same page (M5.3 continued): `--registry
//! <name>` takes a registry *name* defined in Cargo config files and
//! the default (`registry.default`) is `crates-io` — so the default
//! spellings ride the tool's own default with no flag, and any other
//! configured name reaches the spawn verbatim. The rows are argv atoms
//! (I7: never a shell string).

/// Whether `registry` names the default registry (the state the tool
/// already defaults to — `registry.default`, which is crates-io): the
/// argv table emits no flag for it, the token side resolves the same
/// way (`halyard_publish::auth`'s `is_crates_io` — both spellings are
/// pinned by tests on both sides, so argv and env can never disagree
/// about which registry a run publishes to).
#[must_use]
pub fn is_default_registry(registry: &str) -> bool {
    registry == "crates.io" || registry == "crates-io"
}

/// The `cargo` argv for one publish: live `publish --manifest-path
/// <manifest>`; dry `publish --dry-run --manifest-path <manifest>
/// --offline` (the documented hermetic check — a forecast must not
/// touch the network, I5). A configured non-default registry reaches
/// the spawn as `--registry <name>` (the Cargo Book's documented flag;
/// the default registry rides the tool's own default with no flag).
/// `manifest` is the repo-relative manifest file path; the caller roots
/// the process at the repo root.
#[must_use]
pub fn cargo_publish_args(manifest: &str, registry: &str, dry: bool) -> Vec<String> {
    let mut args = vec!["publish".to_owned()];
    if !is_default_registry(registry) {
        args.push("--registry".to_owned());
        args.push(registry.to_owned());
    }
    if dry {
        args.push("--dry-run".to_owned());
    }
    args.push("--manifest-path".to_owned());
    args.push(manifest.to_owned());
    if dry {
        // The Cargo Book's own example combination: the hermetic dry
        // check (no network — I5).
        args.push("--offline".to_owned());
    }
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_argv_is_manifest_scoped_only() {
        let args = cargo_publish_args("crates/a/Cargo.toml", "crates.io", false);
        assert_eq!(
            args,
            vec!["publish", "--manifest-path", "crates/a/Cargo.toml"]
        );
    }

    #[test]
    fn dry_argv_is_the_documented_hermetic_check() {
        let args = cargo_publish_args("Cargo.toml", "crates.io", true);
        assert_eq!(
            args,
            vec![
                "publish",
                "--dry-run",
                "--manifest-path",
                "Cargo.toml",
                "--offline"
            ]
        );
    }

    #[test]
    fn the_default_registry_rides_the_tools_own_default() {
        // Both accepted default spellings — the §13 dotted one and
        // Cargo's dashed config spelling — emit no flag.
        assert!(is_default_registry("crates.io"));
        assert!(is_default_registry("crates-io"));
        assert_eq!(
            cargo_publish_args("Cargo.toml", "crates-io", false),
            vec!["publish", "--manifest-path", "Cargo.toml"]
        );
    }

    #[test]
    fn a_configured_registry_reaches_the_spawn_verbatim() {
        let args = cargo_publish_args("crates/a/Cargo.toml", "my-registry", false);
        assert_eq!(
            args,
            vec![
                "publish",
                "--registry",
                "my-registry",
                "--manifest-path",
                "crates/a/Cargo.toml"
            ]
        );
        // The dry form keeps the flag (the tool's dry check runs
        // against the same registry the live run would publish to).
        let dry = cargo_publish_args("Cargo.toml", "corp.repo", true);
        assert_eq!(
            dry,
            vec![
                "publish",
                "--registry",
                "corp.repo",
                "--dry-run",
                "--manifest-path",
                "Cargo.toml",
                "--offline"
            ]
        );
    }
}

//! The cargo publish argv table (M5.2 step 2, §13.4): the shell owns
//! the spawn, the adapter owns the grammar. Verified against the Cargo
//! Book (`docs/research/publish-tooling.md`): `--dry-run` "Perform all
//! checks without uploading", `--manifest-path path`, `--offline`
//! "Prevents Cargo from accessing the network for any reason" — the
//! book's own example pins `publish --dry-run --manifest-path …
//! --offline`, which is exactly the dry row here. The rows are argv
//! atoms (I7: never a shell string).

/// The `cargo` argv for one publish: live `publish --manifest-path
/// <manifest>`; dry `publish --dry-run --manifest-path <manifest>
/// --offline` (the documented hermetic check — a forecast must not
/// touch the network, I5). `manifest` is the repo-relative manifest
/// file path; the caller roots the process at the repo root.
#[must_use]
pub fn cargo_publish_args(manifest: &str, dry: bool) -> Vec<String> {
    let mut args = vec!["publish".to_owned()];
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
        let args = cargo_publish_args("crates/a/Cargo.toml", false);
        assert_eq!(
            args,
            vec!["publish", "--manifest-path", "crates/a/Cargo.toml"]
        );
    }

    #[test]
    fn dry_argv_is_the_documented_hermetic_check() {
        let args = cargo_publish_args("Cargo.toml", true);
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
}

//! The cargo publish argv table (M5.2 step 2, §13.4): the shell owns the
//! spawn, the adapter owns the grammar — verified against the Cargo Book
//! (`docs/research/publish-tooling.md`), pinned here.

use halyard_eco_cargo::cargo_publish_args;

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
    // Cargo Book: `--dry-run` "Perform all checks without uploading";
    // the book's own example pins `--dry-run --manifest-path …
    // --offline` — the forecast must not touch the network (I5).
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

#[test]
fn argv_never_interpolates_the_manifest_into_a_shell_string() {
    // I7: the returned rows are argv atoms (no shell quoting to break).
    let args = cargo_publish_args("weird path/Cargo.toml", false);
    assert!(args.contains(&"weird path/Cargo.toml".to_owned()));
}

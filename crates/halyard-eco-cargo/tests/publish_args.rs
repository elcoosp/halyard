//! The cargo publish argv table (M5.2 step 2, §13.4): the shell owns the
//! spawn, the adapter owns the grammar — verified against the Cargo Book
//! (`docs/research/publish-tooling.md`), pinned here. The `--registry`
//! mapping is M5.3 continued (verified on the same page): a configured
//! non-default registry reaches the spawn as `--registry <name>`; the
//! default spellings ride the tool's own documented default with no
//! flag.

use halyard_eco_cargo::{cargo_publish_args, is_default_registry};

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
    // Cargo Book: `--dry-run` "Perform all checks without uploading";
    // the book's own example pins `--dry-run --manifest-path …
    // --offline` — the forecast must not touch the network (I5).
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
fn argv_never_interpolates_the_manifest_into_a_shell_string() {
    // I7: the returned rows are argv atoms (no shell quoting to break).
    let args = cargo_publish_args("weird path/Cargo.toml", "crates.io", false);
    assert!(args.contains(&"weird path/Cargo.toml".to_owned()));
}

#[test]
fn a_configured_registry_reaches_the_spawn_verbatim() {
    // The Cargo Book's `--registry` takes a registry *name* defined in
    // Cargo config files; the default (`registry.default`) is crates-io.
    let args = cargo_publish_args("Cargo.toml", "corp.repo", true);
    assert_eq!(
        args,
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

#[test]
fn the_default_spellings_emit_no_flag() {
    // Both accepted default spellings — the §13 dotted one and Cargo's
    // dashed config spelling — are the state the tool already defaults
    // to; halyard never passes a flag for it (the no-guess rule), and
    // the token side resolves the same spellings the same way.
    assert!(is_default_registry("crates.io"));
    assert!(is_default_registry("crates-io"));
    assert!(!is_default_registry("corp.repo"));
    assert_eq!(
        cargo_publish_args("Cargo.toml", "crates-io", false),
        vec!["publish", "--manifest-path", "Cargo.toml"]
    );
}

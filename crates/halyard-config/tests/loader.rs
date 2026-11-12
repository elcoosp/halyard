//! Loader tests (M1.8): unknown keys produce `E-CFG-UNKNOWN-KEY` errors
//! with did-you-mean suggestions, snapshot-tested (§17 M1.8 acceptance:
//! "invalid configs produce snapshot-tested diagnostics").

use halyard_config::{ConfigError, load_from_str};
use insta::assert_snapshot;

#[test]
fn documented_example_loads() {
    let src = r#"
[repo]
base_branch = "main"

[versioning]
cascade = "when-unsatisfied"

[commits.types]
feat = "minor"

[[package]]
name = "internal-tool"
release = false
"#;
    let cfg = load_from_str(src).unwrap_or_else(|e| panic!("valid config: {e}"));
    assert_eq!(
        cfg.repo.as_ref().map(|r| r.base_branch.clone()),
        Some(Some("main".to_owned()))
    );
    assert_eq!(cfg.package.len(), 1);
}

#[test]
fn unknown_top_level_key_suggests_closest() {
    let err = load_from_str("[repoz]\nbase_branch = \"main\"\n")
        .err()
        .unwrap_or_else(|| panic!("must fail"));
    assert_eq!(err.code(), "E-CFG-UNKNOWN-KEY");
    assert_snapshot!(err.to_string());
}

#[test]
fn unknown_nested_key_suggests_closest() {
    let err = load_from_str("[repo]\nbase_branh = \"main\"\n")
        .err()
        .unwrap_or_else(|| panic!("must fail"));
    assert_snapshot!(err.to_string());
}

#[test]
fn unknown_key_inside_package_array_is_caught() {
    let err = load_from_str("[[package]]\nname = \"x\"\nreleas = false\n")
        .err()
        .unwrap_or_else(|| panic!("must fail"));
    assert_snapshot!(err.to_string());
}

#[test]
fn no_close_match_lists_valid_keys() {
    let err = load_from_str("[repo]\nzzzzzzzz = 1\n")
        .err()
        .unwrap_or_else(|| panic!("must fail"));
    assert!(err.to_string().contains("valid keys"), "{}", err);
    assert_snapshot!(err.to_string());
}

#[test]
fn invalid_enum_value_is_a_parse_error() {
    let err = load_from_str("[versioning]\ncascade = \"sometimes\"\n")
        .err()
        .unwrap_or_else(|| panic!("must fail"));
    assert_eq!(err.code(), "E-CFG-TOML");
    assert_snapshot!(err.to_string());
}

#[test]
fn diagnostics_are_deterministic() {
    let src = "[repoz]\n";
    let first = load_from_str(src).err().map(|e| e.to_string());
    let second = load_from_str(src).err().map(|e| e.to_string());
    assert_eq!(
        first, second,
        "same input ⇒ byte-identical diagnostics (I3)"
    );
}

#[test]
fn load_from_file_reports_path_in_errors() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let path = dir.path().join("halyard.toml");
    std::fs::write(&path, "[repo]\nnope = true\n").unwrap_or_else(|e| panic!("write fixture: {e}"));
    let err = halyard_config::load(&path)
        .err()
        .unwrap_or_else(|| panic!("must fail"));
    assert!(err.to_string().contains("halyard.toml"), "{}", err);
}

#[test]
fn missing_file_is_a_read_error() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let err = halyard_config::load(&dir.path().join("absent.toml"))
        .err()
        .unwrap_or_else(|| panic!("must fail"));
    assert_eq!(err.code(), "E-CFG-TOML");
    assert_eq!(err.code(), ConfigError::READ_CODE);
}

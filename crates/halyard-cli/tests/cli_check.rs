//! CLI `check` smoke tests (§11.6, M4.3): build a real git fixture
//! workspace, run the compiled binary with `check` verdicts — the
//! pending-release gate (`--exit-code`, §15.4 exit 3), the accepted
//! title lint with its per-package prediction, and the `E-CLI-TITLE`
//! rejections.

use std::process::Command;

use halyard_test_support::{CommitSpec, GitFixture};

type BoxError = Box<dyn std::error::Error>;

fn fixture_dir(label: &str) -> Result<std::path::PathBuf, BoxError> {
    let dir =
        std::env::temp_dir().join(format!("halyard-cli-check-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// One workspace, one `feat` commit: p1 and its dependent p2 are pending
/// a patch/minor release.
fn build_workspace(label: &str) -> Result<GitFixture, BoxError> {
    let dir = fixture_dir(label)?;
    let mut fx = GitFixture::init_in(&dir)?;
    let scaffold = vec![
        (
            "Cargo.toml".to_owned(),
            "[workspace]\nresolver = \"2\"\nmembers = [\"crates/p1\", \"crates/p2\"]\n".to_owned(),
        ),
        (
            "crates/p1/Cargo.toml".to_owned(),
            "[package]\nname = \"p1\"\nversion = \"0.1.0\"\nedition = \"2021\"\n".to_owned(),
        ),
        (
            "crates/p2/Cargo.toml".to_owned(),
            "[package]\nname = \"p2\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\np1 = \"0.1\"\n".to_owned(),
        ),
        ("crates/p1/src/lib.rs".to_owned(), "pub const C: usize = 1;\n".to_owned()),
        ("crates/p2/src/lib.rs".to_owned(), "pub const C: usize = 2;\n".to_owned()),
    ];
    fx.commit_with(&CommitSpec::files(scaffold, "feat: scaffold workspace"))?;
    Ok(fx)
}

/// Runs the binary; returns (exit code, combined output).
fn halyard(root: &std::path::Path, args: &[&str]) -> Result<(u8, String), BoxError> {
    let out = Command::new(env!("CARGO_BIN_EXE_halyard"))
        .arg("-C")
        .arg(root)
        .args(args)
        .env("NO_COLOR", "1")
        .output()?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    // The child's exit code: a byte in practice (the CLI's codes are
    // 0/1/3); `try_from` keeps the cast lint-free.
    let code = u8::try_from(out.status.code().unwrap_or_default()).unwrap_or_default();
    Ok((code, text))
}

#[test]
fn exit_code_gate_reports_3_when_a_release_is_pending() -> Result<(), BoxError> {
    let fx = build_workspace("gate")?;
    let (code, out) = halyard(fx.root(), &["check", "--exit-code"])?;
    assert_eq!(code, 3, "pending release must exit 3 (§15.4): {out}");
    assert!(out.contains("pending: 2 release(s)"), "{out}");
    assert!(out.contains("p1 0.1.0 → 0.1.1"), "{out}");
    Ok(())
}

#[test]
fn exit_code_gate_is_quiet_when_nothing_is_pending() -> Result<(), BoxError> {
    // A repo whose only commit is non-conventional: the planner skips it.
    let dir = fixture_dir("gate-quiet")?;
    let mut fx = GitFixture::init_in(&dir)?;
    fx.commit_with(&CommitSpec::files(
        vec![
            (
                "Cargo.toml".to_owned(),
                "[package]\nname = \"solo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n".to_owned(),
            ),
            (
                "src/lib.rs".to_owned(),
                "pub const C: usize = 1;\n".to_owned(),
            ),
        ],
        "update stuff without the conventional grammar",
    ))?;
    let (code, out) = halyard(fx.root(), &["check", "--exit-code"])?;
    assert_eq!(code, 0, "no pending release exits 0: {out}");
    assert!(out.contains("no pending releases"), "{out}");
    Ok(())
}

#[test]
fn accepted_title_predicts_the_pre_major_remap() -> Result<(), BoxError> {
    let fx = build_workspace("predict")?;
    // `feat` conveys minor; both packages sit on 0.y.z, so the §10.2
    // cargo-compatible policy remaps minor → patch.
    let (code, out) = halyard(fx.root(), &["check", "--pr-title", "feat: add the thing"])?;
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("conveys minor"), "{out}");
    assert!(out.contains("if merged, this will release:"), "{out}");
    assert!(
        out.contains("p1 0.1.0 → patch (0.x policy: minor → patch)"),
        "{out}"
    );
    assert!(
        out.contains("p2 0.1.0 → patch (0.x policy: minor → patch)"),
        "{out}"
    );
    Ok(())
}

#[test]
fn breaking_title_conveys_major_after_the_remap() -> Result<(), BoxError> {
    let fx = build_workspace("breaking")?;
    // `feat!` conveys major; §10.2 remaps major → minor on 0.y.z.
    let (code, out) = halyard(fx.root(), &["check", "--pr-title", "feat!: drop v1"])?;
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("conveys major"), "{out}");
    assert!(out.contains("(0.x policy: major → minor)"), "{out}");
    Ok(())
}

#[test]
fn json_format_is_a_stable_document() -> Result<(), BoxError> {
    let fx = build_workspace("json")?;
    let (code, out) = halyard(
        fx.root(),
        &[
            "--format",
            "json",
            "check",
            "--pr-title",
            "feat: add the thing",
        ],
    )?;
    assert_eq!(code, 0, "{out}");
    let value: serde_json::Value = serde_json::from_str(out.trim())?;
    let field = |key: &str| value.get(key).and_then(serde_json::Value::as_str);
    assert_eq!(field("verdict"), Some("accepted"));
    assert_eq!(field("kind"), Some("feat"));
    assert_eq!(field("level"), Some("minor"));
    assert_eq!(
        value.get("pending").and_then(serde_json::Value::as_bool),
        Some(true)
    );
    let predicted = value
        .get("predicted")
        .and_then(serde_json::Value::as_array)
        .unwrap_or_else(|| panic!("predicted rows missing: {out}"));
    assert_eq!(predicted.len(), 2, "{out}");
    let first = predicted
        .first()
        .unwrap_or_else(|| unreachable!("two rows"));
    assert_eq!(
        first.get("package").and_then(serde_json::Value::as_str),
        Some("p1")
    );
    assert_eq!(
        first.get("bump").and_then(serde_json::Value::as_str),
        Some("patch")
    );
    assert_eq!(
        first.get("remapped").and_then(serde_json::Value::as_bool),
        Some(true)
    );
    Ok(())
}

#[test]
fn non_conventional_title_fails_with_the_diagnosis() -> Result<(), BoxError> {
    let fx = build_workspace("reject")?;
    let (code, out) = halyard(fx.root(), &["check", "--pr-title", "updated the parser"])?;
    assert_eq!(code, 1, "a rejected title fails the check: {out}");
    assert!(out.contains("is not a conventional commit"), "{out}");
    assert!(out.contains("type(scope): description"), "{out}");
    // The stable §15.4 code travels in the JSON error channel.
    let (code, out) = halyard(
        fx.root(),
        &[
            "--format",
            "json",
            "check",
            "--pr-title",
            "updated the parser",
        ],
    )?;
    assert_eq!(code, 1, "{out}");
    let value: serde_json::Value = serde_json::from_str(out.trim())?;
    let code = value
        .get("error")
        .and_then(|error| error.get("code"))
        .and_then(serde_json::Value::as_str);
    assert_eq!(code, Some("E-CLI-TITLE"));
    Ok(())
}

#[test]
fn typo_kind_fails_with_did_you_mean() -> Result<(), BoxError> {
    let fx = build_workspace("typo")?;
    let (code, out) = halyard(fx.root(), &["check", "--pr-title", "feet: adjust fixtures"])?;
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("did you mean \"feat\""), "{out}");
    Ok(())
}

#[test]
fn autosquash_title_fails_the_check() -> Result<(), BoxError> {
    let fx = build_workspace("fixup")?;
    let (code, out) = halyard(fx.root(), &["check", "--pr-title", "fixup! feat: earlier"])?;
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("fixup!/squash!"), "{out}");
    Ok(())
}

#[test]
fn unmapped_legitimate_kind_is_a_quiet_no_bump() -> Result<(), BoxError> {
    // `docs:` is unmapped by the §10.1 defaults and nothing is close to
    // it — a legitimate no-bump verdict, not a typo. The pending
    // packages stay pending at `none` (the prediction names every
    // pending package; the bump column shows what the title conveys).
    let fx = build_workspace("docs")?;
    let (code, out) = halyard(fx.root(), &["check", "--pr-title", "docs: the readme"])?;
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("conveys none"), "{out}");
    assert!(out.contains("p1 0.1.0 → none"), "{out}");
    assert!(out.contains("p2 0.1.0 → none"), "{out}");
    Ok(())
}

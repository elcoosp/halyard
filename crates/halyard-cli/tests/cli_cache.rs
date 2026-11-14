//! CLI persistent-cache wiring tests (M2.6 close-out, §8.7): the planning
//! pipeline consults the per-commit cache under
//! `$GIT_DIR/halyard/cache/v1/`, `--no-cache` really skips it, and a
//! corrupted cache degrades to an uncached run (§8.7: the cache must
//! never break planning).

use std::process::Command;

use halyard_test_support::{CommitSpec, GitFixture};

type BoxError = Box<dyn std::error::Error>;

fn fixture_dir(label: &str) -> Result<std::path::PathBuf, BoxError> {
    let dir =
        std::env::temp_dir().join(format!("halyard-cli-cache-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// A single-package repo whose manifest version is already tagged, so
/// the plan is stable across runs (the reconciliation stays Tagged).
fn build_workspace(label: &str) -> Result<GitFixture, BoxError> {
    let dir = fixture_dir(label)?;
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
        "feat: scaffold the crate",
    ))?;
    fx.tag_annotated("v0.1.0", "solo v0.1.0")?;
    Ok(fx)
}

/// Runs the binary; returns (exit code, stdout, stderr) so cache
/// warnings can be asserted separately from the stable stdout document.
fn halyard_streams(
    root: &std::path::Path,
    args: &[&str],
) -> Result<(u8, String, String), BoxError> {
    let out = Command::new(env!("CARGO_BIN_EXE_halyard"))
        .arg("-C")
        .arg(root)
        .args(args)
        .env("NO_COLOR", "1")
        .output()?;
    let code = u8::try_from(out.status.code().unwrap_or_default()).unwrap_or_default();
    Ok((
        code,
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    ))
}

fn cache_file(root: &std::path::Path) -> std::path::PathBuf {
    root.join(".git")
        .join("halyard")
        .join("cache")
        .join("v1")
        .join("cache.bin")
}

#[test]
fn plan_warms_and_reuses_the_persistent_cache() -> Result<(), BoxError> {
    let fx = build_workspace("warm")?;
    let (code, first, _) = halyard_streams(fx.root(), &["--format", "json", "plan"])?;
    assert_eq!(code, 0, "{first}");
    let cache = cache_file(fx.root());
    assert!(
        cache.is_file(),
        "the §8.7 cache is written under $GIT_DIR/halyard/cache/v1/"
    );
    let size_after_first = std::fs::metadata(&cache)?.len();
    assert!(size_after_first > 0, "the warm run stored records");
    // The second run consults the cache and must produce the identical
    // plan document (determinism holds across the cached path, §8.7).
    let (code, second, _) = halyard_streams(fx.root(), &["--format", "json", "plan"])?;
    assert_eq!(code, 0, "{second}");
    assert_eq!(
        first, second,
        "cached and uncached plans agree byte-for-byte (I3)"
    );
    Ok(())
}

#[test]
fn no_cache_skips_the_persistent_cache() -> Result<(), BoxError> {
    let fx = build_workspace("no-cache")?;
    let (code, first, _) = halyard_streams(fx.root(), &["--format", "json", "--no-cache", "plan"])?;
    assert_eq!(code, 0, "{first}");
    assert!(
        !cache_file(fx.root()).exists(),
        "--no-cache leaves no cache file behind"
    );
    // And the flag still plans identically.
    let (code, uncached) = {
        let (code, out, _) = halyard_streams(fx.root(), &["--format", "json", "plan"])?;
        (code, out)
    };
    assert_eq!(code, 0, "{uncached}");
    assert_eq!(
        first, uncached,
        "--no-cache output matches the cached path (I3)"
    );
    Ok(())
}

#[test]
fn corrupt_cache_degrades_to_an_uncached_run() -> Result<(), BoxError> {
    // §8.7: the cache is an optimization, never a correctness surface —
    // a corrupted file must warn and plan on, not fail the command.
    let fx = build_workspace("corrupt")?;
    let (code, warm, _) = halyard_streams(fx.root(), &["--format", "json", "plan"])?;
    assert_eq!(code, 0, "{warm}");
    let cache = cache_file(fx.root());
    std::fs::write(&cache, b"garbage, not a halyard cache file")?;
    let (code, stdout, stderr) = halyard_streams(fx.root(), &["--format", "json", "plan"])?;
    assert_eq!(code, 0, "a corrupt cache must not fail planning: {stderr}");
    assert_eq!(
        warm, stdout,
        "the degraded run still plans identically (I3)"
    );
    assert!(
        stderr.contains("E-CACHE"),
        "the warning names the stable code: {stderr}"
    );
    Ok(())
}

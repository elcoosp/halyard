//! CLI `bench` gates (§16.2, ADR-0042): the built-in measurement of the
//! planning pipeline over the current repo — the §14 model stages
//! (`discover` → `config` → `history` → `plan`) timed per pass and
//! aggregated with the nearest-rank percentiles the budget table uses.
//! Pins: the human stage table and shape line, the JSON schema
//! stability (the timings are wall-clock; the keys, the stage order and
//! the shape are not), the `--no-cache` cold posture, the
//! sample-count contract and the fail-closed zero-iteration refusal.
//! Written first (red on the unrecognized subcommand), implemented
//! second.

use std::process::Command;

use halyard_test_support::fixture::{CommitSpec, GitFixture};

type BoxError = Box<dyn std::error::Error>;

/// Runs the binary; returns (exit ok, stdout, stderr) — the streams
/// stay separate because the JSON document must parse alone.
fn halyard(root: &std::path::Path, args: &[&str]) -> Result<(bool, String, String), BoxError> {
    let out = Command::new(env!("CARGO_BIN_EXE_halyard"))
        .arg("-C")
        .arg(root)
        .args(args)
        .env("NO_COLOR", "1")
        .output()?;
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    Ok((out.status.success(), stdout, stderr))
}

/// A two-package cargo workspace (`lib` ← `app`), fully released: the
/// scaffold commit touches both packages and both carry their release
/// tags, so the measured pipeline is the §14.1 no-op-warm shape (zero
/// pending commits — the honest steady state a fully-released repo
/// benches).
fn released_fixture(label: &str) -> Result<GitFixture, BoxError> {
    let mut fx = GitFixture::new(label)?;
    let scaffold = vec![
        (
            "Cargo.toml".to_owned(),
            "[workspace]\nresolver = \"2\"\nmembers = [\"crates/lib\", \"crates/app\"]\n".to_owned(),
        ),
        (
            "crates/lib/Cargo.toml".to_owned(),
            "[package]\nname = \"lib\"\nversion = \"0.1.0\"\nedition = \"2021\"\n".to_owned(),
        ),
        (
            "crates/lib/src/lib.rs".to_owned(),
            "pub const L: usize = 1;\n".to_owned(),
        ),
        (
            "crates/app/Cargo.toml".to_owned(),
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\nlib = { version = \"0.1\", path = \"../lib\" }\n".to_owned(),
        ),
        (
            "crates/app/src/lib.rs".to_owned(),
            "pub const A: usize = 2;\n".to_owned(),
        ),
    ];
    fx.commit_with(&CommitSpec::files(scaffold, "feat: scaffold workspace"))?;
    fx.run_git(&["tag", "-a", "lib-v0.1.0", "-m", "lib 0.1.0"])?;
    fx.run_git(&["tag", "-a", "app-v0.1.0", "-m", "app 0.1.0"])?;
    Ok(fx)
}

#[test]
fn bench_prints_the_stage_table_and_the_shape() -> Result<(), BoxError> {
    // The human report: one row per pipeline stage (the `--profile`
    // trace's own span vocabulary — the same names the observability
    // surface uses), the nearest-rank percentile columns, the pass
    // posture and the repo shape the timings belong to.
    let fx = released_fixture("cli-bench-human")?;
    let (ok, stdout, stderr) = halyard(fx.root(), &["bench"])?;
    assert!(ok, "bench renders: {stdout}{stderr}");
    assert!(stdout.contains("stage"), "the stage table header: {stdout}");
    for stage in ["discover", "config", "history", "plan"] {
        assert!(
            stdout.contains(stage),
            "the {stage} row rides the trace vocabulary: {stdout}"
        );
    }
    assert!(
        stdout.contains("8 timed"),
        "the default pass count is named: {stdout}"
    );
    assert!(
        stdout.contains("shape:") && stdout.contains("2 packages"),
        "the measured shape is reported beside the timings: {stdout}"
    );
    Ok(())
}

#[test]
fn bench_json_schema_is_stable_and_stages_name_sorted() -> Result<(), BoxError> {
    // The timings are wall-clock (two runs never agree on values), so
    // the stability contract pins the schema, not the numbers: the same
    // keys, the same name-sorted stage order and the same shape on both
    // runs.
    let fx = released_fixture("cli-bench-json")?;
    let (ok, first, stderr) = halyard(fx.root(), &["bench", "--format", "json"])?;
    assert!(ok, "bench renders: {first}{stderr}");
    let (ok, second, _) = halyard(fx.root(), &["bench", "--format", "json"])?;
    assert!(ok, "bench re-renders: {second}");
    let a: serde_json::Value = serde_json::from_str(first.trim())?;
    let b: serde_json::Value = serde_json::from_str(second.trim())?;
    for key in ["iterations", "cache", "shape", "stages"] {
        assert!(
            a.get(key).is_some() && b.get(key).is_some(),
            "both documents carry {key}: {first}"
        );
    }
    assert_eq!(
        a.pointer("/iterations"),
        Some(&serde_json::json!(8)),
        "the default pass count: {first}"
    );
    assert_eq!(
        a.pointer("/cache"),
        Some(&serde_json::json!("warm")),
        "the default posture: {first}"
    );
    assert_eq!(
        a.pointer("/shape"),
        b.pointer("/shape"),
        "the shape is run-stable: {first}"
    );
    let stage_names = |doc: &serde_json::Value| -> Vec<String> {
        doc.pointer("/stages")
            .and_then(serde_json::Value::as_array)
            .expect("stages is an array")
            .iter()
            .map(|row| {
                row.get("stage")
                    .and_then(serde_json::Value::as_str)
                    .expect("stage is a string")
                    .to_owned()
            })
            .collect()
    };
    assert_eq!(
        stage_names(&a),
        stage_names(&b),
        "the stage order is run-stable"
    );
    let mut sorted = stage_names(&a).clone();
    sorted.sort();
    assert_eq!(stage_names(&a), sorted, "the stages are name-sorted (I3)");
    let stages = a
        .pointer("/stages")
        .and_then(serde_json::Value::as_array)
        .expect("stages is an array");
    for row in stages {
        let mut keys: Vec<&str> = row
            .as_object()
            .expect("a stage row is an object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec!["mean_us", "min_us", "p50_us", "p99_us", "samples", "stage"],
            "the stage row schema: {row}"
        );
    }
    Ok(())
}

#[test]
fn bench_iterations_controls_the_sample_count() -> Result<(), BoxError> {
    // `--iterations N` times exactly N passes; each stage's `samples`
    // row says so (the measurement is honest about its own basis).
    let fx = released_fixture("cli-bench-iterations")?;
    let (ok, stdout, stderr) = halyard(
        fx.root(),
        &["bench", "--iterations", "2", "--format", "json"],
    )?;
    assert!(ok, "bench renders: {stdout}{stderr}");
    let doc: serde_json::Value = serde_json::from_str(stdout.trim())?;
    assert_eq!(
        doc.pointer("/iterations"),
        Some(&serde_json::json!(2)),
        "the requested pass count: {stdout}"
    );
    let stages = doc
        .pointer("/stages")
        .and_then(serde_json::Value::as_array)
        .expect("stages is an array");
    for row in stages {
        assert_eq!(
            row.get("samples"),
            Some(&serde_json::json!(2)),
            "every stage sampled once per pass: {row}"
        );
    }
    Ok(())
}

#[test]
fn bench_no_cache_reports_the_cold_posture() -> Result<(), BoxError> {
    // `--no-cache` is the §14.1 cold measurement (the §8.7 per-commit
    // cache is out of the loop); the report names the posture so the
    // two runs are never compared blind.
    let fx = released_fixture("cli-bench-no-cache")?;
    let (ok, stdout, stderr) = halyard(fx.root(), &["bench", "--no-cache", "--format", "json"])?;
    assert!(ok, "bench renders: {stdout}{stderr}");
    let doc: serde_json::Value = serde_json::from_str(stdout.trim())?;
    assert_eq!(
        doc.pointer("/cache"),
        Some(&serde_json::json!("cold")),
        "the no-cache posture: {stdout}"
    );
    Ok(())
}

#[test]
fn bench_refuses_zero_iterations_fail_closed() -> Result<(), BoxError> {
    // A zero-iteration request measures nothing — a caller bug, refused
    // with the stable code (§15.4: errors teach) before any pass runs.
    // The code rides the JSON error document (the machine-consumer
    // surface the other refusals pin).
    let fx = released_fixture("cli-bench-zero")?;
    let (ok, stdout, stderr) = halyard(
        fx.root(),
        &["bench", "--iterations", "0", "--format", "json"],
    )?;
    assert!(!ok, "zero iterations refuses: {stdout}{stderr}");
    let value: serde_json::Value = serde_json::from_str(stdout.trim())?;
    assert_eq!(
        value.pointer("/error/code"),
        Some(&serde_json::json!("E-CLI-BENCH")),
        "the refusal carries the stable code: {stdout}"
    );
    assert!(
        value
            .pointer("/error/message")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|m| m.contains("at least one timed pass")),
        "the refusal teaches the fix: {stdout}"
    );
    Ok(())
}

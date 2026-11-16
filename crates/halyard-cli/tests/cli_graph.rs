//! CLI `graph` gates (§16.2): the command's own consumer test — its
//! absence at HEAD is exactly what let the flag collision ship (the
//! global `--format` and the graph's own rendering flag shared one
//! argument id, which panicked on every invocation before the rename to
//! `--graph-format`). Pins the four renderings and the global/output
//! format interplay.

use std::process::Command;

use halyard_test_support::fixture::{CommitSpec, GitFixture};

type BoxError = Box<dyn std::error::Error>;

/// A one-package cargo repo (the zero-config discovery shape).
fn fixture(label: &str) -> Result<GitFixture, BoxError> {
    let mut fx = GitFixture::new(label)?;
    let scaffold = vec![
        (
            "Cargo.toml".to_owned(),
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2021\"\n".to_owned(),
        ),
        (
            "src/lib.rs".to_owned(),
            "pub const C: usize = 1;\n".to_owned(),
        ),
    ];
    fx.commit_with(&CommitSpec::files(scaffold, "feat: scaffold"))?;
    Ok(fx)
}

/// Runs the binary; returns (exit ok, combined output).
fn halyard(root: &std::path::Path, args: &[&str]) -> Result<(bool, String), BoxError> {
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
    Ok((out.status.success(), text))
}

#[test]
fn graph_text_is_the_default_rendering() -> Result<(), BoxError> {
    let fx = fixture("cli-graph-text")?;
    let (ok, text) = halyard(fx.root(), &["graph"])?;
    assert!(ok, "graph runs without a panic: {text}");
    assert!(
        text.contains("(no workspace edges)"),
        "a single-package repo has no edges and says so honestly: {text}"
    );
    Ok(())
}

#[test]
fn graph_renderings_ride_the_graph_format_flag() -> Result<(), BoxError> {
    let fx = fixture("cli-graph-renders")?;
    let (ok, mermaid) = halyard(fx.root(), &["graph", "--graph-format", "mermaid"])?;
    assert!(ok, "{mermaid}");
    assert!(
        mermaid.contains("graph TD"),
        "the mermaid rendering is a graph snippet: {mermaid}"
    );
    let (ok, dot) = halyard(fx.root(), &["graph", "--graph-format", "dot"])?;
    assert!(ok, "{dot}");
    assert!(
        dot.contains("digraph") || dot.contains("->"),
        "the dot rendering is graphviz-shaped: {dot}"
    );
    Ok(())
}

#[test]
fn the_global_format_still_selects_the_output() -> Result<(), BoxError> {
    let fx = fixture("cli-graph-output-format")?;
    let (ok, text) = halyard(fx.root(), &["graph", "--format", "json"])?;
    assert!(
        ok,
        "the global --format json parses and the command runs: {text}"
    );
    let value: serde_json::Value = serde_json::from_str(text.trim())
        .map_err(|e| format!("stdout is one JSON document: {e}\n{text}"))?;
    assert!(
        value.is_array(),
        "the json output format renders the edges array: {text}"
    );
    Ok(())
}

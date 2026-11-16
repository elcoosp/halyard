//! CLI `man` gates (§16.2, shell integration): `halyard man` renders the
//! `halyard(1)` page and `halyard man <command>` renders one
//! subcommand's page, both from the clap definition itself — the single
//! source of truth the completions renderer uses, so a flag added to the
//! parser appears on the page without a second edit. Pins:
//!
//! * the top page's roff sections (NAME/SYNOPSIS/DESCRIPTION/COMMANDS/
//!   OPTIONS/EXIT CODES) and every subcommand listed;
//! * a per-command page's `halyard <command>` synopsis and its OPTIONS
//!   section;
//! * roff escaping — long flags render `\-\-flag` (a literal `-` starts
//!   a control line in roff);
//! * I3 byte-stability across runs;
//! * an unknown topic is clap's own usage error (exit 2), not a panic —
//!   the valid topics are named.

use std::process::Command;

use halyard_test_support::fixture::{CommitSpec, GitFixture};

type BoxError = Box<dyn std::error::Error>;

/// A one-package cargo repo (the page content does not depend on the
/// repo; the fixture only gives the command a working directory).
fn fixture(label: &str) -> Result<GitFixture, BoxError> {
    let mut fx = GitFixture::new(label)?;
    let scaffold = vec![(
        "Cargo.toml".to_owned(),
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2021\"\n".to_owned(),
    )];
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
fn the_top_page_carries_the_roff_sections_and_every_command() -> Result<(), BoxError> {
    let fx = fixture("cli-man-top")?;
    let (ok, page) = halyard(fx.root(), &["man"])?;
    assert!(ok, "the top page renders: {page}");
    for section in [
        ".TH HALYARD 1",
        ".SH NAME",
        ".SH SYNOPSIS",
        ".SH DESCRIPTION",
        ".SH COMMANDS",
        ".SH OPTIONS",
        ".SH EXIT CODES",
    ] {
        assert!(page.contains(section), "the page carries {section}: {page}");
    }
    for command in [
        "plan",
        "apply",
        "pr",
        "release",
        "publish",
        "migrate",
        "init",
        "explain",
        "check",
        "affected",
        "fetch",
        "status",
        "graph",
        "doctor",
        "forge-smoke",
        "schema",
        "completions",
        "man",
    ] {
        assert!(
            page.contains(&format!(".B {command}\n")),
            "COMMANDS lists {command}: {page}"
        );
    }
    Ok(())
}

#[test]
fn a_command_page_synopsizes_the_command_and_lists_its_options() -> Result<(), BoxError> {
    let fx = fixture("cli-man-command")?;
    let (ok, page) = halyard(fx.root(), &["man", "affected"])?;
    assert!(ok, "the affected page renders: {page}");
    assert!(
        page.contains(".SH NAME") && page.contains("halyard\\-affected"),
        "the page's NAME is the hyphen-escaped halyard-affected: {page}"
    );
    assert!(
        page.contains(".B halyard affected"),
        "the SYNOPSIS shows the command: {page}"
    );
    assert!(
        page.contains("\\-\\-base"),
        "the affected page lists its --base option roff-escaped: {page}"
    );
    Ok(())
}

#[test]
fn every_subcommand_has_a_page() -> Result<(), BoxError> {
    let fx = fixture("cli-man-all")?;
    for command in [
        "plan",
        "apply",
        "pr",
        "release",
        "publish",
        "migrate",
        "init",
        "explain",
        "check",
        "affected",
        "fetch",
        "status",
        "graph",
        "doctor",
        "forge-smoke",
        "schema",
        "completions",
        "man",
    ] {
        let (ok, page) = halyard(fx.root(), &["man", command])?;
        assert!(ok, "the {command} page renders: {page}");
        assert!(
            page.contains(".SH NAME") && page.contains(".SH SYNOPSIS"),
            "the {command} page carries the NAME and SYNOPSIS sections: {page}"
        );
    }
    Ok(())
}

#[test]
fn pages_are_byte_stable_across_runs() -> Result<(), BoxError> {
    let fx = fixture("cli-man-stable")?;
    let (ok, first) = halyard(fx.root(), &["man", "plan"])?;
    assert!(ok, "{first}");
    let (ok, second) = halyard(fx.root(), &["man", "plan"])?;
    assert!(ok, "{second}");
    assert_eq!(first, second, "I3: the same page renders byte-identical");
    Ok(())
}

#[test]
fn an_unknown_topic_is_the_usage_error_not_a_panic() -> Result<(), BoxError> {
    let fx = fixture("cli-man-unknown")?;
    let (ok, text) = halyard(fx.root(), &["man", "does-not-exist"])?;
    assert!(!ok, "an unknown topic refuses");
    assert!(
        text.contains("expected") || text.contains("possible values"),
        "the refusal names the valid topics: {text}"
    );
    assert!(
        !text.contains("Mismatch between definition and access"),
        "no clap downcast panic: {text}"
    );
    Ok(())
}

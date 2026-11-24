//! The composite action template's contract with the shipped surface
//! (§12 distribution, the M5.4 own-artifact slice, ADR-0052).
//!
//! Plan line pinned (`docs/HALYARD_ARCHITECTURE.md` §12,
//! "Distribution"):
//! "tiny JS-free *composite* action that (1) resolves OS/arch, (2)
//! downloads the pre-built static binary from GitHub Releases, (3)
//! verifies checksum **and** GitHub artifact attestation, (4) caches by
//! version, (5) execs `halyard action`." — the §17 M5.4 row's
//! acceptance ("`gh attestation verify` passes") is the release
//! pipeline's half of the same contract.
//!
//! The template cannot drift from the CLI: its `inputs` block must be
//! exactly the grammar's §12 list ([`halyard_action::distribution::
//! ACTION_INPUTS`]), every argv-mapped input must ride a flag the real
//! binary accepts (the man/completions single-source rule — the surface
//! is the shipped binary itself), the `mode` vocabulary must be the
//! binary's own enumeration, and the re-exposed `outputs` must be the
//! §12 keys the writers emit.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use halyard_action::distribution::{
    ACTION_INPUTS, ACTION_MODES, ACTION_OUTPUTS, CHECKSUMS_FILE, parse_action_template_inputs,
};

type BoxError = Box<dyn std::error::Error>;

/// The repo root (the template lives at the §7.1 as-built path, not
/// inside any crate).
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../")
}

/// Runs the built binary and returns its combined output text.
fn halyard_output(args: &[&str]) -> Result<String, BoxError> {
    let out = Command::new(env!("CARGO_BIN_EXE_halyard"))
        .args(args)
        .env("NO_COLOR", "1")
        .output()?;
    Ok(format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    ))
}

#[test]
fn action_template_lives_at_the_plan_declared_path() -> Result<(), BoxError> {
    let template = repo_root().join("action/action.yml");
    let text = fs::read_to_string(&template).map_err(|e| {
        format!("the §12 composite action template must exist at action/action.yml: {e}")
    })?;
    assert!(
        text.contains("using: composite"),
        "the template is a composite action (§12: \"tiny JS-free composite action\")"
    );
    assert!(
        text.contains(CHECKSUMS_FILE),
        "the checksum step names the manifest file the release pipeline wrote ({CHECKSUMS_FILE})"
    );
    Ok(())
}

#[test]
fn template_inputs_are_exactly_the_s12_list() -> Result<(), BoxError> {
    let text = fs::read_to_string(repo_root().join("action/action.yml"))?;
    let template_inputs = parse_action_template_inputs(&text)
        .map_err(|e| format!("the template's inputs block parses: {e}"))?;
    assert_eq!(
        template_inputs.len(),
        ACTION_INPUTS.len(),
        "the template carries exactly the §12 input list"
    );
    for input in ACTION_INPUTS {
        let rendered = template_inputs
            .iter()
            .find(|entry| entry.name == input.name)
            .unwrap_or_else(|| panic!("the template declares the §12 input `{}`", input.name));
        assert_eq!(
            rendered.required, input.required,
            "input `{}`: the required flag matches the grammar",
            input.name
        );
        assert_eq!(
            rendered.default.as_deref(),
            input.default,
            "input `{}`: the default matches the grammar",
            input.name
        );
    }
    Ok(())
}

#[test]
fn argv_mapped_inputs_ride_real_flags() -> Result<(), BoxError> {
    let action_help = halyard_output(&["action", "--help"])?;
    let root_help = halyard_output(&["--help"])?;
    for input in ACTION_INPUTS {
        let halyard_action::distribution::InputSurface::Arg { flag } = input.surface else {
            continue;
        };
        let needle = format!("--{flag}");
        // `--config` is a global flag (root help); the action's own
        // flags live on the subcommand's help.
        let surface = if input.name == "config" {
            &root_help
        } else {
            &action_help
        };
        assert!(
            surface.contains(&needle),
            "the §12 input `{}` maps to the real flag `{needle}`",
            input.name
        );
    }
    Ok(())
}

#[test]
fn mode_input_values_are_the_binary_enumeration() -> Result<(), BoxError> {
    let output = halyard_output(&["action", "--mode", "bogus"])?;
    for mode in ACTION_MODES {
        assert!(
            output.contains(mode),
            "the binary's own `--mode` enumeration carries `{mode}` (§12 inputs: auto|pr|release|plan|check); got: {output}"
        );
    }
    Ok(())
}

#[test]
fn template_outputs_are_the_s12_keys() -> Result<(), BoxError> {
    let text = fs::read_to_string(repo_root().join("action/action.yml"))?;
    for output in ACTION_OUTPUTS {
        assert!(
            text.contains(&format!("{output}:")),
            "the template re-exposes the §12 output `{output}`"
        );
    }
    Ok(())
}

#[test]
fn release_pipeline_pins_the_attestation_grammar() -> Result<(), BoxError> {
    let workflow =
        fs::read_to_string(repo_root().join(".github/workflows/release.yml")).map_err(|e| {
            format!("the release pipeline must exist at .github/workflows/release.yml: {e}")
        })?;
    assert!(
        workflow.contains("id-token: write"),
        "M5.4: the attestation job holds the verified `id-token: write` permission"
    );
    assert!(
        workflow.contains("actions/attest"),
        "M5.4: the pipeline generates artifact attestations (the `gh attestation verify` acceptance's producer)"
    );
    assert!(
        workflow.contains(CHECKSUMS_FILE),
        "the release pipeline writes the {CHECKSUMS_FILE} manifest the composite action verifies"
    );
    Ok(())
}

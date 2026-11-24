//! `check-release`: the §12 composite distribution's drift gate
//! (ADR-0052). The two hand-written templates — the composite action
//! (`action/action.yml`) and the release pipeline
//! (`.github/workflows/release.yml`) — must stay pinned to the
//! single-sourced grammar (`halyard_action::distribution`): the §12
//! input/output vocabulary, the runner-platform table and asset names,
//! the checksum manifest's canonical order, the attestation grammar.
//! The same single-source-of-truth discipline as `schema-check` (the
//! frozen files must match the typed source) and `check-layers` (the
//! manifests must match the rules): the check runs over the repo's
//! real files and refuses the first divergence with its location.
//!
//! Gate: `cargo run -p xtask -- check-release` — wired into CI beside
//! the other structure gates.

use std::path::Path;
use std::path::PathBuf;

use halyard_action::distribution::{
    CHECKSUMS_FILE, RELEASE_REPO, RELEASE_TARGETS, parse_action_template_inputs,
};

/// Locates the repository root from the crate's own position (the
/// `schema-check` convention — no dependence on the caller's cwd).
fn workspace_root() -> Result<PathBuf, String> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../")
        .canonicalize()
        .map_err(|e| format!("cannot resolve the workspace root from the xtask crate: {e}"))
}

/// Runs the full template check: both files, every pinned surface.
pub fn check_release() -> Result<(), String> {
    let root = workspace_root()?;
    let action_text = std::fs::read_to_string(root.join("action/action.yml"))
        .map_err(|e| format!("cannot read action/action.yml: {e}"))?;
    let release_text = std::fs::read_to_string(root.join(".github/workflows/release.yml"))
        .map_err(|e| format!("cannot read .github/workflows/release.yml: {e}"))?;
    let component_text = std::fs::read_to_string(root.join("templates/halyard.yml"))
        .map_err(|e| format!("cannot read templates/halyard.yml: {e}"))?;
    check_action_template(&action_text)?;
    check_release_workflow(&release_text)?;
    check_gitlab_component(&component_text)?;
    println!(
        "check-release: action/action.yml + .github/workflows/release.yml + templates/halyard.yml pinned to the distribution grammar ({} targets)",
        RELEASE_TARGETS.len()
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// The composite action template
// ---------------------------------------------------------------------------

fn check_action_template(text: &str) -> Result<(), String> {
    check_action_inputs(text)?;
    // The composite grammar (§12: "tiny JS-free composite action").
    if !text.contains("using: composite") {
        return Err("action/action.yml runs.using must be `composite`".to_owned());
    }
    check_platform_table(text)?;
    check_step_surfaces(text)?;
    check_action_outputs(text)?;
    Ok(())
}

/// The §12 input list: names, required flags and defaults must be the
/// grammar's own (the restricted reader refuses any shape it cannot
/// pin).
fn check_action_inputs(text: &str) -> Result<(), String> {
    let template_inputs = parse_action_template_inputs(text).map_err(|error| {
        format!("action/action.yml inputs block does not fit the grammar: {error}")
    })?;
    if template_inputs.len() != halyard_action::distribution::ACTION_INPUTS.len() {
        return Err(format!(
            "action/action.yml declares {} inputs, the §12 grammar pins {}",
            template_inputs.len(),
            halyard_action::distribution::ACTION_INPUTS.len()
        ));
    }
    for input in halyard_action::distribution::ACTION_INPUTS {
        let entry = template_inputs
            .iter()
            .find(|entry| entry.name == input.name)
            .ok_or_else(|| {
                format!(
                    "action/action.yml is missing the §12 input `{}`",
                    input.name
                )
            })?;
        if entry.required != input.required {
            return Err(format!(
                "action/action.yml input `{}`: required = {}, the grammar pins {}",
                input.name, entry.required, input.required
            ));
        }
        if entry.default.as_deref() != input.default {
            return Err(format!(
                "action/action.yml input `{}`: default = {:?}, the grammar pins {:?}",
                input.name, entry.default, input.default
            ));
        }
    }
    Ok(())
}

/// The runner-platform table: every `OS/ARCH) … target=<triple> ;
/// asset=<name>` row must match the pinned target set, the set must be
/// complete, and the fall-through arm must fail closed.
fn check_platform_table(text: &str) -> Result<(), String> {
    let mut resolved = 0_usize;
    let mut fail_closed_arm = false;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some((labels, rest)) = split_case_row(line) else {
            continue;
        };
        let is_platform_row = rest.contains("target=") || rest.contains("asset=");
        let is_fall_through = labels == "*" && rest.contains("exit 1");
        if !is_platform_row && !is_fall_through {
            continue;
        }
        if labels == "*" {
            if rest.contains("exit 1") {
                fail_closed_arm = true;
            }
            continue;
        }
        let Some((runner_os, runner_arch)) = labels.split_once('/') else {
            return Err(format!(
                "action/action.yml: malformed platform row {line:?}"
            ));
        };
        let triple = after(rest, "target=")
            .map(cut_at_separator)
            .ok_or_else(|| {
                format!(
                    "action/action.yml: platform row {labels} carries no target= (line {line:?})"
                )
            })?;
        let asset = after(rest, "asset=").map(cut_at_separator).ok_or_else(|| {
            format!("action/action.yml: platform row {labels} carries no asset= (line {line:?})")
        })?;
        let target = halyard_action::distribution::resolve_target(runner_os, runner_arch).map_err(
            |error| {
                format!("action/action.yml: platform row {labels} is not a pinned target: {error}")
            },
        )?;
        if triple != target.triple {
            return Err(format!(
                "action/action.yml: platform row {labels} names target {triple:?}, the grammar pins {:?}",
                target.triple
            ));
        }
        let expected_asset = target.asset_name();
        if asset != expected_asset {
            return Err(format!(
                "action/action.yml: platform row {labels} names asset {asset:?}, the grammar pins {expected_asset:?}"
            ));
        }
        resolved += 1;
    }
    if !fail_closed_arm {
        return Err(
            "action/action.yml: the platform case table's fall-through arm must fail closed (`exit 1`)"
                .to_owned(),
        );
    }
    if resolved != RELEASE_TARGETS.len() {
        return Err(format!(
            "action/action.yml: the platform table resolves {resolved} targets, the grammar pins {}",
            RELEASE_TARGETS.len()
        ));
    }
    Ok(())
}

/// The §12 steps 2–5 surface: the checksum manifest and the
/// attestation verifier over this repository, the version-keyed cache,
/// and every input riding the surface the grammar names.
fn check_step_surfaces(text: &str) -> Result<(), String> {
    // The §12 step-2/3 surface: the checksum manifest and the
    // attestation verifier over this repository.
    if !text.contains(CHECKSUMS_FILE) {
        return Err(format!(
            "action/action.yml never names {CHECKSUMS_FILE} (the §12 step-3 checksum half)"
        ));
    }
    if !text.contains("gh attestation verify") {
        return Err(
            "action/action.yml never runs `gh attestation verify` (the §12 step-3 attestation half)"
                .to_owned(),
        );
    }
    if !text.contains(&format!("-R {RELEASE_REPO}")) {
        return Err(format!(
            "action/action.yml: the attestation verify must name -R {RELEASE_REPO}"
        ));
    }

    // §12 step 4: the version-keyed cache.
    if !text.contains("actions/cache@v4") {
        return Err(
            "action/action.yml never caches (the §12 step-4 `caches by version`)".to_owned(),
        );
    }

    // §12 step 5: the exec surface — every input rides the surface the
    // grammar names.
    if !text.contains("action --mode") {
        return Err("action/action.yml: the exec step must run `halyard action --mode`".to_owned());
    }
    if !text.contains("working-directory: ${{ inputs.working-directory }}") {
        return Err(
            "action/action.yml: the exec steps must ride the `working-directory` input".to_owned(),
        );
    }
    for input in halyard_action::distribution::ACTION_INPUTS {
        match input.surface {
            halyard_action::distribution::InputSurface::Arg { flag } => {
                if !text.contains(&format!("--{flag}"))
                    || !text.contains(&format!("inputs.{}", input.name))
                {
                    return Err(format!(
                        "action/action.yml: the §12 input `{}` must ride the real flag --{flag}",
                        input.name
                    ));
                }
            }
            halyard_action::distribution::InputSurface::Env { names, expr } => {
                for name in names {
                    let needle = format!("{name}: ${{{{ {expr} }}}}");
                    if !text.contains(&needle) {
                        return Err(format!(
                            "action/action.yml: the §12 input `{}` must set the env {name} (expected {needle:?})",
                            input.name
                        ));
                    }
                }
            }
            halyard_action::distribution::InputSurface::Exec { command } => {
                if !text.contains(command) {
                    return Err(format!(
                        "action/action.yml: the §12 input `{}` maps to the `{command}` executor, which the template never runs",
                        input.name
                    ));
                }
            }
            halyard_action::distribution::InputSurface::Cwd => {
                // Covered by the working-directory pin above.
            }
        }
    }
    Ok(())
}

/// The §12 outputs the writers emit, re-exposed from the exec step.
fn check_action_outputs(text: &str) -> Result<(), String> {
    let lines: Vec<&str> = text.lines().collect();
    for output in halyard_action::distribution::ACTION_OUTPUTS {
        let key = format!("  {output}:");
        let value_needle = format!("steps.halyard.outputs.{output}");
        let key_index = lines.iter().position(|line| *line == key);
        let found = key_index.is_some_and(|index| {
            lines
                .iter()
                .skip(index + 1)
                .take(2)
                .any(|following| following.contains(&value_needle))
        });
        if !found {
            return Err(format!(
                "action/action.yml: the §12 output `{output}` must re-expose {value_needle}"
            ));
        }
    }
    Ok(())
}

/// Splits a `case` table row into its labels and body (`Linux/X64)
/// target=… ; asset=… ;;`).
fn split_case_row(line: &str) -> Option<(&str, &str)> {
    let (labels, rest) = line.trim().split_once(')')?;
    Some((labels, rest))
}

/// The text after `needle` in `haystack`, if present.
fn after<'a>(haystack: &'a str, needle: &str) -> Option<&'a str> {
    let index = haystack.find(needle)?;
    Some(&haystack[index + needle.len()..])
}

/// Cuts a `case`-row value at the row's ` ;` separator and trims it.
fn cut_at_separator(value: &str) -> &str {
    let value = value.split(" ;").next().unwrap_or(value);
    value.trim()
}

// ---------------------------------------------------------------------------
// The release pipeline
// ---------------------------------------------------------------------------

fn check_release_workflow(text: &str) -> Result<(), String> {
    // The trigger: the release tags.
    if !text.contains("tags:") || !text.contains("\"v*\"") {
        return Err(
            ".github/workflows/release.yml must trigger on the `v*` release tags".to_owned(),
        );
    }

    // The M5.4 attestation permissions (VERIFIED grammar: id-token +
    // attestations; contents: write for the release upload).
    for permission in ["contents: write", "id-token: write", "attestations: write"] {
        if !text.contains(permission) {
            return Err(format!(
                ".github/workflows/release.yml is missing the permission `{permission}`"
            ));
        }
    }

    // The build matrix: exactly the pinned target set.
    let mut matrix = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(triple) = trimmed.strip_prefix("target: ") {
            matrix.push(triple.trim().to_owned());
        }
    }
    let pinned: Vec<&str> = RELEASE_TARGETS.iter().map(|target| target.triple).collect();
    let mut matrix_sorted = matrix.clone();
    matrix_sorted.sort_unstable();
    let mut pinned_sorted = pinned;
    pinned_sorted.sort_unstable();
    if matrix_sorted != pinned_sorted {
        return Err(format!(
            ".github/workflows/release.yml build matrix {matrix_sorted:?} != the pinned target set {pinned_sorted:?}"
        ));
    }

    // The attestation producer (the M5.4 row's `gh attestation verify`
    // acceptance rides this step).
    if !text.contains("uses: actions/attest") {
        return Err(
            ".github/workflows/release.yml never generates attestations (actions/attest)"
                .to_owned(),
        );
    }

    // The checksum manifest: assembled over the pinned assets in the
    // grammar's canonical (sorted) order, then uploaded with the
    // release.
    let manifest_order = manifest_asset_order(text)?;
    let expected = {
        let mut names: Vec<String> = RELEASE_TARGETS
            .iter()
            .map(halyard_action::distribution::ReleaseTarget::asset_name)
            .collect();
        names.sort();
        names
    };
    if manifest_order != expected {
        return Err(format!(
            ".github/workflows/release.yml assembles {CHECKSUMS_FILE} over {manifest_order:?}; the grammar's canonical order is {expected:?}"
        ));
    }
    if !text.contains("gh release upload") || !text.contains(CHECKSUMS_FILE) {
        return Err(format!(
            ".github/workflows/release.yml must upload {CHECKSUMS_FILE} with the release assets"
        ));
    }
    Ok(())
}

/// Extracts the asset list the manifest-assembly step feeds to
/// `sha256sum`, in order.
fn manifest_asset_order(text: &str) -> Result<Vec<String>, String> {
    let mut in_assembly = false;
    let mut order = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim_end();
        if trimmed.contains("sha256sum --") {
            in_assembly = true;
            continue;
        }
        if in_assembly {
            let inner = trimmed.trim_start();
            if inner.starts_with('>') {
                break;
            }
            let asset = inner
                .trim_start_matches([' ', '-', '\\'])
                .trim_end_matches('\\')
                .trim();
            if asset.is_empty() {
                continue;
            }
            if !asset.starts_with("halyard-") {
                return Err(format!(
                    ".github/workflows/release.yml: unexpected manifest input {asset:?}"
                ));
            }
            order.push(asset.to_owned());
        }
    }
    if order.is_empty() {
        return Err(format!(
            ".github/workflows/release.yml: no {CHECKSUMS_FILE} assembly found (a `sha256sum --` block over the pinned assets)"
        ));
    }
    Ok(order)
}

// ---------------------------------------------------------------------------
// The GitLab CI component (M7's CI-component row, ADR-0053)
// ---------------------------------------------------------------------------

/// Checks the GitLab CI component (`templates/halyard.yml`, the M7
/// row's "CI component" deliverable): the `spec:` inputs block, the
/// §12 mode vocabulary, the checksum verify and the download URL must
/// pin to the same grammar the composite action pins to. The
/// attestation verify is the GitHub lane's own surface (the `gh` CLI
/// over GitHub-hosted attestations) — the component documents its
/// absence, it never pretends it.
fn check_gitlab_component(text: &str) -> Result<(), String> {
    // The component grammar: `spec:` above the `---`, the job config
    // below (VERIFIED: docs.gitlab.com/ci/components — `spec.inputs`
    // with `default`/`description`, `$[[ inputs.NAME ]]` interpolation
    // below the separator).
    let Some((spec, job)) = text.split_once("\n---\n") else {
        return Err(
            "templates/halyard.yml must carry the `spec:` block above the `---` separator"
                .to_owned(),
        );
    };
    if !spec.contains("spec:") || !spec.contains("inputs:") {
        return Err("templates/halyard.yml: the spec block must declare inputs".to_owned());
    }

    // The pinned inputs, in the component's declaration order: the
    // job's stage, the §12 mode, the dry-run posture.
    for input in ["stage", "mode", "dry-run"] {
        let needle = format!("    {input}:");
        if !spec.contains(&needle) {
            return Err(format!(
                "templates/halyard.yml: the spec block is missing the `{input}` input"
            ));
        }
    }

    // The mode input's values are the §12 vocabulary (the component
    // documents the same five modes the binary enumerates).
    for mode in halyard_action::distribution::ACTION_MODES {
        if !text.contains(mode) {
            return Err(format!(
                "templates/halyard.yml: the mode input must name the §12 vocabulary value `{mode}`"
            ));
        }
    }

    // The stage rides the documented interpolation.
    if !job.contains("stage: $[[ inputs.stage ]]") {
        return Err(
            "templates/halyard.yml: the job's stage must ride `$[[ inputs.stage ]]`".to_owned(),
        );
    }

    // The exec line runs the binary with the mode input, and the dry
    // posture rides the component's own input.
    if !job.contains("halyard action --mode") {
        return Err(
            "templates/halyard.yml: the job must run `halyard action --mode` (the §12 step-5 surface)"
                .to_owned(),
        );
    }
    if !job.contains("$[[ inputs.dry-run ]]") {
        return Err(
            "templates/halyard.yml: the dry posture must ride `$[[ inputs.dry-run ]]`".to_owned(),
        );
    }

    // The checksum verify reads the same manifest the release pipeline
    // wrote (`sha256sum -c` over the pinned line) — the GitLab lane
    // verifies the same bytes the composite action does.
    if !job.contains(CHECKSUMS_FILE) {
        return Err(format!(
            "templates/halyard.yml: the job must verify the {CHECKSUMS_FILE} manifest line"
        ));
    }
    if !job.contains("sha256sum -c") {
        return Err("templates/halyard.yml: the job's verify must run `sha256sum -c`".to_owned());
    }

    // The download host is the release origin the grammar pins (the
    // component declares the repo as a job variable and downloads
    // from its releases).
    if !job.contains(&format!("GITHUB_REPO: {RELEASE_REPO}")) {
        return Err(format!(
            "templates/halyard.yml: the job must declare GITHUB_REPO: {RELEASE_REPO} (the release origin)"
        ));
    }
    if !job.contains("releases/download/") {
        return Err(format!(
            "templates/halyard.yml: the job must download from github.com/{RELEASE_REPO} releases"
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use halyard_action::distribution::ArchiveKind;

    #[test]
    fn the_gate_is_green_over_the_shipped_templates() {
        check_release().unwrap_or_else(|error| panic!("check-release must be green: {error}"));
    }

    #[test]
    fn the_archive_kinds_follow_the_platform() {
        let windows = RELEASE_TARGETS
            .iter()
            .find(|target| target.archive == ArchiveKind::Zip)
            .expect("the pinned set carries the Windows zip");
        let unix = RELEASE_TARGETS
            .iter()
            .find(|target| target.archive == ArchiveKind::TarGz)
            .expect("the pinned set carries tar.gz targets");
        assert_eq!(
            windows.asset_name(),
            format!("halyard-{}{}", windows.triple, windows.archive.extension())
        );
        assert_eq!(
            unix.asset_name(),
            format!("halyard-{}{}", unix.triple, unix.archive.extension())
        );
        // The §12 input table stays the pinned eight.
        assert_eq!(halyard_action::distribution::ACTION_INPUTS.len(), 8);
    }
}

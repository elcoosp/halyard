//! The §12 composite distribution's grammar (the M5.4 own-artifact
//! slice, ADR-0052): the plain-data contract the composite action
//! template (`action/action.yml`) and the release pipeline
//! (`.github/workflows/release.yml`) both pin, so the three artifacts
//! cannot drift apart. The templates are hand-written YAML; every
//! piece of them that must agree with the shipped binary or with each
//! other is single-sourced HERE:
//!
//! * the §12 input list ([`ACTION_INPUTS`]) and the §12 output keys
//!   ([`ACTION_OUTPUTS`]) — the plan's own pinned vocabulary (§12:
//!   "`mode` (`auto|pr|release|plan|check`), `token`,
//!   `app-id`/`app-private-key`, `config`, `publish` (bool),
//!   `dry-run`, `working-directory`");
//! * the release-target set ([`RELEASE_TARGETS`]) — the static
//!   binaries the pipeline builds and the composite action resolves
//!   the runner to, with the runner-label mapping (`RUNNER_OS`/
//!   `RUNNER_ARCH`) and the asset-name grammar;
//! * the checksum manifest ([`ChecksumsManifest`]) — the
//!   `sha256sum`-compatible grammar (`<digest>␣␣<file>`, sorted, I3)
//!   the pipeline writes and the composite action verifies;
//! * the restricted template reader ([`parse_action_template_inputs`])
//!   — the fail-closed parser the template checks (the xtask gate and
//!   the CLI contract test) run over the template's own `inputs` block.
//!
//! Layer note: `action → core` + `serde_json` (unchanged) + `sha2`
//! (the digest the checksum grammar verifies) — pure data and pure
//! functions, no I/O; the files themselves are read by the gates
//! (`cargo run -p xtask -- check-release`, the CLI contract test), the
//! runners execute the templates.
//!
//! VERIFIED quotes live in `docs/research/own-artifact-attestation.md`
//! (the `gh attestation verify` grammar, the composite-action metadata
//! grammar, the PowerShell `Get-FileHash` digest form).

use std::collections::BTreeMap;
use std::fmt;

use sha2::{Digest, Sha256};

use crate::ActionError;

// ---------------------------------------------------------------------------
// The pinned constants
// ---------------------------------------------------------------------------

/// The release origin the composite action downloads from and the
/// pipeline attests against (§12: "downloads the pre-built static
/// binary from GitHub Releases").
pub const RELEASE_REPO: &str = "elcoosp/halyard";

/// The checksum manifest the pipeline writes and the composite action
/// verifies (§12 step 3: "verifies checksum **and** GitHub artifact
/// attestation").
pub const CHECKSUMS_FILE: &str = "SHA256SUMS";

/// The §12 `mode` input's vocabulary — the binary's own `ActionMode`
/// enumeration (`auto|pr|release|plan|check`, §12's pinned order).
pub const ACTION_MODES: &[&str] = &["auto", "pr", "release", "plan", "check"];

/// The §12 output keys the composite action re-exposes from the
/// `halyard action` step. The per-package `--version`/`--tag` keys the
/// writers emit are dynamic (one pair per release) and a composite
/// action's `outputs` block is a static declaration — they stay
/// readable on the step's own outputs (`steps.<id>.outputs`), never
/// re-exposed.
pub const ACTION_OUTPUTS: &[&str] = &[
    "released",
    "published",
    "pr_number",
    "pr_url",
    "releases_json",
    "plan_json",
    "paths_released",
];

// ---------------------------------------------------------------------------
// Release targets and asset names
// ---------------------------------------------------------------------------

/// The archive form one release asset ships as (`§12`: the
/// "pre-built static binary").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArchiveKind {
    /// A gzip-compressed tarball (the Unix targets).
    TarGz,
    /// A zip archive (the Windows target — PowerShell's native
    /// `Compress-Archive` form).
    Zip,
}

impl ArchiveKind {
    /// The asset's file extension, including the leading dot.
    #[must_use]
    pub const fn extension(self) -> &'static str {
        match self {
            ArchiveKind::TarGz => ".tar.gz",
            ArchiveKind::Zip => ".zip",
        }
    }
}

/// One pinned release target: the static triple the pipeline builds,
/// the runner labels the composite action resolves, and the archive
/// the asset ships as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReleaseTarget {
    /// The Rust target triple (`cargo build --target`).
    pub triple: &'static str,
    /// The `RUNNER_OS` label the composite action resolves
    /// (`Linux` / `macOS` / `Windows` — GitHub's own values).
    pub runner_os: &'static str,
    /// The `RUNNER_ARCH` label (`X64` / `ARM64`).
    pub runner_arch: &'static str,
    /// The archive form the asset ships as.
    pub archive: ArchiveKind,
    /// The executable's name inside the archive.
    pub binary_name: &'static str,
}

impl ReleaseTarget {
    /// The release asset's name (`halyard-<triple><ext>`): the
    /// pipeline packages under it, the composite action downloads it
    /// and the checksum manifest names it — one grammar, three
    /// consumers.
    #[must_use]
    pub fn asset_name(&self) -> String {
        format!("halyard-{}{}", self.triple, self.archive.extension())
    }

    /// The asset's pinned-release download URL.
    #[must_use]
    pub fn download_url(&self, tag: &str) -> String {
        format!(
            "https://github.com/{RELEASE_REPO}/releases/download/{tag}/{}",
            self.asset_name()
        )
    }
}

/// The pinned release-target set (§7.1's platform row: the CI matrix's
/// Linux/macOS/Windows, both x64 and arm64 where the platform runs
/// them). The pipeline builds exactly this set; `require_coverage`
/// refuses a manifest that skips one.
pub const RELEASE_TARGETS: &[ReleaseTarget] = &[
    ReleaseTarget {
        triple: "x86_64-unknown-linux-musl",
        runner_os: "Linux",
        runner_arch: "X64",
        archive: ArchiveKind::TarGz,
        binary_name: "halyard",
    },
    ReleaseTarget {
        triple: "aarch64-unknown-linux-musl",
        runner_os: "Linux",
        runner_arch: "ARM64",
        archive: ArchiveKind::TarGz,
        binary_name: "halyard",
    },
    ReleaseTarget {
        triple: "x86_64-apple-darwin",
        runner_os: "macOS",
        runner_arch: "X64",
        archive: ArchiveKind::TarGz,
        binary_name: "halyard",
    },
    ReleaseTarget {
        triple: "aarch64-apple-darwin",
        runner_os: "macOS",
        runner_arch: "ARM64",
        archive: ArchiveKind::TarGz,
        binary_name: "halyard",
    },
    ReleaseTarget {
        triple: "x86_64-pc-windows-msvc",
        runner_os: "Windows",
        runner_arch: "X64",
        archive: ArchiveKind::Zip,
        binary_name: "halyard.exe",
    },
];

/// Resolves the composite action's runtime platform (the runner's own
/// labels) to its pinned target. Unknown labels fail closed — the
/// action never downloads an asset it cannot name.
///
/// # Errors
///
/// [`ActionError::Distribution`] naming the labels when the pair has
/// no pinned target.
pub fn resolve_target(
    runner_os: &str,
    runner_arch: &str,
) -> Result<&'static ReleaseTarget, ActionError> {
    RELEASE_TARGETS
        .iter()
        .find(|target| target.runner_os == runner_os && target.runner_arch == runner_arch)
        .ok_or_else(|| ActionError::Distribution {
            message: format!(
                "no pinned release target for RUNNER_OS={runner_os:?} RUNNER_ARCH={runner_arch:?}; \
                 the pinned set is {:?}",
                RELEASE_TARGETS.iter().map(|t| t.triple).collect::<Vec<_>>()
            ),
        })
}

// ---------------------------------------------------------------------------
// The checksum manifest
// ---------------------------------------------------------------------------

fn sha256_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(64);
    for byte in digest {
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

fn is_lowercase_hex(text: &str) -> bool {
    text.len() == 64
        && text
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
}

/// One `<digest>␣␣<file>` line of the [`CHECKSUMS_FILE`] manifest (the
/// `sha256sum`-compatible check grammar: exactly two separating
/// spaces, lowercase hex digest).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChecksumLine {
    /// The asset's file name.
    pub file: String,
    /// The asset's SHA-256 digest, lowercase hex.
    pub sha256: String,
}

/// The checksum manifest: the pipeline writes it (sorted by file name,
/// I3), the composite action verifies the downloaded asset against its
/// line, and `require_coverage` proves it names every pinned target.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChecksumsManifest {
    lines: Vec<ChecksumLine>,
}

impl ChecksumsManifest {
    /// Parses a manifest in the checker grammar. Fail-closed: a line
    /// that is not `<64 lowercase hex>␣␣<file>`, a duplicate file, or
    /// an unsorted file order is refused with [`ActionError::
    /// Distribution`] — a manifest the checker would misread never
    /// passes.
    ///
    /// # Errors
    ///
    /// [`ActionError::Distribution`] naming the offending line.
    pub fn parse(text: &str) -> Result<Self, ActionError> {
        let mut lines = Vec::new();
        let mut previous_file: Option<&str> = None;
        for line in text.lines().filter(|line| !line.is_empty()) {
            let Some((sha256, file)) = line.split_once("  ") else {
                return Err(ActionError::Distribution {
                    message: format!(
                        "checksum line {line:?} is not the `<digest>  <file>` grammar \
                         (two separating spaces)"
                    ),
                });
            };
            if !is_lowercase_hex(sha256) {
                return Err(ActionError::Distribution {
                    message: format!(
                        "checksum line {line:?}: the digest must be 64 lowercase hex characters"
                    ),
                });
            }
            if file.is_empty() || file.contains(' ') {
                return Err(ActionError::Distribution {
                    message: format!(
                        "checksum line {line:?}: the file name must be non-empty and space-free"
                    ),
                });
            }
            if let Some(previous) = previous_file
                && file < previous
            {
                return Err(ActionError::Distribution {
                    message: format!(
                        "checksum manifest is not sorted by file name: {file:?} follows {previous:?} (I3)"
                    ),
                });
            }
            if lines
                .iter()
                .any(|existing: &ChecksumLine| existing.file == file)
            {
                return Err(ActionError::Distribution {
                    message: format!("checksum manifest names {file:?} twice"),
                });
            }
            previous_file = Some(file);
            lines.push(ChecksumLine {
                file: file.to_owned(),
                sha256: sha256.to_owned(),
            });
        }
        if lines.is_empty() {
            return Err(ActionError::Distribution {
                message: "the checksum manifest is empty".to_owned(),
            });
        }
        Ok(Self { lines })
    }

    /// Builds the manifest from the release assets (the pipeline's
    /// assembly half): digests computed here, lines sorted by file
    /// name.
    ///
    /// # Errors
    ///
    /// [`ActionError::Distribution`] on a duplicate file name.
    pub fn from_files(assets: &[(&str, &[u8])]) -> Result<Self, ActionError> {
        let mut by_file = BTreeMap::new();
        for (file, bytes) in assets {
            let line = ChecksumLine {
                file: (*file).to_owned(),
                sha256: sha256_hex(bytes),
            };
            if by_file.insert((*file).to_owned(), line).is_some() {
                return Err(ActionError::Distribution {
                    message: format!("the asset list names {file:?} twice"),
                });
            }
        }
        Ok(Self {
            lines: by_file.into_values().collect(),
        })
    }

    /// The canonical rendering (the checker's own input form, one
    /// line per asset, sorted — I3).
    #[must_use]
    pub fn render(&self) -> String {
        let mut text = String::new();
        for line in &self.lines {
            text.push_str(&line.sha256);
            text.push_str("  ");
            text.push_str(&line.file);
            text.push('\n');
        }
        text
    }

    /// The manifest's lines, in render order.
    #[must_use]
    pub fn lines(&self) -> &[ChecksumLine] {
        &self.lines
    }

    /// Verifies one downloaded asset against its line (§12 step 3's
    /// checksum half): the manifest must name the file and the
    /// downloaded bytes must digest to the recorded value.
    ///
    /// # Errors
    ///
    /// [`ActionError::Distribution`] when the file is unnamed or the
    /// digest mismatches (a tampered or partial download).
    pub fn verify(&self, file: &str, bytes: &[u8]) -> Result<(), ActionError> {
        let line = self
            .lines
            .iter()
            .find(|line| line.file == file)
            .ok_or_else(|| ActionError::Distribution {
                message: format!("the checksum manifest does not name {file:?}"),
            })?;
        let actual = sha256_hex(bytes);
        if actual != line.sha256 {
            return Err(ActionError::Distribution {
                message: format!(
                    "the downloaded {file:?} does not match its checksum line: expected {}, got {actual}",
                    line.sha256
                ),
            });
        }
        Ok(())
    }

    /// Proves the manifest covers the pinned target set (the
    /// pipeline's gate): every target's asset must be named — a
    /// release that ships without one platform's binary is refused,
    /// never silently partial.
    ///
    /// # Errors
    ///
    /// [`ActionError::Distribution`] naming the uncovered asset.
    pub fn require_coverage(&self, targets: &[&ReleaseTarget]) -> Result<(), ActionError> {
        for target in targets {
            let asset = target.asset_name();
            if !self.lines.iter().any(|line| line.file == asset) {
                return Err(ActionError::Distribution {
                    message: format!(
                        "the checksum manifest does not cover the pinned target {asset:?}"
                    ),
                });
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// The §12 input/output vocabulary
// ---------------------------------------------------------------------------

/// How the composite action feeds one §12 input to the shipped
/// surface. Every variant names a real surface of the `halyard` shell
/// — the template cannot pass an input anywhere the binary does not
/// read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputSurface {
    /// A CLI flag of the `halyard action` command (or the global flag
    /// the root command accepts).
    Arg {
        /// The flag's long form, without the leading dashes.
        flag: &'static str,
    },
    /// Environment variables the shell's own resolution reads (the
    /// forge-token contract, the App-JWT env wire-up).
    Env {
        /// The variables, in the order the template sets them.
        names: &'static [&'static str],
        /// The template expression the variables are set from (the
        /// `inputs.<name>` reference, with the token's
        /// `|| github.token` fall-back spelled out).
        expr: &'static str,
    },
    /// An extra executor the composite action runs when the input is
    /// `true` (§13's `halyard publish`; `dry-run` gates it to `--dry`).
    Exec {
        /// The executor command, as the template's publish step runs it.
        command: &'static str,
    },
    /// The exec steps' working directory (the shell's `-C` flag, §16.2).
    Cwd,
}

/// One §12 composite-action input: the plan's pinned vocabulary, its
/// default, and the surface it rides.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActionInput {
    /// The input's name (the template's `inputs:` key, §12's spelling).
    pub name: &'static str,
    /// Whether the template declares it required (every §12 input has
    /// a documented default or an honest empty — all optional).
    pub required: bool,
    /// The template's default value (`None` = no `default:` row).
    pub default: Option<&'static str>,
    /// The surface the input rides.
    pub surface: InputSurface,
}

/// The §12 input list (§12: "`mode` (`auto|pr|release|plan|check`),
/// `token`, `app-id`/`app-private-key`, `config`, `publish` (bool),
/// `dry-run`, `working-directory`"), in the template's declaration
/// order. `token` rides both spellings the shell reads — the GitHub
/// lane's native `GITHUB_TOKEN` and the cross-lane `HALYARD_FORGE_TOKEN`
/// contract.
pub const ACTION_INPUTS: &[ActionInput] = &[
    ActionInput {
        name: "mode",
        required: false,
        default: Some("plan"),
        surface: InputSurface::Arg { flag: "mode" },
    },
    ActionInput {
        name: "token",
        required: false,
        default: Some(""),
        surface: InputSurface::Env {
            names: &["GITHUB_TOKEN", "HALYARD_FORGE_TOKEN"],
            expr: "inputs.token || github.token",
        },
    },
    ActionInput {
        name: "app-id",
        required: false,
        default: Some(""),
        surface: InputSurface::Env {
            names: &["HALYARD_APP_ID"],
            expr: "inputs.app-id",
        },
    },
    ActionInput {
        name: "app-private-key",
        required: false,
        default: Some(""),
        surface: InputSurface::Env {
            names: &["HALYARD_APP_PRIVATE_KEY"],
            expr: "inputs.app-private-key",
        },
    },
    ActionInput {
        name: "config",
        required: false,
        default: Some(""),
        surface: InputSurface::Arg { flag: "config" },
    },
    ActionInput {
        name: "publish",
        required: false,
        default: Some("false"),
        surface: InputSurface::Exec {
            command: "halyard publish",
        },
    },
    ActionInput {
        name: "dry-run",
        required: false,
        default: Some("false"),
        surface: InputSurface::Arg { flag: "dry" },
    },
    ActionInput {
        name: "working-directory",
        required: false,
        default: Some("."),
        surface: InputSurface::Cwd,
    },
];

// ---------------------------------------------------------------------------
// The restricted template reader
// ---------------------------------------------------------------------------

/// One parsed `inputs:` entry of the composite template (the pieces
/// the contract pins: the name, the required flag, the default).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TemplateInput {
    /// The input's name.
    pub name: String,
    /// The declared `required:` value (`false` when absent).
    pub required: bool,
    /// The declared `default:` value (`None` when absent).
    pub default: Option<String>,
}

/// Parses the `inputs:` block of the composite template with the
/// restricted reader — the template's own regular shape, fail-closed
/// on anything else (the same restricted-grammar discipline as the
/// pnpm-lock scanner, §19 decision 2). The block starts at a
/// top-level `inputs:` line and ends at the next top-level key; each
/// entry is a two-space-indented `<name>:` line whose sub-keys sit at
/// four spaces. An unknown sub-key refuses — a template the reader
/// misreads must fail the gate, not pass it silently.
///
/// # Errors
///
/// [`ActionError::Distribution`] naming the offending line when the
/// block is absent or a line does not fit the grammar.
pub fn parse_action_template_inputs(yaml: &str) -> Result<Vec<TemplateInput>, ActionError> {
    let mut inputs = Vec::new();
    let mut in_block = false;
    for line in yaml.lines() {
        if !in_block {
            if line == "inputs:" {
                in_block = true;
            }
            continue;
        }
        // The block ends at the next top-level key (column 0).
        if !line.is_empty() && !line.starts_with(' ') {
            break;
        }
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            continue;
        }
        // Entry names sit at exactly two spaces.
        let indent = trimmed.len() - trimmed.trim_start_matches(' ').len();
        if indent == 2 {
            let Some(name) = trimmed.trim_end_matches(':').strip_prefix("  ") else {
                continue;
            };
            if name.is_empty() || name.contains(':') {
                return Err(ActionError::Distribution {
                    message: format!("template inputs entry {trimmed:?} is not a plain name"),
                });
            }
            inputs.push(TemplateInput {
                name: name.to_owned(),
                required: false,
                default: None,
            });
            continue;
        }
        // Sub-keys sit at exactly four spaces under the last entry.
        let Some(entry) = inputs.last_mut() else {
            return Err(ActionError::Distribution {
                message: format!("template line {trimmed:?} precedes the first input entry"),
            });
        };
        let Some(sub) = trimmed.strip_prefix("    ") else {
            return Err(ActionError::Distribution {
                message: format!(
                    "template inputs line {trimmed:?} is not a 2-space entry name or a 4-space sub-key"
                ),
            });
        };
        if let Some(value) = sub.strip_prefix("required: ") {
            let required = match value.trim_matches('\'') {
                "true" => true,
                "false" => false,
                other => {
                    return Err(ActionError::Distribution {
                        message: format!(
                            "template input `{}`: required must be true/false, got {other:?}",
                            entry.name
                        ),
                    });
                }
            };
            entry.required = required;
        } else if let Some(value) = sub.strip_prefix("default: ") {
            entry.default = Some(value.trim_matches('\'').to_owned());
        } else if sub.starts_with("description:") {
            // Free text — not pinned by the contract.
        } else {
            return Err(ActionError::Distribution {
                message: format!(
                    "template input `{}`: unknown sub-key {sub:?} (the grammar pins \
                     description/required/default)",
                    entry.name
                ),
            });
        }
    }
    if !in_block {
        return Err(ActionError::Distribution {
            message: "the template has no top-level `inputs:` block".to_owned(),
        });
    }
    Ok(inputs)
}

impl fmt::Display for ActionInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "1111111111111111111111111111111111111111111111111111111111111111  halyard-a.zip\n\
                          2222222222222222222222222222222222222222222222222222222222222222  halyard-b.tar.gz\n";

    #[test]
    fn asset_names_follow_the_archive_kind() {
        let unix = RELEASE_TARGETS
            .iter()
            .find(|t| t.triple == "x86_64-unknown-linux-musl")
            .unwrap_or_else(|| unreachable!("the pinned set carries the musl x64 target"));
        let windows = RELEASE_TARGETS
            .iter()
            .find(|t| t.archive == ArchiveKind::Zip)
            .unwrap_or_else(|| unreachable!("the pinned set carries the Windows zip"));
        assert_eq!(
            unix.asset_name(),
            "halyard-x86_64-unknown-linux-musl.tar.gz"
        );
        assert_eq!(windows.asset_name(), "halyard-x86_64-pc-windows-msvc.zip");
        assert_eq!(
            windows.binary_name, "halyard.exe",
            "the Windows archive carries the .exe"
        );
    }

    #[test]
    fn resolve_target_maps_the_runner_labels() {
        let target = resolve_target("Linux", "X64").unwrap_or_else(|e| unreachable!("{e}"));
        assert_eq!(target.triple, "x86_64-unknown-linux-musl");
        let arm = resolve_target("macOS", "ARM64").unwrap_or_else(|e| unreachable!("{e}"));
        assert_eq!(arm.triple, "aarch64-apple-darwin");
        let error = resolve_target("Plan9", "X64").unwrap_err();
        let ActionError::Distribution { message } = error else {
            unreachable!("resolve errors ride the distribution variant")
        };
        assert!(message.contains("no pinned release target"), "{message}");
    }

    #[test]
    fn manifest_round_trips_through_the_checker_grammar() {
        let manifest = ChecksumsManifest::parse(SAMPLE).unwrap_or_else(|e| unreachable!("{e}"));
        assert_eq!(manifest.render(), SAMPLE);
        assert_eq!(manifest.lines().len(), 2);
    }

    #[test]
    fn manifest_parse_refuses_bad_grammar() {
        // One space — the checker's binary-marker ambiguity, refused.
        assert!(ChecksumsManifest::parse("abc  f\n").is_err());
        assert!(
            ChecksumsManifest::parse(
                "1111111111111111111111111111111111111111111111111111111111111111 f\n"
            )
            .is_err()
        );
        // Uppercase hex.
        assert!(
            ChecksumsManifest::parse(
                "111111111111111111111111111111111111111111111111111111111111111A  f\n"
            )
            .is_err()
        );
        // Unsorted.
        assert!(ChecksumsManifest::parse("1111111111111111111111111111111111111111111111111111111111111111  b\n1111111111111111111111111111111111111111111111111111111111111111  a\n").is_err());
        // Duplicate.
        assert!(ChecksumsManifest::parse("1111111111111111111111111111111111111111111111111111111111111111  a\n1111111111111111111111111111111111111111111111111111111111111111  a\n").is_err());
        // Empty.
        assert!(ChecksumsManifest::parse("").is_err());
    }

    #[test]
    fn verify_refuses_a_tampered_download() {
        let manifest = ChecksumsManifest::from_files(&[("halyard-b.tar.gz", b"payload")])
            .unwrap_or_else(|e| unreachable!("{e}"));
        manifest
            .verify("halyard-b.tar.gz", b"payload")
            .unwrap_or_else(|e| unreachable!("{e}"));
        let error = manifest
            .verify("halyard-b.tar.gz", b"tampered")
            .unwrap_err();
        let ActionError::Distribution { message } = error else {
            unreachable!("verify errors ride the distribution variant")
        };
        assert!(
            message.contains("does not match its checksum line"),
            "{message}"
        );
        let error = manifest
            .verify("halyard-missing.zip", b"payload")
            .unwrap_err();
        let ActionError::Distribution { message } = error else {
            unreachable!("verify errors ride the distribution variant")
        };
        assert!(message.contains("does not name"), "{message}");
    }

    #[test]
    fn from_files_renders_sorted_and_refuses_duplicates() {
        let manifest = ChecksumsManifest::from_files(&[("z.tar.gz", b"z"), ("a.tar.gz", b"a")])
            .unwrap_or_else(|e| unreachable!("{e}"));
        let rendered = manifest.render();
        let a_index = rendered
            .find("a.tar.gz")
            .unwrap_or_else(|| unreachable!("a.tar.gz renders"));
        let z_index = rendered
            .find("z.tar.gz")
            .unwrap_or_else(|| unreachable!("z.tar.gz renders"));
        assert!(a_index < z_index, "I3: the manifest sorts by file name");
        assert!(ChecksumsManifest::from_files(&[("x", b"1"), ("x", b"2")]).is_err());
    }

    #[test]
    fn require_coverage_names_the_missing_target() {
        let manifest =
            ChecksumsManifest::from_files(&[("halyard-x86_64-unknown-linux-musl.tar.gz", b"x")])
                .unwrap_or_else(|e| unreachable!("{e}"));
        let all: Vec<&ReleaseTarget> = RELEASE_TARGETS.iter().collect();
        let error = manifest.require_coverage(&all).unwrap_err();
        let ActionError::Distribution { message } = error else {
            unreachable!("coverage errors ride the distribution variant")
        };
        assert!(message.contains("does not cover"), "{message}");
        let covered_assets: Vec<(String, &[u8])> = RELEASE_TARGETS
            .iter()
            .map(|t| (t.asset_name(), t.triple.as_bytes()))
            .collect();
        let covered = ChecksumsManifest::from_files(
            &covered_assets
                .iter()
                .map(|(name, bytes)| (name.as_str(), *bytes))
                .collect::<Vec<_>>(),
        )
        .unwrap_or_else(|e| unreachable!("{e}"));
        covered
            .require_coverage(&all)
            .unwrap_or_else(|e| unreachable!("{e}"));
    }

    #[test]
    fn the_s12_input_list_is_complete_and_defaults_are_honest() {
        // §12's pinned vocabulary, spelled exactly.
        let names: Vec<&str> = ACTION_INPUTS.iter().map(|input| input.name).collect();
        assert_eq!(
            names,
            vec![
                "mode",
                "token",
                "app-id",
                "app-private-key",
                "config",
                "publish",
                "dry-run",
                "working-directory"
            ]
        );
        assert!(
            ACTION_INPUTS.iter().all(|input| !input.required),
            "every §12 input carries a default or an honest empty"
        );
        // The mode input's default is the safe forecast posture.
        let mode = ACTION_INPUTS
            .iter()
            .find(|input| input.name == "mode")
            .unwrap_or_else(|| unreachable!("mode is in the §12 list"));
        assert_eq!(mode.default, Some("plan"));
        assert_eq!(ACTION_MODES.len(), 5);
    }

    #[test]
    fn template_reader_pins_the_regular_shape() {
        let template = "name: 'halyard'\ninputs:\n  mode:\n    description: 'mode'\n    required: false\n    default: 'plan'\n  token:\n    description: 'token'\n    required: false\n    default: ''\noutputs:\n  released:\n";
        let inputs = parse_action_template_inputs(template).unwrap_or_else(|e| unreachable!("{e}"));
        assert_eq!(
            inputs,
            vec![
                TemplateInput {
                    name: "mode".to_owned(),
                    required: false,
                    default: Some("plan".to_owned()),
                },
                TemplateInput {
                    name: "token".to_owned(),
                    required: false,
                    default: Some(String::new()),
                },
            ]
        );
        // The block must exist.
        assert!(parse_action_template_inputs("runs:\n  using: composite\n").is_err());
        // Unknown sub-keys fail closed.
        assert!(parse_action_template_inputs("inputs:\n  mode:\n    deprecation: 'x'\n").is_err());
    }
}

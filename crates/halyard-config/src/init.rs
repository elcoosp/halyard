//! The `halyard init` scaffold engine (§16.1/§16.2, ADR-0029): turn the
//! shell's discovery survey into a create-only write plan — the
//! documented `halyard.toml` skeleton, plus the §16.1 GitHub Actions
//! release workflow with `--github` — and land it through the injected
//! [`DraftSink`].
//!
//! Zero-config is the contract (§16.1): `halyard plan` works without
//! this file, so the skeleton sets **no keys** — the documented defaults
//! ride as comments and the loaded skeleton equals the built-in defaults
//! exactly (an invariant the tests pin). The command exists so the knobs
//! have a documented home in the repo, not because a config is required.
//!
//! Conventions (the M4.7 migration's, reused):
//!
//! * the engine stays pure — the shell surveys the tree (discovery) and
//!   hands over plain data ([`InitSurvey`]); every output is plain data;
//! * validate-then-act (§15.3) — [`plan_init`] is a pure decision,
//!   [`run_init`] gates **every** target's existence before the first
//!   write, so a refusal never leaves a half-scaffold behind;
//! * create-only, fail-closed — an existing `halyard.toml` or workflow
//!   file is never clobbered (`E-CFG-INIT`, [`ConfigError::Init`]);
//!   the production [`AtomicScaffoldSink`] writes atomically (temp
//!   sibling → fsync → rename);
//! * deterministic (I3) — the shape line and the skeleton are pure
//!   functions of the survey; `PartialEq`-comparable plans.

use serde::Serialize;
use std::io::Write as _;

use crate::error::ConfigError;
use crate::loader::load_from_str;
use crate::migrate::DraftSink;

/// The scaffold's configuration target: the repo-root `halyard.toml`
/// the v1 loader reads by convention.
pub const INIT_CONFIG_TARGET: &str = "halyard.toml";

/// The scaffold's workflow target: the §16.1 one-file release workflow.
pub const INIT_WORKFLOW_TARGET: &str = ".github/workflows/release.yml";

/// What the shell discovered about the repository (plain data; the
/// engine stays pure). The CLI fills this from `halyard-discover`;
/// tests and downstream callers fill it by hand.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InitSurvey {
    /// The number of packages discovery found. Zero refuses: there is
    /// nothing to scaffold for.
    pub packages: usize,
    /// The ecosystems present, one entry per package (`"cargo"`,
    /// `"npm"`, …), in discovery order — the shape rendering counts
    /// them, so no pre-deduplication is needed.
    pub ecosystems: Vec<String>,
    /// Whether the workspace is the single-package shape (one package;
    /// the `auto` tag style resolves to `v{version}`).
    pub single_package: bool,
}

/// One file the scaffold plans to land: the path (repo-relative, always
/// [`INIT_CONFIG_TARGET`] or [`INIT_WORKFLOW_TARGET`]) and its exact
/// bytes. What the plan carries is what lands — no re-serialization.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ScaffoldFile {
    /// The repo-relative target path.
    pub path: String,
    /// The file's contents, verbatim.
    pub contents: String,
}

/// The init write plan (ADR-0029): plain data, computed before any side
/// effect (validate-then-act, §15.3).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct InitPlan {
    /// The detected shape, one line (the survey's rendering; every
    /// report carries it so the human output names what was found).
    pub shape: String,
    /// The files to land, in write order: the config skeleton first,
    /// then the workflow when `--github` was requested.
    pub files: Vec<ScaffoldFile>,
}

/// One landed file's report row.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct InitFileReport {
    /// The repo-relative path written.
    pub path: String,
    /// The byte count of what landed (matches the file on disk).
    pub bytes: usize,
}

/// The init run's report: what the survey found and what landed. Plain
/// data — the command's JSON carries it verbatim.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct InitReport {
    /// The detected shape (verbatim from the plan).
    pub shape: String,
    /// The landed files, in write order.
    pub files: Vec<InitFileReport>,
}

/// Renders the survey's shape line: `1 package (cargo) — single-package
/// layout` or `3 packages (cargo: 2, npm: 1) — workspace layout`. Pure
/// and deterministic (I3): the ecosystem counts are `BTreeMap`-ordered,
/// and a multi-ecosystem breakdown always spells every count.
fn render_shape(survey: &InitSurvey) -> String {
    let mut counts = std::collections::BTreeMap::new();
    for ecosystem in &survey.ecosystems {
        *counts.entry(ecosystem.as_str()).or_insert(0) += 1;
    }
    let ecosystems = counts
        .iter()
        .map(|(name, count)| {
            if counts.len() == 1 {
                (*name).to_owned()
            } else {
                format!("{name}: {count}")
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    let layout = if survey.single_package {
        "single-package layout"
    } else {
        "workspace layout"
    };
    let noun = if survey.packages == 1 {
        "package"
    } else {
        "packages"
    };
    format!("{} {noun} ({ecosystems}) — {layout}", survey.packages)
}

/// The §16.1 release workflow, verbatim (the one file `halyard init
/// --github` generates; the architecture's §16.1 contract). The
/// `halyard/action` composite itself landed with the distribution
/// milestone (ADR-0052) — the workflow references it the way the
/// architecture documents the finished surface.
const WORKFLOW_TEMPLATE: &str = "on: { push: { branches: [main] } }
permissions: { contents: write, pull-requests: write, id-token: write }
jobs:
  release:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with: { fetch-depth: 0, filter: blob:none }
      - uses: halyard/action@v1
        with: { publish: true }
";

/// The comment-only `halyard.toml` skeleton: every documented default
/// rides as a comment, no key is ever set (loading it yields the
/// built-in defaults exactly — pinned by the tests). The rows use the
/// §16.4 spellings, so uncommenting one produces a valid v1 config.
fn render_config_draft(shape: &str) -> String {
    format!(
        "# halyard — the release engine's configuration, scaffolded by `halyard init`.
#
# Detected shape: {shape}.
#
# Every key is optional: an absent key means the documented default, and
# this file may stay exactly as it is — halyard plans the same zero-config
# release plan with or without it. Uncomment a row to override the default.
#
#   halyard plan             # the zero-config plan, right now
#   halyard schema config    # the machine-readable v1 schema
#
# Frozen v1 schema:
#   https://github.com/elcoosp/halyard/blob/main/schemas/halyard-config-v1.json
#
# [repo]
# # base_branch = \"main\"            # default: the remote's HEAD
# # tag_style = \"auto\"              # single package: v{{version}}; workspace: {{name}}-v{{version}}
#
# [versioning]
# # policy = \"independent\"          # independent | fixed | linked
# # cascade = \"when-unsatisfied\"    # always | when-unsatisfied | never
# # channel = \"rc\"                  # prerelease channel; removing it promotes the leftover
# # maintenance_branches = [\"1.x\"]  # maintenance lines; a backport stays on its line
#
# [commits]
# # types.feat = \"minor\"
# # types.fix = \"patch\"
# # types.perf = \"patch\"
#
# [changelog]
# # style = \"keep-a-changelog\"      # keep-a-changelog | conventional | compact | template path
#
# [publish]
# # enabled = false                 # publishing stays opt-in
#
# [forge]
# # commit_mode = \"auto\"            # git | api | auto
#
# [[package]]
# # name = \"app\"
# # release = false                 # a package halyard must not release
#
# [[link]]
# # from = \"app\"
# # to = \"lib\"
"
    )
}

/// The deterministic scaffold decision over the survey (ADR-0029):
///
/// * zero packages → [`ConfigError::Init`] — there is nothing to
///   scaffold for, and the refusal names what discovery looked for;
/// * otherwise the config skeleton always plans (the shape line is the
///   survey's rendering), plus the §16.1 workflow when `github` is set.
///
/// The skeleton is validated through the strict loader before it is
/// planned — what lands is exactly what the v1 surface accepts (the
/// migration's draft discipline, ADR-0023).
pub fn plan_init(survey: &InitSurvey, github: bool) -> Result<InitPlan, ConfigError> {
    if survey.packages == 0 {
        return Err(ConfigError::Init {
            path: ".".to_owned(),
            message: "discovery found no package manifests (looked for Cargo.toml, \
                      package.json, pnpm-workspace.yaml, pyproject.toml, go.mod/go.work, \
                      pom.xml and .csproj) — halyard plans Cargo, npm/pnpm, PyPI/uv, Go, \
                      Maven and NuGet repositories; run `halyard init` from the repository \
                      root"
                .to_owned(),
        });
    }
    let shape = render_shape(survey);
    let draft = render_config_draft(&shape);
    // The skeleton must be a load-able v1 document before it is planned
    // (it carries no keys, so this pins the comment-only contract at
    // plan time, not only in tests).
    let loaded = load_from_str(&draft).map_err(|error| ConfigError::Init {
        path: INIT_CONFIG_TARGET.to_owned(),
        message: format!("the generated skeleton stopped parsing — refusing to land it: {error}"),
    })?;
    if loaded != crate::model::HalyardConfig::default() {
        return Err(ConfigError::Init {
            path: INIT_CONFIG_TARGET.to_owned(),
            message: "the generated skeleton stopped being key-free — the scaffold never \
                      silently sets configuration"
                .to_owned(),
        });
    }
    let mut files = vec![ScaffoldFile {
        path: INIT_CONFIG_TARGET.to_owned(),
        contents: draft,
    }];
    if github {
        files.push(ScaffoldFile {
            path: INIT_WORKFLOW_TARGET.to_owned(),
            contents: WORKFLOW_TEMPLATE.to_owned(),
        });
    }
    Ok(InitPlan { shape, files })
}

/// Runs the scaffold plan over the sink: gates **every** target's
/// existence before the first write (a refusal leaves no half-scaffold
/// — not even the config skeleton), then writes in plan order. The
/// production sink is [`AtomicScaffoldSink`]; tests wire
/// [`crate::migrate::MemoryDraftSink`] (the recording fake).
pub fn run_init(plan: InitPlan, sink: &dyn DraftSink) -> Result<InitReport, ConfigError> {
    for file in &plan.files {
        if sink.exists(&file.path)? {
            return Err(ConfigError::Init {
                path: file.path.clone(),
                message: "the target already exists — the scaffold never overwrites; \
                          remove it or merge the scaffold into it by hand"
                    .to_owned(),
            });
        }
    }
    let mut files = Vec::with_capacity(plan.files.len());
    for file in plan.files {
        sink.write(&file.path, &file.contents)?;
        files.push(InitFileReport {
            path: file.path,
            bytes: file.contents.len(),
        });
    }
    Ok(InitReport {
        shape: plan.shape,
        files,
    })
}

/// Temp-file counter (process-unique; combined with the pid), mirroring
/// the edit engine's atomic protocol (§9.1).
static INIT_TEMP_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// The production sink for the scaffold: the real filesystem with the
/// §9.1 atomic protocol (temp sibling in the target's directory,
/// `fsync`, rename) and the one difference the scaffold needs: the
/// target's parent directory is **created** when missing
/// (`.github/workflows/` usually does not exist yet — the workflow
/// scaffold's whole point). Create-only otherwise: an existing target
/// is refused with [`ConfigError::Init`], never overwritten, and a
/// refused or failed write leaves no partial file behind.
#[derive(Clone, Copy, Debug, Default)]
pub struct AtomicScaffoldSink;

impl DraftSink for AtomicScaffoldSink {
    fn exists(&self, path: &str) -> Result<bool, ConfigError> {
        Ok(std::path::Path::new(path).exists())
    }

    fn write(&self, path: &str, contents: &str) -> Result<(), ConfigError> {
        let path = std::path::Path::new(path);
        let refuse = |message: String| ConfigError::Init {
            path: path.display().to_string(),
            message,
        };
        if path.exists() {
            return Err(refuse(
                "the target already exists — the scaffold never overwrites; remove it \
                 or merge the scaffold into it by hand"
                    .to_owned(),
            ));
        }
        // A bare `halyard.toml` has an empty parent path — that *is*
        // the current directory (the CLI `-C` chdir lands here).
        let dir = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| std::path::Path::new("."));
        if let Err(source) = std::fs::create_dir_all(dir) {
            return Err(refuse(format!(
                "the target's directory could not be created (the scaffold creates \
                 the workflow's parent directories and nothing else): {source}"
            )));
        }
        let temp = dir.join(format!(
            ".halyard-init-{}-{}.tmp",
            std::process::id(),
            INIT_TEMP_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let write_result = (|| -> std::io::Result<()> {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp)?;
            file.write_all(contents.as_bytes())?;
            file.sync_all()?;
            drop(file);
            std::fs::rename(&temp, path)
        })();
        if let Err(source) = write_result {
            let _ = std::fs::remove_file(&temp);
            return Err(refuse(format!(
                "the atomic write failed (no partial file is left behind): {source}"
            )));
        }
        // Best-effort directory durability (POSIX); a filesystem that
        // refuses the dir fsync does not fail an already-renamed write.
        #[cfg(unix)]
        if let Ok(dir_file) = std::fs::File::open(dir) {
            let _ = dir_file.sync_all();
        }
        Ok(())
    }
}

//! [`EcosystemAdapter`] implementation for Go modules (M8, the §17 M8
//! **Go modules** cell — *"Go modules (tag-only, `go.work`, subdir
//! tags)"*, ADR-0057).
//!
//! Parses `go.mod` and `go.work` with the hand-rolled lexer of [`grammar`]
//! at the VERIFIED reference grammar (`docs/research/go-module-grammars.md`,
//! from `go.dev/ref/mod`). The ecosystem is **tag-only**: the go.mod
//! grammar carries no version field for the module itself — versions are
//! the repository's semantic version tags — so rows plan from the release
//! base tag (§8.2), carry [`RawVersionSource::Tag`], no editable version
//! site, and no registry publish decision; a module rooted in a
//! subdirectory tags `{subdir}/v{version}` (the shared template grammar,
//! appendix A.3, through [`tag::template_for`]).
//!
//! Workspaces ride `go.work`'s `use` directives (the documented shape —
//! *"a `use` adds a module on disk to the set of main modules"*), with
//! the root module claimed only when a `use .` names it. Without a
//! `go.work` the single root `go.mod` is the workspace (the go command's
//! own posture); nested go.mods nothing claims are skipped (the orphan
//! manifest posture). Internal edges are the workspace-proven ones: a
//! require naming a `use` member, and a require named by a local-path
//! `replace` (go.work's or the module's own).

use halyard_core::EcosystemAdapter;
use halyard_core::discover::{
    DiscoverCtx, DiscoverError, Facets, ManifestFormat, RawDep, RawPackage, RawVersionSource,
};
use halyard_core::package::{DepKind, Ecosystem, ManifestSite, RelPath, ReqSpec};

pub mod edit;
pub mod grammar;
pub mod tag;

/// The go-family adapter (§7.3 discovery + tag-only planning subset).
#[derive(Clone, Copy, Debug, Default)]
pub struct GoModAdapter;

impl GoModAdapter {
    /// New adapter.
    pub fn new() -> Self {
        Self
    }
}

/// The release version the `go.mod` **bytes** carry — always `None` by
/// the grammar's own shape (`go.dev/ref/mod`: a go.mod names the module
/// path and its dependencies, never the module's own version; versions
/// are the repository's semantic version tags). The probe input the
/// tag-target pinpoint walk consumes for the go ecosystem (see
/// `halyard_git::tag_target::ManifestVersionProbe`; the git engine never
/// parses manifests itself): a go manifest has no version to pinpoint,
/// so its targets keep the documented HEAD fallback.
#[must_use]
pub fn manifest_version(_bytes: &[u8]) -> Option<String> {
    None
}

impl EcosystemAdapter for GoModAdapter {
    fn id(&self) -> &'static str {
        "go"
    }

    fn discover(&self, ctx: &DiscoverCtx) -> Result<Vec<RawPackage>, DiscoverError> {
        // Workspace mode rides go.work (the documented grammar); the root
        // module is a member only when `use .` names it.
        if let Some(bytes) = ctx.file("go.work") {
            let text = utf8(bytes, "go.work")?;
            let work = grammar::parse_gowork(text, "go.work")?;
            let dirs = member_dirs(&work.uses)?;
            // Every member manifest is a named requirement (a `use`
            // directive is a requirement, not a candidate root — the npm
            // posture for explicit members).
            let mut parsed: Vec<(String, grammar::GoMod)> = Vec::new();
            for dir in &dirs {
                let manifest_path = manifest_of(dir);
                let Some(bytes) = ctx.file(&manifest_path) else {
                    return Err(DiscoverError::Glob {
                        pattern: manifest_path.clone(),
                        message: "workspace member has no go.mod in the committed tree (go: a \
                                  `use` directive names the directory containing the module's \
                                  go.mod)"
                            .to_owned(),
                    });
                };
                let text = utf8(bytes, &manifest_path)?;
                let gomod = grammar::parse_gomod(text, &manifest_path)?;
                parsed.push((manifest_path, gomod));
            }
            let member_paths: std::collections::BTreeSet<String> = parsed
                .iter()
                .map(|(_, gomod)| gomod.module.clone())
                .collect();
            let replaced_paths: std::collections::BTreeSet<String> = work
                .replaces
                .iter()
                .filter(|row| matches!(row.target, grammar::ReplaceTarget::Path(_)))
                .map(|row| row.old.clone())
                .collect();
            let mut packages = Vec::new();
            // Fixed order (§6.1 merge determinism): the repo-root module
            // first, then the sorted member directories.
            for (manifest_path, gomod) in &parsed {
                let root = root_of(manifest_path);
                if root.is_empty() {
                    packages.push(package_from_mod(
                        gomod,
                        &root,
                        manifest_path,
                        &member_paths,
                        &replaced_paths,
                    )?);
                }
            }
            for (manifest_path, gomod) in &parsed {
                let root = root_of(manifest_path);
                if !root.is_empty() {
                    packages.push(package_from_mod(
                        gomod,
                        &root,
                        manifest_path,
                        &member_paths,
                        &replaced_paths,
                    )?);
                }
            }
            return Ok(packages);
        }
        // No go.work: the single root module is the workspace (go.dev:
        // *"When no go.work file exists, the workspace consists of the
        // single module containing the current directory"*). Nested
        // go.mods nothing claims are skipped (the orphan-manifest
        // posture).
        let Some(bytes) = ctx.file("go.mod") else {
            return Ok(Vec::new());
        };
        let text = utf8(bytes, "go.mod")?;
        let gomod = grammar::parse_gomod(text, "go.mod")?;
        let replaced_paths: std::collections::BTreeSet<String> = gomod
            .replaces
            .iter()
            .filter(|row| matches!(row.target, grammar::ReplaceTarget::Path(_)))
            .map(|row| row.old.clone())
            .collect();
        let empty_names = std::collections::BTreeSet::new();
        Ok(vec![package_from_mod(
            &gomod,
            "",
            "go.mod",
            &empty_names,
            &replaced_paths,
        )?])
    }

    fn edit_versions(
        &self,
        file: &halyard_core::package::RelPath,
        content: &[u8],
        patches: &[halyard_core::plan::PlanPatch],
    ) -> Result<halyard_core::edit::EditOutcome, halyard_core::edit::EditFailure> {
        edit::edit_versions(file, content, patches)
    }
}

/// The go.mod path of a member directory (empty = the repository root).
fn manifest_of(dir: &str) -> String {
    if dir.is_empty() {
        "go.mod".to_owned()
    } else {
        format!("{dir}/go.mod")
    }
}

/// The package root of a member manifest path (`api/go.mod` → `api`,
/// `go.mod` → the repository root).
fn root_of(manifest_path: &str) -> String {
    if manifest_path == "go.mod" {
        return String::new();
    }
    manifest_path
        .strip_suffix("/go.mod")
        .unwrap_or(manifest_path)
        .to_owned()
}

/// Normalizes `use` arguments to repo-relative directories (`.` → the
/// root, `./api` → `api`); a path escaping the repository root or an
/// absolute path is a grammar failure (`E-DISC-GLOB`) — halyard plans the
/// committed tree only.
fn member_dirs(uses: &[String]) -> Result<Vec<String>, DiscoverError> {
    let mut dirs = std::collections::BTreeSet::new();
    for raw in uses {
        if raw.is_empty() || raw.starts_with('/') {
            return Err(DiscoverError::Glob {
                pattern: raw.clone(),
                message: "an empty or absolute use path cannot name a module in the committed \
                          tree"
                    .to_owned(),
            });
        }
        let trimmed = raw.trim_matches('/');
        let normalized = trimmed
            .strip_prefix("./")
            .unwrap_or(trimmed)
            .trim_end_matches('/');
        if normalized.split('/').any(|c| c == "..") {
            return Err(DiscoverError::Glob {
                pattern: raw.clone(),
                message: "a use path outside the committed tree cannot be planned".to_owned(),
            });
        }
        // `.` and `./` name the repository root (the empty directory).
        let normalized = if normalized == "." { "" } else { normalized };
        dirs.insert(normalized.to_owned());
    }
    Ok(dirs.into_iter().collect())
}

fn utf8<'a>(bytes: &'a [u8], file: &str) -> Result<&'a str, DiscoverError> {
    std::str::from_utf8(bytes).map_err(|_| DiscoverError::Manifest {
        file: file.to_owned(),
        format: ManifestFormat::Gomod,
        message: "manifest is not valid UTF-8".to_owned(),
        span: None,
    })
}

/// Builds one raw package row from a parsed go.mod (§6.1 merge row) —
/// the tag-only shape: no manifest version (`RawVersionSource::Tag`, no
/// editable site), unpublishable (no registry publish decision exists),
/// and the per-package subdir tag template. `member_paths` are the
/// workspace's module paths; `replaced_paths` the locally-replaced module
/// paths — requires naming either are proven-internal edges.
fn package_from_mod(
    gomod: &grammar::GoMod,
    root: &str,
    manifest_path: &str,
    member_paths: &std::collections::BTreeSet<String>,
    replaced_paths: &std::collections::BTreeSet<String>,
) -> Result<RawPackage, DiscoverError> {
    let manifest = RelPath::new(manifest_path).ok_or(DiscoverError::Manifest {
        file: manifest_path.to_owned(),
        format: ManifestFormat::Gomod,
        message: "manifest path is not a normalized repo-relative path".to_owned(),
        span: None,
    })?;
    let deps: Vec<RawDep> = gomod
        .requires
        .iter()
        .map(|row| RawDep {
            name: row.path.clone(),
            kind: DepKind::Normal,
            req: ReqSpec::Other {
                raw: halyard_core::BString::from(row.version.as_str()),
            },
            site: ManifestSite {
                file: manifest.clone(),
                spans: vec![row.version_span],
            },
            raw: halyard_core::BString::from(row.version.as_str()),
            internal: member_paths.contains(&row.path) || replaced_paths.contains(&row.path),
        })
        .collect();
    Ok(RawPackage {
        name: gomod.module.clone(),
        ecosystem: Ecosystem::Go,
        root: root.to_owned(),
        manifest: manifest_path.to_owned(),
        // Tag-only (ADR-0057): the version is the repository's release
        // tag, never a manifest row — the planner materializes it from
        // the release base tag (§8.2).
        version: None,
        version_source: RawVersionSource::Tag,
        publishable: false,
        deps,
        tag_template: Some(tag::template_for(root)),
        changelog: None,
        version_site: None,
        include: halyard_core::package::PathFilter::new(Vec::new(), Vec::new()),
        facets: Facets {
            cdylib: false,
            wasm_bindgen: false,
            napi: false,
        },
        warnings: Vec::new(),
    })
}

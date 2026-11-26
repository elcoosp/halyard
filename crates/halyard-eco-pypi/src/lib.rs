//! [`EcosystemAdapter`] implementation for Python / `PyPI` (M8 step 1,
//! ADR-0056, the §17 M8 `PyPI` cell — *"(`pyproject`, `uv`, PEP 440 strategy,
//! trusted publishing)"* — the discovery + manifest-edit subset).
//!
//! Parses `pyproject.toml` through span-accurate `toml_edit` (the same
//! sanctioned parser the cargo adapter uses, invariant I6) at the PEP
//! 621 grammar: `[project]` name/version/dependencies, uv workspaces
//! (`[tool.uv.workspace]` `members`/`exclude` globs, the root is a
//! member, `[tool.uv.sources]` `workspace = true` marks proven-internal
//! rows with the root's definitions applying to all members unless a
//! member overrides the name), `[tool.uv] package = false` marks an
//! application member (unpublishable — the npm `private: true`
//! posture), and PEP 735 `[dependency-groups]` dev rows. The PEP 440
//! strategy ([`pep440`]) pairs canonical spellings with the plan
//! contract's semver type; every recognized-but-non-canonical spelling
//! fails closed at discovery (warned, unpublishable, never rewritten).
//!
//! The manifest-edit seam ([`edit`]) re-applies the plan's own-version
//! byte patches **semantically and in place** (invariant I4): the
//! version value's inner bytes swap for the PEP 440 rendering of the
//! plan's semver target — the semver hyphen form never reaches
//! `pyproject.toml`.

use halyard_core::discover::{
    DiscoverCtx, DiscoverError, DiscoveryWarning, Facets, ManifestFormat, RawDep, RawPackage,
    RawVersionSource,
};
use halyard_core::package::{DepKind, Ecosystem, ManifestSite, RelPath, ReqSpec, Span};
use halyard_core::{EcosystemAdapter, glob};

pub mod edit;
pub mod pep440;
pub mod pep508;
// M8 step 2 (§13.4): the publish argv grammar (the shell spawns, the
// adapter owns the tool grammar — the `refresh_command` convention).
pub mod publish;
pub use publish::uv_publish_args;

/// The pypi-family adapter (§7.3 discovery + manifest-edit subset).
#[derive(Clone, Copy, Debug, Default)]
pub struct PyPiAdapter;

impl PyPiAdapter {
    /// New adapter.
    pub fn new() -> Self {
        Self
    }
}

/// The release version the `pyproject.toml` **bytes** carry (`None` when
/// the bytes are not parsable TOML, carry no PEP 621 `[project]`
/// version, or the version is not a canonical PEP 440 spelling) — the
/// probe input the tag-target pinpoint walk consumes for the pypi
/// ecosystem (see `halyard_git::tag_target::ManifestVersionProbe`; the
/// git engine never parses manifests itself). The returned text is the
/// semver pairing (the plan contract's spelling).
#[must_use]
pub fn manifest_version(bytes: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(bytes).ok()?;
    let doc = toml_edit::Document::parse(text).ok()?;
    let version = doc
        .get("project")?
        .get("version")?
        .as_value()?
        .as_str()?
        .to_owned();
    let version = pep440::parse_pep440(&version)?;
    Some(version.to_string())
}

/// The span-carrying parsed document (`toml_edit::Document<&str>`).
pub(crate) type ParsedDoc<'a> = toml_edit::Document<&'a str>;

/// Parses `raw` into a span-carrying document.
pub(crate) fn parse_document<'a>(raw: &'a str, file: &str) -> Result<ParsedDoc<'a>, DiscoverError> {
    toml_edit::Document::parse(raw).map_err(|e| DiscoverError::Manifest {
        file: file.to_owned(),
        format: ManifestFormat::Toml,
        message: e.to_string(),
        span: e.span().map(span_of),
    })
}

/// `toml_edit`'s inclusive-end byte span → the core [`Span`] (inclusive
/// start, exclusive end), as the cargo adapter maps it.
pub(crate) fn span_of(span: std::ops::Range<usize>) -> Span {
    Span {
        start: u32::try_from(span.start).unwrap_or(u32::MAX),
        end: u32::try_from(span.end.saturating_sub(1)).unwrap_or(u32::MAX),
    }
}

/// The inner byte span of a quoted TOML string value (excluding the
/// delimiters — the npm `json_span` convention, which the plan patches
/// and the edit half share). `None` for multi-line or unquoted values:
/// not an editable version site.
pub(crate) fn inner_span(value: &toml_edit::Item, text: &str) -> Option<Span> {
    let value = value.as_value()?;
    let span = value.span()?;
    let start = span.start;
    let end = span.end;
    let bytes = text.as_bytes();
    let quote = match bytes.get(start)? {
        b'"' => b'"',
        b'\'' => b'\'',
        _ => return None,
    };
    if bytes.get(end.checked_sub(1)?) != Some(&quote) {
        return None;
    }
    // Multi-line delimiters (`"""` / `'''`) are not version sites: the
    // second byte repeats the quote and the literal spans ≥ 3 bytes.
    if (end - start) >= 3
        && bytes.get(start + 1) == Some(&quote)
        && bytes.get(start + 2) == Some(&quote)
    {
        return None;
    }
    Some(Span {
        start: u32::try_from(start + 1).unwrap_or(u32::MAX),
        end: u32::try_from(end - 1).unwrap_or(u32::MAX),
    })
}

/// The manifest-level uv sources of `doc` — the names whose
/// `[tool.uv.sources]` entry carries `workspace = true`, normalized for
/// member matching (`docs/research/pypi-uv-grammars.md` §1).
fn workspace_true_names(doc: &ParsedDoc<'_>) -> std::collections::BTreeSet<String> {
    let mut names = std::collections::BTreeSet::new();
    let Some(sources) = doc
        .get("tool")
        .and_then(toml_edit::Item::as_table)
        .and_then(|uv| uv.get("uv"))
        .and_then(toml_edit::Item::as_table)
        .and_then(|uv| uv.get("sources"))
        .and_then(toml_edit::Item::as_table)
    else {
        return names;
    };
    for (name, entry) in sources {
        let is_workspace = entry
            .as_table_like()
            .and_then(|t| t.get("workspace"))
            .and_then(toml_edit::Item::as_value)
            .and_then(toml_edit::Value::as_bool)
            == Some(true);
        if is_workspace {
            names.insert(pep440::normalize_name(name));
        }
    }
    names
}

/// The manifest-level names of ANY `[tool.uv.sources]` entry (workspace
/// or not) — the names a member's own table shadows the root's
/// definitions for.
fn any_source_names(doc: &ParsedDoc<'_>) -> std::collections::BTreeSet<String> {
    let mut names = std::collections::BTreeSet::new();
    let Some(sources) = doc
        .get("tool")
        .and_then(toml_edit::Item::as_table)
        .and_then(|uv| uv.get("uv"))
        .and_then(toml_edit::Item::as_table)
        .and_then(|uv| uv.get("sources"))
        .and_then(toml_edit::Item::as_table)
    else {
        return names;
    };
    for (name, _) in sources {
        names.insert(pep440::normalize_name(name));
    }
    names
}

/// The effective proven-internal names for one manifest: the member's
/// own workspace rows, plus the root's workspace rows for every name the
/// member does not itself shadow (uv's override rule, research §1).
fn effective_internal_names(
    root: Option<&ParsedDoc<'_>>,
    own: &ParsedDoc<'_>,
) -> std::collections::BTreeSet<String> {
    let mut names = workspace_true_names(own);
    if let Some(root) = root {
        let shadowed = any_source_names(own);
        for name in workspace_true_names(root) {
            if !shadowed.contains(&name) {
                names.insert(name);
            }
        }
    }
    names
}

impl EcosystemAdapter for PyPiAdapter {
    fn id(&self) -> &'static str {
        "pypi"
    }

    fn discover(&self, ctx: &DiscoverCtx) -> Result<Vec<RawPackage>, DiscoverError> {
        // No root manifest, no pypi row: the orchestrator merges
        // adapters, so an empty result is correct.
        let Some(root_bytes) = ctx.file("pyproject.toml") else {
            return Ok(Vec::new());
        };
        let root_text = utf8(root_bytes, "pyproject.toml")?;
        let root_doc = parse_document(root_text, "pyproject.toml")?;
        let has_project = root_doc
            .get("project")
            .and_then(toml_edit::Item::as_table)
            .is_some();

        let workspace = root_doc
            .get("tool")
            .and_then(toml_edit::Item::as_table)
            .and_then(|tool| tool.get("uv"))
            .and_then(toml_edit::Item::as_table)
            .and_then(|uv| uv.get("workspace"))
            .and_then(toml_edit::Item::as_table);

        let Some(workspace) = workspace else {
            // No uv workspace: the root [project] is the single package
            // (§6.1 single-package mode); nested pyprojects are not
            // claimed by anything and are skipped (the npm posture for
            // orphan package.json files).
            return if has_project {
                Ok(vec![package_from_doc(
                    &root_doc,
                    root_text,
                    String::new(),
                    "pyproject.toml",
                    &std::collections::BTreeSet::new(),
                )?])
            } else {
                Ok(Vec::new())
            };
        };

        // Workspace members (uv: the root is "also a workspace member",
        // so the root's [project] is required — a workspace is rooted at
        // "that package").
        if !has_project {
            return Err(DiscoverError::Manifest {
                file: "pyproject.toml".to_owned(),
                format: ManifestFormat::Toml,
                message: "[tool.uv.workspace] requires a [project] table in the root manifest \
                          (a uv workspace is rooted at a package)"
                    .to_owned(),
                span: None,
            });
        }
        let member_dirs = member_dirs(ctx, workspace)?;
        let mut packages = Vec::new();
        // The root first (fixed order, §6.1 merge determinism), then the
        // sorted member directories.
        packages.push(package_from_doc(
            &root_doc,
            root_text,
            String::new(),
            "pyproject.toml",
            &workspace_true_names(&root_doc),
        )?);
        for dir in member_dirs {
            let manifest_path = format!("{dir}/pyproject.toml");
            let Some(bytes) = ctx.file(&manifest_path) else {
                return Err(DiscoverError::Glob {
                    pattern: manifest_path.clone(),
                    message: "workspace member has no pyproject.toml in the committed tree (uv: \
                              every member directory must contain one)"
                        .to_owned(),
                });
            };
            let text = utf8(bytes, &manifest_path)?;
            let doc = parse_document(text, &manifest_path)?;
            if doc
                .get("project")
                .and_then(toml_edit::Item::as_table)
                .is_none()
            {
                return Err(DiscoverError::Manifest {
                    file: manifest_path.clone(),
                    format: ManifestFormat::Toml,
                    message: "workspace member defines no [project] table".to_owned(),
                    span: None,
                });
            }
            packages.push(package_from_doc(
                &doc,
                text,
                dir,
                &manifest_path,
                &effective_internal_names(Some(&root_doc), &doc),
            )?);
        }
        Ok(packages)
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

/// Expands the workspace's member globs against the committed tree (uv:
/// `members` is required, `exclude` optional, "lists of globs"; every
/// included directory "must contain a pyproject.toml file"). Explicit
/// (glob-less) members error on a missing directory — a named member is
/// a requirement, not a candidate root (the npm posture, §6.4).
fn member_dirs(
    ctx: &DiscoverCtx,
    workspace: &toml_edit::Table,
) -> Result<Vec<String>, DiscoverError> {
    let Some(members) = string_list(workspace.get("members")) else {
        return Err(DiscoverError::Manifest {
            file: "pyproject.toml".to_owned(),
            format: ManifestFormat::Toml,
            message: "[tool.uv.workspace] members must be an array of strings (required by the \
                      uv workspace grammar)"
                .to_owned(),
            span: None,
        });
    };
    let excludes = string_list(workspace.get("exclude")).unwrap_or_default();

    let dirs = ctx.dirs();
    let mut selected: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for pattern in &members {
        if !pattern.chars().any(|c| matches!(c, '*' | '?' | '[' | ']')) {
            // Explicit directory member.
            if dirs.contains(&pattern.as_str())
                || ctx.file(&format!("{pattern}/pyproject.toml")).is_some()
            {
                selected.insert(pattern.clone());
                continue;
            }
            return Err(DiscoverError::Glob {
                pattern: pattern.clone(),
                message: "explicit member has no directory in the committed tree".to_owned(),
            });
        }
        // Glob: expand against the committed directories that carry a
        // manifest (glob members are candidate roots, not requirements).
        for dir in &dirs {
            if glob::matches(pattern, dir) && ctx.file(&format!("{dir}/pyproject.toml")).is_some() {
                selected.insert((*dir).to_owned());
            }
        }
    }
    for pattern in &excludes {
        if pattern.chars().any(|c| matches!(c, '*' | '?' | '[' | ']')) {
            let ghosts: Vec<String> = selected
                .iter()
                .filter(|dir| glob::matches(pattern, dir))
                .cloned()
                .collect();
            for ghost in ghosts {
                selected.remove(&ghost);
            }
        } else {
            selected.remove(pattern);
        }
    }
    Ok(selected.into_iter().collect())
}

/// String-array helper (`None` when the item is not an array of strings).
fn string_list(item: Option<&toml_edit::Item>) -> Option<Vec<String>> {
    let arr = item?.as_array()?;
    let mut out = Vec::new();
    for value in arr {
        out.push(value.as_str()?.to_owned());
    }
    Some(out)
}

fn utf8<'a>(bytes: &'a [u8], file: &str) -> Result<&'a str, DiscoverError> {
    std::str::from_utf8(bytes).map_err(|_| DiscoverError::Manifest {
        file: file.to_owned(),
        format: ManifestFormat::Toml,
        message: "manifest is not valid UTF-8".to_owned(),
        span: None,
    })
}

/// The PEP 440 strategy at the version row (ADR-0056): a canonical
/// spelling pairs with the plan contract's semver type and carries its
/// inner-span edit site; a recognized-but-non-canonical spelling (epoch,
/// post/dev/local, implicit segments, leading zeros, aliases), a
/// non-string value, a dynamic version and a missing version all fail
/// closed — warned, unpublishable, no edit site, never guessed.
type VersionSite = (
    Option<semver::Version>,
    Option<ManifestSite>,
    RawVersionSource,
);

fn version_site_of(
    project: &toml_edit::Table,
    text: &str,
    manifest: &RelPath,
    name: &str,
    warnings: &mut Vec<DiscoveryWarning>,
) -> VersionSite {
    let Some(item) = project.get("version") else {
        return (
            warn_missing_version(project, name, warnings),
            None,
            version_source_missing(project),
        );
    };
    let (Some(text_value), Some(span)) = (
        item.as_value().and_then(toml_edit::Value::as_str),
        inner_span(item, text),
    ) else {
        warnings.push(DiscoveryWarning {
            code: "uneditable-version",
            subject: name.to_owned(),
            message: "the `version` value is not a plain quoted string; halyard cannot edit it \
                      in place"
                .to_owned(),
        });
        return (None, None, RawVersionSource::Local);
    };
    if let Some(parsed) = pep440::parse_pep440(text_value) {
        let site = ManifestSite {
            file: manifest.clone(),
            spans: vec![span],
        };
        (Some(parsed), Some(site), RawVersionSource::Local)
    } else {
        warnings.push(DiscoveryWarning {
            code: "non-canonical-version",
            subject: name.to_owned(),
            message: format!(
                "version {text_value:?} is not a canonical PEP 440 spelling halyard can \
                 plan (epoch/post/dev/local/implicit segments are recognized but never \
                 rewritten)"
            ),
        });
        (None, None, RawVersionSource::Local)
    }
}

/// The version is absent from `[project]`: a PEP 621 `dynamic = ["version"]`
/// row means the build backend fills it (warned, `Dynamic`, unpublishable);
/// otherwise the manifest simply has none (warned, unpublishable).
fn warn_missing_version(
    project: &toml_edit::Table,
    name: &str,
    warnings: &mut Vec<DiscoveryWarning>,
) -> Option<semver::Version> {
    let dynamic = project
        .get("dynamic")
        .and_then(toml_edit::Item::as_array)
        .is_some_and(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .any(|s| s == "version")
        });
    let (code, message) = if dynamic {
        (
            "dynamic-version",
            "`version` is PEP 621 dynamic; halyard plans static versions only",
        )
    } else {
        (
            "no-version",
            "no `version` in [project]; package treated as unpublished",
        )
    };
    warnings.push(DiscoveryWarning {
        code,
        subject: name.to_owned(),
        message: message.to_owned(),
    });
    None
}

/// The version source for a missing version row: `Dynamic` when the
/// manifest declares it dynamic, `Local` otherwise.
fn version_source_missing(project: &toml_edit::Table) -> RawVersionSource {
    let dynamic = project
        .get("dynamic")
        .and_then(toml_edit::Item::as_array)
        .is_some_and(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .any(|s| s == "version")
        });
    if dynamic {
        RawVersionSource::Dynamic
    } else {
        RawVersionSource::Local
    }
}

/// Builds one raw package row from a parsed PEP 621 manifest (§6.1
/// merge row). `internal_names` carries the effective proven-internal
/// workspace rows for this manifest (own sources over the root's,
/// shadowed per name).
fn package_from_doc(
    doc: &ParsedDoc<'_>,
    text: &str,
    dir: String,
    manifest_path: &str,
    internal_names: &std::collections::BTreeSet<String>,
) -> Result<RawPackage, DiscoverError> {
    let mut warnings: Vec<DiscoveryWarning> = Vec::new();
    let manifest = RelPath::new(manifest_path).ok_or(DiscoverError::Manifest {
        file: manifest_path.to_owned(),
        format: ManifestFormat::Toml,
        message: "manifest path is not a normalized repo-relative path".to_owned(),
        span: None,
    })?;
    let project = doc
        .get("project")
        .and_then(toml_edit::Item::as_table)
        .ok_or(DiscoverError::Manifest {
            file: manifest_path.to_owned(),
            format: ManifestFormat::Toml,
            message: "pyproject.toml defines no [project] table".to_owned(),
            span: None,
        })?;
    let name = project
        .get("name")
        .and_then(toml_edit::Item::as_value)
        .and_then(toml_edit::Value::as_str)
        .map(str::to_owned)
        .ok_or(DiscoverError::Manifest {
            file: manifest_path.to_owned(),
            format: ManifestFormat::Toml,
            message: "[project] defines no `name` (a uv package row needs one)".to_owned(),
            span: None,
        })?;

    // The PEP 440 strategy (ADR-0056): canonical spellings plan;
    // recognized-but-non-canonical spellings fail closed — warned,
    // unpublishable, no version site (never guessed).
    let mut publishable = true;
    let (version, version_site, version_source) =
        version_site_of(project, text, &manifest, &name, &mut warnings);
    publishable = publishable && version.is_some();

    // `tool.uv.package = false` marks an application member (uv: "will
    // force a project package not to be built and installed") — the npm
    // `private: true` posture at uv's grammar.
    let packaged = matches!(
        doc.get("tool")
            .and_then(toml_edit::Item::as_table)
            .and_then(|tool| tool.get("uv"))
            .and_then(toml_edit::Item::as_table)
            .and_then(|uv| uv.get("package"))
            .and_then(toml_edit::Item::as_value)
            .and_then(toml_edit::Value::as_bool),
        None | Some(true)
    );
    publishable = publishable && packaged;

    // Dependency rows (§6.4 posture): `dependencies` (Normal),
    // `optional-dependencies` extras (Optional), PEP 735
    // `dependency-groups` (Dev). The specifier is foreign grammar
    // (ADR-0006): carried verbatim, the name normalized for the join.
    let deps = collect_deps(doc, text, internal_names, &manifest, &mut warnings);

    Ok(RawPackage {
        name,
        ecosystem: Ecosystem::PyPi,
        root: dir,
        manifest: manifest_path.to_owned(),
        version,
        version_source,
        publishable,
        deps,
        tag_template: None,
        changelog: None,
        version_site,
        include: halyard_core::package::PathFilter::new(Vec::new(), Vec::new()),
        facets: Facets {
            cdylib: false,
            wasm_bindgen: false,
            napi: false,
        },
        warnings,
    })
}

/// The dependency-row sink (§6.4's fixed-surface collection): the
/// parsed rows accumulate into `deps`, the refusals into `warnings`,
/// and `internal` comes from the effective `[tool.uv.sources]` names
/// (the `PyPA`-normalized identity both uv and `PyPI` match on).
struct DepSink<'a> {
    deps: Vec<RawDep>,
    warnings: &'a mut Vec<DiscoveryWarning>,
    manifest: &'a RelPath,
    internal_names: &'a std::collections::BTreeSet<String>,
}

impl DepSink<'_> {
    /// Parses one requirement row into a [`RawDep`] (or warns and
    /// skips): the name normalizes through the `PyPA` rule, the
    /// specifier rides verbatim (ADR-0006).
    fn push(&mut self, text: &str, span: Option<Span>, kind: DepKind, subject: &str) {
        let Some(parsed) = pep508::parse_requirement(text) else {
            self.warnings.push(DiscoveryWarning {
                code: "ignored-requirement",
                subject: subject.to_owned(),
                message: format!(
                    "requirement {text:?} does not start with a valid project name; ignored"
                ),
            });
            return;
        };
        let internal = self.internal_names.contains(&parsed.name);
        self.deps.push(RawDep {
            name: parsed.name,
            kind,
            req: ReqSpec::Other {
                raw: halyard_core::BString::from(parsed.raw.as_str()),
            },
            site: ManifestSite {
                file: self.manifest.clone(),
                spans: span.into_iter().collect(),
            },
            raw: halyard_core::BString::from(parsed.raw.as_str()),
            internal,
        });
    }
}

/// Collects the three dependency surfaces in fixed order (§6.4's
/// fixed-field discipline): `dependencies`, `optional-dependencies`
/// (extras, Optional), `dependency-groups` (PEP 735, Dev). Internal
/// classification rides the effective `[tool.uv.sources]` names — a
/// workspace member's own table shadows the root's per name.
fn collect_deps(
    doc: &ParsedDoc<'_>,
    raw: &str,
    internal_names: &std::collections::BTreeSet<String>,
    manifest: &RelPath,
    warnings: &mut Vec<DiscoveryWarning>,
) -> Vec<RawDep> {
    let Some(project) = doc.get("project").and_then(toml_edit::Item::as_table) else {
        return Vec::new();
    };
    let mut sink = DepSink {
        deps: Vec::new(),
        warnings,
        manifest,
        internal_names,
    };
    // `dependencies` rows keep their document order.
    if let Some(rows) = project
        .get("dependencies")
        .and_then(toml_edit::Item::as_array)
    {
        for row in rows {
            let Some(text) = row.as_str() else {
                sink.warnings.push(DiscoveryWarning {
                    code: "ignored-value",
                    subject: "dependencies".to_owned(),
                    message: "a `dependencies` row is not a string; ignored".to_owned(),
                });
                continue;
            };
            sink.push(
                text,
                element_span(row, raw),
                DepKind::Normal,
                "dependencies",
            );
        }
    }
    // Extras and dependency groups: tables of string arrays, keys sorted
    // for determinism (I3 — the document order of a TOML table is
    // source order, but the merge contract sorts containers).
    for (field, kind) in [
        ("optional-dependencies", DepKind::Optional),
        ("dependency-groups", DepKind::Dev),
    ] {
        let Some(table) = project.get(field).and_then(toml_edit::Item::as_table) else {
            continue;
        };
        let mut groups: Vec<(&str, &toml_edit::Array)> = table
            .iter()
            .filter_map(|(key, item)| item.as_array().map(|arr| (key, arr)))
            .collect();
        groups.sort_unstable_by_key(|(key, _)| *key);
        for (group, rows) in groups {
            for row in rows {
                let Some(text) = row.as_str() else {
                    sink.warnings.push(DiscoveryWarning {
                        code: "ignored-value",
                        subject: group.to_owned(),
                        message: format!("a {field}.{group} row is not a string; ignored"),
                    });
                    continue;
                };
                sink.push(text, element_span(row, raw), kind, group);
            }
        }
    }
    sink.deps
}

/// The inner span of one string element of a TOML array, recovered from
/// the raw text (array elements carry value spans like any value).
fn element_span(row: &toml_edit::Value, text: &str) -> Option<Span> {
    let span = row.span()?;
    let bytes = text.as_bytes();
    let quote = match bytes.get(span.start)? {
        b'"' => b'"',
        b'\'' => b'\'',
        _ => return None,
    };
    if bytes.get(span.end.checked_sub(1)?) != Some(&quote) {
        return None;
    }
    if (span.end - span.start) >= 3
        && bytes.get(span.start + 1) == Some(&quote)
        && bytes.get(span.start + 2) == Some(&quote)
    {
        return None;
    }
    Some(Span {
        start: u32::try_from(span.start + 1).unwrap_or(u32::MAX),
        end: u32::try_from(span.end - 1).unwrap_or(u32::MAX),
    })
}

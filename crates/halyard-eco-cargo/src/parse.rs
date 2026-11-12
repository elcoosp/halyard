//! `toml_edit` span-accurate Cargo manifest parsing (§6.2).
//!
//! One pass per manifest produces a [`ManifestDraft`]: the resolved
//! version plus its [`RawVersionSource`], every workspace-relevant
//! dependency row with its edit site (§9.1), the link-rule facets and the
//! §6.3 warnings. Spans point at the requirement text — the exact bytes a
//! later patch (M3.1) will replace.
//!
//! Path/`workspace = true` dependencies are marked **internal** (the
//! manifest proves they target workspace members); plain version
//! requirements are registry-resolved by cargo even when the name matches
//! a member, so they stay external — that distinction is what keeps the
//! differential comparison against `cargo metadata` exact (M1.5).

use halyard_core::discover::{
    DiscoverError, DiscoveryWarning, Facets, ManifestFormat, RawVersionSource,
};
use halyard_core::package::{DepKind, ManifestSite, RelPath, ReqSpec, Span, TagTemplate};

use crate::{normalize_join, parse_tag_template, span_of, spans_of, warning};

/// The workspace-level facts the root manifest contributes (§6.3).
pub(crate) struct WorkspaceFacts {
    /// `[package]` present? (the root package is a workspace member).
    pub has_package: bool,
    /// `[workspace] members` patterns; `Some` ⇔ a `[workspace]` table
    /// exists (an empty list = root-only workspace).
    pub workspace_members: Option<Vec<String>>,
    /// `[workspace] exclude` patterns.
    pub workspace_excludes: Vec<String>,
    /// `[workspace.package].version` text + span (inherited versions
    /// resolve through it, §6.3).
    pub inherited_version: Option<(String, Option<Span>)>,
    /// `[workspace.dependencies]`: name → entry.
    pub workspace_deps: std::collections::BTreeMap<String, WorkspaceDepEntry>,
}

/// One `[workspace.dependencies]` entry.
pub(crate) struct WorkspaceDepEntry {
    /// The requirement a `workspace = true` member inherits.
    pub req: ReqSpec,
    /// Byte span of the requirement text in the root manifest — the edit
    /// site for inheriting members (§6.3).
    pub version_span: Option<Span>,
}

/// Parsed member manifest (pre-merge).
pub(crate) struct ManifestDraft {
    pub name: String,
    pub version: Option<semver::Version>,
    pub version_source: RawVersionSource,
    pub publishable: bool,
    pub deps: Vec<ParsedDep>,
    pub tag_template: Option<TagTemplate>,
    /// Byte site of the package's own version value (M1.9): the file plus
    /// the span of the version text — the root manifest for inherited
    /// versions (§6.3), the own manifest otherwise.
    pub version_site: Option<ManifestSite>,
    pub facets: Facets,
    pub warnings: Vec<DiscoveryWarning>,
}

/// One dependency row with its edit site.
pub(crate) struct ParsedDep {
    pub name: String,
    pub kind: DepKind,
    pub req: ReqSpec,
    pub site: ManifestSite,
    /// Manifest-proven workspace-internal requirement (see module docs).
    pub internal: bool,
}

/// The span-carrying parsed document (`toml_edit::Document<&str>`).
pub(crate) type ParsedDoc<'a> = toml_edit::Document<&'a str>;

/// Parses `raw` into a span-carrying document.
pub(crate) fn parse_document<'a>(
    raw: &'a str,
    file: &str,
) -> Result<ParsedDoc<'a>, DiscoverError> {
    toml_edit::Document::parse(raw).map_err(|e| DiscoverError::Manifest {
        file: file.to_owned(),
        format: ManifestFormat::Toml,
        message: e.to_string(),
        span: e.span().map(span_of),
    })
}

/// True when the document defines a `[workspace]` table.
pub(crate) fn has_workspace_table(doc: &ParsedDoc<'_>) -> bool {
    doc.get("workspace").is_some()
}

/// Extracts the workspace-level facts from the root manifest.
pub(crate) fn workspace_facts(doc: &ParsedDoc<'_>) -> Result<WorkspaceFacts, DiscoverError> {
    let has_package = doc.get("package").is_some();
    let ws = doc.get("workspace").and_then(toml_edit::Item::as_table);
    let (members, excludes) = match ws {
        Some(table) => {
            let members = match table.get("members") {
                Some(_) => string_list(table.get("members")).ok_or(
                    DiscoverError::Manifest {
                        file: "Cargo.toml".to_owned(),
                        format: ManifestFormat::Toml,
                        message: "[workspace] members must be an array of strings".to_owned(),
                        span: None,
                    },
                )?,
                None => Vec::new(),
            };
            let excludes = string_list(table.get("exclude")).unwrap_or_default();
            (Some(members), excludes)
        }
        None => (None, Vec::new()),
    };
    let inherited_version = ws
        .and_then(|t| t.get("package"))
        .and_then(toml_edit::Item::as_table)
        .and_then(|t| t.get("version"))
        .and_then(item_string_and_span);
    let mut workspace_deps = std::collections::BTreeMap::new();
    if let Some(deps) = ws
        .and_then(|t| t.get("dependencies"))
        .and_then(toml_edit::Item::as_table)
    {
        for (name, item) in deps.iter() {
            if let Some(entry) = requirement_from_item(item) {
                workspace_deps.insert(name.to_owned(), entry);
            }
        }
    }
    Ok(WorkspaceFacts {
        has_package,
        workspace_members: members,
        workspace_excludes: excludes,
        inherited_version,
        workspace_deps,
    })
}

/// String-array helper (`None` when the item is not an array of strings).
fn string_list(item: Option<&toml_edit::Item>) -> Option<Vec<String>> {
    let arr = item?.as_array()?;
    let mut out = Vec::new();
    for value in arr.iter() {
        out.push(value.as_str()?.to_owned());
    }
    Some(out)
}

/// Extracts `(text, span)` from a string-valued item (dotted or inline).
fn item_string_and_span(item: &toml_edit::Item) -> Option<(String, Option<Span>)> {
    let value = item.as_value()?;
    let text = value.as_str()?.to_owned();
    Some((text, value.span().map(span_of)))
}

/// Classifies a `[workspace.dependencies]` entry into a requirement.
fn requirement_from_item(item: &toml_edit::Item) -> Option<WorkspaceDepEntry> {
    match item {
        toml_edit::Item::Value(value) => {
            let text = value.as_str()?;
            Some(WorkspaceDepEntry {
                req: req_from_text(text),
                version_span: value.span().map(span_of),
            })
        }
        toml_edit::Item::Table(table) => {
            let version = table.get("version");
            Some(WorkspaceDepEntry {
                req: version
                    .and_then(|v| v.as_value())
                    .and_then(|v| v.as_str())
                    .map(req_from_text)
                    .unwrap_or(ReqSpec::PathOnly),
                version_span: version
                    .and_then(toml_edit::Item::as_value)
                    .and_then(toml_edit::Value::span)
                    .map(span_of),
            })
        }
        _ => None,
    }
}

/// Parses a requirement string; unparseable text is carried verbatim
/// (`ReqSpec::Other`, ADR-0006) so a foreign grammar never fails
/// discovery.
fn req_from_text(text: &str) -> ReqSpec {
    match semver::VersionReq::parse(text) {
        Ok(req) => ReqSpec::VersionReq(req),
        Err(_) => ReqSpec::Other {
            raw: halyard_core::BString::from(text),
        },
    }
}

/// True when an item is the dotted/inline `workspace = true` inherit
/// marker.
fn is_workspace_inherit(item: &toml_edit::Item) -> bool {
    let inline = item
        .as_value()
        .and_then(|v| v.as_inline_table())
        .and_then(|t| t.get("workspace"))
        .and_then(|v| v.as_bool());
    let dotted = item
        .as_table()
        .and_then(|t| t.get("workspace"))
        .and_then(|v| v.as_bool());
    inline == Some(true) || dotted == Some(true)
}

/// Borrowed accessor over the two dependency-value shapes (inline table
/// or long-form/dotted table) so field reads never clone.
enum Fields<'a> {
    Inline(&'a toml_edit::InlineTable),
    Table(&'a toml_edit::Table),
}

impl Fields<'_> {
    /// Field as string (borrowed, no clone).
    fn get_str(&self, key: &str) -> Option<&str> {
        match self {
            Self::Inline(t) => t.get(key).and_then(toml_edit::Value::as_str),
            Self::Table(t) => t.get(key).and_then(toml_edit::Item::as_str),
        }
    }

    /// Field as boolean.
    fn get_bool(&self, key: &str) -> Option<bool> {
        match self {
            Self::Inline(t) => t.get(key).and_then(toml_edit::Value::as_bool),
            Self::Table(t) => t.get(key).and_then(toml_edit::Item::as_bool),
        }
    }

    /// Span of the field literal (the edit site).
    fn get_span(&self, key: &str) -> Option<Span> {
        match self {
            Self::Inline(t) => t.get(key).and_then(toml_edit::Value::span).map(span_of),
            Self::Table(t) => t.get(key).and_then(toml_edit::Item::span).map(span_of),
        }
    }
}

/// Builds a dependency row from the long-form/inline-table fields.
fn table_dep(
    name: &str,
    fields: Fields<'_>,
    kind: DepKind,
    manifest: &RelPath,
    root_manifest: &RelPath,
    facts: &WorkspaceFacts,
) -> Option<ParsedDep> {
    let dep_name = fields.get_str("package").unwrap_or(name).to_owned();
    let optional = fields.get_bool("optional").unwrap_or(false);
    let kind = if optional && kind == DepKind::Normal {
        DepKind::Optional
    } else {
        kind
    };
    // `workspace = true`: requirement + edit site live in the root
    // manifest's `[workspace.dependencies]` (§6.3).
    if fields.get_bool("workspace") == Some(true) {
        let entry = facts.workspace_deps.get(&dep_name)?;
        return Some(ParsedDep {
            name: dep_name,
            kind,
            req: entry.req.clone(),
            site: ManifestSite {
                file: root_manifest.clone(),
                spans: entry.version_span.into_iter().collect(),
            },
            internal: true,
        });
    }
    let version = fields.get_str("version");
    let req = version.map(req_from_text).unwrap_or(ReqSpec::PathOnly);
    let version_span = fields.get_span("version");
    Some(ParsedDep {
        name: dep_name,
        kind,
        req,
        site: ManifestSite {
            file: manifest.clone(),
            spans: version_span.into_iter().collect(),
        },
        internal: false,
    })
}

/// Collects the dependency rows of one dependency section table.
fn collect_section(
    table: &toml_edit::Table,
    kind: DepKind,
    manifest: &RelPath,
    root_manifest: &RelPath,
    base_dir: &str,
    facts: &WorkspaceFacts,
    out: &mut Vec<ParsedDep>,
) {
    for (name, item) in table.iter() {
        match item {
            toml_edit::Item::Value(value) => {
                // Plain string form.
                if let Some(text) = value.as_str() {
                    out.push(ParsedDep {
                        name: name.to_owned(),
                        kind,
                        req: req_from_text(text),
                        site: ManifestSite {
                            file: manifest.clone(),
                            spans: value.span().map(span_of).into_iter().collect(),
                        },
                        internal: false,
                    });
                    continue;
                }
                if let Some(inline) = value.as_inline_table() {
                    if let Some(dep) = table_dep(
                        name,
                        Fields::Inline(inline),
                        kind,
                        manifest,
                        root_manifest,
                        facts,
                    ) {
                        out.push(dep);
                    }
                }
            }
            toml_edit::Item::Table(sub) => {
                // Long form `[dependencies.foo]` or dotted
                // `foo.workspace = true`.
                if let Some(mut dep) = table_dep(
                    name,
                    Fields::Table(sub),
                    kind,
                    manifest,
                    root_manifest,
                    facts,
                ) {
                    // Path form: internal when it normalizes inside the
                    // repository (name resolution at merge time decides
                    // whether it actually hits a member).
                    if let Some(path) = sub
                        .get("path")
                        .and_then(|v| v.as_value())
                        .and_then(|v| v.as_str())
                    {
                        dep.internal = normalize_join(base_dir, path).is_some();
                    }
                    out.push(dep);
                }
            }
            _ => {}
        }
    }
}

/// Parses one member (or root) manifest into a draft.
pub(crate) fn package_draft(
    doc: &ParsedDoc<'_>,
    file: &str,
    facts: &WorkspaceFacts,
    base_dir: &str,
) -> Result<ManifestDraft, DiscoverError> {
    let manifest = RelPath::new(file).ok_or(DiscoverError::Manifest {
        file: file.to_owned(),
        format: ManifestFormat::Toml,
        message: "manifest path is not a normalized repo-relative path".to_owned(),
        span: None,
    })?;
    let root_manifest =
        RelPath::new("Cargo.toml").expect("root manifest path is always valid");
    let mut warnings: Vec<DiscoveryWarning> = Vec::new();
    let package = doc.get("package").and_then(toml_edit::Item::as_table).ok_or(
        DiscoverError::Manifest {
            file: file.to_owned(),
            format: ManifestFormat::Toml,
            message: "member manifest has no [package] table".to_owned(),
            span: None,
        },
    )?;
    let name = package
        .get("name")
        .and_then(|v| v.as_value())
        .and_then(|v| v.as_str())
        .ok_or(DiscoverError::Manifest {
            file: file.to_owned(),
            format: ManifestFormat::Toml,
            message: "[package] is missing its `name`".to_owned(),
            span: None,
        })?
        .to_owned();

    // Version: local string, or `workspace = true` inheritance (§6.3).
    // The byte site of the value is captured alongside (M1.9): the plan's
    // own-version patch targets it; inherited versions site into the root
    // manifest so all inheritors share one edit.
    let version_item = package.get("version");
    let (version, version_source, version_site) = match version_item {
        Some(item) if is_workspace_inherit(item) => match &facts.inherited_version {
            Some((text, span)) => (
                semver::Version::parse(text).ok(),
                RawVersionSource::WorkspaceInherited {
                    file: "Cargo.toml".to_owned(),
                    key_path: "package.version".to_owned(),
                },
                Some(ManifestSite {
                    file: root_manifest.clone(),
                    spans: spans_of(*span),
                }),
            ),
            None => {
                return Err(DiscoverError::Manifest {
                    file: file.to_owned(),
                    format: ManifestFormat::Toml,
                    message: "version.workspace = true but [workspace.package] has no version"
                        .to_owned(),
                    span: None,
                });
            }
        },
        Some(item) => match item.as_value().and_then(|v| v.as_str()) {
            Some(text) => {
                let parsed = semver::Version::parse(text).ok();
                if parsed.is_none() {
                    warnings.push(warning(
                        "invalid-version",
                        &name,
                        format!(
                            "version {text:?} is not valid semver; package treated as unpublished"
                        ),
                    ));
                }
                let site = ManifestSite {
                    file: manifest.clone(),
                    spans: spans_of(item.span().map(span_of)),
                };
                (parsed, RawVersionSource::Local, Some(site))
            }
            None => {
                warnings.push(warning(
                    "no-version",
                    &name,
                    "no usable `version` in [package]; package treated as unpublished".to_owned(),
                ));
                (None, RawVersionSource::Local, None)
            }
        },
        None => {
            warnings.push(warning(
                "no-version",
                &name,
                "no `version` in [package]; package treated as unpublished".to_owned(),
            ));
            (None, RawVersionSource::Local, None)
        }
    };

    // `publish = false` flips publishability; `publish = [registries]`
    // keeps it on (§6.3).
    let publishable = match package.get("publish") {
        Some(item) => item
            .as_value()
            .and_then(|v| v.as_bool())
            .map(|b| b)
            .unwrap_or(true),
        None => true,
    };

    // `package.metadata.halyard` overrides (§6.3): `tag_style` for now.
    let tag_template = package
        .get("metadata")
        .and_then(toml_edit::Item::as_table)
        .and_then(|t| t.get("halyard"))
        .and_then(toml_edit::Item::as_table)
        .and_then(|t| t.get("tag_style"))
        .and_then(|v| v.as_value())
        .and_then(|v| v.as_str())
        .map(|raw| parse_tag_template(raw, file, None))
        .transpose()?;

    // Facets for the §6.5 link rules.
    let cdylib = package
        .get("lib")
        .and_then(toml_edit::Item::as_table)
        .and_then(|t| t.get("crate-type"))
        .and_then(|v| v.as_value())
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .any(|t| t == "cdylib")
        })
        .unwrap_or(false);

    // Dependency sections (root-level and per-target, §6.3).
    let mut deps: Vec<ParsedDep> = Vec::new();
    for (section, kind) in [
        ("dependencies", DepKind::Normal),
        ("dev-dependencies", DepKind::Dev),
        ("build-dependencies", DepKind::Build),
    ] {
        if let Some(table) = doc.get(section).and_then(toml_edit::Item::as_table) {
            collect_section(table, kind, &manifest, &root_manifest, base_dir, facts, &mut deps);
        }
    }
    if let Some(targets) = doc.get("target").and_then(toml_edit::Item::as_table) {
        for (_cfg, target_item) in targets.iter() {
            if let Some(target_table) = target_item.as_table() {
                for (section, kind) in [
                    ("dependencies", DepKind::Normal),
                    ("dev-dependencies", DepKind::Dev),
                    ("build-dependencies", DepKind::Build),
                ] {
                    if let Some(table) =
                        target_table.get(section).and_then(toml_edit::Item::as_table)
                    {
                        collect_section(
                            table,
                            kind,
                            &manifest,
                            &root_manifest,
                            base_dir,
                            facts,
                            &mut deps,
                        );
                    }
                }
            }
        }
    }

    // wasm-bindgen facet: any dependency named wasm-bindgen (§6.5).
    let wasm_bindgen = deps.iter().any(|d| d.name == "wasm-bindgen");

    // `[patch]` / `[replace]` are ignored with a warning (§6.3).
    for section in ["patch", "replace"] {
        if doc.get(section).is_some() {
            warnings.push(warning(
                "ignored-section",
                file,
                format!("[{section}] is ignored by discovery (registry overrides are not \
                         modeled)"),
            ));
        }
    }

    Ok(ManifestDraft {
        name,
        version,
        version_source,
        publishable,
        deps,
        tag_template,
        version_site,
        facets: Facets {
            cdylib,
            wasm_bindgen,
            napi: false,
        },
        warnings,
    })
}

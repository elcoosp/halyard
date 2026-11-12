//! [`EcosystemAdapter`] implementation for Cargo (M1.5, §6.3).
//!
//! Parses manifests directly with `toml_edit` spans — never `cargo
//! metadata` (§6.2, invariant I6) — and reports every dependency
//! requirement with its byte-range edit site (§9.1).
//!
//! Covered Cargo surface: virtual workspaces, root packages, member globs
//! and excludes, `version.workspace = true` inheritance,
//! `[workspace.dependencies]` with `workspace = true` members, renames
//! (`package = "…"`, `[dependencies.foo]` table form), `[target.'cfg(…)']`
//! tables, `[build-dependencies]`/`[dev-dependencies]`, optional deps,
//! `publish = false` / `publish = [registries]`, `package.metadata.halyard`
//! overrides (`tag_style`), `[patch]`/`[replace]` ignored with a warning,
//! nested-workspace rejection and version-less packages (unpublishable +
//! warning, §6.3).

use halyard_core::discover::{
    DiscoverCtx, DiscoverError, ManifestFormat, RawDep, RawPackage,
};
use halyard_core::package::{Ecosystem, Span, TagTemplate};
use halyard_core::{EcosystemAdapter, DiscoveryWarning};

mod parse;

use parse::{ManifestDraft, WorkspaceFacts};

/// The Cargo adapter (§7.3 discovery subset).
#[derive(Clone, Copy, Debug, Default)]
pub struct CargoAdapter;

impl CargoAdapter {
    /// New adapter.
    pub fn new() -> Self {
        Self
    }
}

impl EcosystemAdapter for CargoAdapter {
    fn id(&self) -> &'static str {
        "cargo"
    }

    fn discover(&self, ctx: &DiscoverCtx) -> Result<Vec<RawPackage>, DiscoverError> {
        // No root manifest → not a Cargo repository; the orchestrator
        // merges adapters, so an empty result is correct here.
        let Some(root_bytes) = read_manifest(ctx, "Cargo.toml")? else {
            return Ok(Vec::new());
        };
        let root_raw = manifest_str(&root_bytes, "Cargo.toml")?;
        let root_doc = parse::parse_document(root_raw, "Cargo.toml")?;
        let facts = parse::workspace_facts(&root_doc)?;

        if facts.workspace_members.is_none() && !facts.has_package {
            // A root manifest that is neither package nor workspace.
            return Err(DiscoverError::Manifest {
                file: "Cargo.toml".to_owned(),
                format: ManifestFormat::Toml,
                message: "manifest has neither [package] nor [workspace]".to_owned(),
                span: None,
            });
        }

        // Member directories, sorted for determinism (I3). The root
        // package (when present) is always a member (cargo semantics) and
        // sorts first because its directory path is empty.
        let member_dirs = enumerate_members(ctx, &facts)?;

        let mut packages = Vec::new();
        for dir in member_dirs {
            let manifest_path = if dir.is_empty() {
                "Cargo.toml".to_owned()
            } else {
                format!("{dir}/Cargo.toml")
            };
            let Some(bytes) = read_manifest(ctx, &manifest_path)? else {
                return Err(DiscoverError::Glob {
                    pattern: manifest_path.clone(),
                    message: "member directory has no Cargo.toml".to_owned(),
                });
            };
            let raw = manifest_str(&bytes, &manifest_path)?;
            let doc = parse::parse_document(raw, &manifest_path)?;
            // Nested workspaces are rejected with guidance (§6.3). The
            // root manifest may carry [workspace] by definition.
            if manifest_path != "Cargo.toml" && parse::has_workspace_table(&doc) {
                return Err(DiscoverError::NestedWorkspace {
                    outer: "Cargo.toml".to_owned(),
                    inner: manifest_path,
                });
            }
            let draft =
                parse::package_draft(&doc, &manifest_path, &facts, dir.as_str())?;
            packages.push(build_raw_package(dir, manifest_path, draft));
        }
        fill_raw_texts(ctx, &mut packages);
        Ok(packages)
    }
}

/// Decodes manifest bytes; non-UTF-8 manifests are `E-DISC-MANIFEST`.
fn manifest_str<'a>(bytes: &'a [u8], path: &str) -> Result<&'a str, DiscoverError> {
    std::str::from_utf8(bytes).map_err(|_| DiscoverError::Manifest {
        file: path.to_owned(),
        format: ManifestFormat::Toml,
        message: "manifest is not valid UTF-8".to_owned(),
        span: None,
    })
}

/// Reads a manifest from the context; `Ok(None)` when absent.
fn read_manifest(ctx: &DiscoverCtx, path: &str) -> Result<Option<Vec<u8>>, DiscoverError> {
    match ctx.file(path) {
        Some(bytes) => Ok(Some(bytes.to_vec())),
        None => {
            // Distinguish "absent" from "present but unreadable": the
            // orchestrator registers every file it walked, so absence here
            // means the manifest is not in the committed tree.
            Ok(None)
        }
    }
}

/// Expands the member list: explicit paths and globs against the committed
/// directory inventory, minus `exclude`, sorted (§6.2).
fn enumerate_members(ctx: &DiscoverCtx, facts: &WorkspaceFacts) -> Result<Vec<String>, DiscoverError> {
    let dirs = ctx.dirs();
    let Some(patterns) = &facts.workspace_members else {
        // Single-package mode: the root package only.
        return Ok(vec![String::new()]);
    };
    let mut members: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for pattern in patterns {
        if halyard_core::glob::matches(pattern, "") || !has_glob_meta(pattern) {
            // Explicit directory member (or the workspace root itself).
            if dirs.iter().any(|d| *d == pattern.as_str())
                || pattern.is_empty()
                || ctx.file(&format!("{pattern}/Cargo.toml")).is_some()
                || ctx.file("Cargo.toml").is_some() && pattern.is_empty()
            {
                members.insert(pattern.clone());
                continue;
            }
            return Err(DiscoverError::Glob {
                pattern: pattern.clone(),
                message: "explicit member has no directory in the committed tree".to_owned(),
            });
        }
        // Glob pattern: match committed directories that carry a manifest.
        for dir in &dirs {
            if halyard_core::glob::matches(pattern, dir)
                && ctx.file(&format!("{dir}/Cargo.toml")).is_some()
            {
                members.insert((*dir).to_owned());
            }
        }
    }
    for pattern in &facts.workspace_excludes {
        let excluded: Vec<String> = members
            .iter()
            .filter(|dir| halyard_core::glob::matches(pattern, dir) && !dir.is_empty())
            .cloned()
            .collect();
        for dir in excluded {
            members.remove(&dir);
        }
    }
    // The workspace root package is a member whenever the root manifest
    // has [package] (cargo semantics), even when an exclude would match
    // it.
    if facts.has_package {
        members.insert(String::new());
    }
    Ok(members.into_iter().collect())
}

/// True when the pattern uses glob metacharacters.
fn has_glob_meta(pattern: &str) -> bool {
    pattern.chars().any(|c| matches!(c, '*' | '?' | '[' | ']'))
}

/// Converts a parsed draft into the orchestrator-facing row.
fn build_raw_package(dir: String, manifest: String, draft: ManifestDraft) -> RawPackage {
    RawPackage {
        name: draft.name,
        ecosystem: Ecosystem::Cargo,
        root: dir,
        manifest,
        version: draft.version,
        version_source: draft.version_source,
        publishable: draft.publishable,
        deps: draft
            .deps
            .into_iter()
            .map(|d| RawDep {
                name: d.name,
                kind: d.kind,
                req: d.req,
                site: d.site,
                // Filled by `fill_raw_texts` once the manifest bytes are
                // reachable again (the adapter keeps them in the context).
                raw: halyard_core::BString::default(),
                internal: d.internal,
            })
            .collect(),
        tag_template: draft.tag_template,
        changelog: None,
        version_site: draft.version_site,
        include: halyard_core::package::PathFilter::new(Vec::new(), Vec::new()),
        facets: draft.facets,
        warnings: draft.warnings,
    }
}

/// Slices the raw requirement text out of the manifest bytes (ADR-0008:
/// the planner's rewriter needs the text as written, not `VersionReq`'s
/// normalized `Display`). Empty when the site carries no span or the span
/// does not round-trip (defensive: discovery-produced spans always do).
fn slice_site(bytes: &[u8], site: &halyard_core::ManifestSite) -> halyard_core::BString {
    let Some(span) = site.spans.first() else {
        return halyard_core::BString::default();
    };
    let (start, end) = (span.start as usize, span.end as usize);
    match bytes.get(start..end) {
        Some(text) => halyard_core::BString::from(text),
        None => halyard_core::BString::default(),
    }
}

/// Fills every dep row's raw text from the context's manifest bytes.
fn fill_raw_texts(ctx: &DiscoverCtx, packages: &mut [RawPackage]) {
    for pkg in packages {
        for dep in &mut pkg.deps {
            let bytes = ctx.file(dep.site.file.as_str());
            if let Some(bytes) = bytes {
                dep.raw = slice_site(bytes, &dep.site);
            }
        }
    }
}

/// Shared helpers re-exported for tests.
pub(crate) fn span_of(range: std::ops::Range<usize>) -> Span {
    Span {
        start: u32::try_from(range.start).unwrap_or(u32::MAX),
        end: u32::try_from(range.end).unwrap_or(u32::MAX),
    }
}

/// `span_of` for optional ranges (a `None` span yields no sites).
pub(crate) fn spans_of(span: Option<Span>) -> Vec<Span> {
    span.into_iter().collect()
}

/// Warning constructor used by the parser.
pub(crate) fn warning(code: &'static str, subject: &str, message: String) -> DiscoveryWarning {
    DiscoveryWarning {
        code,
        subject: subject.to_owned(),
        message,
    }
}

/// Resolves a relative path from a member directory into a normalized
/// repo-relative path (handles `..`, `.` and duplicated separators).
pub(crate) fn normalize_join(base: &str, rel: &str) -> Option<String> {
    let mut stack: Vec<&str> = Vec::new();
    if !base.is_empty() {
        stack.extend(base.split('/'));
    }
    for part in rel.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                stack.pop()?;
            }
            _ => stack.push(part),
        }
    }
    Some(stack.join("/"))
}

/// Convenience re-export for the orchestrator: tag template parse with the
/// manifest-format error mapping.
pub(crate) fn parse_tag_template(
    raw: &str,
    file: &str,
    span: Option<Span>,
) -> Result<TagTemplate, DiscoverError> {
    TagTemplate::parse(raw).map_err(|e| DiscoverError::Manifest {
        file: file.to_owned(),
        format: ManifestFormat::Toml,
        message: e.to_string(),
        span,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_join_handles_parent_segments() {
        assert_eq!(
            normalize_join("crates/a", "../core").as_deref(),
            Some("crates/core")
        );
        assert_eq!(normalize_join("crates/a", "./sub").as_deref(), Some("crates/a/sub"));
        assert_eq!(normalize_join("", "Cargo.toml").as_deref(), Some("Cargo.toml"));
        // Escaping the repository root is not a valid repo-relative path.
        assert_eq!(normalize_join("a", "../../b"), None);
        assert_eq!(normalize_join("a/b", "../..").as_deref(), Some(""));
    }

    #[test]
    fn glob_meta_detection() {
        assert!(has_glob_meta("crates/*"));
        assert!(has_glob_meta("a/b?"));
        assert!(!has_glob_meta("crates/core"));
    }
}

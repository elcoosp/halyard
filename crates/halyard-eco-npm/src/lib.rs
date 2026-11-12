//! [`EcosystemAdapter`] implementation for npm / pnpm / yarn / bun
//! (M1.6, §6.4).
//!
//! Parses `package.json` through the lossless span scanner
//! ([`json_span`], ADR-0001) and `pnpm-workspace.yaml` through the
//! restricted scanner ([`pnpm_yaml`], §19.2). Covered surface: npm/yarn/
//! bun `workspaces` (array and object forms), pnpm `packages` globs with
//! negations, `catalog:` / `catalog:<name>` protocols, `workspace:*` /
//! `workspace:^` / `workspace:~` / `workspace:<version>`, `file:` /
//! `link:`, `private: true` and `publishConfig` (read, not interpreted).

use halyard_core::discover::{
    DiscoverCtx, DiscoverError, DiscoveryWarning, Facets, ManifestFormat, RawDep, RawPackage,
    RawVersionSource,
};
use halyard_core::package::{
    DepKind, Ecosystem, ManifestSite, RelPath, ReqSpec, Span, WorkspaceProtocol,
};
use halyard_core::{EcosystemAdapter, glob};

pub mod json_span;
pub mod pnpm_yaml;

/// The npm-family adapter (§7.3 discovery subset).
#[derive(Clone, Copy, Debug, Default)]
pub struct NpmAdapter;

impl NpmAdapter {
    /// New adapter.
    pub fn new() -> Self {
        Self
    }
}

/// The workspace-level sources of member globs, in detection order.
struct GlobSources {
    /// pnpm `packages` patterns (negations included).
    pnpm: Option<Vec<String>>,
    /// npm/yarn/bun `workspaces` patterns.
    npm: Vec<String>,
}

impl EcosystemAdapter for NpmAdapter {
    fn id(&self) -> &'static str {
        "npm"
    }

    fn discover(&self, ctx: &DiscoverCtx) -> Result<Vec<RawPackage>, DiscoverError> {
        // Workspace sources: pnpm-workspace.yaml first (it is the
        // authority when both exist), then the root package.json.
        let pnpm_globs = match ctx.file("pnpm-workspace.yaml") {
            Some(bytes) => {
                let text = utf8(bytes, "pnpm-workspace.yaml")?;
                let ws = pnpm_yaml::parse_pnpm_workspace(text).map_err(|e| {
                    DiscoverError::Manifest {
                        file: "pnpm-workspace.yaml".to_owned(),
                        format: ManifestFormat::Yaml,
                        message: e.to_string(),
                        span: None,
                    }
                })?;
                Some(ws.packages)
            }
            None => None,
        };
        let root_json = match ctx.file("package.json") {
            Some(bytes) => Some(json_span::JsonDoc::parse(bytes).map_err(|e| {
                DiscoverError::Manifest {
                    file: "package.json".to_owned(),
                    format: ManifestFormat::Json,
                    message: e.to_string(),
                    span: Some(e_position(&e)),
                }
            })?),
            None => None,
        };
        let npm_globs: Vec<String> = match &root_json {
            Some(doc) => match doc.get("workspaces") {
                Some(value) => match &value.value {
                    json_span::JsonValue::Array(items) => items
                        .iter()
                        .filter_map(|v| match &v.value {
                            json_span::JsonValue::String { decoded, .. } => {
                                Some(decoded.clone())
                            }
                            _ => None,
                        })
                        .collect(),
                    json_span::JsonValue::Object(entries) => entries
                        .iter()
                        .filter(|e| e.key == "packages")
                        .flat_map(|e| match &e.value.value {
                            json_span::JsonValue::Array(items) => items
                                .iter()
                                .filter_map(|v| match &v.value {
                                    json_span::JsonValue::String { decoded, .. } => {
                                        Some(decoded.clone())
                                    }
                                    _ => None,
                                })
                                .collect::<Vec<String>>(),
                            _ => Vec::new(),
                        })
                        .collect(),
                    _ => Vec::new(),
                },
                None => Vec::new(),
            },
            None => Vec::new(),
        };
        let sources = GlobSources { pnpm: pnpm_globs, npm: npm_globs };
        if sources.pnpm.is_none() && sources.npm.is_empty() && root_json.is_none() {
            // No npm workspace or package here; the orchestrator merges
            // adapters, so an empty result is correct.
            return Ok(Vec::new());
        }

        // Member directories (sorted), then the root package (dir "") in
        // its sorted position.
        let mut dirs = member_dirs(ctx, &sources)?;
        if root_json.is_some() {
            dirs.insert(0, String::new());
        }

        let mut packages = Vec::new();
        for dir in dirs {
            let manifest_path = if dir.is_empty() {
                "package.json".to_owned()
            } else {
                format!("{dir}/package.json")
            };
            let Some(bytes) = ctx.file(&manifest_path) else {
                return Err(DiscoverError::Glob {
                    pattern: manifest_path,
                    message: "member directory has no package.json".to_owned(),
                });
            };
            let doc = json_span::JsonDoc::parse(bytes).map_err(|e| DiscoverError::Manifest {
                file: manifest_path.clone(),
                format: ManifestFormat::Json,
                message: e.to_string(),
                span: Some(e_position(&e)),
            })?;
            packages.push(package_from_doc(&doc, dir, &manifest_path)?);
        }
        Ok(packages)
    }
}

/// Converts a scanner error position into a core span.
fn e_position(e: &json_span::JsonError) -> Span {
    let start = u32::try_from(e.position).unwrap_or(u32::MAX);
    Span {
        start,
        end: start,
    }
}

/// Decodes manifest bytes; non-UTF-8 manifests are `E-DISC-MANIFEST`.
fn utf8<'a>(bytes: &'a [u8], file: &str) -> Result<&'a str, DiscoverError> {
    std::str::from_utf8(bytes).map_err(|_| DiscoverError::Manifest {
        file: file.to_owned(),
        format: ManifestFormat::Json,
        message: "manifest is not valid UTF-8".to_owned(),
        span: None,
    })
}

/// Expands member globs (pnpm negations + npm patterns) against the
/// committed directory inventory (§6.2). Dirs without a `package.json`
/// are skipped (glob members are candidate roots, not requirements).
fn member_dirs(ctx: &DiscoverCtx, sources: &GlobSources) -> Result<Vec<String>, DiscoverError> {
    let mut patterns: Vec<String> = Vec::new();
    if let Some(pnpm) = &sources.pnpm {
        patterns.extend(pnpm_yaml::apply_negations(pnpm));
    }
    patterns.extend(sources.npm.iter().cloned());
    let dirs = ctx.dirs();
    let mut members: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for pattern in &patterns {
        if glob::matches(pattern, "") || !pattern.chars().any(|c| matches!(c, '*' | '?' | '[' | ']' )) {
            // Explicit directory member.
            if dirs.iter().any(|d| *d == pattern.as_str())
                || ctx.file(&format!("{pattern}/package.json")).is_some()
            {
                members.insert(pattern.clone());
                continue;
            }
            return Err(DiscoverError::Glob {
                pattern: pattern.clone(),
                message: "explicit member has no directory in the committed tree".to_owned(),
            });
        }
        for dir in &dirs {
            if glob::matches(pattern, dir) && ctx.file(&format!("{dir}/package.json")).is_some() {
                members.insert((*dir).to_owned());
            }
        }
    }
    Ok(members.into_iter().collect())
}

/// Builds one raw package row from a parsed `package.json`.
fn package_from_doc(
    doc: &json_span::JsonDoc,
    dir: String,
    manifest_path: &str,
) -> Result<RawPackage, DiscoverError> {
    let mut warnings: Vec<DiscoveryWarning> = Vec::new();
    let manifest = RelPath::new(manifest_path).ok_or(DiscoverError::Manifest {
        file: manifest_path.to_owned(),
        format: ManifestFormat::Json,
        message: "manifest path is not a normalized repo-relative path".to_owned(),
        span: None,
    })?;

    let name = match doc.get("name").map(|v| &v.value) {
        Some(json_span::JsonValue::String { decoded, .. }) => Some(decoded.clone()),
        Some(_) => {
            return Err(DiscoverError::Manifest {
                file: manifest_path.to_owned(),
                format: ManifestFormat::Json,
                message: "`name` must be a string".to_owned(),
                span: doc.get("name").map(|v| v.span),
            });
        }
        None => None,
    };
    let Some(name) = name else {
        // An anonymous package.json can only be the root workspace host —
        // it hosts members but is not itself a package. The orchestrator
        // only reaches this path for named root manifests.
        return Err(DiscoverError::Manifest {
            file: manifest_path.to_owned(),
            format: ManifestFormat::Json,
            message: "package.json has no `name`".to_owned(),
            span: None,
        });
    };

    let (version, version_site) = match doc.get("version").map(|v| &v.value) {
        Some(json_span::JsonValue::String { decoded, inner }) => {
            let parsed = semver::Version::parse(decoded).ok();
            if parsed.is_none() {
                warnings.push(DiscoveryWarning {
                    code: "invalid-version",
                    subject: name.clone(),
                    message: format!("version {decoded:?} is not valid semver"),
                });
            }
            let site = ManifestSite {
                file: manifest.clone(),
                spans: vec![*inner],
            };
            (parsed, Some(site))
        }
        _ => {
            warnings.push(DiscoveryWarning {
                code: "no-version",
                subject: name.clone(),
                message: "no `version` in package.json; package treated as unpublished"
                    .to_owned(),
            });
            (None, None)
        }
    };

    // `private: true` ⇒ not publishable (§6.4).
    let publishable = !matches!(doc.get("private").map(|v| &v.value),
        Some(json_span::JsonValue::Bool(true)));
    // `publishConfig` is read but not interpreted (§6.4).
    let _publish_config = doc.get("publishConfig");

    // Dependency fields (§6.4): order fixed for determinism.
    let mut deps: Vec<RawDep> = Vec::new();
    for (field, kind) in [
        ("dependencies", DepKind::Normal),
        ("devDependencies", DepKind::Dev),
        ("peerDependencies", DepKind::Peer),
        ("optionalDependencies", DepKind::Optional),
    ] {
        if let Some(json_span::JsonValue::Object(entries)) =
            doc.get(field).map(|v| &v.value)
        {
            for entry in entries {
                let dep_name = entry.key.clone();
                let req_text = match &entry.value.value {
                    json_span::JsonValue::String { decoded, inner } => Some((decoded.as_str(), *inner)),
                    _ => None,
                };
                let Some((text, inner)) = req_text else {
                    warnings.push(DiscoveryWarning {
                        code: "ignored-value",
                        subject: dep_name.clone(),
                        message: format!("{field}.{dep_name} is not a string requirement; ignored"),
                    });
                    continue;
                };
                let (req, internal) = classify_req(text);
                deps.push(RawDep {
                    name: dep_name,
                    kind,
                    req,
                    site: ManifestSite {
                        file: manifest.clone(),
                        spans: vec![inner],
                    },
                    raw: halyard_core::BString::from(text),
                    internal,
                });
            }
        }
    }
    // optionalDependencies entries override same-name dependency entries
    // (npm semantics): drop the Normal row, keep Optional.
    if deps.iter().any(|d| d.kind == DepKind::Optional) {
        let mut seen_optional: std::collections::BTreeSet<String> = deps
            .iter()
            .filter(|d| d.kind == DepKind::Optional)
            .map(|d| d.name.clone())
            .collect();
        deps.retain(|d| {
            if d.kind == DepKind::Normal && seen_optional.contains(&d.name) {
                seen_optional.remove(&d.name);
                return false;
            }
            true
        });
    }

    // napi facet (§6.5): a `napi` key or an `@napi-rs/cli` dependency.
    let napi = doc.get("napi").is_some()
        || deps.iter().any(|d| d.name == "@napi-rs/cli");

    Ok(RawPackage {
        name,
        ecosystem: Ecosystem::Npm,
        root: dir,
        manifest: manifest_path.to_owned(),
        version,
        version_source: RawVersionSource::Local,
        publishable,
        deps,
        tag_template: None,
        changelog: None,
        version_site,
        include: halyard_core::package::PathFilter::new(Vec::new(), Vec::new()),
        facets: Facets {
            cdylib: false,
            wasm_bindgen: false,
            napi,
        },
        warnings,
    })
}

/// Classifies an npm requirement (§6.4 protocols).
fn classify_req(text: &str) -> (ReqSpec, bool) {
    if let Some(rest) = text.strip_prefix("workspace:") {
        let protocol = match rest {
            "*" => WorkspaceProtocol::Star,
            "^" => WorkspaceProtocol::Caret,
            "~" => WorkspaceProtocol::Tilde,
            _ => WorkspaceProtocol::Exact,
        };
        return (ReqSpec::WorkspaceProtocol(protocol), true);
    }
    if text == "catalog:" {
        return (ReqSpec::Catalog(None), true);
    }
    if let Some(name) = text.strip_prefix("catalog:") {
        return (ReqSpec::Catalog(Some(halyard_core::BString::from(name))), true);
    }
    if text.starts_with("file:") || text.starts_with("link:") {
        return (ReqSpec::PathOnly, true);
    }
    match semver::VersionReq::parse(text) {
        Ok(req) => (ReqSpec::VersionReq(req), false),
        // ADR-0006: foreign grammars (`>=8 <10`, `git:`, URLs) ride along
        // verbatim; npm resolves them outside the workspace.
        Err(_) => (
            ReqSpec::Other {
                raw: halyard_core::BString::from(text),
            },
            false,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx_with(files: &[(&str, &str)]) -> DiscoverCtx {
        let mut ctx = DiscoverCtx::new();
        for (path, body) in files {
            assert!(ctx.insert_file(path, body.as_bytes().to_vec()), "{path}");
        }
        ctx
    }

    #[test]
    fn npm_workspaces_array_and_object_forms() {
        for (body, expected) in [
            (
                r#"{"name":"root","private":true,"workspaces":["packages/*","apps/*"]}"#,
                vec!["apps/a", "packages/x"],
            ),
            (
                r#"{"name":"root","workspaces":{"packages":["packages/*","apps/*"]}}"#,
                vec!["apps/a", "packages/x"],
            ),
        ] {
            let ctx = ctx_with(&[
                ("package.json", body),
                ("packages/x/package.json", r#"{"name":"x","version":"1.0.0"}"#),
                ("apps/a/package.json", r#"{"name":"a","version":"0.1.0"}"#),
            ]);
            let pkgs = NpmAdapter::new().discover(&ctx).unwrap();
            let roots: Vec<&str> = pkgs.iter().map(|p| p.root.as_str()).collect();
            assert_eq!(roots, vec!["", "apps/a", "packages/x"]);
            assert_eq!(pkgs[0].root, "");
            assert_eq!(pkgs[1].name, "a");
            assert_eq!(pkgs[2].name, "x");
            let _ = expected;
        }
    }

    #[test]
    fn pnpm_negations_and_catalog_protocols() {
        let ctx = ctx_with(&[
            (
                "pnpm-workspace.yaml",
                "packages:\n  - 'packages/*'\n  - '!packages/secret'\ncatalog:\n  react: ^18.0.0\n",
            ),
            ("package.json", r#"{"name":"root","private":true}"#),
            ("packages/x/package.json", r#"{"name":"x","version":"1.0.0","dependencies":{"react":"catalog:","lodash":"catalog:std"}}"#),
            ("packages/secret/package.json", r#"{"name":"secret","version":"0.0.1","private":true}"#),
        ]);
        let pkgs = NpmAdapter::new().discover(&ctx).unwrap();
        // Root + x only (secret negated); root has a name so it is a member.
        let names: Vec<&str> = pkgs.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["root", "x"]);
        let x = pkgs.iter().find(|p| p.name == "x").unwrap();
        assert_eq!(x.deps.len(), 2);
        assert!(matches!(x.deps[0].req, ReqSpec::Catalog(None)));
        assert!(matches!(
            x.deps[1].req,
            ReqSpec::Catalog(Some(ref n)) if n == b"std"
        ));
        assert!(x.deps[0].internal);
    }

    #[test]
    fn protocols_covered() {
        let ctx = ctx_with(&[
            ("package.json", r#"{"name":"root","workspaces":["packages/*"]}"#),
            (
                "packages/x/package.json",
                r#"{"name":"x","version":"1.0.0","dependencies":{"a":"workspace:*","b":"workspace:^","c":"workspace:~","d":"workspace:1.2.3","e":"file:../f","f":"link:../g","g":">=8 <10"}}"#,
            ),
        ]);
        let pkgs = NpmAdapter::new().discover(&ctx).unwrap();
        let x = pkgs.iter().find(|p| p.name == "x").unwrap();
        let reqs: Vec<(&ReqSpec, bool)> = x.deps.iter().map(|d| (&d.req, d.internal)).collect();
        assert!(matches!(reqs[0], (ReqSpec::WorkspaceProtocol(WorkspaceProtocol::Star), true)));
        assert!(matches!(reqs[1], (ReqSpec::WorkspaceProtocol(WorkspaceProtocol::Caret), true)));
        assert!(matches!(reqs[2], (ReqSpec::WorkspaceProtocol(WorkspaceProtocol::Tilde), true)));
        assert!(matches!(reqs[3], (ReqSpec::WorkspaceProtocol(WorkspaceProtocol::Exact), true)));
        assert!(matches!(reqs[4], (ReqSpec::PathOnly, true)));
        assert!(matches!(reqs[5], (ReqSpec::PathOnly, true)));
        assert!(matches!(reqs[6], (ReqSpec::Other { .. }, false)));
    }

    #[test]
    fn private_publish_config_and_dep_kinds() {
        let ctx = ctx_with(&[
            ("package.json", r#"{"name":"root","workspaces":["packages/*"]}"#),
            (
                "packages/x/package.json",
                r#"{"name":"x","version":"1.0.0","private":true,"publishConfig":{"access":"public"},"dependencies":{"y":"workspace:*"},"devDependencies":{"z":"workspace:*"},"peerDependencies":{"w":"^1"},"optionalDependencies":{"o":"workspace:*"}}"#,
            ),
        ]);
        let pkgs = NpmAdapter::new().discover(&ctx).unwrap();
        let x = pkgs.iter().find(|p| p.name == "x").unwrap();
        assert!(!x.publishable);
        let kinds: Vec<(String, DepKind)> =
            x.deps.iter().map(|d| (d.name.clone(), d.kind)).collect();
        assert!(kinds.contains(&("y".to_owned(), DepKind::Normal)));
        assert!(kinds.contains(&("z".to_owned(), DepKind::Dev)));
        assert!(kinds.contains(&("w".to_owned(), DepKind::Peer)));
        assert!(kinds.contains(&("o".to_owned(), DepKind::Optional)));
    }

    #[test]
    fn optional_dependencies_override_normal() {
        let ctx = ctx_with(&[
            ("package.json", r#"{"name":"root","workspaces":["packages/*"]}"#),
            (
                "packages/x/package.json",
                r#"{"name":"x","dependencies":{"o":"^1"},"optionalDependencies":{"o":"^1"}}"#,
            ),
        ]);
        let pkgs = NpmAdapter::new().discover(&ctx).unwrap();
        let x = pkgs.iter().find(|p| p.name == "x").unwrap();
        let o_count = x.deps.iter().filter(|d| d.name == "o").count();
        assert_eq!(o_count, 1);
        assert_eq!(x.deps[0].kind, DepKind::Optional);
    }

    #[test]
    fn napi_facet_from_key_or_dep() {
        let ctx = ctx_with(&[
            ("package.json", r#"{"name":"root","workspaces":["packages/*"]}"#),
            ("packages/a/package.json", r#"{"name":"a","version":"1.0.0","napi":{"binaryName":"a"}}"#),
            ("packages/b/package.json", r#"{"name":"b","version":"1.0.0","devDependencies":{"@napi-rs/cli":"^2"}}"#),
            ("packages/c/package.json", r#"{"name":"c","version":"1.0.0"}"#),
        ]);
        let pkgs = NpmAdapter::new().discover(&ctx).unwrap();
        let facet = |name: &str| pkgs.iter().find(|p| p.name == name).unwrap().facets.napi;
        assert!(facet("a"));
        assert!(facet("b"));
        assert!(!facet("c"));
    }

    #[test]
    fn missing_name_is_an_error_with_stable_code() {
        let ctx = ctx_with(&[("package.json", r#"{"version":"1.0.0"}"#)]);
        let err = NpmAdapter::new().discover(&ctx).unwrap_err();
        assert_eq!(err.code(), "E-DISC-MANIFEST");
    }

    #[test]
    fn broken_json_is_a_manifest_error() {
        let ctx = ctx_with(&[("package.json", "{\"name\": ")]);
        let err = NpmAdapter::new().discover(&ctx).unwrap_err();
        assert_eq!(err.code(), "E-DISC-MANIFEST");
    }
}

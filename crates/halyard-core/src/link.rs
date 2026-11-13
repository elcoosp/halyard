//! Polyglot link detection (§6.5): cross-ecosystem edges between native
//! crates and their binding packages, each with a bump policy.
//!
//! Rules (each proven from manifest facts collected during discovery —
//! see [`Facets`](crate::discover::Facets)):
//!
//! | Rule | Detection | Default mode |
//! |---|---|---|
//! | [`LinkRule::NapiRs`] | npm package with a `napi` key / `@napi-rs/cli` dep whose directory hosts a `cdylib` Cargo.toml; npm platform sub-packages under `npm/` group with the main package | lockstep |
//! | [`LinkRule::WasmPack`] | Cargo package depending on `wasm-bindgen` with a `package.json` in the same directory | lockstep |
//! | [`LinkRule::Explicit`] | `halyard.toml: [[link]]` | configured (default cascade) |
//! | [`LinkRule::WorkspacePath`] | an npm package referenced through a `workspace:` requirement whose directory also hosts a Cargo.toml | cascade |
//!
//! Edge direction: `from` is the upstream (the bumped source), `to` the
//! dependent that moves because of it — `Cargo → Npm` for binding pairs.
//! Precedence when two rules claim the same pair: `Explicit` wins over the
//! manifest heuristics, napi/wasm-pack win over the generic
//! workspace-path rule (one edge per pair).

use crate::discover::DiscoverError;
use crate::id::PkgId;
use crate::package::ReqSpec;
use crate::package::Workspace;

/// Bump semantics of a link (§6.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkMode {
    /// Bump the dependent by at least `patch` when the source bumps.
    Cascade,
    /// Source and dependent always carry the same version (bindings
    /// pattern).
    Lockstep,
}

impl LinkMode {
    /// Wire keyword used by plans and diagnostics.
    pub const fn wire_key(self) -> &'static str {
        match self {
            Self::Cascade => "cascade",
            Self::Lockstep => "lockstep",
        }
    }
}

/// Which rule produced a link (visible via `halyard graph --explain`,
/// M1.10).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum LinkRule {
    /// napi-rs layout (§6.5 row 1).
    NapiRs,
    /// wasm-pack / wasm-bindgen layout (§6.5 row 2).
    WasmPack,
    /// `halyard.toml: [[link]]` (§6.5 row 3).
    Explicit,
    /// npm `workspace:` requirement to a co-located crate (§6.5 row 4).
    WorkspacePath,
}

impl LinkRule {
    /// Wire keyword.
    pub const fn wire_key(self) -> &'static str {
        match self {
            Self::NapiRs => "napi-rs",
            Self::WasmPack => "wasm-pack",
            Self::Explicit => "explicit",
            Self::WorkspacePath => "workspace-path",
        }
    }

    /// Precedence rank (lower wins when rules collide on one pair).
    fn rank(self) -> u8 {
        match self {
            Self::Explicit => 0,
            Self::NapiRs | Self::WasmPack => 1,
            Self::WorkspacePath => 2,
        }
    }
}

/// One polyglot link edge (§6.5): `from` is bumped first, `to` follows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LinkEdge {
    /// Upstream package (the release source).
    pub from: PkgId,
    /// Dependent package (moves because of `from`).
    pub to: PkgId,
    /// Bump semantics.
    pub mode: LinkMode,
    /// The rule that detected this link.
    pub rule: LinkRule,
}

/// A user-declared link (`halyard.toml: [[link]]`) in ecosystem-agnostic
/// form — names, resolved by [`detect_links`] against the merged
/// workspace. The config crate maps its `LinkCfg` onto this at call time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExplicitLinkSpec {
    /// Source package name.
    pub from: String,
    /// Target package name.
    pub to: String,
    /// Link mode; `None` = the cascade default (§6.5).
    pub mode: Option<LinkMode>,
}

/// Detects all polyglot links for `ws` (§6.5). Deterministic: the result
/// is deduplicated per `(from, to)` pair by rule precedence and sorted by
/// `(from, to)`. Errors with `E-DISC-LINK-UNKNOWN` when an explicit
/// [`ExplicitLinkSpec`] names a package discovery did not find.
///
/// # Errors
///
/// [`DiscoverError::UnknownLink`] for an explicit link naming an unknown
/// package.
pub fn detect_links(
    ws: &Workspace,
    explicit: &[ExplicitLinkSpec],
) -> Result<Vec<LinkEdge>, DiscoverError> {
    let mut candidates: Vec<LinkEdge> = Vec::new();

    // Rule 3: explicit [[link]] (highest precedence; names resolved
    // against all packages, first match in id order when a name is shared
    // across ecosystems — npm scoped names keep this unambiguous).
    for spec in explicit {
        let from = resolve_name(ws, &spec.from).ok_or(DiscoverError::UnknownLink {
            name: spec.from.clone(),
        })?;
        let to = resolve_name(ws, &spec.to).ok_or(DiscoverError::UnknownLink {
            name: spec.to.clone(),
        })?;
        candidates.push(LinkEdge {
            from,
            to,
            mode: spec.mode.unwrap_or(LinkMode::Cascade),
            rule: LinkRule::Explicit,
        });
    }

    // Rules 1 & 2: directory co-location between a cargo package and an
    // npm package (empty root = repo root matches root-level pairs).
    let cargo_pkgs: Vec<&crate::package::Package> = ws
        .packages()
        .iter()
        .filter(|p| matches!(p.ecosystem, crate::package::Ecosystem::Cargo))
        .collect();
    for npm in ws.packages() {
        if !matches!(npm.ecosystem, crate::package::Ecosystem::Npm) {
            continue;
        }
        let npm_root = npm.root.as_str();
        // Rule 1: napi-rs — npm binding with a cdylib crate beside it.
        if npm.facets.napi {
            for cargo in &cargo_pkgs {
                if cargo.root.as_str() == npm_root && cargo.facets.cdylib {
                    candidates.push(LinkEdge {
                        from: cargo.id,
                        to: npm.id,
                        mode: LinkMode::Lockstep,
                        rule: LinkRule::NapiRs,
                    });
                }
            }
            // Platform sub-packages (`npm/<target>` inside the main
            // package root) group lockstep with the main package.
            let prefix = format!("{npm_root}/npm/");
            for other in ws.packages() {
                if other.ecosystem == npm.ecosystem
                    && other.id != npm.id
                    && other.root.as_str().starts_with(&prefix)
                {
                    candidates.push(LinkEdge {
                        from: npm.id,
                        to: other.id,
                        mode: LinkMode::Lockstep,
                        rule: LinkRule::NapiRs,
                    });
                }
            }
        }
        // Rule 2: wasm-pack — wasm-bindgen crate + package.json in the
        // same directory.
        for cargo in &cargo_pkgs {
            if cargo.facets.wasm_bindgen && cargo.root.as_str() == npm_root {
                candidates.push(LinkEdge {
                    from: cargo.id,
                    to: npm.id,
                    mode: LinkMode::Lockstep,
                    rule: LinkRule::WasmPack,
                });
            }
        }
    }

    // Rule 4: workspace-path — an npm package referenced through a
    // `workspace:` requirement whose directory also hosts a Cargo.toml is
    // a binding package; link the co-located crate (cascade default).
    let mut referenced: std::collections::BTreeSet<PkgId> = std::collections::BTreeSet::new();
    for pkg in ws.packages() {
        for dep in &pkg.deps {
            if matches!(dep.req, ReqSpec::WorkspaceProtocol(_)) {
                referenced.insert(dep.to);
            }
        }
    }
    for target in referenced {
        if let Some(pkg) = ws.package(target)
            && matches!(pkg.ecosystem, crate::package::Ecosystem::Npm)
        {
            for cargo in &cargo_pkgs {
                if cargo.root.as_str() == pkg.root.as_str() {
                    candidates.push(LinkEdge {
                        from: cargo.id,
                        to: pkg.id,
                        mode: LinkMode::Cascade,
                        rule: LinkRule::WorkspacePath,
                    });
                }
            }
        }
    }

    Ok(dedup_by_precedence(candidates))
}

/// Resolves a package name; ties across ecosystems resolve to the lowest
/// id (deterministic; scoped npm names avoid this in practice).
fn resolve_name(ws: &Workspace, name: &str) -> Option<PkgId> {
    ws.packages()
        .iter()
        .find(|p| ws.interner().get(p.name) == Some(name))
        .map(|p| p.id)
}

/// Keeps one edge per `(from, to)` pair: lowest rule rank wins, first
/// detected wins within a rank, then sorts by `(from, to)`.
fn dedup_by_precedence(edges: Vec<LinkEdge>) -> Vec<LinkEdge> {
    let mut best: std::collections::BTreeMap<(u32, u32), LinkEdge> =
        std::collections::BTreeMap::new();
    for edge in edges {
        let key = (edge.from.to_u32(), edge.to.to_u32());
        match best.get(&key) {
            Some(existing) if existing.rule.rank() <= edge.rule.rank() => {}
            _ => {
                best.insert(key, edge);
            }
        }
    }
    best.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discover::Facets;
    use crate::package::{
        DepEdge, Ecosystem, GroupId, ManifestSite, Package, PathFilter, RelPath, TagTemplate,
        VersionSource, WorkspaceProtocol,
    };

    fn pkg(
        ws: &mut Workspace,
        name: &str,
        ecosystem: Ecosystem,
        root: &str,
        facets: Facets,
    ) -> PkgId {
        let pkg = Package {
            id: PkgId::FIRST,
            name: ws.intern(name),
            ecosystem,
            root: RelPath::new(root).unwrap(),
            manifest: RelPath::new(&format!("{root}/manifest")).unwrap_or_else(|| {
                // Root-level packages: manifest sits at the repo root.
                RelPath::new("manifest").unwrap()
            }),
            version: Some(semver::Version::parse("0.1.0").unwrap()),
            version_source: VersionSource::Local,
            publishable: true,
            deps: Vec::new(),
            tag_template: TagTemplate::parse("{name}-v{version}").unwrap(),
            changelog: None,
            version_site: None,
            include: PathFilter::new(Vec::new(), Vec::new()),
            group: GroupId::new(0),
            facets,
        };
        ws.push_package(pkg)
    }

    fn add_dep(ws: &mut Workspace, from: PkgId, to: PkgId, req: ReqSpec) {
        let dep = DepEdge {
            to,
            kind: crate::package::DepKind::Normal,
            req,
            site: ManifestSite {
                file: RelPath::new("package.json").unwrap(),
                spans: Vec::new(),
            },
            raw: crate::BString::default(),
        };
        if let Some(pkg) = ws.package_mut(from) {
            pkg.deps.push(dep);
        }
    }

    #[test]
    fn napi_rs_links_cdylib_crate_and_platform_subpackages() {
        let mut ws = Workspace::new();
        let crate_id = pkg(
            &mut ws,
            "native",
            Ecosystem::Cargo,
            "packages/native",
            Facets {
                cdylib: true,
                ..Facets::default()
            },
        );
        let main = pkg(
            &mut ws,
            "@org/native",
            Ecosystem::Npm,
            "packages/native",
            Facets {
                napi: true,
                ..Facets::default()
            },
        );
        let platform = pkg(
            &mut ws,
            "@org/native-linux-x64-gnu",
            Ecosystem::Npm,
            "packages/native/npm/linux-x64-gnu",
            Facets::default(),
        );
        let _unrelated = pkg(
            &mut ws,
            "app",
            Ecosystem::Npm,
            "packages/app",
            Facets::default(),
        );

        let links = detect_links(&ws, &[]).unwrap();
        assert_eq!(links.len(), 2);
        assert!(links.contains(&LinkEdge {
            from: crate_id,
            to: main,
            mode: LinkMode::Lockstep,
            rule: LinkRule::NapiRs,
        }));
        assert!(links.contains(&LinkEdge {
            from: main,
            to: platform,
            mode: LinkMode::Lockstep,
            rule: LinkRule::NapiRs,
        }));
    }

    #[test]
    fn wasm_pack_links_colocation() {
        let mut ws = Workspace::new();
        let crate_id = pkg(
            &mut ws,
            "engine",
            Ecosystem::Cargo,
            "crates/engine",
            Facets {
                wasm_bindgen: true,
                ..Facets::default()
            },
        );
        let binding = pkg(
            &mut ws,
            "engine-js",
            Ecosystem::Npm,
            "crates/engine",
            Facets::default(),
        );
        // A non-wasm crate beside an npm package must not link.
        let _plain = pkg(
            &mut ws,
            "plain",
            Ecosystem::Cargo,
            "crates/plain",
            Facets::default(),
        );
        let _plain_js = pkg(
            &mut ws,
            "plain-js",
            Ecosystem::Npm,
            "crates/plain",
            Facets::default(),
        );

        let links = detect_links(&ws, &[]).unwrap();
        assert_eq!(
            links,
            vec![LinkEdge {
                from: crate_id,
                to: binding,
                mode: LinkMode::Lockstep,
                rule: LinkRule::WasmPack,
            }]
        );
    }

    #[test]
    fn workspace_path_rule_links_referenced_colocations() {
        let mut ws = Workspace::new();
        let crate_id = pkg(
            &mut ws,
            "core",
            Ecosystem::Cargo,
            "crates/core",
            Facets::default(),
        );
        let binding = pkg(
            &mut ws,
            "@org/core",
            Ecosystem::Npm,
            "crates/core",
            Facets::default(),
        );
        let app = pkg(
            &mut ws,
            "app",
            Ecosystem::Npm,
            "packages/app",
            Facets::default(),
        );
        // app references the binding via workspace:* → triggers rule 4.
        add_dep(
            &mut ws,
            app,
            binding,
            ReqSpec::WorkspaceProtocol(WorkspaceProtocol::Star),
        );
        // An unreferenced co-location pair must NOT link.
        let _lonely_crate = pkg(
            &mut ws,
            "lonely",
            Ecosystem::Cargo,
            "crates/lonely",
            Facets::default(),
        );
        let _lonely_js = pkg(
            &mut ws,
            "lonely-js",
            Ecosystem::Npm,
            "crates/lonely",
            Facets::default(),
        );

        let links = detect_links(&ws, &[]).unwrap();
        assert_eq!(
            links,
            vec![LinkEdge {
                from: crate_id,
                to: binding,
                mode: LinkMode::Cascade,
                rule: LinkRule::WorkspacePath,
            }]
        );
    }

    #[test]
    fn explicit_link_resolves_names_and_default_mode() {
        let mut ws = Workspace::new();
        let crate_id = pkg(
            &mut ws,
            "my-crate",
            Ecosystem::Cargo,
            "crates/a",
            Facets::default(),
        );
        let node = pkg(
            &mut ws,
            "@scope/my-crate-node",
            Ecosystem::Npm,
            "crates/a/node",
            Facets::default(),
        );

        let links = detect_links(
            &ws,
            &[ExplicitLinkSpec {
                from: "my-crate".to_owned(),
                to: "@scope/my-crate-node".to_owned(),
                mode: Some(LinkMode::Lockstep),
            }],
        )
        .unwrap();
        assert_eq!(
            links,
            vec![LinkEdge {
                from: crate_id,
                to: node,
                mode: LinkMode::Lockstep,
                rule: LinkRule::Explicit,
            }]
        );

        let defaulted = detect_links(
            &ws,
            &[ExplicitLinkSpec {
                from: "my-crate".to_owned(),
                to: "@scope/my-crate-node".to_owned(),
                mode: None,
            }],
        )
        .unwrap();
        assert_eq!(
            defaulted.first().expect("one defaulted link").mode,
            LinkMode::Cascade
        );
    }

    #[test]
    fn unknown_explicit_link_is_an_error() {
        let mut ws = Workspace::new();
        let _ = pkg(&mut ws, "a", Ecosystem::Cargo, "a", Facets::default());
        let err = detect_links(
            &ws,
            &[ExplicitLinkSpec {
                from: "a".to_owned(),
                to: "ghost".to_owned(),
                mode: None,
            }],
        )
        .unwrap_err();
        assert_eq!(err.code(), "E-DISC-LINK-UNKNOWN");
    }

    #[test]
    fn precedence_keeps_one_edge_per_pair() {
        // A napi pair that is also referenced via workspace: gets exactly
        // one edge — napi (lockstep) wins over workspace-path (cascade).
        let mut ws = Workspace::new();
        let crate_id = pkg(
            &mut ws,
            "native",
            Ecosystem::Cargo,
            "crates/native",
            Facets {
                cdylib: true,
                ..Facets::default()
            },
        );
        let main = pkg(
            &mut ws,
            "@org/native",
            Ecosystem::Npm,
            "crates/native",
            Facets {
                napi: true,
                ..Facets::default()
            },
        );
        let app = pkg(
            &mut ws,
            "app",
            Ecosystem::Npm,
            "packages/app",
            Facets::default(),
        );
        add_dep(
            &mut ws,
            app,
            main,
            ReqSpec::WorkspaceProtocol(WorkspaceProtocol::Caret),
        );

        let links = detect_links(&ws, &[]).unwrap();
        assert_eq!(links.len(), 1);
        let link = links.first().expect("one napi edge");
        assert_eq!(link.rule, LinkRule::NapiRs);
        assert_eq!(link.mode, LinkMode::Lockstep);
        assert_eq!(link.from, crate_id);
        assert_eq!(link.to, main);
    }

    #[test]
    fn explicit_overrides_heuristics() {
        let mut ws = Workspace::new();
        let _ = pkg(
            &mut ws,
            "native",
            Ecosystem::Cargo,
            "crates/native",
            Facets {
                cdylib: true,
                ..Facets::default()
            },
        );
        let _ = pkg(
            &mut ws,
            "@org/native",
            Ecosystem::Npm,
            "crates/native",
            Facets {
                napi: true,
                ..Facets::default()
            },
        );
        let links = detect_links(
            &ws,
            &[ExplicitLinkSpec {
                from: "native".to_owned(),
                to: "@org/native".to_owned(),
                mode: Some(LinkMode::Cascade),
            }],
        )
        .unwrap();
        assert_eq!(links.len(), 1, "explicit replaces the napi edge");
        let link = links.first().expect("one explicit edge");
        assert_eq!(link.rule, LinkRule::Explicit);
        assert_eq!(link.mode, LinkMode::Cascade);
    }

    #[test]
    fn output_is_sorted_and_deterministic() {
        let mut ws = Workspace::new();
        let c1 = pkg(
            &mut ws,
            "c1",
            Ecosystem::Cargo,
            "crates/c1",
            Facets {
                cdylib: true,
                ..Facets::default()
            },
        );
        let n1 = pkg(
            &mut ws,
            "@o/c1",
            Ecosystem::Npm,
            "crates/c1",
            Facets {
                napi: true,
                ..Facets::default()
            },
        );
        let c2 = pkg(
            &mut ws,
            "c2",
            Ecosystem::Cargo,
            "crates/c2",
            Facets {
                cdylib: true,
                ..Facets::default()
            },
        );
        let n2 = pkg(
            &mut ws,
            "@o/c2",
            Ecosystem::Npm,
            "crates/c2",
            Facets {
                napi: true,
                ..Facets::default()
            },
        );

        let links = detect_links(&ws, &[]).unwrap();
        let keys: Vec<(u32, u32)> = links
            .iter()
            .map(|l| (l.from.to_u32(), l.to.to_u32()))
            .collect();
        assert_eq!(
            keys,
            vec![(c1.to_u32(), n1.to_u32()), (c2.to_u32(), n2.to_u32())]
        );
        let again = detect_links(&ws, &[]).unwrap();
        assert_eq!(links, again);
    }
}

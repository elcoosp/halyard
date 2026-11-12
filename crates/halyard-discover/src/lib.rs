//! Topology detection and the discovery orchestrator (§6.1, M1.5–M1.7).
//!
//! Pipeline:
//!
//! 1. [`discover_repo`] walks the repository (respecting `.gitignore`,
//!    pruning `target/` / `node_modules/` / `.git/`), reads every manifest
//!    (`Cargo.toml`, `package.json`, `pnpm-workspace.yaml`) into a
//!    [`DiscoverCtx`]; no subprocess is ever spawned (invariant I6).
//! 2. [`discover_from_ctx`] (pure) dispatches to the registered
//!    [`EcosystemAdapter`]s in fixed order, merges the [`RawPackage`]
//!    rows into a [`Workspace`] (interning names, resolving
//!    proven-internal deps to [`DepEdge`]s, checking duplicates), builds
//!    the [`WorkspaceGraph`] and runs the §6.5 polyglot link rules.
//!
//! Determinism (invariant I3): adapters see ordered containers, member
//! directories are sorted, links are deduplicated and sorted — identical
//! inputs yield byte-identical workspaces regardless of filesystem order
//! (proptest-covered).

use std::path::Path;

use halyard_core::discover::{
    DiscoverCtx, DiscoverError, DiscoveryWarning, EcosystemAdapter, ManifestFormat, RawDep,
    RawPackage, RawVersionSource,
};
use halyard_core::package::{
    DepEdge, Ecosystem, GroupId, GroupPolicy, Package, PathFilter, RelPath, TagTemplate,
    VersionSource, Workspace,
};
use halyard_core::link::{detect_links, ExplicitLinkSpec};
use halyard_core::graph::WorkspaceGraph;
use halyard_core::PkgId;

pub use halyard_core::graph::GraphError;

/// The discovery output: workspace, graph view and collected warnings.
#[derive(Clone, Debug)]
pub struct Discovery {
    /// The merged workspace (packages, groups, links).
    pub workspace: Workspace,
    /// CSR/topo/SCC view over the workspace (§6.6).
    pub graph: WorkspaceGraph,
    /// Non-fatal observations (§6.3), in deterministic order.
    pub warnings: Vec<DiscoveryWarning>,
}

/// Orchestrator input beyond the committed tree: explicit `[[link]]`
/// entries from the config layer (config itself is resolved by the M1.8
/// loader and mapped by the caller).
#[derive(Clone, Debug, Default)]
pub struct DiscoverInput {
    /// `halyard.toml: [[link]]` entries, ecosystem-agnostic form.
    pub links: Vec<ExplicitLinkSpec>,
}

/// One registered adapter with its fixed ecosystem mapping (§6.1 merge
/// order: cargo first).
struct AdapterEntry {
    ecosystem: Ecosystem,
    adapter: Box<dyn EcosystemAdapter>,
}

/// Adapters in fixed dispatch order.
fn adapters() -> Vec<AdapterEntry> {
    vec![
        AdapterEntry {
            ecosystem: Ecosystem::Cargo,
            adapter: Box::new(halyard_eco_cargo::CargoAdapter::new()),
        },
        AdapterEntry {
            ecosystem: Ecosystem::Npm,
            adapter: Box::new(halyard_eco_npm::NpmAdapter::new()),
        },
    ]
}

/// The pure orchestrator (§6.1 step 3): merge adapters' rows into a
/// workspace, build the graph, detect polyglot links.
///
/// # Errors
///
/// [`DiscoverError`] for duplicate package names (`E-DISC-DUPLICATE`),
/// unknown `[[link]]` names (`E-DISC-LINK-UNKNOWN`), and everything the
/// adapters surface (`E-DISC-*`).
pub fn discover_from_ctx(
    ctx: &DiscoverCtx,
    input: &DiscoverInput,
) -> Result<Discovery, DiscoverError> {
    let mut workspace = Workspace::new();
    let mut warnings: Vec<DiscoveryWarning> = Vec::new();
    let mut raw_rows: Vec<(Ecosystem, Vec<RawPackage>)> = Vec::new();

    for entry in adapters() {
        let rows = entry.adapter.discover(ctx)?;
        raw_rows.push((entry.ecosystem, rows));
    }

    // Register packages in adapter order, checking duplicates per
    // ecosystem (§6.3 — cargo forbids duplicates, npm names are scoped).
    let mut seen: std::collections::BTreeMap<(String, String), String> =
        std::collections::BTreeMap::new();
    for (ecosystem, rows) in &raw_rows {
        for row in rows {
            let label = ecosystem_label(workspace.interner(), *ecosystem);
            let key = (label.to_owned(), row.name.clone());
            if let Some(first) = seen.get(&key) {
                return Err(DiscoverError::Duplicate {
                    name: row.name.clone(),
                    first: first.clone(),
                    second: row.manifest.clone(),
                });
            }
            seen.insert(key, row.manifest.clone());
            register_package(&mut workspace, *ecosystem, row)?;
            for warning in &row.warnings {
                warnings.push(warning.clone());
            }
        }
    }

    // One default independent group per workspace (§5.4); finer grouping
    // (fixed/linked) is planner/config policy from M1.9 on.
    let group_name = workspace.intern("default");
    let members: Vec<PkgId> = workspace.packages().iter().map(|p| p.id).collect();
    workspace.add_group(
        group_name,
        GroupPolicy::Independent,
        members,
        TagTemplate::parse("{name}-v{version}").expect("default tag template is always valid"),
    );

    // Resolve proven-internal dependency rows to edges (§5.3: out edges
    // to workspace members only).
    for (ecosystem, rows) in &raw_rows {
        for row in rows {
            let Some(pkg) = workspace.find_by_name(&row.name) else {
                continue;
            };
            let mut edges: Vec<DepEdge> = Vec::new();
            for dep in &row.deps {
                if let Some(edge) = resolve_dep(&workspace, *ecosystem, dep) {
                    edges.push(edge);
                }
            }
            if let Some(pkg_mut) = workspace.package_mut(pkg) {
                pkg_mut.deps = edges;
            }
        }
    }

    // Graph view (CSR + topo + SCC, §6.6). Cycle *detection* stays
    // available through `graph.topo_order()` (E-GRAPH-CYCLE with the
    // printed path); discovery itself stays total so `halyard graph`
    // (M1.10) can print the cycle.
    let graph = WorkspaceGraph::build(&workspace);

    // Polyglot links (§6.5) — recorded on the workspace.
    let links = detect_links(&workspace, &input.links)?;
    for link in links {
        workspace.push_link(link);
    }

    Ok(Discovery {
        workspace,
        graph,
        warnings,
    })
}

/// Stable ecosystem label for duplicate detection ("cargo"/"npm"/custom).
fn ecosystem_label(interner: &halyard_core::Interner, ecosystem: Ecosystem) -> &str {
    ecosystem.resolve(interner)
}

/// Converts one [`RawPackage`] into a registered [`Package`].
fn register_package(
    workspace: &mut Workspace,
    ecosystem: Ecosystem,
    row: &RawPackage,
) -> Result<PkgId, DiscoverError> {
    let manifest_error = |file: &str, message: String| DiscoverError::Manifest {
        file: file.to_owned(),
        format: ManifestFormat::Toml,
        message,
        span: None,
    };
    let root = RelPath::new(&row.root).ok_or_else(|| {
        manifest_error(&row.manifest, "package root is not a normalized path".to_owned())
    })?;
    let manifest = RelPath::new(&row.manifest)
        .ok_or_else(|| manifest_error(&row.manifest, "manifest path is not normalized".to_owned()))?;
    let version_source = match &row.version_source {
        RawVersionSource::Local => VersionSource::Local,
        RawVersionSource::WorkspaceInherited { file, key_path } => {
            VersionSource::WorkspaceInherited {
                file: RelPath::new(file).ok_or_else(|| {
                    manifest_error(
                        file,
                        "inherited-version manifest path is not normalized".to_owned(),
                    )
                })?,
                key_path: halyard_core::BString::from(key_path.as_str()),
            }
        }
        RawVersionSource::Dynamic => VersionSource::Dynamic,
    };
    let name_sym = workspace.intern(&row.name);
    let pkg = Package {
        id: PkgId::FIRST,
        name: name_sym,
        ecosystem,
        root,
        manifest,
        version: row.version.clone(),
        version_source,
        publishable: row.publishable,
        deps: Vec::new(),
        tag_template: row
            .tag_template
            .clone()
            .unwrap_or_else(default_tag_template),
        changelog: row.changelog.as_ref().and_then(|c| RelPath::new(c)),
        include: PathFilter::new(Vec::new(), Vec::new()),
        group: GroupId::new(0),
        facets: row.facets,
    };
    Ok(workspace.push_package(pkg))
}

/// `{name}-v{version}` — the independent-package default (appendix A.3).
fn default_tag_template() -> TagTemplate {
    TagTemplate::parse("{name}-v{version}").expect("default tag template is always valid")
}

/// Resolves one raw dependency: proven-internal rows match a same-
/// ecosystem member by name; everything else is registry-resolved by the
/// package manager and stays external (this is what makes the cargo
/// differential exact).
fn resolve_dep(workspace: &Workspace, ecosystem: Ecosystem, dep: &RawDep) -> Option<DepEdge> {
    if !dep.internal {
        return None;
    }
    let target = workspace
        .packages()
        .iter()
        .find(|p| {
            p.ecosystem == ecosystem && workspace.interner().get(p.name) == Some(dep.name.as_str())
        })
        .map(|p| p.id)?;
    Some(DepEdge {
        to: target,
        kind: dep.kind,
        req: dep.req.clone(),
        site: dep.site.clone(),
    })
}

/// Repo-root-relative discovery entry point (§6.1 steps 1–2): walk +
/// read + [`discover_from_ctx`].
///
/// The repository root is `root` itself; when it carries no `.git`, the
/// walk still works (git-less checkouts are valid for discovery) — the
/// `gix::discover` refinement lands with the git engine (M2).
pub fn discover_repo(root: &Path, input: &DiscoverInput) -> Result<Discovery, DiscoverError> {
    let ctx = collect_ctx(root)?;
    discover_from_ctx(&ctx, input)
}

/// Walks `root` and reads every manifest-shaped file into a
/// [`DiscoverCtx`].
pub fn collect_ctx(root: &Path) -> Result<DiscoverCtx, DiscoverError> {
    let mut ctx = DiscoverCtx::new();
    let walker = ignore::WalkBuilder::new(root)
        .hidden(true)
        .filter_entry(|entry| {
            // Belt and braces: prune build dirs even without .gitignore.
            if entry.depth() == 0 {
                return true;
            }
            !matches!(
                entry.file_name().to_str(),
                Some(".git") | Some("target") | Some("node_modules")
            )
        })
        .build();
    for entry in walker {
        let entry = entry.map_err(|e| DiscoverError::Io {
            path: root.display().to_string(),
            message: e.to_string(),
        })?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(rel) = path.strip_prefix(root).ok().and_then(|r| r.to_str()) else {
            continue;
        };
        let rel = rel.replace('\\', "/");
        let is_manifest = rel == "Cargo.toml"
            || rel == "package.json"
            || rel == "pnpm-workspace.yaml"
            || rel.ends_with("/Cargo.toml")
            || rel.ends_with("/package.json");
        if !is_manifest {
            continue;
        }
        let bytes = std::fs::read(path).map_err(|e| DiscoverError::Io {
            path: rel.clone(),
            message: e.to_string(),
        })?;
        ctx.insert_file(&rel, bytes);
    }
    Ok(ctx)
}

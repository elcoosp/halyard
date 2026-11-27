//! Topology detection and the discovery orchestrator (§6.1, M1.5–M1.7).
//!
//! Pipeline:
//!
//! 1. [`discover_repo`] walks the repository (respecting `.gitignore`,
//!    pruning `target/` / `node_modules/` / `.git/`), reads every manifest
//!    (`Cargo.toml`, `package.json`, `pnpm-workspace.yaml`,
//!    `pyproject.toml`, `go.mod`/`go.work`, `pom.xml`, SDK-style
//!    `.csproj`, `pubspec.yaml`) plus the config-declared `[[generic]]`
//!    version files (M8 ADR-0062) into a [`DiscoverCtx`]; no subprocess is
//!    ever spawned
//!    (invariant I6).
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

use halyard_core::PkgId;
use halyard_core::discover::{
    DiscoverCtx, DiscoverError, DiscoveryWarning, EcosystemAdapter, GenericFileSpec,
    ManifestFormat, RawDep, RawPackage, RawVersionSource,
};
use halyard_core::graph::WorkspaceGraph;
use halyard_core::link::{ExplicitLinkSpec, detect_links};
use halyard_core::package::{
    DepEdge, Ecosystem, GroupId, GroupPolicy, Package, PathFilter, RelPath, TagTemplate,
    VersionSource, Workspace,
};

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
/// entries and the config-declared `generic` packages (M8 ADR-0062)
/// from the config layer (config itself is resolved by the M1.8 loader
/// and mapped by the caller).
#[derive(Clone, Debug, Default)]
pub struct DiscoverInput {
    /// `halyard.toml: [[link]]` entries, ecosystem-agnostic form.
    pub links: Vec<ExplicitLinkSpec>,
    /// `halyard.toml: [[generic]]` entries, validated by the loader
    /// (`E-CFG-GENERIC`): one entry is one config-declared package over
    /// a regex-edited version file.
    pub generic: Vec<GenericFileSpec>,
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
        // M8 step 1 (ADR-0056): the pypi adapter joins the fixed
        // dispatch order after npm (§6.1 merge order: cargo first).
        AdapterEntry {
            ecosystem: Ecosystem::PyPi,
            adapter: Box::new(halyard_eco_pypi::PyPiAdapter::new()),
        },
        // M8 (ADR-0057): the go adapter joins the fixed dispatch order
        // after pypi (§6.1 merge order: cargo first).
        AdapterEntry {
            ecosystem: Ecosystem::Go,
            adapter: Box::new(halyard_eco_gomod::GoModAdapter::new()),
        },
        // M8 (ADR-0058): the maven adapter joins the fixed dispatch
        // order after go (§6.1 merge order: cargo first).
        AdapterEntry {
            ecosystem: Ecosystem::Maven,
            adapter: Box::new(halyard_eco_maven::MavenAdapter::new()),
        },
        // M8 (ADR-0059): the nuget adapter joins the fixed dispatch
        // order after maven (§6.1 merge order: cargo first).
        AdapterEntry {
            ecosystem: Ecosystem::Nuget,
            adapter: Box::new(halyard_eco_nuget::NuGetAdapter::new()),
        },
        // M8 (ADR-0060): the dart adapter joins the fixed dispatch
        // order after nuget (§6.1 merge order: cargo first).
        AdapterEntry {
            ecosystem: Ecosystem::Dart,
            adapter: Box::new(halyard_eco_dart::DartAdapter::new()),
        },
        // M8 (ADR-0061): the rubygems adapter joins the fixed dispatch
        // order after dart (§6.1 merge order: cargo first).
        AdapterEntry {
            ecosystem: Ecosystem::RubyGems,
            adapter: Box::new(halyard_eco_rubygems::RubyGemsAdapter::new()),
        },
    ]
}

/// The registered adapters in fixed dispatch order (§6.1), as trait
/// objects. The effectful shells (M3.6 `apply` in `halyard-edit` /
/// `halyard-cli`) route manifest edits through them without depending on
/// the eco crates directly (§7.2 layering: the adapter trait lives in
/// core, the implementations here).
pub fn registered_adapters() -> Vec<&'static dyn EcosystemAdapter> {
    static CARGO: halyard_eco_cargo::CargoAdapter = halyard_eco_cargo::CargoAdapter;
    static NPM: halyard_eco_npm::NpmAdapter = halyard_eco_npm::NpmAdapter;
    static PYPI: halyard_eco_pypi::PyPiAdapter = halyard_eco_pypi::PyPiAdapter;
    static GO: halyard_eco_gomod::GoModAdapter = halyard_eco_gomod::GoModAdapter;
    static MAVEN: halyard_eco_maven::MavenAdapter = halyard_eco_maven::MavenAdapter;
    static NUGET: halyard_eco_nuget::NuGetAdapter = halyard_eco_nuget::NuGetAdapter;
    static DART: halyard_eco_dart::DartAdapter = halyard_eco_dart::DartAdapter;
    static RUBYGEMS: halyard_eco_rubygems::RubyGemsAdapter = halyard_eco_rubygems::RubyGemsAdapter;
    vec![&CARGO, &NPM, &PYPI, &GO, &MAVEN, &NUGET, &DART, &RUBYGEMS]
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

    // M8 (ADR-0062): the config-declared `generic` packages ride the
    // same orchestrator — the adapter is constructed per-run from the
    // `DiscoverInput` specs (discovery is config-driven, not
    // tree-scanned) and joins after the fixed adapters (§6.1 merge
    // order).
    if !input.generic.is_empty() {
        let generic = halyard_eco_generic::GenericAdapter::new(&input.generic);
        raw_rows.push((Ecosystem::Generic, generic.discover(ctx)?));
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
        TagTemplate::independent_default(),
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
        manifest_error(
            &row.manifest,
            "package root is not a normalized path".to_owned(),
        )
    })?;
    let manifest = RelPath::new(&row.manifest).ok_or_else(|| {
        manifest_error(&row.manifest, "manifest path is not normalized".to_owned())
    })?;
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
        RawVersionSource::Tag => VersionSource::Tag,
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
            .unwrap_or_else(TagTemplate::independent_default),
        changelog: row.changelog.as_ref().and_then(|c| RelPath::new(c)),
        version_site: row.version_site.clone(),
        include: PathFilter::new(Vec::new(), Vec::new()),
        group: GroupId::new(0),
        facets: row.facets,
    };
    Ok(workspace.push_package(pkg))
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
        raw: dep.raw.clone(),
    })
}

/// Repo-root-relative discovery entry point (§6.1 steps 1–2): walk +
/// read + [`discover_from_ctx`].
///
/// The repository root is `root` itself; when it carries no `.git`, the
/// walk still works (git-less checkouts are valid for discovery) — the
/// `gix::discover` refinement lands with the git engine (M2). The
/// config-declared `[[generic]]` version files (M8 ADR-0062) join the
/// manifest walk: the walk cannot know them by shape, so the input's
/// specs name the files to read.
pub fn discover_repo(root: &Path, input: &DiscoverInput) -> Result<Discovery, DiscoverError> {
    let ctx = collect_ctx(root, &input.generic)?;
    discover_from_ctx(&ctx, input)
}

/// Walks `root` and reads every manifest-shaped file — plus the
/// config-declared `[[generic]]` version files — into a
/// [`DiscoverCtx`].
pub fn collect_ctx(
    root: &Path,
    generic_specs: &[GenericFileSpec],
) -> Result<DiscoverCtx, DiscoverError> {
    let mut ctx = DiscoverCtx::new();
    // The declared version files join the inventory first (the adapter
    // refuses a declared-but-missing file; a non-normalized path would
    // be ignored by `insert_file`, so the loader's validation is the
    // authority — belt and braces, both refuse).
    for spec in generic_specs {
        let path = root.join(&spec.file);
        let bytes = std::fs::read(&path).map_err(|e| DiscoverError::Io {
            path: spec.file.clone(),
            message: e.to_string(),
        })?;
        ctx.insert_file(&spec.file, bytes);
    }
    let walker = ignore::WalkBuilder::new(root)
        .hidden(true)
        .filter_entry(|entry| {
            // Belt and braces: prune build dirs even without .gitignore.
            if entry.depth() == 0 {
                return true;
            }
            !matches!(
                entry.file_name().to_str(),
                Some(".git" | "target" | "node_modules")
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
            || rel == "pyproject.toml"
            || rel == "go.mod"
            || rel == "go.work"
            || rel == "pom.xml"
            || rel == "pubspec.yaml"
            || rel.ends_with("/Cargo.toml")
            || rel.ends_with("/package.json")
            || rel.ends_with("/pyproject.toml")
            || rel.ends_with("/go.mod")
            || rel.ends_with("/pom.xml")
            || rel.ends_with("/pubspec.yaml")
            // M8 (ADR-0059): the nuget adapter owns the SDK-style
            // `.csproj` grammar — every project file joins the walk.
            || rel.ends_with(".csproj")
            // M8 (ADR-0061): the rubygems adapter owns the `.gemspec`
            // grammar — every gemspec file joins the walk.
            || rel.ends_with(".gemspec");
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

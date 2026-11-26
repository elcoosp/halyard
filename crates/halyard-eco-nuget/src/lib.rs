//! [`EcosystemAdapter`] implementation for `NuGet` (M8, the §17 `NuGet`
//! cell — the next listed cell after the shipped `Maven/Gradle` half
//! (ADR-0058), ADR-0059).
//!
//! Parses SDK-style `.csproj` with the hand-rolled scanner of
//! [`grammar`] at the VERIFIED reference grammar
//! (`docs/research/nuget-grammars.md`, from `learn.microsoft.com`: the
//! pack-target property table, the package-versioning reference, the
//! project-SDK overview, the customize-by-directory page). Versions are
//! manifest rows under the pack target's documented resolution —
//! *"`PackageVersion` ... Default is the value of `$(Version)`"* — but
//! under `NuGet`'s own version grammar, not semver (`NuGetVersion`
//! *"only requires the major segment to be defined"*: `1`, `1.0`,
//! `1.0.0.0` are all accepted): the adapter applies the PEP 440
//! discipline of ADR-0056 — only semver-parseable spellings are
//! canonical (planned with an editable site); every `NuGet`-specific
//! spelling (`1.0.0.0`, a two-component `1.0`, a
//! `VersionPrefix`/`VersionSuffix` composite) is
//! recognized-but-non-canonical — **warned, unpublishable, no edit
//! site, never rewritten**.
//!
//! The package identity follows the documented chain — `PackageId`
//! defaults to *"the `AssemblyName` or directory name as the name of
//! the package"*, `AssemblyName` to the project file's stem — evaluated
//! per project file. Every `.csproj` declaring an SDK (the `Sdk`
//! attribute or the top-level `<Sdk>` element) is a package; a classic
//! (non-SDK) project file is consumed unmodeled (its metadata may live
//! outside the file). Conditions are build evaluation semantics
//! (documented on `PropertyGroup`/`ItemGroup`/properties): a conditioned
//! value never plans a row or an edge.
//!
//! Internal edges are the path-proven ones: a `<ProjectReference
//! Include="..\Lib\Lib.csproj"/>` whose normalized path names a
//! discovered member's manifest (*"Project to project references are
//! considered by default as `NuGet` package references"*). A
//! `<PackageReference>` resolves through registry sources — never
//! proven-internal (the Maven reactor rule has no `NuGet` counterpart).
//! Shared `Directory.Build.props`/`Directory.Build.targets` imports are
//! consumed unmodeled with a per-project warning.
//!
//! `NuGet` publishing carries no argv grammar yet (no verified `dotnet`
//! arm) — rows carry `publishable = false` (the engine's disabled
//! shape; ADR-0059).

use halyard_core::EcosystemAdapter;
use halyard_core::discover::{
    DiscoverCtx, DiscoverError, DiscoveryWarning, Facets, ManifestFormat, RawDep, RawPackage,
    RawVersionSource,
};
use halyard_core::package::{DepKind, Ecosystem, ManifestSite, RelPath, ReqSpec};
use semver::Version;

pub mod edit;
pub mod grammar;

/// The release version the `.csproj` **bytes** carry — the
/// semver-canonical pairing text when the project's pack version
/// (`PackageVersion`, else `Version`) is a canonical spelling, `None`
/// otherwise (non-semver `NuGet` spellings never pair; the documented
/// HEAD fallback governs). The probe input the tag-target pinpoint walk
/// consumes for the nuget ecosystem (see
/// `halyard_git::tag_target::ManifestVersionProbe`; the git engine
/// never parses manifests itself).
#[must_use]
pub fn manifest_version(bytes: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(bytes).ok()?;
    let csproj = grammar::parse_csproj(text, "project.csproj").ok()?;
    let field = csproj
        .package_version
        .as_ref()
        .or(csproj.version.as_ref())?;
    Version::parse(&field.text).ok().map(|_| field.text.clone())
}

/// The nuget-family adapter (§7.3 discovery + manifest-edit subset).
#[derive(Clone, Copy, Debug, Default)]
pub struct NuGetAdapter;

impl NuGetAdapter {
    /// New adapter.
    pub fn new() -> Self {
        Self
    }
}

/// One parsed candidate manifest (plain data before row assembly).
struct Parsed {
    /// The parsed csproj.
    csproj: grammar::Csproj,
    /// The manifest's repo-relative path.
    manifest: String,
    /// The project's repo-relative directory ("" = the repository root).
    dir: String,
    /// The manifest file's stem (the identity chain's last step).
    stem: String,
}

impl EcosystemAdapter for NuGetAdapter {
    fn id(&self) -> &'static str {
        "nuget"
    }

    fn discover(&self, ctx: &DiscoverCtx) -> Result<Vec<RawPackage>, DiscoverError> {
        // The candidate manifests: every `.csproj` the committed tree
        // carries, in the context's sorted order (§6.1 merge
        // determinism).
        let paths: Vec<String> = ctx
            .tree()
            .filter(|path| path.ends_with(".csproj"))
            .map(ToOwned::to_owned)
            .collect();
        if paths.is_empty() {
            return Ok(Vec::new());
        }
        // The shared property files: consumed unmodeled, warned per
        // project (§7 of the research note — the imported values are
        // evaluation semantics).
        let shared_props = ctx.tree().any(|path| {
            path == "Directory.Build.props"
                || path == "Directory.Build.targets"
                || path.ends_with("/Directory.Build.props")
                || path.ends_with("/Directory.Build.targets")
        });

        let mut parsed: Vec<Parsed> = Vec::new();
        for path in &paths {
            let bytes = ctx.file(path).ok_or_else(|| DiscoverError::Io {
                path: path.clone(),
                message: "the manifest left the committed tree mid-discovery".to_owned(),
            })?;
            let text = utf8(bytes, path)?;
            let csproj = grammar::parse_csproj(text, path)?;
            let dir = dir_of(path);
            let stem = stem_of(path);
            parsed.push(Parsed {
                csproj,
                manifest: path.clone(),
                dir,
                stem,
            });
        }

        // The identity map: the planning members' manifest paths →
        // identity names (the ProjectReference resolution target; a
        // conditioned project plans no row and carries no identity).
        let identities: std::collections::BTreeMap<String, String> = parsed
            .iter()
            .filter(|project| project.csproj.sdk.is_some() && !project.csproj.conditioned_root)
            .map(|project| (project.manifest.clone(), identity_of(project).to_owned()))
            .collect();

        let mut packages = Vec::new();
        for project in &parsed {
            if let Some(row) = row(project, &identities, ctx, shared_props)? {
                packages.push(row);
            }
        }
        Ok(packages)
    }

    fn edit_versions(
        &self,
        file: &RelPath,
        content: &[u8],
        patches: &[halyard_core::plan::PlanPatch],
    ) -> Result<halyard_core::edit::EditOutcome, halyard_core::edit::EditFailure> {
        edit::edit_versions(file, content, patches)
    }
}

/// The identity chain (§2 of the research note): `<PackageId>` →
/// `<AssemblyName>` → the manifest file's stem.
fn identity_of(project: &Parsed) -> &str {
    if let Some(field) = project.csproj.package_id.as_ref() {
        return field.text.as_str();
    }
    if let Some(field) = project.csproj.assembly_name.as_ref() {
        return field.text.as_str();
    }
    project.stem.as_str()
}

/// Builds one raw package row from a parsed project (§6.1 merge row);
/// `None` for the declines (a classic project file, a conditioned
/// root).
fn row(
    project: &Parsed,
    identities: &std::collections::BTreeMap<String, String>,
    ctx: &DiscoverCtx,
    shared_props: bool,
) -> Result<Option<RawPackage>, DiscoverError> {
    let csproj = &project.csproj;
    if csproj.conditioned_root {
        // The whole file may not apply (a conditioned `<Project>` is
        // legal MSBuild): consumed unmodeled, no row.
        return Ok(None);
    }
    if csproj.sdk.is_none() {
        // A classic (non-SDK) project: its metadata may live outside
        // the file — the orphan-manifest posture (the Maven skip).
        return Ok(None);
    }
    let owner = identity_of(project).to_owned();

    let mut warnings: Vec<DiscoveryWarning> = Vec::new();
    for scan in &csproj.warnings {
        warnings.push(DiscoveryWarning {
            code: scan.code,
            subject: owner.clone(),
            message: scan.message.clone(),
        });
    }
    if shared_props {
        warnings.push(DiscoveryWarning {
            code: "ignored-section",
            subject: owner.clone(),
            message: "a Directory.Build.props/Directory.Build.targets import is in the \
                      tree — imported property values are evaluation semantics a plan \
                      does not evaluate (the project file's own rows still plan)"
                .to_owned(),
        });
    }
    if csproj.unpackable {
        warnings.push(DiscoveryWarning {
            code: "not-packable",
            subject: owner.clone(),
            message: "<IsPackable>false</IsPackable> — the project cannot pack; the row \
                      versions and tags and never publishes"
                .to_owned(),
        });
    }

    // The pack version (§3 of the research note): `PackageVersion` →
    // `Version` → the `VersionPrefix`/`VersionSuffix` composite → none.
    let manifest = rel_path(&project.manifest)?;
    let (version_value, version_site) = resolve_version(csproj, &manifest, &owner, &mut warnings);
    let deps = reference_deps(project, identities, ctx, &manifest, &owner, &mut warnings)?;

    Ok(Some(RawPackage {
        name: owner,
        ecosystem: Ecosystem::Nuget,
        root: project.dir.clone(),
        manifest: project.manifest.clone(),
        version: version_value,
        version_source: RawVersionSource::Local,
        // ADR-0059: no `dotnet` publish grammar exists (no argv arm, no
        // registry probe, no auth arm) — the row versions and tags, and
        // the publish engine records the disabled shape.
        publishable: false,
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
    }))
}

/// The pack version's value and edit site: canonical semver pairs with
/// the element's span (the site the plan's patch lands on); every
/// `NuGet`-legal spelling the plan contract cannot render is warned,
/// unpublishable, never rewritten.
fn resolve_version(
    csproj: &grammar::Csproj,
    manifest: &RelPath,
    owner: &str,
    warnings: &mut Vec<DiscoveryWarning>,
) -> (Option<Version>, Option<ManifestSite>) {
    let slot = csproj
        .package_version
        .as_ref()
        .or(csproj.version.as_ref())
        .cloned();
    if let Some(field) = slot {
        if let Ok(value) = Version::parse(&field.text) {
            let site = ManifestSite {
                file: manifest.clone(),
                spans: vec![field.span],
            };
            return (Some(value), Some(site));
        }
        warnings.push(DiscoveryWarning {
            code: "non-canonical-version",
            subject: owner.to_owned(),
            message: format!(
                "version {:?} is a NuGet spelling the plan contract cannot render \
                 (NuGetVersion accepts \"1.0\", \"1.0.0.0\" and build metadata as \
                 equal); package treated as unpublishable and never rewritten",
                field.text
            ),
        });
        return (None, None);
    }
    if csproj.version_prefix.is_some() || csproj.version_suffix.is_some() {
        warnings.push(DiscoveryWarning {
            code: "non-canonical-version",
            subject: owner.to_owned(),
            message: "the version resolves through the VersionPrefix/VersionSuffix \
                      composite — evaluation semantics a plan does not compute; package \
                      treated as unpublishable and never rewritten"
                .to_owned(),
        });
        return (None, None);
    }
    warnings.push(DiscoveryWarning {
        code: "no-version",
        subject: owner.to_owned(),
        message: "no <PackageVersion> or <Version> in the project and no imported value \
                  is evaluated; package treated as unpublished"
            .to_owned(),
    });
    (None, None)
}

/// The reference rows: `PackageReference` (registry-resolved, always
/// external) and `ProjectReference` (the path-proven internal edge).
fn reference_deps(
    project: &Parsed,
    identities: &std::collections::BTreeMap<String, String>,
    ctx: &DiscoverCtx,
    manifest: &RelPath,
    owner: &str,
    warnings: &mut Vec<DiscoveryWarning>,
) -> Result<Vec<RawDep>, DiscoverError> {
    let mut deps = package_reference_deps(project, manifest, owner, warnings);
    project_reference_deps(
        project, identities, ctx, manifest, owner, &mut deps, warnings,
    )?;
    Ok(deps)
}

/// The `<PackageReference>` rows: registry-resolved, always external —
/// the range text rides verbatim (`ReqSpec::Other`, the Maven posture).
fn package_reference_deps(
    project: &Parsed,
    manifest: &RelPath,
    owner: &str,
    warnings: &mut Vec<DiscoveryWarning>,
) -> Vec<RawDep> {
    let mut deps = Vec::new();
    for row in &project.csproj.package_refs {
        let Some(include) = row.include.as_ref() else {
            warnings.push(DiscoveryWarning {
                code: "external-dep",
                subject: owner.to_owned(),
                message: "a <PackageReference> row without an Include attribute is \
                          corrupt and is consumed unmodeled"
                    .to_owned(),
            });
            continue;
        };
        let raw = row
            .version
            .as_ref()
            .map_or(String::new(), |version| version.text.clone());
        let spans = row
            .version
            .as_ref()
            .map_or_else(Vec::new, |version| vec![version.span]);
        deps.push(RawDep {
            name: include.text.clone(),
            kind: DepKind::Normal,
            req: ReqSpec::Other {
                raw: halyard_core::BString::from(raw.as_str()),
            },
            site: ManifestSite {
                file: manifest.clone(),
                spans,
            },
            raw: halyard_core::BString::from(raw.as_str()),
            internal: false,
        });
    }
    deps
}

/// The `<ProjectReference>` rows: the path-proven internal edge — the
/// normalized `Include` must name a discovered member's manifest; an
/// unevaluable/foreign shape warns, a missing file refuses.
fn project_reference_deps(
    project: &Parsed,
    identities: &std::collections::BTreeMap<String, String>,
    ctx: &DiscoverCtx,
    manifest: &RelPath,
    owner: &str,
    deps: &mut Vec<RawDep>,
    warnings: &mut Vec<DiscoveryWarning>,
) -> Result<(), DiscoverError> {
    for row in &project.csproj.project_refs {
        let Some(include) = row.include.as_ref() else {
            warnings.push(DiscoveryWarning {
                code: "external-dep",
                subject: owner.to_owned(),
                message: "a <ProjectReference> row without an Include attribute is \
                          corrupt and is consumed unmodeled"
                    .to_owned(),
            });
            continue;
        };
        if include.text.contains("$(") {
            warnings.push(DiscoveryWarning {
                code: "ignored-section",
                subject: owner.to_owned(),
                message: format!(
                    "a <ProjectReference> through an MSBuild expression ({:?}) is \
                     evaluation semantics — consumed unmodeled",
                    include.text
                ),
            });
            continue;
        }
        let Some(resolved) = normalize_reference(&project.dir, &include.text) else {
            return Err(DiscoverError::Glob {
                pattern: include.text.clone(),
                message: "a project reference outside the committed tree cannot be \
                          planned"
                    .to_owned(),
            });
        };
        if !resolved.to_ascii_lowercase().ends_with(".csproj") {
            // The Include grammar names project files; a non-csproj
            // project file (a `.vbproj`/`.fsproj`) is an unmodeled
            // ecosystem shape (recorded-open) — warned, not refused.
            warnings.push(DiscoveryWarning {
                code: "ignored-section",
                subject: owner.to_owned(),
                message: format!(
                    "the project reference {resolved:?} names a project file type the \
                     cell does not plan yet — consumed unmodeled"
                ),
            });
            continue;
        }
        if let Some(name) = identities.get(&resolved) {
            deps.push(RawDep {
                name: name.clone(),
                kind: DepKind::Normal,
                req: ReqSpec::Other {
                    raw: halyard_core::BString::from(""),
                },
                site: ManifestSite {
                    file: manifest.clone(),
                    spans: vec![include.span],
                },
                raw: halyard_core::BString::from(include.text.as_str()),
                internal: true,
            });
        } else if ctx.file(&resolved).is_some() {
            // The referenced file exists but plans no row (a classic
            // project): the edge cannot be proven — consumed unmodeled.
            warnings.push(DiscoveryWarning {
                code: "ignored-section",
                subject: owner.to_owned(),
                message: format!(
                    "the project reference {resolved:?} names a file that plans no nuget \
                     row (a non-SDK project) — consumed unmodeled"
                ),
            });
        } else {
            return Err(DiscoverError::Glob {
                pattern: resolved,
                message: "a project reference to a file outside the committed tree \
                          cannot be planned"
                    .to_owned(),
            });
        }
    }
    Ok(())
}

/// Resolves one `Include` path against the referencing project's
/// directory (backslashes normalize to forward slashes; `..` climbs; a
/// rooted or escaping path refuses).
fn normalize_reference(dir: &str, include: &str) -> Option<String> {
    if include.is_empty() {
        return None;
    }
    let text = include.replace('\\', "/");
    if text.starts_with('/') {
        return None;
    }
    let mut parts: Vec<&str> = dir.split('/').filter(|part| !part.is_empty()).collect();
    for component in text.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            _ => parts.push(component),
        }
    }
    RelPath::new(&parts.join("/")).map(|rel| rel.as_str().to_owned())
}

fn rel_path(path: &str) -> Result<RelPath, DiscoverError> {
    RelPath::new(path).ok_or_else(|| DiscoverError::Manifest {
        file: path.to_owned(),
        format: ManifestFormat::Csproj,
        message: "manifest path is not a normalized repo-relative path".to_owned(),
        span: None,
    })
}

/// The repo-relative directory of a csproj path (`App.csproj` → the
/// root, `src/app/App.csproj` → `src/app`).
fn dir_of(manifest_path: &str) -> String {
    match manifest_path.rsplit_once('/') {
        Some((dir, _)) => dir.to_owned(),
        None => String::new(),
    }
}

/// The manifest file's stem (the identity chain's last step —
/// `$(AssemblyName)` defaults to the project file name without
/// extension).
fn stem_of(manifest_path: &str) -> String {
    let file = manifest_path
        .rsplit_once('/')
        .map_or(manifest_path, |(_, f)| f);
    file.strip_suffix(".csproj").unwrap_or(file).to_owned()
}

fn utf8<'a>(bytes: &'a [u8], file: &str) -> Result<&'a str, DiscoverError> {
    std::str::from_utf8(bytes).map_err(|_| DiscoverError::Manifest {
        file: file.to_owned(),
        format: ManifestFormat::Csproj,
        message: "manifest is not valid UTF-8".to_owned(),
        span: None,
    })
}

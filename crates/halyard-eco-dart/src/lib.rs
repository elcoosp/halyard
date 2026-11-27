//! [`EcosystemAdapter`] implementation for Dart/pub (M8, the §17
//! `Dart/pub` cell — the next listed cell after `NuGet` (ADR-0059),
//! ADR-0060).
//!
//! Parses `pubspec.yaml` with the hand-rolled scanner of [`grammar`] at
//! the VERIFIED reference grammar (`docs/research/dart-pub-grammars.md`,
//! from `dart.dev/tools/pub`). The version rides the pub grammar's own
//! semver shape (*"A version number is three numbers separated by dots.
//! It can also optionally have a build … or prerelease … suffix. When you
//! select a version, follow semantic versioning."*) — every spelling the
//! plan contract's semver parses is canonical (editable site); every
//! other spelling (`1.2`) is warned, unpublishable, no edit site, never
//! rewritten (the ADR-0056 discipline).
//!
//! Workspaces ride the pub-workspace grammar: *"Add a pubspec.yaml at the
//! repository root directory with a `workspace` entry enumerating the
//! paths to the packages of the repository (the workspace packages)"*,
//! each member carrying `resolution: workspace` (*"add a `resolution`
//! entry"*), nested workspaces walked recursively (*"Pub discovers auth
//! and api through the workspace entry of the server package"*), and the
//! documented glob form (`packages/*`) expanded over the committed tree.
//! With a workspace root present, an unclaimed pubspec is the documented
//! stray-file refusal (*"pub get will report an error and fail to
//! resolve"*); without one, standalone pubspecs are each their own
//! package (the orphan-manifest posture).
//!
//! Internal edges carry two proofs: a dependency **naming** a workspace
//! member (*"If any of the workspace packages depend on each other, they
//! will automatically resolve to the one in the workspace, regardless of
//! the source"*) and a `path:` dependency whose resolved path names a
//! member manifest (*"considered relative to the directory containing
//! your pubspec"*).
//!
//! pub.dev publishing documents no token env var (only `PUB_CACHE` and
//! `PUB_HOSTED_URL`) — rows carry `publishable = false` (the engine's
//! disabled shape; ADR-0060 D5, the Maven/NuGet posture) and the planner
//! registry arm names `pub.dev`.

use halyard_core::EcosystemAdapter;
use halyard_core::discover::{
    DiscoverCtx, DiscoverError, DiscoveryWarning, Facets, ManifestFormat, RawDep, RawPackage,
    RawVersionSource,
};
use halyard_core::package::{DepKind, Ecosystem, ManifestSite, RelPath, ReqSpec};
use semver::Version;

pub mod edit;
pub mod grammar;

/// The dart-family adapter (§7.3 discovery + manifest-edit subset).
#[derive(Clone, Copy, Debug, Default)]
pub struct DartAdapter;

impl DartAdapter {
    /// New adapter.
    pub fn new() -> Self {
        Self
    }
}

/// The release version the `pubspec.yaml` **bytes** carry — the version
/// text when it parses as the plan contract's semver, `None` otherwise
/// (two-component spellings and versionless pubspecs never pair; the
/// documented HEAD fallback governs). The probe input the tag-target
/// pinpoint walk consumes for the dart ecosystem (see
/// `halyard_git::tag_target::ManifestVersionProbe`; the git engine never
/// parses manifests itself).
#[must_use]
pub fn manifest_version(bytes: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(bytes).ok()?;
    let pubspec = grammar::parse_pubspec(text).ok()?;
    let field = pubspec.version?;
    Version::parse(&field.text).ok().map(|_| field.text)
}

impl EcosystemAdapter for DartAdapter {
    fn id(&self) -> &'static str {
        "dart"
    }

    fn edit_versions(
        &self,
        file: &RelPath,
        content: &[u8],
        patches: &[halyard_core::plan::PlanPatch],
    ) -> Result<halyard_core::edit::EditOutcome, halyard_core::edit::EditFailure> {
        edit::edit_versions(file, content, patches)
    }

    fn discover(&self, ctx: &DiscoverCtx) -> Result<Vec<RawPackage>, DiscoverError> {
        // Every pubspec.yaml in the committed tree, sorted (I3).
        let pubspecs: Vec<String> = ctx
            .tree()
            .filter(|path| *path == "pubspec.yaml" || path.ends_with("/pubspec.yaml"))
            .map(str::to_owned)
            .collect();
        if pubspecs.is_empty() {
            return Ok(Vec::new());
        }

        match ctx.file("pubspec.yaml") {
            Some(root_bytes) => {
                let root = parse(root_bytes, "pubspec.yaml")?;
                if root.resolution.as_deref() == Some("workspace") && root.workspace.is_empty() {
                    return Err(DiscoverError::Manifest {
                        file: "pubspec.yaml".to_owned(),
                        format: ManifestFormat::Pubspec,
                        message: "the repository-root pubspec declares `resolution: workspace` \
                                  but no `workspace` entries — a workspace root enumerates its \
                                  members (the documented workspace form)"
                            .to_owned(),
                        span: None,
                    });
                }
                if root.workspace.is_empty() {
                    // The standalone posture: the root (if it carries a
                    // name) and every other pubspec are independent
                    // packages.
                    discover_standalone(ctx, &pubspecs, &root)
                } else {
                    // The workspace posture: the root enumerates its
                    // members; unclaimed pubspecs are the documented
                    // stray-file refusal.
                    discover_workspace(ctx, &pubspecs, &root)
                }
            }
            None => discover_standalone(ctx, &pubspecs, &grammar::Pubspec::default()),
        }
    }
}

/// The resolved version of one pubspec row: the value, its raw-string
/// source and its edit site (canonical semver only).
struct RawVersion {
    /// The parsed semver value (`None` = absent or non-canonical).
    value: Option<Version>,
    /// The version comes from the pubspec's own row.
    source: RawVersionSource,
    /// The edit site (the version value's span), when canonical.
    site: Option<ManifestSite>,
}

/// Parses one pubspec's bytes into the adapter's grammar error shape.
fn parse(bytes: &[u8], file: &str) -> Result<grammar::Pubspec, DiscoverError> {
    let text = std::str::from_utf8(bytes).map_err(|_| DiscoverError::Manifest {
        file: file.to_owned(),
        format: ManifestFormat::Pubspec,
        message: "manifest is not valid UTF-8".to_owned(),
        span: None,
    })?;
    grammar::parse_pubspec(text).map_err(|error| DiscoverError::Manifest {
        file: file.to_owned(),
        format: ManifestFormat::Pubspec,
        message: error.to_string(),
        span: None,
    })
}

/// The workspace posture: the root's `workspace` entries (and any
/// member's own, recursively) claim every member pubspec; a pubspec the
/// walk does not claim is the documented stray-file refusal.
fn discover_workspace(
    ctx: &DiscoverCtx,
    pubspecs: &[String],
    root: &grammar::Pubspec,
) -> Result<Vec<RawPackage>, DiscoverError> {
    let mut claimed: Vec<Member> = Vec::new();
    let mut listed: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    walk_members(ctx, root, "pubspec.yaml", "", &mut claimed, &mut listed)?;

    // The stray-file rule: with a workspace root present, an unclaimed
    // pubspec **between the root and a claimed member** is pub's own
    // resolution error ("pub get will report an error and fail to
    // resolve"); an unclaimed pubspec outside the members' ancestor
    // chain resolves independently and is consumed unmodeled (the
    // orphan-manifest posture).
    for path in pubspecs {
        if path == "pubspec.yaml" || listed.contains(path) {
            continue;
        }
        let dir = dir_of(path);
        let between = claimed.iter().any(|member| {
            member.dir == dir || (!dir.is_empty() && member.dir.starts_with(&format!("{dir}/")))
        });
        if between {
            return Err(DiscoverError::Manifest {
                file: path.clone(),
                format: ManifestFormat::Pubspec,
                message: "a stray pubspec.yaml between the workspace root and the members — \
                          pub get reports the error and fails to resolve (the documented \
                          stray-file rule), so discovery refuses instead of guessing"
                    .to_owned(),
                span: None,
            });
        }
    }

    // Rows: the root first, then the members in sorted-manifest order
    // (§6.1 merge determinism).
    claimed.sort_by(|a, b| a.manifest.cmp(&b.manifest));
    let mut rows = Vec::new();
    let root_manifest = RelPath::new("pubspec.yaml").ok_or_else(manifest_path_error)?;
    let root_name = root
        .name
        .as_ref()
        .ok_or_else(|| name_required("pubspec.yaml"))?;
    let root_version = version_of(root, "pubspec.yaml")?;
    let mut warnings = warnings_of(root, &root_name.text);
    let root_deps = DepInputs {
        ctx,
        pubspec: root,
        manifest: &root_manifest,
        dir: "",
        owner: &root_name.text,
        member_names: &claimed_member_names(&claimed, &root_name.text),
        name_proves_internal: true,
    }
    .rows(&mut warnings);
    rows.push(RawPackage {
        name: root_name.text.clone(),
        ecosystem: Ecosystem::Dart,
        root: String::new(),
        manifest: "pubspec.yaml".to_owned(),
        version: root_version.value,
        version_source: root_version.source,
        publishable: false,
        deps: root_deps,
        tag_template: None,
        changelog: None,
        version_site: root_version.site,
        include: halyard_core::package::PathFilter::new(Vec::new(), Vec::new()),
        facets: Facets::default(),
        warnings,
    });
    let member_names = claimed_member_names(&claimed, &root_name.text);
    for member in &claimed {
        rows.push(member_row(ctx, member, &member_names)?);
    }
    Ok(rows)
}

/// The standalone posture: no workspace root claims the tree, so every
/// pubspec is its own package (the orphan-manifest posture — pub treats
/// them as independent resolutions). Internal edges are the path-proven
/// ones only (a hosted dep naming another package in the tree resolves
/// from the registry, not the repo).
fn discover_standalone(
    ctx: &DiscoverCtx,
    pubspecs: &[String],
    root: &grammar::Pubspec,
) -> Result<Vec<RawPackage>, DiscoverError> {
    let mut rows = Vec::new();
    // The root row first (when the root pubspec exists and carries a
    // name), then the rest in sorted-manifest order.
    if ctx.file("pubspec.yaml").is_some() {
        let manifest = RelPath::new("pubspec.yaml").ok_or_else(manifest_path_error)?;
        let name = root
            .name
            .as_ref()
            .ok_or_else(|| name_required("pubspec.yaml"))?;
        let version = version_of(root, "pubspec.yaml")?;
        let mut warnings = warnings_of(root, &name.text);
        let deps = DepInputs {
            ctx,
            pubspec: root,
            manifest: &manifest,
            dir: "",
            owner: &name.text,
            member_names: &std::collections::BTreeSet::new(),
            name_proves_internal: false,
        }
        .rows(&mut warnings);
        rows.push(RawPackage {
            name: name.text.clone(),
            ecosystem: Ecosystem::Dart,
            root: String::new(),
            manifest: "pubspec.yaml".to_owned(),
            version: version.value,
            version_source: version.source,
            publishable: false,
            deps,
            tag_template: None,
            changelog: None,
            version_site: version.site,
            include: halyard_core::package::PathFilter::new(Vec::new(), Vec::new()),
            facets: Facets::default(),
            warnings,
        });
    }
    for path in pubspecs {
        if path == "pubspec.yaml" {
            continue;
        }
        let bytes = ctx.file(path).ok_or_else(|| missing_tree_file(path))?;
        let pubspec = parse(bytes, path)?;
        let manifest = RelPath::new(path).ok_or_else(manifest_path_error)?;
        let name = pubspec.name.as_ref().ok_or_else(|| name_required(path))?;
        let version = version_of(&pubspec, path)?;
        let mut warnings = warnings_of(&pubspec, &name.text);
        let deps = DepInputs {
            ctx,
            pubspec: &pubspec,
            manifest: &manifest,
            dir: &dir_of(path),
            owner: &name.text,
            member_names: &std::collections::BTreeSet::new(),
            name_proves_internal: false,
        }
        .rows(&mut warnings);
        rows.push(RawPackage {
            name: name.text.clone(),
            ecosystem: Ecosystem::Dart,
            root: dir_of(path),
            manifest: path.clone(),
            version: version.value,
            version_source: version.source,
            publishable: false,
            deps,
            tag_template: None,
            changelog: None,
            version_site: version.site,
            include: halyard_core::package::PathFilter::new(Vec::new(), Vec::new()),
            facets: Facets::default(),
            warnings,
        });
    }
    Ok(rows)
}

/// One walked member (plain data before row assembly).
struct Member {
    /// The parsed pubspec.
    pubspec: grammar::Pubspec,
    /// The member's manifest path (repo-relative).
    manifest: String,
    /// The member's repo-relative directory ("" = the repository root).
    dir: String,
}

/// The member names the internal-edge proof checks (the root's name plus
/// every member's).
fn claimed_member_names(claimed: &[Member], root_name: &str) -> std::collections::BTreeSet<String> {
    let mut names: std::collections::BTreeSet<String> = claimed
        .iter()
        .filter_map(|member| member.pubspec.name.as_ref().map(|field| field.text.clone()))
        .collect();
    names.insert(root_name.to_owned());
    names
}

/// Walks a workspace's `workspace` entries (recursively), enforcing the
/// committed-tree, `resolution: workspace` and no-double-claiming rules.
fn walk_members(
    ctx: &DiscoverCtx,
    owner: &grammar::Pubspec,
    owner_manifest: &str,
    owner_dir: &str,
    claimed: &mut Vec<Member>,
    listed: &mut std::collections::BTreeSet<String>,
) -> Result<(), DiscoverError> {
    for entry in &owner.workspace {
        for manifest in member_manifests(ctx, &entry.path, owner_dir)? {
            if !listed.insert(manifest.clone()) {
                return Err(DiscoverError::Manifest {
                    file: owner_manifest.to_owned(),
                    format: ManifestFormat::Pubspec,
                    message: format!(
                        "workspace entry {manifest:?} lists a pubspec another workspace \
                         entry already claimed — one pubspec, one owner"
                    ),
                    span: None,
                });
            }
            let bytes = ctx
                .file(&manifest)
                .ok_or_else(|| missing_tree_file(&manifest))?;
            let pubspec = parse(bytes, &manifest)?;
            if pubspec.resolution.as_deref() != Some("workspace") {
                return Err(DiscoverError::Manifest {
                    file: manifest.clone(),
                    format: ManifestFormat::Pubspec,
                    message: format!(
                        "the member pubspec declares no `resolution: workspace` — a pubspec \
                         listed by the `workspace` entry of {owner_manifest} must carry the \
                         documented resolution marker"
                    ),
                    span: None,
                });
            }
            let dir = dir_of(&manifest);
            // Nested workspaces: a member's own `workspace` entries walk
            // recursively relative to the member's directory.
            if !pubspec.workspace.is_empty() {
                walk_members(ctx, &pubspec, &manifest, &dir, claimed, listed)?;
            }
            claimed.push(Member {
                pubspec,
                manifest,
                dir,
            });
        }
    }
    Ok(())
}

/// Resolves one `workspace` entry to its member manifest path(s). An
/// explicit entry names `<dir>/pubspec.yaml`; the documented glob form
/// (`packages/*` — exactly one `*` as a whole path segment) expands over
/// the committed tree; any other glob shape refuses fail-closed.
fn member_manifests(
    ctx: &DiscoverCtx,
    entry: &str,
    owner_dir: &str,
) -> Result<Vec<String>, DiscoverError> {
    let trimmed = entry.trim_matches('/');
    if trimmed.is_empty() || trimmed.starts_with('/') {
        return Err(glob_error(
            entry,
            "an empty or absolute workspace entry cannot name a member in the committed tree",
        ));
    }
    if trimmed.split('/').any(|c| c == "..") {
        return Err(glob_error(
            entry,
            "a workspace entry outside the committed tree cannot be planned",
        ));
    }
    let segments: Vec<&str> = trimmed.split('/').collect();
    let stars = segments
        .iter()
        .filter(|segment| segment.contains('*'))
        .count();
    if stars > 1 {
        return Err(unsupported_glob(entry));
    }
    if let Some(position) = segments.iter().position(|segment| segment.contains('*')) {
        let Some(segment) = segments.get(position).copied() else {
            return Err(unsupported_glob(entry));
        };
        if segment != "*" {
            return Err(unsupported_glob(entry));
        }
        // Expand relative to the owning pubspec's directory: a member
        // pubspec's directory is `owner_dir` followed by exactly the
        // pattern's segments (the `*` matching any one segment).
        let owner_segments: Vec<&str> = owner_dir.split('/').filter(|s| !s.is_empty()).collect();
        let mut out = Vec::new();
        for path in ctx.tree() {
            let Some(dir) = path.strip_suffix("/pubspec.yaml") else {
                continue;
            };
            let dir_segments: Vec<&str> = dir.split('/').collect();
            let Some((prefix, scoped)) = dir_segments.split_at_checked(owner_segments.len()) else {
                continue;
            };
            if prefix == owner_segments.as_slice()
                && scoped.len() == segments.len()
                && segments
                    .iter()
                    .zip(scoped.iter())
                    .all(|(segment, dir_segment)| *segment == "*" || segment == dir_segment)
            {
                out.push(format!("{dir}/pubspec.yaml"));
            }
        }
        out.sort();
        if out.is_empty() {
            return Err(glob_error(
                entry,
                "the glob expands to no member pubspec in the committed tree",
            ));
        }
        return Ok(out);
    }
    // The explicit form: a directory entry names `<dir>/pubspec.yaml`; a
    // pubspec.yaml entry names the file.
    let joined = if owner_dir.is_empty() {
        trimmed.to_owned()
    } else {
        format!("{owner_dir}/{trimmed}")
    };
    let manifest = if is_yaml_name(&joined) {
        joined
    } else {
        format!("{joined}/pubspec.yaml")
    };
    if ctx.file(&manifest).is_none() {
        return Err(DiscoverError::Glob {
            pattern: entry.to_owned(),
            message: format!(
                "workspace entry names {manifest:?}, which has no pubspec.yaml in the \
                 committed tree (pub: the `workspace` entry enumerates the paths to the \
                 packages of the repository)"
            ),
        });
    }
    Ok(vec![manifest])
}

/// An unsupported glob shape: the pubspec's `workspace` entry carries a
/// pattern the shipped grammar does not model — a manifest-grammar
/// refusal (`E-DISC-MANIFEST`), never a guess.
fn unsupported_glob(entry: &str) -> DiscoverError {
    DiscoverError::Manifest {
        file: String::new(),
        format: ManifestFormat::Pubspec,
        message: format!(
            "workspace entry {entry:?}: only the documented single-glob form (`packages/*`, \
             one `*` as a whole path segment) is supported — the pattern refuses instead of \
             guessing"
        ),
        span: None,
    }
}

fn glob_error(pattern: &str, message: &str) -> DiscoverError {
    DiscoverError::Glob {
        pattern: pattern.to_owned(),
        message: message.to_owned(),
    }
}

fn missing_tree_file(path: &str) -> DiscoverError {
    DiscoverError::Glob {
        pattern: path.to_owned(),
        message: "the pubspec is not in the committed tree".to_owned(),
    }
}

fn name_required(file: &str) -> DiscoverError {
    DiscoverError::Manifest {
        file: file.to_owned(),
        format: ManifestFormat::Pubspec,
        message: "pubspec has no `name` — the field is required for every package".to_owned(),
        span: None,
    }
}

fn manifest_path_error() -> DiscoverError {
    DiscoverError::Manifest {
        file: String::new(),
        format: ManifestFormat::Pubspec,
        message: "manifest path is not a normalized repo-relative path".to_owned(),
        span: None,
    }
}

/// The repo-relative directory of a pubspec path (`pubspec.yaml` → the
/// root, `packages/a/pubspec.yaml` → `packages/a`).
fn dir_of(manifest_path: &str) -> String {
    match manifest_path.rsplit_once('/') {
        Some((dir, _)) => dir.to_owned(),
        None => String::new(),
    }
}

/// Whether a path names a `pubspec.yaml` file itself (the case-sensitive
/// comparison goes through `Path::extension`, the pedantic gate's shape).
fn is_yaml_name(path: &str) -> bool {
    std::path::Path::new(path)
        .extension()
        .is_some_and(|extension| extension == "yaml")
}

/// The version of a pubspec's own `version` field (canonical semver only
/// — the ADR-0056 discipline).
fn version_of(pubspec: &grammar::Pubspec, file: &str) -> Result<RawVersion, DiscoverError> {
    let Some(field) = &pubspec.version else {
        return Ok(RawVersion {
            value: None,
            source: RawVersionSource::Local,
            site: None,
        });
    };
    let site = ManifestSite {
        file: RelPath::new(file).ok_or_else(manifest_path_error)?,
        spans: vec![field.span],
    };
    match Version::parse(&field.text) {
        Ok(value) => Ok(RawVersion {
            value: Some(value),
            source: RawVersionSource::Local,
            site: Some(site),
        }),
        Err(_) => Ok(RawVersion {
            // The pub grammar itself is semver — a spelling that does not
            // parse is not a pub version at all (the documented
            // three-number grammar); warned and never rewritten.
            value: None,
            source: RawVersionSource::Local,
            site: None,
        }),
    }
}

/// The row-level warnings a parsed pubspec carries (grammar-shape facts,
/// not dependency facts).
fn warnings_of(pubspec: &grammar::Pubspec, subject: &str) -> Vec<DiscoveryWarning> {
    let mut warnings = Vec::new();
    if let Some(field) = &pubspec.version
        && Version::parse(&field.text).is_err()
    {
        warnings.push(DiscoveryWarning {
            code: "non-canonical-version",
            subject: subject.to_owned(),
            message: format!(
                "version {:?} is a spelling the plan contract cannot render (pub's own grammar \
                 is three numbers separated by dots); package treated as unpublishable and \
                 never rewritten",
                field.text
            ),
        });
    }
    if pubspec.version.is_none() {
        warnings.push(DiscoveryWarning {
            code: "no-version",
            subject: subject.to_owned(),
            message: "no `version` in the pubspec; package treated as unpublished (pub's \
                      implicitly-versioned-0.0.0 local-only posture)"
                .to_owned(),
        });
    }
    if pubspec.publish_to_none {
        warnings.push(DiscoveryWarning {
            code: "publish-disabled",
            subject: subject.to_owned(),
            message: "`publish_to: none` prevents publication anywhere (the documented \
                      opt-out); the row records the unpublishable posture"
                .to_owned(),
        });
    }
    if !pubspec.overrides.is_empty() {
        warnings.push(DiscoveryWarning {
            code: "unmodeled-override",
            subject: subject.to_owned(),
            message: "`dependency_overrides` pins local sources and carries no published \
                      requirement; the section is consumed unmodeled"
                .to_owned(),
        });
    }
    warnings
}

/// Assembles one member row (§6.1 merge row).
fn member_row(
    ctx: &DiscoverCtx,
    member: &Member,
    member_names: &std::collections::BTreeSet<String>,
) -> Result<RawPackage, DiscoverError> {
    let manifest = RelPath::new(&member.manifest).ok_or_else(manifest_path_error)?;
    let name = member
        .pubspec
        .name
        .as_ref()
        .ok_or_else(|| name_required(&member.manifest))?;
    let version = version_of(&member.pubspec, &member.manifest)?;
    let mut warnings = warnings_of(&member.pubspec, &name.text);
    let deps = DepInputs {
        ctx,
        pubspec: &member.pubspec,
        manifest: &manifest,
        dir: &member.dir,
        owner: &name.text,
        member_names,
        name_proves_internal: true,
    }
    .rows(&mut warnings);
    Ok(RawPackage {
        name: name.text.clone(),
        ecosystem: Ecosystem::Dart,
        root: member.dir.clone(),
        manifest: member.manifest.clone(),
        version: version.value,
        version_source: version.source,
        publishable: false,
        deps,
        tag_template: None,
        changelog: None,
        version_site: version.site,
        include: halyard_core::package::PathFilter::new(Vec::new(), Vec::new()),
        facets: Facets::default(),
        warnings,
    })
}

/// The dependency-row inputs for one package (the shape `deps_of`
/// consumes; the fields bundle the per-package context the edge proofs
/// need).
struct DepInputs<'a> {
    /// The committed tree (the `path:` resolution's existence checks).
    ctx: &'a DiscoverCtx,
    /// The parsed pubspec whose rows are walked.
    pubspec: &'a grammar::Pubspec,
    /// The package's manifest (the requirement site's file).
    manifest: &'a RelPath,
    /// The manifest's repo-relative directory (the `path:` resolution
    /// base, `""` = the root).
    dir: &'a str,
    /// The package name (the warning subject).
    owner: &'a str,
    /// The workspace member names (the name-proven edge's check set;
    /// empty in the standalone posture).
    member_names: &'a std::collections::BTreeSet<String>,
    /// Whether the name-proven edge applies (the workspace posture
    /// only — a standalone hosted dep resolves from the registry, not
    /// the repo).
    name_proves_internal: bool,
}

impl DepInputs<'_> {
    /// Builds the dependency rows (a `path:` dep naming a member
    /// manifest is the path-proven internal edge; in a workspace, a dep
    /// **naming** a member is the name-proven edge regardless of its
    /// declared source).
    fn rows(&self, warnings: &mut Vec<DiscoveryWarning>) -> Vec<RawDep> {
        let mut rows = Vec::new();
        for (dep, kind) in self
            .pubspec
            .deps
            .iter()
            .map(|dep| (dep, DepKind::Normal))
            .chain(self.pubspec.dev_deps.iter().map(|dep| (dep, DepKind::Dev)))
        {
            let mut internal = false;
            if self.name_proves_internal && self.member_names.contains(&dep.name) {
                // The name-proven edge: "they will automatically resolve
                // to the one in the workspace, regardless of the source".
                internal = true;
            } else if let Some(path) = &dep.path {
                match resolve_path_dep(self.ctx, self.dir, path) {
                    PathResolution::Member => internal = true,
                    PathResolution::ExternalNonMember => warnings.push(DiscoveryWarning {
                        code: "external-path-dep",
                        subject: self.owner.to_owned(),
                        message: format!(
                            "the `path:` dependency {} names {path:?}, which is no member \
                             pubspec in the committed tree; the edge stays registry- \
                             unmodeled and verbatim",
                            dep.name
                        ),
                    }),
                    PathResolution::OutsideTree => warnings.push(DiscoveryWarning {
                        code: "external-path-dep",
                        subject: self.owner.to_owned(),
                        message: format!(
                            "the `path:` dependency {} names {path:?}, outside the committed \
                             tree; the edge stays registry-unmodeled and verbatim",
                            dep.name
                        ),
                    }),
                }
            }
            let (raw, spans) = match &dep.constraint {
                Some(field) => (
                    halyard_core::BString::from(field.text.as_str()),
                    vec![field.span],
                ),
                // Source-keyed rows without a version constraint (`path:`,
                // `sdk:`) ride verbatim with no site to rewrite.
                None => (halyard_core::BString::from(""), Vec::new()),
            };
            rows.push(RawDep {
                name: dep.name.clone(),
                kind,
                req: ReqSpec::Other { raw: raw.clone() },
                site: ManifestSite {
                    file: self.manifest.clone(),
                    spans,
                },
                raw,
                internal,
            });
        }
        rows
    }
}

/// Where a resolved `path:` dependency landed.
enum PathResolution {
    /// The path names a member pubspec in the committed tree.
    Member,
    /// The path lands inside the tree but names no member pubspec.
    ExternalNonMember,
    /// The path leaves the committed tree (an absolute path or `..`
    /// climbing above the root).
    OutsideTree,
}

/// Resolves a `path:` dependency value *"relative to the directory
/// containing your pubspec"* (`dir` = that directory, `""` = the root).
fn resolve_path_dep(ctx: &DiscoverCtx, dir: &str, path: &str) -> PathResolution {
    let Some(normalized) = normalize_path(dir, path) else {
        return PathResolution::OutsideTree;
    };
    // The path may name the package's directory or the pubspec file
    // itself.
    if is_yaml_name(&normalized) {
        return if ctx.file(&normalized).is_some() {
            PathResolution::Member
        } else {
            PathResolution::ExternalNonMember
        };
    }
    let manifest = format!("{normalized}/pubspec.yaml");
    if ctx.file(&manifest).is_some() {
        PathResolution::Member
    } else {
        PathResolution::ExternalNonMember
    }
}

/// Lexically normalizes a relative path (`.` and `..` segments resolved;
/// `None` when the result leaves the tree; an absolute path also leaves
/// the tree).
fn normalize_path(dir: &str, path: &str) -> Option<String> {
    if path.starts_with('/') {
        return None;
    }
    let mut out: Vec<&str> = if dir.is_empty() {
        Vec::new()
    } else {
        dir.split('/').collect()
    };
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                out.pop()?;
            }
            other => out.push(other),
        }
    }
    Some(out.join("/"))
}

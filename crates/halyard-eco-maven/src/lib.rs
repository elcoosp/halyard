//! [`EcosystemAdapter`] implementation for Maven (M8, the §17
//! `Maven/Gradle` cell — the next listed cell after `PyPI` (ADR-0056)
//! and Go (ADR-0057), ADR-0058).
//!
//! Parses `pom.xml` with the hand-rolled scanner of [`grammar`] at the
//! VERIFIED reference grammar (`docs/research/maven-pom-grammars.md`,
//! from `maven.apache.org/pom.html` and the `maven-artifact`
//! `ComparableVersion` javadoc). Versions are manifest rows — but under
//! Maven's own comparator, not semver (`"Generic implementation of
//! version comparison"` over arbitrary strings, `snapshot` a
//! *"well-known qualifier"*): the adapter applies the PEP 440 discipline
//! of ADR-0056 — only semver-parseable spellings are canonical (planned
//! with an editable site); every Maven-specific spelling (`1.0`,
//! `1.0-SNAPSHOT`, a `${...}` property reference) is
//! recognized-but-non-canonical — **warned, unpublishable, no edit site,
//! never rewritten**.
//!
//! Workspaces ride the aggregator grammar (*"A pom packaged project may
//! aggregate the build of a set of projects by listing them as modules,
//! which are relative paths to the directories or the POM files of those
//! projects"*), recursively: a member that itself carries `<modules>` is
//! an aggregator whose members follow the same rules. A member pom
//! without its own `<version>` inherits the parent's (the documented
//! inheritance list — *"groupId, version, ..."*), planning through the
//! existing workspace-inheritance machinery (one edit site, on the
//! aggregator pom). A member's `<parent>` coordinates must match the
//! aggregator that listed it (*"uses the POM it finds there only if its
//! coordinates match the <parent> element"* — a mismatch refuses
//! instead of guessing). Nested poms nothing claims are skipped (the
//! orphan-manifest posture).
//!
//! Internal edges are the coordinate-proven ones: a `<dependency>` whose
//! (groupId, artifactId) pair names a discovered member. Maven Central
//! publishing carries no argv grammar yet — rows carry
//! `publishable = false` (the engine's disabled shape; ADR-0058 D5).

use halyard_core::EcosystemAdapter;
use halyard_core::discover::{
    DiscoverCtx, DiscoverError, DiscoveryWarning, Facets, ManifestFormat, RawDep, RawPackage,
    RawVersionSource,
};
use halyard_core::package::{DepKind, Ecosystem, ManifestSite, RelPath, ReqSpec};
use semver::Version;

pub mod edit;
pub mod gradle;
pub mod grammar;
pub mod settings;

/// The maven-family adapter (§7.3 discovery + manifest-edit subset).
#[derive(Clone, Copy, Debug, Default)]
pub struct MavenAdapter;

impl MavenAdapter {
    /// New adapter.
    pub fn new() -> Self {
        Self
    }
}

/// The release version the `pom.xml` **bytes** carry — the
/// semver-canonical pairing text when the project's own `<version>` is a
/// canonical spelling, `None` otherwise (non-semver Maven spellings
/// never pair; the documented HEAD fallback governs). The probe input
/// the tag-target pinpoint walk consumes for the maven ecosystem (see
/// `halyard_git::tag_target::ManifestVersionProbe`; the git engine never
/// parses manifests itself).
#[must_use]
pub fn manifest_version(bytes: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(bytes).ok()?;
    let pom = grammar::parse_pom(text, "pom.xml").ok()?;
    let version = pom.version?;
    Version::parse(&version.text).ok().map(|_| version.text)
}

impl EcosystemAdapter for MavenAdapter {
    fn id(&self) -> &'static str {
        "maven"
    }

    fn discover(&self, ctx: &DiscoverCtx) -> Result<Vec<RawPackage>, DiscoverError> {
        // The Gradle half (ADR-0058 continued — `build.gradle(.kts)`
        // literal assignments) rides the same Maven ecosystem as the
        // pom.xml half: both publish to Maven Central, the Maven/Gradle
        // cell's single registry. Collected first, appended after the
        // pom rows so the merge order stays pom-then-gradle (§6.1).
        let gradle = gradle_rows(ctx)?;
        let Some(bytes) = ctx.file("pom.xml") else {
            return Ok(gradle);
        };
        let text = utf8(bytes, "pom.xml")?;
        let root = grammar::parse_pom(text, "pom.xml")?;
        let mut packages = Vec::new();

        // The root pom is a package when it carries coordinates (a
        // pom-packaging artifact is a real artifact, §6.1
        // single-package mode when no `<modules>` list follows).
        let root_version = version_of(&root, "pom.xml")?;
        if root.modules.is_empty() {
            packages.push(row(&root, "", "pom.xml", root_version, &[])?);
            packages.extend(gradle);
            return Ok(packages);
        }
        packages.push(row(&root, "", "pom.xml", root_version.clone(), &[])?);

        // Members: the module entries, recursively (a member with its
        // own `<modules>` aggregates further). Members follow the root
        // in sorted-manifest order (§6.1 merge determinism).
        let mut walked: Vec<Member> = Vec::new();
        let mut listed: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        walk_members(ctx, &root, "pom.xml", &mut walked, &mut listed)?;
        walked.sort_by(|a, b| a.manifest.cmp(&b.manifest));
        let member_coordinate_set: std::collections::BTreeSet<(String, String)> = walked
            .iter()
            .filter_map(|member| member.coordinates.clone())
            .collect();
        let member_coordinates: Vec<(String, String)> = member_coordinate_set.into_iter().collect();
        for member in &walked {
            let Some(parent) = &member.pom.parent else {
                return Err(DiscoverError::Manifest {
                    file: member.manifest.clone(),
                    format: ManifestFormat::Pom,
                    message: format!(
                        "member pom declares no <parent> — a module listed by <modules> in \
                         {} must name its aggregator",
                        member.aggregator
                    ),
                    span: None,
                });
            };
            // The documented parent rule: the POM is used *"only if its
            // coordinates match the <parent> element"* — the member's
            // parent must name the aggregator that listed it.
            if parent.group_id != member.parent_coordinates.0
                || parent.artifact_id != member.parent_coordinates.1
            {
                return Err(DiscoverError::Manifest {
                    file: member.manifest.clone(),
                    format: ManifestFormat::Pom,
                    message: format!(
                        "<parent> coordinates {}:{} do not match the aggregator {}:{} that \
                         lists this module",
                        parent.group_id,
                        parent.artifact_id,
                        member.parent_coordinates.0,
                        member.parent_coordinates.1
                    ),
                    span: None,
                });
            }
            let version = if let Some(field) = &member.pom.version {
                version_from_field(field, &member.manifest)?
            } else {
                // Inheritance (the documented list): the aggregator's
                // version, through the workspace-inheritance machinery
                // (one edit site, on the aggregator pom).
                let site = match member.parent_version_site {
                    Some(span) => {
                        let file = rel_path(&member.aggregator)?;
                        Some(ManifestSite {
                            file,
                            spans: vec![span],
                        })
                    }
                    None => None,
                };
                RawVersion {
                    value: member.parent_version.clone(),
                    source: RawVersionSource::WorkspaceInherited {
                        file: member.aggregator.clone(),
                        key_path: "project.version".to_owned(),
                    },
                    site,
                }
            };
            packages.push(row(
                &member.pom,
                &member.dir,
                &member.manifest,
                version,
                &member_coordinates,
            )?);
        }
        packages.extend(gradle);
        Ok(packages)
    }

    fn edit_versions(
        &self,
        file: &RelPath,
        content: &[u8],
        patches: &[halyard_core::plan::PlanPatch],
    ) -> Result<halyard_core::edit::EditOutcome, halyard_core::edit::EditFailure> {
        // ADR-0058 continued: a Gradle build file routes to the Gradle
        // literal-assignment patch; a pom.xml keeps the pom patch (the
        // Maven/Gradle cell's two grammars, one ecosystem).
        if file.as_str().ends_with("build.gradle") || file.as_str().ends_with("build.gradle.kts") {
            edit::edit_gradle_versions(file, content, patches)
        } else {
            edit::edit_versions(file, content, patches)
        }
    }
}

/// The Gradle half of discovery (ADR-0058 continued — the Maven/Gradle
/// cell's Gradle half): every `build.gradle` / `build.gradle.kts` in the
/// committed tree is a Maven row (the Maven/Gradle cell's single
/// ecosystem — both publish to Maven Central; the per-file shape the
/// Dockerfile and gemspec cells established), and a sibling
/// `settings.gradle` / `settings.gradle.kts` (the `settings` module's
/// VERIFIED grammar) governs its directory subtree with the
/// multi-project aggregator shape: the root build file is named by
/// `rootProject.name`, each include-proven member is named by the
/// include path's last element, and an unincluded build file keeps the
/// standalone row (warned — the per-file shape never silently shrinks).
/// The version is the documented literal `version = '…'` /
/// `version = "…"` assignment (Gradle's Project API: *"The version of
/// this project. Gradle always uses the `toString()` value of the
/// version. The version defaults to `unspecified`."*); the last
/// assignment in document order wins (the Project setter's ordinary
/// overwrite). Only semver-parseable literals are canonical; every
/// other right-hand side is recognized-but-non-canonical — warned,
/// unpublishable, no edit site, never rewritten.
fn gradle_rows(ctx: &DiscoverCtx) -> Result<Vec<RawPackage>, DiscoverError> {
    // Every `build.gradle` / `build.gradle.kts` in the committed tree
    // (sorted walk order, I3) is a candidate manifest — the documented
    // build-file names. The settings files join the walk as the
    // governance source (`settings_governance`); they are never package
    // manifests themselves (a settings file carries no version slot).
    let manifests: Vec<String> = ctx
        .tree()
        .filter(|path| {
            **path == *"build.gradle"
                || path.ends_with("/build.gradle")
                || **path == *"build.gradle.kts"
                || path.ends_with("/build.gradle.kts")
        })
        .map(std::borrow::ToOwned::to_owned)
        .collect();
    let governance = settings_governance(ctx)?;
    let mut packages = Vec::new();
    let mut parsed = Vec::new();
    for path in &manifests {
        let Some((row, file)) = gradle_manifest_row(ctx, path, &governance)? else {
            continue;
        };
        parsed.push(file);
        packages.push(row);
    }
    attach_settings_warnings(&governance, &manifests, &mut packages);
    resolve_project_edges(&governance, &parsed, &mut packages)?;
    Ok(packages)
}

/// One build file's row: the parse, the settings-governed name, and the
/// version capture. `None` when the tree's bytes are unavailable (the
/// manifest filter and the context inventory disagree — impossible on
/// the orchestrator's own walk, tolerated deterministically).
fn gradle_manifest_row(
    ctx: &DiscoverCtx,
    path: &str,
    governance: &[GradleSettings],
) -> Result<Option<(RawPackage, gradle::GradleBuild)>, DiscoverError> {
    let Some(bytes) = ctx.file(path) else {
        return Ok(None);
    };
    let text = std::str::from_utf8(bytes).map_err(|_| DiscoverError::Manifest {
        file: path.to_owned(),
        format: ManifestFormat::GradleBuild,
        message: "manifest is not valid UTF-8".to_owned(),
        span: None,
    })?;
    let file = gradle::parse_gradle_build(text).map_err(|error| DiscoverError::Manifest {
        file: path.to_owned(),
        format: ManifestFormat::GradleBuild,
        message: error.to_string(),
        span: None,
    })?;
    let owner_dir = gradle_dir_of(path);
    // The nearest settings file whose directory contains the build
    // file governs its name (the multi-project aggregator shape); an
    // ambiguous settings pair declines provenance (fail-closed).
    let governing = governing_settings(governance, &owner_dir);
    let mut warnings: Vec<DiscoveryWarning> = Vec::new();
    let name = match governing {
        Some((_, settings)) if settings.ambiguous => dir_chain_name(&owner_dir, path),
        Some((_, settings)) => {
            let rel = relative_to(&settings.dir, &owner_dir);
            if owner_dir == settings.dir {
                // The governed root: the settings file's own project
                // (the documented directory default when no literal
                // name row applies — at the repository root the
                // committed tree cannot know the checkout directory's
                // name, so the manifest path remains).
                settings
                    .root_name
                    .clone()
                    .unwrap_or_else(|| dir_chain_name(&owner_dir, path))
            } else if let Some(member) = settings.members.get(&rel) {
                member.clone()
            } else {
                warnings.push(DiscoveryWarning {
                    code: "unincluded-gradle-build",
                    subject: dir_chain_name(&owner_dir, path),
                    message: format!(
                        "the build file rides outside {}'s include set — the per-file \
                         row keeps the standalone shape",
                        settings.file
                    ),
                });
                dir_chain_name(&owner_dir, path)
            }
        }
        None => dir_chain_name(&owner_dir, path),
    };
    Ok(Some((gradle_row(path, &file, name, warnings)?, file)))
}

/// The nearest settings entry governing `dir` (its directory is an
/// ancestor-or-self; the most specific wins) with its index into the
/// governance list, `None` when no settings file governs.
fn governing_settings<'a>(
    governance: &'a [GradleSettings],
    dir: &str,
) -> Option<(usize, &'a GradleSettings)> {
    governance
        .iter()
        .enumerate()
        .filter(|(_, settings)| settings.governs(dir))
        .max_by_key(|(_, settings)| settings.dir.len())
}

/// The settings-level observations (a non-literal name row, a
/// non-literal include argument, a proven member without a build file,
/// an ambiguous settings pair) warn on the settings tree's root row —
/// the settings file's own project — when it exists; a tree with no
/// root build file has no warning carrier and stays silent
/// (deterministic; the rows themselves are unaffected).
fn attach_settings_warnings(
    governance: &[GradleSettings],
    manifests: &[String],
    packages: &mut [RawPackage],
) {
    for settings in governance {
        let mut extra: Vec<DiscoveryWarning> = Vec::new();
        if settings.ambiguous {
            extra.push(DiscoveryWarning {
                code: "ambiguous-settings",
                subject: settings.file.clone(),
                message: "both settings.gradle and settings.gradle.kts exist in this \
                          directory — the provenance source is ambiguous and is declined \
                          (fail-closed: no name or member is guessed)"
                    .to_owned(),
            });
        }
        if settings.root_name_nonliteral {
            extra.push(DiscoveryWarning {
                code: "non-canonical-name",
                subject: settings.file.clone(),
                message: "the rootProject.name row carries a non-literal right-hand side \
                          (a computed value or string template) — the documented \
                          directory default governs the root package's name"
                    .to_owned(),
            });
        }
        if settings.nonliteral_include {
            extra.push(DiscoveryWarning {
                code: "non-canonical-include",
                subject: settings.file.clone(),
                message: "an include argument carries a non-literal shape (a `$` template \
                          or property reference) — that member is not proven by the \
                          grammar and keeps the standalone shape"
                    .to_owned(),
            });
        }
        for (rel, name) in &settings.members {
            if !settings_has_build_file(manifests, &settings.dir, rel) {
                extra.push(DiscoveryWarning {
                    code: "include-without-manifest",
                    subject: name.clone(),
                    message: format!(
                        "the include-proven member {rel:?} carries no build file in the \
                         committed tree — Gradle permits build-less projects, so there is \
                         no package row to plan"
                    ),
                });
            }
        }
        if extra.is_empty() {
            continue;
        }
        // The carrier: the settings directory's own build file, in the
        // sorted manifests order (I3 — `build.gradle` sorts before
        // `build.gradle.kts`).
        let Some(root_manifest) = manifests
            .iter()
            .find(|manifest| gradle_dir_of(manifest) == settings.dir)
        else {
            continue;
        };
        if let Some(row) = packages
            .iter_mut()
            .find(|row| &row.manifest == root_manifest)
        {
            row.warnings.extend(extra);
        }
    }
}

/// The proven `project(':…')` edges (ADR-0058 continued — the
/// recorded-open dependency-edge remainder): the governing settings
/// build's proven members resolve the path; a row the subset cannot
/// prove (a relative path, a `$` template, a member nothing includes,
/// an ambiguous settings pair, no settings at all) warns and stays
/// external — nothing is guessed. The requirement rides
/// `ReqSpec::PathOnly` (a project dependency carries no version
/// constraint — nothing to rewrite), so the edge's spans are empty and
/// the cascade policy stays the planner's own.
fn resolve_project_edges(
    governance: &[GradleSettings],
    parsed: &[gradle::GradleBuild],
    packages: &mut [RawPackage],
) -> Result<(), DiscoverError> {
    // The governed row names per settings entry (the resolution map):
    // the root's dir and every proven member's dir → the row's name.
    let targets: Vec<std::collections::BTreeMap<String, String>> = governance
        .iter()
        .map(|settings| {
            if settings.ambiguous {
                // Provenance declined: no governed target exists.
                return std::collections::BTreeMap::new();
            }
            packages
                .iter()
                .filter(|row| {
                    settings.governs(&row.root)
                        && (row.root == settings.dir
                            || settings
                                .members
                                .contains_key(&relative_to(&settings.dir, &row.root)))
                })
                .map(|row| (row.root.clone(), row.name.clone()))
                .collect()
        })
        .collect();
    for (row_idx, row) in packages.iter_mut().enumerate() {
        let Some(file) = parsed.get(row_idx) else {
            continue;
        };
        if file.project_deps.is_empty() {
            continue;
        }
        let owner_dir = gradle_dir_of(&row.manifest);
        let manifest = RelPath::new(&row.manifest).ok_or_else(|| DiscoverError::Manifest {
            file: row.manifest.clone(),
            format: ManifestFormat::GradleBuild,
            message: "manifest path is not a normalized repo-relative path".to_owned(),
            span: None,
        })?;
        for dep in &file.project_deps {
            if !dep.proven {
                row.warnings.push(DiscoveryWarning {
                    code: "non-canonical-project-dep",
                    subject: row.name.clone(),
                    message: format!(
                        "the project row {:?} is a shape the subset does not resolve (a \
                         relative path or a `$` template) — recognized, consumed unmodeled, \
                         never an edge",
                        dep.path
                    ),
                });
                continue;
            }
            let resolved =
                governing_settings(governance, &owner_dir).and_then(|(gov_idx, settings)| {
                    if settings.ambiguous {
                        return None;
                    }
                    let target_dir = project_target_dir(&settings.dir, &dep.path);
                    targets
                        .get(gov_idx)
                        .and_then(|map| map.get(&target_dir))
                        .cloned()
                });
            match resolved {
                Some(target) => row.deps.push(RawDep {
                    name: target,
                    kind: if dep.configuration.starts_with("test") {
                        DepKind::Dev
                    } else {
                        DepKind::Normal
                    },
                    req: ReqSpec::PathOnly,
                    site: ManifestSite {
                        file: manifest.clone(),
                        spans: Vec::new(),
                    },
                    raw: halyard_core::BString::from(dep.path.as_str()),
                    internal: true,
                }),
                None => row.warnings.push(DiscoveryWarning {
                    code: "unresolved-project-dep",
                    subject: row.name.clone(),
                    message: format!(
                        "the project row {:?} names no include-proven member of the owning \
                         settings build — the edge stays external (fail-closed, nothing is \
                         guessed)",
                        dep.path
                    ),
                }),
            }
        }
    }
    Ok(())
}

/// The target directory one proven `project(':…')` path names: the
/// `':'`-separated hierarchy resolved **relative to the settings
/// directory** (the documented root-project-relative conversion; a
/// bare `:` names the root project itself).
fn project_target_dir(settings_dir: &str, path: &str) -> String {
    let segments: Vec<&str> = path
        .split(':')
        .filter(|segment| !segment.is_empty())
        .collect();
    if segments.is_empty() {
        return settings_dir.to_owned();
    }
    let rel = segments.join("/");
    if settings_dir.is_empty() {
        rel
    } else {
        format!("{settings_dir}/{rel}")
    }
}

/// The identity chain without provenance: the containing directory's
/// own name (`services/api` → `api`), the manifest path at the
/// repository root (the Dockerfile cell's chain — the committed tree
/// cannot know the checkout directory's own name).
fn dir_chain_name(dir: &str, manifest_path: &str) -> String {
    gradle_context_name(dir)
        .map_or_else(|| manifest_path.to_owned(), std::borrow::ToOwned::to_owned)
}

/// The path of `dir` relative to `base` (both repo-relative; `base` is
/// an ancestor-or-self of `dir`).
fn relative_to(base: &str, dir: &str) -> String {
    if base.is_empty() {
        return dir.to_owned();
    }
    dir.get(base.len() + 1..).unwrap_or_default().to_owned()
}

/// Whether the settings tree's member directory carries a build file
/// (`rel` is relative to the settings directory).
fn settings_has_build_file(manifests: &[String], settings_dir: &str, rel: &str) -> bool {
    let member_dir = if settings_dir.is_empty() {
        rel.to_owned()
    } else {
        format!("{settings_dir}/{rel}")
    };
    manifests
        .iter()
        .any(|manifest| gradle_dir_of(manifest) == member_dir)
}

/// Builds one raw package row from a parsed Gradle build file (§6.1
/// merge row): the settings-governed name (the `gradle_rows` resolution
/// — `rootProject.name` for a governed root, the include path's last
/// element for a proven member, the directory chain otherwise), the
/// last-assignment-wins version strategy, and the engine's disabled
/// publish shape (no `gradle` argv grammar — the credential surface is
/// `~/.gradle/gradle.properties` + the `publishing` block, no
/// documented env-var grammar).
fn gradle_row(
    manifest_path: &str,
    file: &gradle::GradleBuild,
    name: String,
    mut warnings: Vec<DiscoveryWarning>,
) -> Result<RawPackage, DiscoverError> {
    let manifest = RelPath::new(manifest_path).ok_or_else(|| DiscoverError::Manifest {
        file: manifest_path.to_owned(),
        format: ManifestFormat::GradleBuild,
        message: "manifest path is not a normalized repo-relative path".to_owned(),
        span: None,
    })?;
    let dir = gradle_dir_of(manifest_path);
    // The last `version = …` assignment in document order wins (Gradle's
    // Project-setter overwrite — the documented `toString()` value is the
    // last assignment's; no override *rule*, no superseded warning).
    let last = file.version_assignments.last();
    let (version, site) = if let Some(gradle::VersionAssignment {
        rhs: gradle::VersionRhs::Literal { value, span },
    }) = last
        && let Ok(parsed) = Version::parse(value)
    {
        (
            Some(parsed),
            Some(ManifestSite {
                file: manifest.clone(),
                spans: vec![*span],
            }),
        )
    } else if let Some(gradle::VersionAssignment {
        rhs: gradle::VersionRhs::Literal { value, .. },
    }) = last
    {
        warnings.push(DiscoveryWarning {
            code: "non-canonical-version",
            subject: name.clone(),
            message: format!(
                "the version literal {value:?} is a spelling the plan contract cannot render \
                     (semver is the plan contract's type); package treated as unpublishable and \
                     never rewritten"
            ),
        });
        (None, None)
    } else if let Some(gradle::VersionAssignment {
        rhs: gradle::VersionRhs::NonLiteral,
    }) = last
    {
        warnings.push(DiscoveryWarning {
            code: "non-canonical-version",
            subject: name.clone(),
            message: "the `version = …` row carries a non-literal right-hand side (a \
                          computed value, property reference, or string template) the plan \
                          contract cannot render; package treated as unpublishable and never \
                          rewritten"
                .to_owned(),
        });
        (None, None)
    } else {
        warnings.push(DiscoveryWarning {
            code: "no-version",
            subject: name.clone(),
            message: "no `version = …` assignment in the build file; Gradle's documented \
                          default is `unspecified` — the package is treated as unpublished"
                .to_owned(),
        });
        (None, None)
    };
    Ok(RawPackage {
        name,
        ecosystem: Ecosystem::Maven,
        root: dir,
        manifest: manifest_path.to_owned(),
        version,
        version_source: RawVersionSource::Local,
        // ADR-0058 D5 (continued): no Gradle publish argv arm exists
        // (the credential surface is `~/.gradle/gradle.properties` + the
        // `publishing` block, no documented env-var grammar) — the row
        // versions and tags, and the publish engine records the Maven
        // cell's disabled shape.
        publishable: false,
        deps: Vec::new(),
        tag_template: None,
        changelog: None,
        version_site: site,
        include: halyard_core::package::PathFilter::new(Vec::new(), Vec::new()),
        facets: Facets::default(),
        warnings,
    })
}

/// The repo-relative directory of a Gradle build file
/// (`build.gradle` → the root, `services/api/build.gradle` →
/// `services/api`).
fn gradle_dir_of(manifest_path: &str) -> String {
    manifest_path
        .rsplit_once('/')
        .map_or(String::new(), |(dir, _)| dir.to_owned())
}

/// The repo-relative directory's own name (`services/api` → `api`, the
/// root → `None`) — the identity chain's fallback when no
/// `rootProject.name` governance applies (the `settings` module's
/// grammar provides the provenance when a settings file governs).
fn gradle_context_name(dir: &str) -> Option<&str> {
    dir.rsplit('/').next().filter(|name| !name.is_empty())
}

/// One settings file's governance over its directory subtree (the
/// multi-project aggregator shape, ADR-0058 continued — the
/// `settings.gradle` `rootProject.name` / `include` grammar at the
/// VERIFIED Settings API).
#[derive(Clone, Debug, Default)]
struct GradleSettings {
    /// The settings file's own path (the warnings' subject — the
    /// discovery warnings name a manifest path).
    file: String,
    /// The settings file's directory (the governed tree's root project;
    /// `""` at the repository root).
    dir: String,
    /// The root project's name proven by the last literal
    /// `rootProject.name` assignment (`None` when the last row is
    /// non-literal or no row exists — the documented directory default
    /// governs then).
    root_name: Option<String>,
    /// Whether the LAST `rootProject.name` row was non-literal (the
    /// setter was called with a computed value — warned, unmodeled).
    root_name_nonliteral: bool,
    /// The include-proven members: the directory **relative to the
    /// settings directory** → the project name (the include path's last
    /// element; the implicit parents *"added if they don't already
    /// exist"* ride the same rule). First provenance wins (a duplicate
    /// include adds the same project once).
    members: std::collections::BTreeMap<String, String>,
    /// Whether any include argument was non-literal (recognized but
    /// non-canonical — that member is unproven).
    nonliteral_include: bool,
    /// Both settings spellings exist in this directory: the provenance
    /// source is ambiguous and is declined (fail-closed).
    ambiguous: bool,
}

impl GradleSettings {
    /// Whether this settings file governs `dir` (its own directory or
    /// any descendant — the root project and its members).
    fn governs(&self, dir: &str) -> bool {
        self.dir.is_empty()
            || dir == self.dir
            || dir
                .strip_prefix(&format!("{}/", self.dir))
                .is_some_and(|rest| !rest.is_empty())
    }
}

/// Parses the committed tree's settings files into the governance list
/// (sorted by directory, I3). Both spellings in one directory are the
/// ambiguous case: provenance declined, the ambiguity warns.
fn settings_governance(ctx: &DiscoverCtx) -> Result<Vec<GradleSettings>, DiscoverError> {
    let mut by_dir: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    for path in ctx.tree().filter(|path| {
        **path == *"settings.gradle"
            || **path == *"settings.gradle.kts"
            || path.ends_with("/settings.gradle")
            || path.ends_with("/settings.gradle.kts")
    }) {
        by_dir
            .entry(gradle_dir_of(path))
            .or_default()
            .push((*path).to_owned());
    }
    let mut out = Vec::new();
    for (dir, paths) in by_dir {
        let Some(settings_path) = paths.first() else {
            continue;
        };
        if paths.len() > 1 {
            out.push(GradleSettings {
                file: settings_path.clone(),
                dir,
                ambiguous: true,
                ..GradleSettings::default()
            });
            continue;
        }
        let Some(bytes) = ctx.file(settings_path) else {
            continue;
        };
        let text = std::str::from_utf8(bytes).map_err(|_| DiscoverError::Manifest {
            file: settings_path.clone(),
            format: ManifestFormat::GradleBuild,
            message: "the settings file is not valid UTF-8".to_owned(),
            span: None,
        })?;
        let parsed = settings::parse_settings(text).map_err(|error| DiscoverError::Manifest {
            file: settings_path.clone(),
            format: ManifestFormat::GradleBuild,
            message: error.to_string(),
            span: None,
        })?;
        let mut gov = GradleSettings {
            file: settings_path.clone(),
            dir,
            ..GradleSettings::default()
        };
        // The last `rootProject.name` assignment in document order wins
        // (the ProjectDescriptor setter's ordinary overwrite — the
        // version row's own rule).
        for assignment in &parsed.root_names {
            match &assignment.rhs {
                settings::NameRhs::Literal { value, .. } => {
                    gov.root_name = Some(value.clone());
                    gov.root_name_nonliteral = false;
                }
                settings::NameRhs::NonLiteral => {
                    gov.root_name = None;
                    gov.root_name_nonliteral = true;
                }
            }
        }
        for call in &parsed.includes {
            for arg in &call.args {
                match &arg.shape {
                    settings::IncludeShape::Path { path: include, .. } => {
                        // The documented path grammar: `':'`-separated,
                        // the last element the project name, the
                        // directory relative to the root project
                        // directory, the implicit parents included.
                        // The documented path grammar: `':'`-separated
                        // segments, the LAST element the project name
                        // and every prefix the implicit parent (the
                        // Settings API's own `a:b` example adds `:a`
                        // *"if it doesn't already exist"*). A bare `":"
                        // names the root project itself — no segment,
                        // no member directory to prove.
                        let mut rel_parts: Vec<&str> = Vec::new();
                        for segment in include.split(':').filter(|s| !s.is_empty()) {
                            rel_parts.push(segment);
                            let rel = rel_parts.join("/");
                            gov.members.entry(rel).or_insert_with(|| segment.to_owned());
                        }
                    }
                    settings::IncludeShape::NonLiteral => {
                        gov.nonliteral_include = true;
                    }
                }
            }
        }
        out.push(gov);
    }
    Ok(out)
}

/// The release version the `build.gradle(.kts)` **bytes** carry — the
/// semver-canonical pairing text when the last `version = …` assignment
/// is a literal canonical spelling, `None` otherwise (non-literal or
/// non-semver spellings never pair; the documented HEAD fallback
/// governs). The probe input the tag-target pinpoint walk consumes for
/// a Gradle build file (see `halyard_git::tag_target::ManifestVersionProbe`;
/// the git engine never parses manifests itself).
#[must_use]
pub fn gradle_manifest_version(bytes: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(bytes).ok()?;
    let file = gradle::parse_gradle_build(text).ok()?;
    let last = file.version_assignments.last()?;
    let gradle::VersionRhs::Literal { value, .. } = &last.rhs else {
        return None;
    };
    Version::parse(value).ok().map(|_| value.clone())
}

/// The resolved version of one pom row: the value, its raw-string source
/// and its edit site (the ADR-0056 discipline — canonical only).
#[derive(Clone)]
struct RawVersion {
    /// The parsed semver value (`None` = absent or non-canonical).
    value: Option<Version>,
    /// Where the version comes from (the pom's own row, or inherited).
    source: RawVersionSource,
    /// The edit site (the version value's span), when canonical.
    site: Option<ManifestSite>,
}

/// A member's walked shape (plain data before row assembly).
struct Member {
    /// The parsed pom.
    pom: grammar::Pom,
    /// The member's repo-relative directory ("" = the repository root).
    dir: String,
    /// The member's pom path.
    manifest: String,
    /// The aggregator that listed this member (pom path).
    aggregator: String,
    /// The aggregator's (groupId, artifactId) — the parent-match target.
    parent_coordinates: (String, String),
    /// The aggregator's version (the inheritance source), canonical or
    /// `None`.
    parent_version: Option<Version>,
    /// The aggregator's version-value span (the shared edit site).
    parent_version_site: Option<halyard_core::package::Span>,
    /// The member's own (groupId, artifactId) — groupId inherited from
    /// the aggregator when the member does not redeclare it (the
    /// documented inheritance list).
    coordinates: Option<(String, String)>,
}

/// Walks an aggregator's `<module>` entries (recursively), enforcing the
/// committed-tree, member-requirement and no-double-listing rules.
fn walk_members(
    ctx: &DiscoverCtx,
    aggregator: &grammar::Pom,
    aggregator_manifest: &str,
    walked: &mut Vec<Member>,
    listed: &mut std::collections::BTreeSet<String>,
) -> Result<(), DiscoverError> {
    let coordinates = (
        aggregator
            .group_id
            .clone()
            .ok_or_else(|| missing_coordinates(aggregator_manifest))?,
        aggregator
            .artifact_id
            .clone()
            .ok_or_else(|| missing_coordinates(aggregator_manifest))?,
    );
    // The aggregator's version as the members' inheritance source — the
    // value (canonical or None) plus the shared edit site.
    let (parent_version, parent_version_site) = match &aggregator.version {
        Some(field) => (Version::parse(&field.text).ok(), Some(field.span)),
        None => (None, None),
    };
    for entry in &aggregator.modules {
        let manifest = module_manifest(entry)?;
        if !listed.insert(manifest.clone()) {
            return Err(DiscoverError::Manifest {
                file: aggregator_manifest.to_owned(),
                format: ManifestFormat::Pom,
                message: format!(
                    "<module> {manifest:?} lists a pom another aggregator already claimed — \
                     one pom, one owner (maven's own reactor rule)"
                ),
                span: None,
            });
        }
        let Some(bytes) = ctx.file(&manifest) else {
            return Err(DiscoverError::Glob {
                pattern: manifest.clone(),
                message: "module has no pom.xml in the committed tree (maven: a <module> names                           a relative path to the directory or POM file of the project)"
                    .to_owned(),
            });
        };
        let text = utf8(bytes, &manifest)?;
        let pom = grammar::parse_pom(text, &manifest)?;
        let dir = dir_of(&manifest);
        let member_coordinates = coordinates_of(&pom, &coordinates);
        walked.push(Member {
            pom,
            dir,
            manifest,
            aggregator: aggregator_manifest.to_owned(),
            parent_coordinates: coordinates.clone(),
            parent_version: parent_version.clone(),
            parent_version_site,
            coordinates: member_coordinates,
        });
    }
    // Recursion: a member that itself aggregates is processed after the
    // flat pass (deterministic breadth per aggregator, sorted rows at
    // the end).
    let mut nested: Vec<(grammar::Pom, String)> = Vec::new();
    for member in walked
        .iter()
        .skip(walked.len().saturating_sub(aggregator.modules.len()))
    {
        if !member.pom.modules.is_empty() {
            nested.push((member.pom.clone(), member.manifest.clone()));
        }
    }
    for (pom, manifest) in nested {
        walk_members(ctx, &pom, &manifest, walked, listed)?;
    }
    Ok(())
}

/// The member's coordinates: its own (groupId, artifactId), with the
/// groupId inherited from the aggregator when not redeclared (the
/// documented inheritance list).
fn coordinates_of(pom: &grammar::Pom, parent: &(String, String)) -> Option<(String, String)> {
    let artifact_id = pom.artifact_id.as_ref()?;
    Some((
        pom.group_id.clone().unwrap_or_else(|| parent.0.clone()),
        artifact_id.clone(),
    ))
}

/// Resolves one `<module>` entry to its member pom path (VERIFIED:
/// *"modules, which are relative paths to the directories or the POM
/// files of those projects"* — a directory entry names `<dir>/pom.xml`,
/// a `.xml` entry names the file).
fn module_manifest(entry: &str) -> Result<String, DiscoverError> {
    if entry.is_empty() || entry.starts_with('/') {
        return Err(DiscoverError::Glob {
            pattern: entry.to_owned(),
            message: "an empty or absolute module path cannot name a member in the committed \
                      tree"
                .to_owned(),
        });
    }
    let trimmed = entry.trim_matches('/');
    if trimmed.split('/').any(|c| c == "..") {
        return Err(DiscoverError::Glob {
            pattern: entry.to_owned(),
            message: "a module path outside the committed tree cannot be planned".to_owned(),
        });
    }
    let manifest = if trimmed.to_ascii_lowercase().ends_with(".xml") {
        trimmed.to_owned()
    } else {
        format!("{trimmed}/pom.xml")
    };
    Ok(manifest)
}

fn missing_coordinates(file: &str) -> DiscoverError {
    DiscoverError::Manifest {
        file: file.to_owned(),
        format: ManifestFormat::Pom,
        message: "pom has no <groupId>/<artifactId> — the aggregator's coordinates cannot be \
                  matched and member <parent> blocks cannot be validated"
            .to_owned(),
        span: None,
    }
}

fn rel_path(path: &str) -> Result<RelPath, DiscoverError> {
    RelPath::new(path).ok_or_else(|| DiscoverError::Manifest {
        file: path.to_owned(),
        format: ManifestFormat::Pom,
        message: "manifest path is not a normalized repo-relative path".to_owned(),
        span: None,
    })
}

/// The repo-relative directory of a pom path (`pom.xml` → the root,
/// `api/pom.xml` → `api`).
fn dir_of(manifest_path: &str) -> String {
    if manifest_path == "pom.xml" {
        return String::new();
    }
    manifest_path
        .strip_suffix("/pom.xml")
        .unwrap_or(manifest_path)
        .to_owned()
}

/// The version of a pom's own `<version>` field (canonical semver only —
/// the ADR-0056 discipline).
fn version_of(pom: &grammar::Pom, file: &str) -> Result<RawVersion, DiscoverError> {
    match &pom.version {
        Some(field) => version_from_field(field, file),
        None => Ok(RawVersion {
            value: None,
            source: RawVersionSource::Local,
            site: None,
        }),
    }
}

/// Validates one captured version field: canonical semver pairs with an
/// edit site; every other Maven-legal spelling is warned and unplanable.
fn version_from_field(
    field: &grammar::ValueField,
    file: &str,
) -> Result<RawVersion, DiscoverError> {
    let site = ManifestSite {
        file: rel_path(file)?,
        spans: vec![field.span],
    };
    match Version::parse(&field.text) {
        Ok(value) => Ok(RawVersion {
            value: Some(value),
            source: RawVersionSource::Local,
            site: Some(site),
        }),
        Err(_) => Ok(RawVersion {
            // Recognized-but-non-canonical: warned, unpublishable, no
            // site, never rewritten (the Maven spellings are real —
            // Maven's ComparableVersion orders them — they are just not
            // the plan contract's type).
            value: None,
            source: RawVersionSource::Local,
            site: None,
        }),
    }
}

fn utf8<'a>(bytes: &'a [u8], file: &str) -> Result<&'a str, DiscoverError> {
    std::str::from_utf8(bytes).map_err(|_| DiscoverError::Manifest {
        file: file.to_owned(),
        format: ManifestFormat::Pom,
        message: "manifest is not valid UTF-8".to_owned(),
        span: None,
    })
}

/// Builds one raw package row from a parsed pom (§6.1 merge row); the
/// pom's `<artifactId>` is present by construction (`parse_pom` refuses
/// a pom without one).
fn row(
    pom: &grammar::Pom,
    root: &str,
    manifest_path: &str,
    version: RawVersion,
    member_coordinates: &[(String, String)],
) -> Result<RawPackage, DiscoverError> {
    let Some(artifact_id) = &pom.artifact_id else {
        return Err(missing_coordinates(manifest_path));
    };
    let manifest = rel_path(manifest_path)?;
    let mut warnings: Vec<DiscoveryWarning> = Vec::new();
    for scan in &pom.warnings {
        warnings.push(DiscoveryWarning {
            code: scan.code,
            subject: manifest_path.to_owned(),
            message: scan.message.clone(),
        });
    }
    if version.value.is_none() && pom.version.is_some() {
        warnings.push(DiscoveryWarning {
            code: "non-canonical-version",
            subject: artifact_id.clone(),
            message: format!(
                "version {:?} is a Maven spelling the plan contract cannot render (Maven's \
                 ComparableVersion is not semver); package treated as unpublishable and never \
                 rewritten",
                pom.version.as_ref().map_or("", |v| v.text.as_str())
            ),
        });
    }
    if pom.version.is_none() {
        warnings.push(DiscoveryWarning {
            code: "no-version",
            subject: artifact_id.clone(),
            message: "no <version> in the pom and none inherited; package treated as \
                      unpublished"
                .to_owned(),
        });
    }
    let deps: Vec<RawDep> = pom
        .dependencies
        .iter()
        .filter_map(|dep| {
            dep_row(
                dep,
                &manifest,
                member_coordinates,
                artifact_id,
                &mut warnings,
            )
        })
        .collect();
    Ok(RawPackage {
        name: artifact_id.clone(),
        ecosystem: Ecosystem::Maven,
        root: root.to_owned(),
        manifest: manifest_path.to_owned(),
        version: version.value,
        version_source: version.source,
        // ADR-0058 D5: no Maven publish grammar exists (no `mvn` argv
        // arm, no registry probe, no auth arm) — the row versions and
        // tags, and the publish engine records the disabled shape.
        publishable: false,
        deps,
        tag_template: None,
        changelog: None,
        version_site: version.site,
        include: halyard_core::package::PathFilter::new(Vec::new(), Vec::new()),
        facets: Facets {
            cdylib: false,
            wasm_bindgen: false,
            napi: false,
        },
        warnings,
    })
}

/// Converts one `<dependency>` row (a corrupt row is warned and skipped;
/// a coordinate naming a member is the proven-internal edge).
fn dep_row(
    row: &grammar::DepRow,
    manifest: &RelPath,
    member_coordinates: &[(String, String)],
    owner: &str,
    warnings: &mut Vec<DiscoveryWarning>,
) -> Option<RawDep> {
    let Some(artifact_id) = &row.artifact_id else {
        warnings.push(DiscoveryWarning {
            code: "external-dep",
            subject: owner.to_owned(),
            message: "a <dependency> row without <artifactId> is corrupt and is consumed \
                      unmodeled"
                .to_owned(),
        });
        return None;
    };
    let internal = row
        .group_id
        .as_ref()
        .is_some_and(|group| member_coordinates.contains(&(group.clone(), artifact_id.clone())));
    let (raw, spans) = match &row.version {
        Some(field) => (
            halyard_core::BString::from(field.text.as_str()),
            vec![field.span],
        ),
        // Dependency-managed rows carry no version of their own (the
        // management block is consumed unmodeled): the row rides
        // verbatim with no site to rewrite.
        None => (halyard_core::BString::from(""), Vec::new()),
    };
    Some(RawDep {
        name: artifact_id.clone(),
        // `<scope>test</scope>` is the dev edge (dev edges don't force
        // cascades by default, §6.3); every other scope is a normal edge.
        kind: if row.scope.as_deref() == Some("test") {
            DepKind::Dev
        } else {
            DepKind::Normal
        },
        req: ReqSpec::Other { raw: raw.clone() },
        site: ManifestSite {
            file: manifest.clone(),
            spans,
        },
        raw,
        internal,
    })
}

//! [`EcosystemAdapter`] implementation for the `Docker image tags`
//! ecosystem (M8, the §17 `Docker image tags` cell — the next listed
//! cell after `generic` (ADR-0062), ADR-0063).
//!
//! A docker package is an image manifest: every `Dockerfile` in the
//! committed tree is a standalone package row (the image is the
//! package; the manifest is the Dockerfile — the `RubyGems` cell's
//! per-file shape, ADR-0061). The version is the OCI-documented version
//! annotation (`org.opencontainers.image.version` — *"version of the
//! packaged software"*, *"version MAY be Semantic
//! versioning-compatible"*) carried by the Dockerfile reference's
//! `LABEL` instruction (*"The LABEL instruction adds metadata to an
//! image. A LABEL is a key-value pair."* — the VERIFIED grammar lives
//! in `docs/research/docker-image-tags.md`). A `FROM` row *"sets the
//! base image"* — its tag is the base image's version, never this
//! package's, so it is consumed unmodeled and never an edit site.
//! Docker's own override rule (*"the most-recently-applied value
//! overrides any previously-set value"*) makes the LAST version label
//! the row's site; superseded rows warn.
//!
//! The label value follows the ADR-0056 fail-closed discipline: only
//! semver-parseable values are canonical; every other spelling is
//! recognized-but-non-canonical — **warned, unpublishable, no edit
//! site, never rewritten**.
//!
//! Docker publishing carries no argv arm yet — building and pushing an
//! image spawns the local daemon, a contract no argv grammar exists for
//! — so rows carry `publishable = false` (the engine's disabled shape;
//! ADR-0063 D5); the planner's registry arm names the documented
//! default registry host (`docker.io`: an unqualified `docker pull
//! debian` resolves to `docker.io/library/debian:latest`).
//!
//! There is no dependency grammar: a `FROM`/`COPY` row names base
//! images and build stages, not workspace members — a docker package
//! declares no outgoing edges (cross-package cascade rides the explicit
//! `[[link]]` config surface).

use halyard_core::EcosystemAdapter;
use halyard_core::discover::{
    DiscoverCtx, DiscoverError, DiscoveryWarning, ManifestFormat, RawPackage, RawVersionSource,
};
use halyard_core::package::{Ecosystem, ManifestSite, RelPath};
use semver::Version;

pub mod edit;
pub mod grammar;

/// The OCI annotations key the version rides (the Image Spec's
/// annotations table: *"version of the packaged software"*).
pub const VERSION_LABEL: &str = "org.opencontainers.image.version";
/// The OCI annotations key the identity chain's head rides
/// (*"Human-readable title of the image"*).
pub const TITLE_LABEL: &str = "org.opencontainers.image.title";

/// The docker-family adapter (§7.3 discovery + manifest-edit subset).
#[derive(Clone, Copy, Debug, Default)]
pub struct DockerAdapter;

impl DockerAdapter {
    /// New adapter.
    pub fn new() -> Self {
        Self
    }
}

/// The release version the `Dockerfile` **bytes** carry — the
/// semver-canonical pairing text when the last `org.opencontainers.image.version`
/// label's value is a canonical spelling, `None` otherwise (non-semver
/// spellings never pair; the documented HEAD fallback governs). The
/// probe input the tag-target pinpoint walk consumes for the docker
/// ecosystem (see `halyard_git::tag_target::ManifestVersionProbe`; the
/// git engine never parses manifests itself).
#[must_use]
pub fn manifest_version(bytes: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(bytes).ok()?;
    let file = grammar::parse_dockerfile(text).ok()?;
    let value = version_value(&file.labels)?.value.clone();
    Version::parse(&value).ok().map(|_| value)
}

impl EcosystemAdapter for DockerAdapter {
    fn id(&self) -> &'static str {
        "docker"
    }

    fn discover(&self, ctx: &DiscoverCtx) -> Result<Vec<RawPackage>, DiscoverError> {
        // Every `Dockerfile` in the committed tree (sorted walk order,
        // I3) is a candidate manifest. The documented default build
        // input is the literal `Dockerfile` name (`docker build PATH`
        // reads `PATH/Dockerfile`); suffixed variants are the
        // recorded-open superset (ADR-0063 D6).
        let manifests: Vec<String> = ctx
            .tree()
            .filter(|path| **path == *"Dockerfile" || path.ends_with("/Dockerfile"))
            .map(std::borrow::ToOwned::to_owned)
            .collect();
        let mut packages = Vec::new();
        for path in &manifests {
            let Some(bytes) = ctx.file(path) else {
                continue;
            };
            let text = std::str::from_utf8(bytes).map_err(|_| DiscoverError::Manifest {
                file: path.clone(),
                format: ManifestFormat::Dockerfile,
                message: "manifest is not valid UTF-8".to_owned(),
                span: None,
            })?;
            let file =
                grammar::parse_dockerfile(text).map_err(|error| DiscoverError::Manifest {
                    file: path.clone(),
                    format: ManifestFormat::Dockerfile,
                    message: error.to_string(),
                    span: None,
                })?;
            packages.push(row(path, &file)?);
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

/// The repo-relative directory of a Dockerfile path
/// (`Dockerfile` → the root, `services/api/Dockerfile` →
/// `services/api`).
fn dir_of(manifest_path: &str) -> String {
    manifest_path
        .rsplit_once('/')
        .map_or(String::new(), |(dir, _)| dir.to_owned())
}

/// The repo-relative directory's own name (`services/api` → `api`, the
/// root → `None`).
fn context_name(dir: &str) -> Option<&str> {
    dir.rsplit('/').next().filter(|name| !name.is_empty())
}

/// The LAST version-label pair (Docker's documented override rule: *"If
/// a label already exists but with a different value, the
/// most-recently-applied value overrides any previously-set value"*) —
/// `None` when the manifest carries no version label.
fn version_value(labels: &[grammar::LabelPair]) -> Option<&grammar::LabelPair> {
    labels.iter().rev().find(|pair| pair.key == VERSION_LABEL)
}

/// Builds one raw package row from a parsed Dockerfile (§6.1 merge
/// row).
fn row(manifest_path: &str, file: &grammar::Dockerfile) -> Result<RawPackage, DiscoverError> {
    let manifest = RelPath::new(manifest_path).ok_or_else(|| DiscoverError::Manifest {
        file: manifest_path.to_owned(),
        format: ManifestFormat::Dockerfile,
        message: "manifest path is not a normalized repo-relative path".to_owned(),
        span: None,
    })?;
    let mut warnings: Vec<DiscoveryWarning> = Vec::new();
    // The identity chain (ADR-0063 D2): the OCI title label → the
    // containing directory's name (the documented build context) → the
    // manifest path. A collision is the orchestrator's per-ecosystem
    // duplicate refusal (`E-DISC-DUPLICATE`).
    let title = file
        .labels
        .iter()
        .find(|pair| pair.key == TITLE_LABEL)
        .map(|pair| pair.value.clone());
    let name =
        title.or_else(|| context_name(&dir_of(manifest_path)).map(std::borrow::ToOwned::to_owned));
    let name = name.unwrap_or_else(|| manifest_path.to_owned());
    // The version row: the LAST version label is the site; superseded
    // rows warn (the override rule is Docker's own semantics — modeled,
    // not guessed).
    let version_pairs = file
        .labels
        .iter()
        .filter(|pair| pair.key == VERSION_LABEL)
        .collect::<Vec<_>>();
    if let [superseded, mid @ .., _last] = version_pairs.as_slice()
        && !mid.is_empty()
    {
        warnings.push(DiscoveryWarning {
            code: "superseded-version-label",
            subject: name.clone(),
            message: format!(
                "an earlier {VERSION_LABEL} row carrying {:?} is superseded; the \
                 most-recently-applied value overrides any previously-set value (the \
                 documented LABEL override rule) — the last row is the edit site",
                superseded.value
            ),
        });
    }
    let last = version_value(&file.labels);
    let (version, site) = if let Some(pair) = last
        && let Ok(value) = Version::parse(&pair.value)
    {
        (
            Some(value),
            Some(ManifestSite {
                file: manifest.clone(),
                spans: vec![pair.span],
            }),
        )
    } else if let Some(pair) = last {
        warnings.push(DiscoveryWarning {
            code: "non-canonical-version",
            subject: name.clone(),
            message: format!(
                "the version label {:?} is a spelling the plan contract cannot render \
                 (semver is the plan contract's type); package treated as \
                 unpublishable and never rewritten",
                pair.value
            ),
        });
        (None, None)
    } else {
        warnings.push(DiscoveryWarning {
            code: "no-version",
            subject: name.clone(),
            message: format!(
                "no {VERSION_LABEL} label row; no version is discoverable, the package \
                 is treated as unpublished"
            ),
        });
        (None, None)
    };
    Ok(RawPackage {
        name,
        ecosystem: Ecosystem::Docker,
        root: dir_of(manifest_path),
        manifest: manifest_path.to_owned(),
        version,
        version_source: RawVersionSource::Local,
        // ADR-0063 D5: no docker publish argv arm exists (building and
        // pushing an image spawns the local daemon) — the row versions
        // and tags, and the publish engine records the disabled shape.
        publishable: false,
        deps: Vec::new(),
        tag_template: None,
        changelog: None,
        version_site: site,
        include: halyard_core::package::PathFilter::new(Vec::new(), Vec::new()),
        facets: halyard_core::Facets::default(),
        warnings,
    })
}

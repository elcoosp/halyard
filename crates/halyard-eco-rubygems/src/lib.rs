//! [`EcosystemAdapter`] implementation for `RubyGems` (M8, the §17
//! `RubyGems` cell — the next listed cell after `Dart/pub` (ADR-0060),
//! ADR-0061).
//!
//! Parses `.gemspec` files with the hand-rolled scanner of [`grammar`]
//! at the VERIFIED reference grammar
//! (`docs/research/rubygems-grammars.md`, from
//! `guides.rubygems.org/specification-reference/` — the
//! `Gem::Specification.new do |s|` block shape, the required attribute
//! list and *"Specifications must be deterministic"* — plus
//! `lib/rubygems/version.rb`'s `Gem::Version` pattern). Versions are
//! manifest rows — but only semver-parseable spellings are canonical
//! (the ADR-0056 discipline): *"Versions shall be represented by three
//! non-negative integers, separated by periods (e.g. 3.1.4)."* Every
//! gem-legal spelling the plan contract's semver cannot render (a
//! two-component `1.2`, the four-segment `1.2.3.1`) — and every
//! computed version the *"must be deterministic"* rule forbids — is
//! recognized-but-non-canonical: **warned, unpublishable, no edit site,
//! never rewritten**.
//!
//! `RubyGems` carries no workspace grammar: every `*.gemspec` in the
//! committed tree is a standalone package row (the gem is the package;
//! the manifest is the gemspec file). The internal edges are the
//! name-proven ones: an `add_dependency` whose gem name matches another
//! discovered gemspec — when both gems ship from one repository, the
//! requirement targets the same artifact the repo publishes, so the
//! edge drives the cascade. Every requirement string (`>= 1.1.4`, the
//! pessimistic `~>`) is a foreign grammar and rides verbatim (ADR-0006).
//!
//! `RubyGems` publishing carries no argv arm yet — the documented flow
//! pushes a *built* `.gem` (`gem push squid-utils-0.1.0.gem`) whose
//! credential surface is the local `~/.gem/credentials` file — so rows
//! carry `publishable = false` (the engine's disabled shape; ADR-0061
//! D5).

use halyard_core::EcosystemAdapter;
use halyard_core::discover::{
    DiscoverCtx, DiscoverError, DiscoveryWarning, Facets, ManifestFormat, RawDep, RawPackage,
    RawVersionSource,
};
use halyard_core::package::{DepKind, Ecosystem, ManifestSite, RelPath, ReqSpec};
use semver::Version;

pub mod edit;
pub mod grammar;

/// The rubygems-family adapter (§7.3 discovery + manifest-edit subset).
#[derive(Clone, Copy, Debug, Default)]
pub struct RubyGemsAdapter;

impl RubyGemsAdapter {
    /// New adapter.
    pub fn new() -> Self {
        Self
    }
}

/// The release version the `.gemspec` **bytes** carry — the
/// semver-canonical pairing text when the gemspec's `version` row is a
/// canonical spelling, `None` otherwise (non-semver `Gem::Version`
/// spellings never pair; the documented HEAD fallback governs). The
/// probe input the tag-target pinpoint walk consumes for the rubygems
/// ecosystem (see `halyard_git::tag_target::ManifestVersionProbe`; the
/// git engine never parses manifests itself).
#[must_use]
pub fn manifest_version(bytes: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(bytes).ok()?;
    let gemspec = grammar::parse_gemspec(text).ok()?;
    let version = gemspec.version?;
    Version::parse(&version.text).ok().map(|_| version.text)
}

impl EcosystemAdapter for RubyGemsAdapter {
    fn id(&self) -> &'static str {
        "rubygems"
    }

    fn discover(&self, ctx: &DiscoverCtx) -> Result<Vec<RawPackage>, DiscoverError> {
        // Every `.gemspec` in the committed tree (sorted walk order,
        // I3) is a candidate manifest.
        let manifests: Vec<String> = ctx
            .tree()
            .filter(|path| path.ends_with(".gemspec"))
            .map(std::borrow::ToOwned::to_owned)
            .collect();
        let mut parsed: Vec<(String, grammar::Gemspec)> = Vec::new();
        for path in &manifests {
            let Some(bytes) = ctx.file(path) else {
                continue;
            };
            let text = utf8(bytes, path)?;
            let gemspec =
                grammar::parse_gemspec(text).map_err(|error| DiscoverError::Manifest {
                    file: path.clone(),
                    format: ManifestFormat::Gemspec,
                    message: error.to_string(),
                    span: None,
                })?;
            parsed.push((path.clone(), gemspec));
        }
        // The name-proven edge set: every discovered gemspec's identity.
        let names: std::collections::BTreeSet<String> = parsed
            .iter()
            .filter_map(|(_, gemspec)| gemspec.name.as_ref().map(|name| name.text.clone()))
            .collect();
        let mut packages = Vec::new();
        for (path, gemspec) in &parsed {
            let Some(name) = &gemspec.name else {
                return Err(DiscoverError::Manifest {
                    file: path.clone(),
                    format: ManifestFormat::Gemspec,
                    message: "the gemspec has no `name` attribute row — the identity has no \
                              documented default (the specification reference lists `name` \
                              among the required gemspec attributes)"
                        .to_owned(),
                    span: None,
                });
            };
            packages.push(row(path, name, gemspec, &names)?);
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

fn utf8<'a>(bytes: &'a [u8], file: &str) -> Result<&'a str, DiscoverError> {
    std::str::from_utf8(bytes).map_err(|_| DiscoverError::Manifest {
        file: file.to_owned(),
        format: ManifestFormat::Gemspec,
        message: "manifest is not valid UTF-8".to_owned(),
        span: None,
    })
}

/// The repo-relative directory of a gemspec path
/// (`app.gemspec` → the root, `gems/app/app.gemspec` → `gems/app`).
fn dir_of(manifest_path: &str) -> String {
    manifest_path
        .rsplit_once('/')
        .map_or(String::new(), |(dir, _)| dir.to_owned())
}

/// Builds one raw package row from a parsed gemspec (§6.1 merge row);
/// the gemspec's `name` row is present by construction (the caller
/// refuses a nameless gemspec before assembling).
fn row(
    manifest_path: &str,
    name: &grammar::ValueField,
    gemspec: &grammar::Gemspec,
    names: &std::collections::BTreeSet<String>,
) -> Result<RawPackage, DiscoverError> {
    let manifest = RelPath::new(manifest_path).ok_or_else(|| DiscoverError::Manifest {
        file: manifest_path.to_owned(),
        format: ManifestFormat::Gemspec,
        message: "manifest path is not a normalized repo-relative path".to_owned(),
        span: None,
    })?;
    let mut warnings: Vec<DiscoveryWarning> = Vec::new();
    for scan in &gemspec.warnings {
        warnings.push(DiscoveryWarning {
            code: scan.code,
            subject: manifest_path.to_owned(),
            message: scan.message.clone(),
        });
    }
    let (version, site) = if let Some(field) = &gemspec.version {
        if let Ok(value) = Version::parse(&field.text) {
            (
                Some(value),
                Some(ManifestSite {
                    file: manifest.clone(),
                    spans: vec![field.span],
                }),
            )
        } else {
            warnings.push(DiscoveryWarning {
                code: "non-canonical-version",
                subject: name.text.clone(),
                message: format!(
                    "version {:?} is a Gem::Version spelling the plan contract cannot render \
                     (semver is the plan contract's type); package treated as unpublishable \
                     and never rewritten",
                    field.text
                ),
            });
            (None, None)
        }
    } else if !gemspec.version_row_present {
        warnings.push(DiscoveryWarning {
            code: "no-version",
            subject: name.text.clone(),
            message: "no `version` attribute row; package treated as unpublished".to_owned(),
        });
        (None, None)
    } else {
        (None, None)
    };
    let deps: Vec<RawDep> = gemspec
        .deps
        .iter()
        .map(|dep| dep_row(dep, DepKind::Normal, &manifest, name, names))
        .chain(
            gemspec
                .dev_deps
                .iter()
                .map(|dep| dep_row(dep, DepKind::Dev, &manifest, name, names)),
        )
        .collect();
    Ok(RawPackage {
        name: name.text.clone(),
        ecosystem: Ecosystem::RubyGems,
        root: dir_of(manifest_path),
        manifest: manifest_path.to_owned(),
        version,
        version_source: RawVersionSource::Local,
        // ADR-0061 D5: no RubyGems publish argv arm exists (publishing
        // pushes a built `.gem` and the documented credential surface is
        // the local `~/.gem/credentials` file) — the row versions and
        // tags, and the publish engine records the disabled shape.
        publishable: false,
        deps,
        tag_template: None,
        changelog: None,
        version_site: site,
        include: halyard_core::package::PathFilter::new(Vec::new(), Vec::new()),
        facets: Facets::default(),
        warnings,
    })
}

/// Converts one `add_*` row (a name matching another discovered gemspec
/// is the proven-internal edge; the gem's own name never self-edges —
/// the requirement resolves against the published artifact).
fn dep_row(
    dep: &grammar::GemDep,
    kind: DepKind,
    manifest: &RelPath,
    owner: &grammar::ValueField,
    names: &std::collections::BTreeSet<String>,
) -> RawDep {
    let internal = dep.name != owner.text && names.contains(&dep.name);
    let raw: Vec<String> = dep
        .requirements
        .iter()
        .map(|requirement| requirement.text.clone())
        .collect();
    let spans: Vec<halyard_core::package::Span> = dep
        .requirements
        .iter()
        .map(|requirement| requirement.span)
        .collect();
    let joined = raw.join(", ");
    RawDep {
        name: dep.name.clone(),
        // `add_development_dependency` is the dev edge (*"Development
        // dependencies aren't installed by default"* — dev edges don't
        // force cascades by default, §6.3); the runtime forms are
        // normal edges.
        kind,
        req: ReqSpec::Other {
            raw: joined.clone().into(),
        },
        site: ManifestSite {
            file: manifest.clone(),
            spans,
        },
        raw: joined.into(),
        internal,
    }
}

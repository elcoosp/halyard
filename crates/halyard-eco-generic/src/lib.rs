//! [`EcosystemAdapter`] implementation for the `generic` ecosystem (M8,
//! the §17 `generic` cell — the next listed cell after `RubyGems`
//! (ADR-0061), ADR-0062).
//!
//! A `generic` package is a package whose version lives in an arbitrary
//! text file located by a user-declared regular expression — the plan's
//! escape hatch for repositories no manifest grammar covers (`VERSION`
//! files, `include/version.h` headers, script constants). There is no
//! fixed grammar to scan for: the packages exist because `halyard.toml`
//! declares them (`[[generic]]`: `name`/`file`/`pattern`), and the
//! locator grammar rides the VERIFIED prior art
//! (`docs/research/generic-version-files.md` — Knope's named capture
//! group: *"The regex pattern must include a named capture group called
//! version … This capture group identifies the version number to
//! replace."*; *"The matched string must be a valid semantic version"*).
//!
//! The captured text follows the ADR-0056 fail-closed discipline: only
//! semver-parseable captures are canonical; every other spelling is
//! recognized-but-non-canonical — **warned, unpublishable, no edit
//! site, never rewritten**. A `generic` release carries **no publish
//! decision** (`publish: None`, the ADR-0057 Go shape minus
//! tag-only-ness): the plan names no registry for the generic
//! ecosystem, and inventing one is precisely what the escape hatch must
//! not do — the tag is the artifact and the user's own pipeline owns
//! publishing.
//!
//! There is no dependency grammar: a generic package declares no
//! outgoing edges (whatever depends on it is invisible to the scan) —
//! cross-package cascade rides the explicit `[[link]]` config surface,
//! which this cell's wire-up feeds into discovery for the first time.

use halyard_core::EcosystemAdapter;
use halyard_core::discover::{
    DiscoverCtx, DiscoverError, DiscoveryWarning, GenericFileSpec, ManifestFormat, RawPackage,
    RawVersionSource,
};
use halyard_core::package::{Ecosystem, ManifestSite, RelPath};
use semver::Version;

pub mod edit;

/// The `generic` adapter (§7.3 discovery + manifest-edit subset) over
/// the config-declared package set. Not a unit type: the declarations
/// ARE the state (discovery is config-driven, not tree-scanned).
#[derive(Clone, Debug, Default)]
pub struct GenericAdapter {
    specs: Vec<GenericFileSpec>,
}

impl GenericAdapter {
    /// New adapter over the config-declared packages (the loader has
    /// validated the pattern grammar — `E-CFG-GENERIC`; the adapter
    /// still compiles fail-closed, never guesses).
    #[must_use]
    pub fn new(specs: &[GenericFileSpec]) -> Self {
        Self {
            specs: specs.to_vec(),
        }
    }
}

/// The release version the declared pattern's named `version` capture
/// group carries in `bytes` — the semver-canonical pairing text when the
/// capture parses, `None` otherwise (non-semver spellings never pair; the
/// documented HEAD fallback governs). The probe input the tag-target
/// pinpoint walk consumes for the generic ecosystem (see
/// `halyard_git::tag_target::ManifestVersionProbe`; the git engine never
/// parses files itself).
#[must_use]
pub fn capture_version(pattern: &str, bytes: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(bytes).ok()?;
    let compiled = regex::Regex::new(pattern).ok()?;
    let group = compiled.captures(text)?.name("version")?;
    Version::parse(group.as_str())
        .ok()
        .map(|_| group.as_str().to_owned())
}

impl EcosystemAdapter for GenericAdapter {
    fn id(&self) -> &'static str {
        "generic"
    }
    fn discover(&self, ctx: &DiscoverCtx) -> Result<Vec<RawPackage>, DiscoverError> {
        // Declaration order is the row order (I3): the config layer
        // delivers the specs sorted, the adapter preserves the order.
        self.specs.iter().map(|spec| row(spec, ctx)).collect()
    }

    fn edit_versions(
        &self,
        file: &RelPath,
        content: &[u8],
        patches: &[halyard_core::plan::PlanPatch],
    ) -> Result<halyard_core::edit::EditOutcome, halyard_core::edit::EditFailure> {
        let spec = self.specs.iter().find(|spec| spec.file == file.as_str());
        edit::edit_versions(spec, file, content, patches)
    }
}

/// Builds one raw package row from one declaration (§6.1 merge row).
fn row(spec: &GenericFileSpec, ctx: &DiscoverCtx) -> Result<RawPackage, DiscoverError> {
    let manifest = RelPath::new(&spec.file).ok_or_else(|| DiscoverError::Glob {
        pattern: spec.file.clone(),
        message: "the declared version file is not a normalized repo-relative path".to_owned(),
    })?;
    let Some(bytes) = ctx.file(&spec.file) else {
        // The declared file missing from the committed tree is the same
        // member-missing-from-tree refusal the dart adapter pinned
        // (`E-DISC-GLOB`): the config names a file discovery cannot see.
        return Err(DiscoverError::Glob {
            pattern: spec.file.clone(),
            message: format!(
                "the declared version file for package {:?} is not in the committed tree",
                spec.name
            ),
        });
    };
    let text = std::str::from_utf8(bytes).map_err(|_| DiscoverError::Manifest {
        file: spec.file.clone(),
        format: ManifestFormat::Text,
        message: "the declared version file is not valid UTF-8 (the regex locator matches \
                  Unicode haystacks)"
            .to_owned(),
        span: None,
    })?;
    let pattern = regex::Regex::new(&spec.pattern).map_err(|error| {
        // Unreachable through the validated loader (E-CFG-GENERIC compiles
        // every pattern); fail closed instead of guessing.
        DiscoverError::Manifest {
            file: spec.file.clone(),
            format: ManifestFormat::Text,
            message: format!("the declared pattern does not compile: {error}"),
            span: None,
        }
    })?;
    let (version, site, warnings) = scan_version(spec, &pattern, text, &manifest)?;
    Ok(RawPackage {
        name: spec.name.clone(),
        ecosystem: Ecosystem::Generic,
        root: dir_of(&spec.file),
        manifest: spec.file.clone(),
        version,
        version_source: RawVersionSource::Local,
        // ADR-0062 D3: no publish decision exists for the generic
        // ecosystem — there is no registry name to record, the tag is
        // the artifact, the user's own pipeline owns publishing.
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

/// The byte offset of `at` as a plan-contract span coordinate.
fn u32_span(at: usize, spec: &GenericFileSpec) -> Result<u32, DiscoverError> {
    u32::try_from(at).map_err(|_| DiscoverError::Manifest {
        file: spec.file.clone(),
        format: ManifestFormat::Text,
        message: "the version file exceeds the span coordinate range".to_owned(),
        span: None,
    })
}

/// One resolved version row: the semver-canonical version, its edit
/// site, and the scan's warnings.
type ScanVersion =
    Result<(Option<Version>, Option<ManifestSite>, Vec<DiscoveryWarning>), DiscoverError>;

/// Resolves the declared file's version row: the leftmost match's named
/// `version` capture (the ADR-0056 discipline governs the captured text)
/// plus the row's warnings. A second match is consumed unmodeled (A2);
/// zero matches warn `no-version`; a non-semver capture warns
/// `non-canonical-version` and carries no edit site — never rewritten.
fn scan_version(
    spec: &GenericFileSpec,
    pattern: &regex::Regex,
    text: &str,
    manifest: &RelPath,
) -> ScanVersion {
    let mut warnings: Vec<DiscoveryWarning> = Vec::new();
    let mut captures = pattern.captures_iter(text);
    let first = captures.next();
    if captures.next().is_some() {
        warnings.push(DiscoveryWarning {
            code: "unmodeled-match",
            subject: spec.name.clone(),
            message: format!(
                "the pattern matches more than once in {}; the leftmost match is the version \
                 row, further matches are consumed unmodeled (the multiple-match grammar is \
                 the recorded-open superset, ADR-0062 A2)",
                spec.file
            ),
        });
    }
    let capture = first.as_ref().and_then(|caps| caps.name("version"));
    if first.is_some() && capture.is_none() {
        // Unreachable through the validated loader (exactly one named
        // `version` group is the load-time contract); fail closed.
        return Err(DiscoverError::Manifest {
            file: spec.file.clone(),
            format: ManifestFormat::Text,
            message: "the declared pattern carries no named `version` capture group".to_owned(),
            span: None,
        });
    }
    let Some(capture) = capture else {
        warnings.push(DiscoveryWarning {
            code: "no-version",
            subject: spec.name.clone(),
            message: format!(
                "the pattern matches nothing in {}; no version is discoverable, the \
                 package is treated as unpublished",
                spec.file
            ),
        });
        return Ok((None, None, warnings));
    };
    let Ok(value) = Version::parse(capture.as_str()) else {
        warnings.push(DiscoveryWarning {
            code: "non-canonical-version",
            subject: spec.name.clone(),
            message: format!(
                "the captured version {:?} is a spelling the plan contract cannot render \
                 (semver is the plan contract's type); package treated as unpublishable \
                 and never rewritten",
                capture.as_str()
            ),
        });
        return Ok((None, None, warnings));
    };
    let site = Some(ManifestSite {
        file: manifest.clone(),
        spans: vec![halyard_core::package::Span {
            start: u32_span(capture.start(), spec)?,
            end: u32_span(capture.end(), spec)?,
        }],
    });
    Ok((Some(value), site, warnings))
}

/// The repo-relative directory of a version-file path
/// (`VERSION` → the root, `src/version.rs` → `src`).
fn dir_of(manifest_path: &str) -> String {
    manifest_path
        .rsplit_once('/')
        .map_or(String::new(), |(dir, _)| dir.to_owned())
}

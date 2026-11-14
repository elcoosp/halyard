//! The §11.1 step-2 **stateless reconciliation** (manifest-vs-tags): the
//! pure decision behind the Release-PR state machine's tag phase.
//!
//! The machine ([`halyard_forge`], §11.1) consumes an already-reconciled
//! pending list — "the caller owns everything the machine cannot see".
//! This module owns that decision as a total function over the
//! discovered [`Workspace`] and the repository's tag names, so the shell
//! (`halyard pr --dry`, the live `release`) and the forecast report the
//! same reconciliation (invariant I3: no hidden state, the manifest and
//! the tags are the only inputs — stateless reconciliation is the
//! architecture's self-healing primitive, §1.2/§11.1).
//!
//! Classification per versioned package (§11.1 step 2):
//!
//! * the manifest version renders through the package's
//!   [`TagTemplate`]; the package's latest tag version is the highest
//!   version any matching tag carries ([`TagTemplate::extract_version`]);
//! * **tagged** — a matching tag already carries the manifest version:
//!   the release already happened, the re-run is a no-op (I1);
//! * **pending** — no matching tag carries the manifest version and it
//!   is not below the latest tag: the tag phase owes the release (the
//!   state after merging a Release PR, or after a partial failure);
//! * **behind** — the manifest version is below the latest tag (a
//!   reverted merge): reported for visibility, never auto-tagged — the
//!   reconciliation never tags downward.
//!
//! Packages without a manifest version (unpublishable, §6.3) have
//! nothing to reconcile and are skipped. The output is sorted by package
//! name, so neither discovery order nor tag scan order can drift it (I3).

use crate::BString;
use crate::package::{Package, Workspace};
use crate::planner::{PlanConfig, effective_template};
use semver::Version;

/// Where a package's manifest version stands relative to its tags
/// (§11.1 step 2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TagState {
    /// A matching tag already carries the manifest version — the release
    /// exists, the re-run is a no-op (I1).
    Tagged,
    /// No matching tag carries the manifest version and it is not below
    /// the latest tag — the tag phase owes this release.
    Pending,
    /// The manifest version is below the latest tag (a reverted merge):
    /// an anomaly the report shows, never auto-tagged.
    Behind,
}

/// One package's reconciliation row ([`reconcile_tags`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageTagState {
    /// Package name as discovered.
    pub package: String,
    /// The version the manifest carries right now.
    pub manifest_version: Version,
    /// The tag the manifest version renders to (the tag the tag phase
    /// would create when the row is pending).
    pub tag: String,
    /// The package's latest matching tag name, when any tag matched its
    /// template (`None` = no release ever, initial state §8.2).
    pub latest_tag: Option<String>,
    /// The version parsed from [`PackageTagState::latest_tag`].
    pub latest_version: Option<Version>,
    /// The classification (§11.1 step 2).
    pub state: TagState,
}

/// One release the tag phase still owes (§11.1 step 2): the shape the
/// forge machine's pending queue consumes — one forge release is keyed
/// by its tag (§11.4, GET-by-tag-first), so two packages rendering the
/// same tag produce one entry (the first in package-name order).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingRelease {
    /// Package name as discovered (the first package in name order
    /// among those sharing the rendered tag).
    pub package: String,
    /// The released version (the manifest version).
    pub version: Version,
    /// The rendered tag (`create_release`'s key, §11.4).
    pub tag: String,
}

/// Reconciles every release-enabled package in `ws` against `tags`
/// (§11.1 step 2). `tags` are the repository's short tag names (e.g.
/// `core-v0.4.1`, `refs/tags/` stripped, as
/// [`halyard_git::refs::enumerate`] yields them). `cfg` carries the
/// config tag-style override and the `release = false` set — the
/// resolution ([`crate::planner::effective_template`]) and the skip set
/// are the planner's own, so the tag phase and the plan can never
/// disagree about a tag's name or a skipped package. The rows are
/// sorted by package name; version-less packages are skipped.
#[must_use]
pub fn reconcile_tags(ws: &Workspace, tags: &[BString], cfg: &PlanConfig) -> Vec<PackageTagState> {
    let mut rows: Vec<PackageTagState> = Vec::new();
    for pkg in ws.packages() {
        let Some(state) = reconcile_package(pkg, ws, tags, cfg) else {
            continue;
        };
        rows.push(state);
    }
    rows.sort_by(|a, b| a.package.cmp(&b.package));
    rows
}

/// The pending subset of [`reconcile_tags`] in the machine-input shape
/// ([`PendingRelease`]), deduplicated by rendered tag and sorted by tag
/// name — the deterministic order the tag phase creates releases in.
#[must_use]
pub fn pending_releases(ws: &Workspace, tags: &[BString], cfg: &PlanConfig) -> Vec<PendingRelease> {
    let mut pending: Vec<PendingRelease> = reconcile_tags(ws, tags, cfg)
        .into_iter()
        .filter(|row| row.state == TagState::Pending)
        .map(|row| PendingRelease {
            package: row.package,
            version: row.manifest_version,
            tag: row.tag,
        })
        .collect();
    pending.sort_by(|a, b| a.tag.cmp(&b.tag));
    pending.dedup_by(|a, b| a.tag == b.tag);
    pending
}

/// Classifies one package (the per-package step of §11.1 step 2: render
/// the manifest tag, take the highest version any matching tag carries,
/// compare). `None` = the package has no manifest version (§6.3) or the
/// config excludes it from releases (`release = false`, the planner's
/// skip set).
fn reconcile_package(
    pkg: &Package,
    ws: &Workspace,
    tags: &[BString],
    cfg: &PlanConfig,
) -> Option<PackageTagState> {
    let manifest_version = pkg.version.clone()?;
    let name = ws.interner().get(pkg.name).unwrap_or_default();
    if cfg.release_disabled.contains(name) {
        return None;
    }
    let eco = pkg.ecosystem.resolve(ws.interner());
    // The planner's own template resolution (appendix A.3) — the tag the
    // plan would carry is the tag the tag phase owes.
    let template = effective_template(ws, pkg, cfg);
    // One pass over the tags for this package's template (§8.2 step 2):
    // highest version wins under the same derived `semver::Version`
    // ordering the base resolution sorts by (§8.2, `refs::resolve_bases`),
    // so the planner and the tag phase can never disagree about what is
    // released. Equal versions tie-break by tag name — unreachable for a
    // single template in practice (distinct tag strings yield distinct
    // derived versions) but kept as determinism armor mirroring
    // `refs.rs`'s candidate sort (I3).
    let mut latest: Option<(Version, String)> = None;
    for tag in tags {
        let tag_name = std::str::from_utf8(tag).unwrap_or_default();
        let Some(version) = template.extract_version(tag_name, name, eco) else {
            continue;
        };
        let better = match &latest {
            None => true,
            Some((best, best_name)) => {
                version > *best || (version == *best && tag_name < best_name.as_str())
            }
        };
        if better {
            latest = Some((version, tag_name.to_owned()));
        }
    }
    let tag = template.render(name, &manifest_version, eco);
    let state = match &latest {
        Some((latest_version, _)) if *latest_version == manifest_version => TagState::Tagged,
        Some((latest_version, _)) if *latest_version > manifest_version => TagState::Behind,
        // No tag ever matched (the initial state) or the manifest
        // version is ahead of the latest tag: the tag phase owes the
        // manifest version (§11.1 "no tag AND V > latest_tag_version").
        _ => TagState::Pending,
    };
    let (latest_version, latest_tag) = match latest {
        Some((version, tag)) => (Some(version), Some(tag)),
        None => (None, None),
    };
    Some(PackageTagState {
        package: name.to_owned(),
        manifest_version,
        tag,
        latest_tag,
        latest_version,
        state,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::{Ecosystem, GroupId, PathFilter, RelPath, TagTemplate, VersionSource};
    use crate::planner::PlanConfig;

    /// Registers one package with the given name, version and template.
    fn push_pkg(ws: &mut Workspace, name: &str, version: &str, template: &str) {
        let pkg = Package {
            id: crate::PkgId::FIRST,
            name: ws.intern(name),
            ecosystem: Ecosystem::Cargo,
            root: RelPath::new("crates/x").expect("valid root"),
            manifest: RelPath::new("crates/x/Cargo.toml").expect("valid manifest"),
            version: Some(Version::parse(version).expect("valid version")),
            version_source: VersionSource::Local,
            publishable: true,
            deps: Vec::new(),
            tag_template: TagTemplate::parse(template).expect("valid template"),
            changelog: None,
            version_site: None,
            include: PathFilter::new(Vec::new(), Vec::new()),
            group: GroupId::new(0),
            facets: crate::discover::Facets::default(),
        };
        ws.push_package(pkg);
    }

    #[test]
    fn version_ordering_matches_base_resolution() {
        // The reconciliation compares versions with the same derived
        // `semver::Version` ordering the §8.2 base resolution sorts by
        // (build metadata included, unlike SemVer precedence) — manifest
        // 0.1.0 under an existing `v0.1.0+build` tag is Behind, never a
        // re-tag. Divergent orderings here would let the planner and the
        // tag phase disagree about what is released.
        let mut ws = Workspace::new();
        push_pkg(&mut ws, "solo", "0.1.0", "v{version}");
        let tags = vec![BString::from("v0.1.0+build")];
        let row = reconcile_tags(&ws, &tags, &PlanConfig::default_values())
            .into_iter()
            .next()
            .expect("one row");
        assert_eq!(row.state, TagState::Behind);
        assert_eq!(row.latest_tag.as_deref(), Some("v0.1.0+build"));
    }

    #[test]
    fn non_utf8_tag_names_never_match() {
        // A non-UTF-8 tag name cannot match a UTF-8 template — it is
        // skipped, never a silent empty-string match.
        let mut ws = Workspace::new();
        push_pkg(&mut ws, "solo", "0.1.0", "v{version}");
        let tags = vec![BString::from(&[0xff, 0xfe][..])];
        let row = reconcile_tags(&ws, &tags, &PlanConfig::default_values())
            .into_iter()
            .next()
            .expect("one row");
        assert_eq!(row.state, TagState::Pending);
        assert_eq!(row.latest_tag, None);
    }
}

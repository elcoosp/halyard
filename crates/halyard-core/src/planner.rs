//! The pure planner (M1.9, §4, appendix A.1): `plan(snapshot, cfg) -> Plan`.
//!
//! Everything here is a function of the snapshot's data — **no I/O, no
//! clocks, no subprocess, no environment reads** (invariants I3/I5/I6).
//! The input is a [`Snapshot`]: the discovered [`Workspace`], a
//! pre-materialized [`HistoryProvider`] (the digest was built by the git
//! engine in the imperative shell) and the resolved [`ReleaseRefs`].
//!
//! Algorithm (appendix A.1):
//!
//! 1. **Direct bumps** — per package, the strongest bump implied by its
//!    conventional commits (configurable type mapping, breaking → major,
//!    0.x remap via [`apply_bump`]). A
//!    `Release-As: <version>` trailer forces the version outright (§10.1
//!    precedence).
//! 2. **Group normalization** — `Fixed`/`Linked` policies and lockstep
//!    polyglot links raise every member to the union's strongest bump
//!    (`Reason::Cascade` with [`CascadeWhy::LockstepGroup`]).
//! 3. **Cascade** — dependency-order fixpoint (bounded by `|V|`
//!    iterations, monotone by construction): a dependent of a bumped
//!    package gains `Patch` when its requirement no longer admits the new
//!    version or when `cascade = "always"` fires for a normal edge of a
//!    publishable crate.
//! 4. **Rewrites + materialization** — unsatisfied requirements are
//!    rewritten through [`rewrite_requirement`]
//!    (appendix A.2), then versions, tags, changelog previews, byte edits,
//!    publish decisions and the skipped list are emitted; the document is
//!    hashed ([`plan_hash`](crate::canonical::plan_hash)).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use semver::Version;

use crate::bump::{
    PreMajorPolicy, PrereleaseChannel, advance_prerelease, apply_bump, promote_to_stable,
    same_triple,
};
use crate::change_file::{ChangeFiles, ChangeNote};
use crate::graph::WorkspaceGraph;
use crate::history::{HistoryDigest, HistoryProvider, ReleaseRefs};
use crate::id::PkgId;
use crate::link::LinkMode;
use crate::package::{
    DepEdge, DepKind, Ecosystem, GroupPolicy, ManifestSite, Package, ReqSpec, TagTemplate,
    Workspace,
};
use crate::plan::{
    Bump, PLAN_SCHEMA_V1, Plan, PlanChangelog, PlanEdit, PlanPatch, PlanPublish, PlanReason,
    PlanRelease, PlanSkip,
};
use crate::reason::{CascadeWhy, Reason};
use crate::rewrite::{RewriteOutcome, UpperBoundPolicy, rewrite_requirement};

/// How the planner treats workspace-wide grouping (§5.4, resolved from
/// `[versioning].policy` by `halyard-config`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GroupMode {
    /// Every package versions on its own (default).
    #[default]
    Independent,
    /// All packages share one version.
    Fixed,
    /// All packages move at the same bump level.
    Linked,
}

/// Cascade policy (§10.4, resolved from `[versioning].cascade`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CascadeMode {
    /// Normal deps of publishable crates always cascade.
    Always,
    /// Cascade only on unsatisfied requirements (default).
    #[default]
    WhenUnsatisfied,
    /// Never cascade automatically.
    Never,
}

/// The planner's configuration knobs (ADR-0007): plain data, materialized
/// from `halyard-config`'s `ResolvedConfig` by
/// `ResolvedConfig::plan_config` so the pure core never depends on the
/// config crate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanConfig {
    /// Base branch the plan is computed against (`None` = auto; rendered
    /// as `HEAD` in the plan document until the shell resolves it).
    pub base_branch: Option<String>,
    /// Repo-wide tag-style override (appendix A.3); `None` keeps each
    /// package's discovery-resolved template, with the single-package
    /// default `v{version}` applied when the workspace has one package.
    pub tag_template: Option<TagTemplate>,
    /// Workspace grouping policy.
    pub group_mode: GroupMode,
    /// Pre-1.0 bump remap (§10.2).
    pub pre_major: PreMajorPolicy,
    /// Cascade policy (§10.4).
    pub cascade: CascadeMode,
    /// Commit type → bump level (§10.1). Types absent from the map
    /// contribute no bump; breaking commits always map to [`Bump::Major`].
    pub commit_types: BTreeMap<String, Bump>,
    /// Upper-bound rewrite policy for crossed ranges (appendix A.2).
    pub upper_bound_policy: UpperBoundPolicy,
    /// Commit types hidden from changelog previews (§9.4).
    pub changelog_hidden_types: BTreeSet<String>,
    /// Master publish switch (opt-in; invariant I7).
    pub publish_enabled: bool,
    /// Registry rendered for cargo releases (§13).
    pub publish_cargo_registry: String,
    /// Registry rendered for npm releases (§13).
    pub publish_npm_registry: String,
    /// npm access level rendered into npm releases' publish contract
    /// (ADR-0033, from `publish.npm.access`); `None` = unset — the
    /// spawned tool's own posture governs, no `--access` flag is
    /// invented. Cargo releases never carry it.
    pub publish_npm_access: Option<String>,
    /// npm provenance request rendered into npm releases' publish
    /// contract (ADR-0033, from `publish.npm.provenance`); `None` =
    /// unset (trusted publishing decides), `Some(true)` = the explicit
    /// `--provenance` request, `Some(false)` = the `--no-provenance`
    /// opt-out. Cargo releases never carry it.
    pub publish_npm_provenance: Option<bool>,
    /// Packages that must not be released (`[[package]] release = false`).
    pub release_disabled: BTreeSet<String>,
    /// Prerelease channel (§10.3, ADR-0025): when set, every non-forced
    /// release target becomes `{target}-{channel}.{n}` with the counter
    /// derived from the release base's and the manifest's own versions —
    /// no state file. `None` plans stable releases (and promotes a
    /// leftover prerelease manifest to its own stable triple).
    pub prerelease_channel: Option<PrereleaseChannel>,
    /// Per-package channel overrides (§10.3, ADR-0026): package name →
    /// channel, materialized from the `[[package]] channel` entries.
    /// The override is the most explicit policy — it wins over
    /// [`Self::prerelease_channel`] (the branch-resolved default) for
    /// that package through [`Self::channel_for_package`]; every other
    /// package keeps the default.
    pub package_channels: BTreeMap<String, PrereleaseChannel>,
    /// Maintenance-line pin (§10.3, ADR-0027): the branch-resolved line
    /// the shell is releasing on (`None` = an unpinned branch plans
    /// free). A non-forced target that would leave the line refuses
    /// (`E-PLAN-MAINTENANCE`) instead of planning across it.
    pub maintenance_line: Option<MaintenanceLine>,
}

impl PlanConfig {
    /// The built-in defaults (§16.4): independent grouping, Cargo-
    /// compatible pre-major policy, cascade only on unsatisfied
    /// requirements, the §10.1 commit mapping, keep-a-changelog hidden
    /// types, publishing off.
    pub fn default_values() -> Self {
        let mut commit_types = BTreeMap::new();
        commit_types.insert("feat".to_owned(), Bump::Minor);
        commit_types.insert("fix".to_owned(), Bump::Patch);
        commit_types.insert("perf".to_owned(), Bump::Patch);
        let hidden = ["chore", "docs", "ci", "test", "style"]
            .into_iter()
            .map(str::to_owned)
            .collect();
        Self {
            base_branch: None,
            tag_template: None,
            group_mode: GroupMode::Independent,
            pre_major: PreMajorPolicy::CARGO_COMPATIBLE,
            cascade: CascadeMode::WhenUnsatisfied,
            commit_types,
            upper_bound_policy: UpperBoundPolicy::Widen,
            changelog_hidden_types: hidden,
            publish_enabled: false,
            publish_cargo_registry: "crates.io".to_owned(),
            publish_npm_registry: "npm".to_owned(),
            publish_npm_access: None,
            publish_npm_provenance: None,
            release_disabled: BTreeSet::new(),
            prerelease_channel: None,
            package_channels: BTreeMap::new(),
            maintenance_line: None,
        }
    }

    /// The effective §10.3 channel for `name` (ADR-0026): the
    /// `[[package]]` override when one matches, else the repo-wide
    /// (branch-resolved) default. The planner consults this at every
    /// channel decision point — the train-absorption rule, promotion
    /// and channelization — so a mixed repo plans each package on its
    /// own policy with no post-pass.
    pub fn channel_for_package(&self, name: &str) -> Option<&PrereleaseChannel> {
        self.package_channels
            .get(name)
            .or(self.prerelease_channel.as_ref())
    }
}

impl Default for PlanConfig {
    fn default() -> Self {
        Self::default_values()
    }
}

/// A maintenance release line (§10.3, ADR-0027): the `1.x` / `1.2.x`
/// branch pattern pins every non-forced target to the line — a bump
/// that would leave the line refuses (`E-PLAN-MAINTENANCE`) instead of
/// planning across it. The same string names the branch and the line
/// it pins.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct MaintenanceLine {
    /// The pinned major.
    pub major: u64,
    /// The pinned minor (`None` = any minor on the major).
    pub minor: Option<u64>,
}

impl MaintenanceLine {
    /// Parses the documented grammar: `MAJOR.x` or `MAJOR.MINOR.x`.
    /// `None` for anything else — the guard never guesses a line.
    pub fn parse(branch: &str) -> Option<Self> {
        let mut parts = branch.split('.');
        let major = parts.next()?.parse::<u64>().ok()?;
        let second = parts.next()?;
        let third = parts.next();
        if parts.next().is_some() {
            return None;
        }
        let minor = match third {
            // `MAJOR.MINOR.x`: the middle part must be numeric.
            Some("x") => {
                if second == "x" {
                    return None;
                }
                Some(second.parse::<u64>().ok()?)
            }
            // `MAJOR.x`: exactly two parts, the tail is the pin.
            None => {
                if second != "x" {
                    return None;
                }
                None
            }
            // Anything else is not the documented grammar.
            Some(_) => return None,
        };
        Some(Self { major, minor })
    }

    /// Whether `version` stays on the line.
    pub fn matches(&self, version: &Version) -> bool {
        if version.major != self.major {
            return false;
        }
        match self.minor {
            Some(minor) => version.minor == minor,
            None => true,
        }
    }
}

impl std::fmt::Display for MaintenanceLine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.minor {
            Some(minor) => write!(f, "{}.{minor}.x", self.major),
            None => write!(f, "{}.x", self.major),
        }
    }
}

/// Planner failures (§15.4 `E-PLAN-*`; each variant has a page under
/// `docs/errors/`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlanError {
    /// A dependency cycle through non-dev edges prevents ordering
    /// (`E-PLAN-CYCLE`); `rendered` is the `a -> b -> a` path.
    Cycle {
        /// The rendered cycle path.
        rendered: String,
    },
    /// A `Release-As` footer carries an unusable version — unparseable,
    /// or older than the manifest version (`E-PLAN-RELEASE-AS`).
    ReleaseAsVersion {
        /// The commit carrying the footer.
        oid: String,
        /// The footer text as written.
        raw: String,
    },
    /// A dependent requirement cannot be rewritten so it admits the new
    /// version (`E-PLAN-UNREWRITABLE`, appendix A.2 error path).
    Unrewritable {
        /// The underlying rewriter failure (`E-EDIT-UNREWRITABLE`
        /// semantics).
        detail: crate::rewrite::VersionError,
        /// The manifest carrying the requirement.
        file: String,
        /// The dependent package name.
        package: String,
    },
    /// A maintenance-line pin (§10.3, ADR-0027) refuses a target that
    /// would leave the line it pins (`E-PLAN-MAINTENANCE`).
    MaintenanceLine {
        /// The package whose target leaves the line.
        package: String,
        /// The line the branch pins, as written.
        line: String,
        /// The offending target.
        target: String,
    },
    /// A change file (§9.4, ADR-0028) names a package the workspace
    /// does not know (`E-PLAN-CHANGE-FILE`) — the strict input
    /// discipline of `E-CFG-UNKNOWN-KEY` applied to the change-file
    /// layer: a typo'd name refuses the plan instead of silently
    /// dropping the file's bump and note.
    ChangeFilePackage {
        /// The change file carrying the unknown name.
        file: String,
        /// The unknown package name.
        package: String,
    },
}

impl PlanError {
    /// Stable §15.4 code of this error.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Cycle { .. } => "E-PLAN-CYCLE",
            Self::ReleaseAsVersion { .. } => "E-PLAN-RELEASE-AS",
            Self::Unrewritable { .. } => "E-PLAN-UNREWRITABLE",
            Self::MaintenanceLine { .. } => "E-PLAN-MAINTENANCE",
            Self::ChangeFilePackage { .. } => "E-PLAN-CHANGE-FILE",
        }
    }
}

impl std::fmt::Display for PlanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cycle { rendered } => write!(
                f,
                "[E-PLAN-CYCLE] dependency cycle through non-dev edges: {rendered}"
            ),
            Self::ReleaseAsVersion { oid, raw } => write!(
                f,
                "[E-PLAN-RELEASE-AS] commit {oid} carries an unusable `Release-As` footer \
                 {raw:?}: it must parse as semver and be newer than the manifest version"
            ),
            Self::Unrewritable {
                detail,
                file,
                package,
            } => write!(
                f,
                "[E-PLAN-UNREWRITABLE] {package}: {detail} (in {file}); adjust the requirement \
                 or set [versioning] cascade = \"never\""
            ),
            Self::MaintenanceLine {
                package,
                line,
                target,
            } => write!(
                f,
                "[E-PLAN-MAINTENANCE] `{package}` would release {target} on the `{line}` \
                 maintenance branch — a release must stay on the line it pins; land the \
                 breaking work on the default branch or force it with `Release-As` (the \
                 explicit escape hatch)"
            ),
            Self::ChangeFilePackage { file, package } => write!(
                f,
                "[E-PLAN-CHANGE-FILE] change file `{file}` names `{package}`, which this \
                 workspace does not know — change files may only name discovered packages; \
                 check the name against `halyard plan --format json`'s release and skip rows"
            ),
        }
    }
}

impl std::error::Error for PlanError {
    /// The structured cause chain, where one exists (the
    /// `EditFailure`/`LockfileFailure` convention): the
    /// [`PlanError::Unrewritable`] rewriter detail is a real inner
    /// error and is surfaced for programmatic consumers instead of
    /// living only in the flattened Display text.
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Unrewritable { detail, .. } => Some(detail),
            Self::Cycle { .. }
            | Self::ReleaseAsVersion { .. }
            | Self::MaintenanceLine { .. }
            | Self::ChangeFilePackage { .. } => None,
        }
    }
}

/// The planner's immutable input (§4): discovered workspace, the
/// materialized history between HEAD and every package's release base,
/// the resolved per-package release refs and the parsed change files
/// (§9.4, ADR-0028 — pure data like the refs; the shell parses
/// `.halyard/changes/*.md`, the planner reads it).
#[derive(Clone, Copy)]
pub struct Snapshot<'a> {
    /// The discovered workspace (packages, groups, links).
    pub workspace: &'a Workspace,
    /// Pre-materialized history digest (§5.7) — built by the shell, read
    /// purely by the planner (I6).
    pub history: &'a dyn HistoryProvider,
    /// Per-package release bases (§8.2); `None` = initial release.
    pub refs: &'a ReleaseRefs,
    /// The parsed change files (§9.4, ADR-0028): per-package explicit
    /// bumps and notes. Empty input plans exactly as a repo without
    /// change files.
    pub change_files: &'a ChangeFiles,
}

/// Plans one release (§4, appendix A.1).
///
/// # Errors
///
/// [`PlanError::Cycle`] when the workspace has a non-dev dependency
/// cycle, [`PlanError::ReleaseAsVersion`] for unusable `Release-As`
/// footers, [`PlanError::Unrewritable`] when a crossed requirement
/// cannot be rewritten under the configured policy,
/// [`PlanError::ChangeFilePackage`] when a change file names a package
/// the workspace does not know.
pub fn plan(snapshot: &Snapshot<'_>, cfg: &PlanConfig) -> Result<Plan, PlanError> {
    let ws = snapshot.workspace;
    let digest = snapshot.history.digest();
    let graph = WorkspaceGraph::build(ws);
    let order = graph.topo_order().map_err(|e| match e {
        crate::graph::GraphError::Cycle { rendered, .. } => PlanError::Cycle { rendered },
    })?;

    validate_change_files(ws, snapshot)?;

    let mut state = State {
        bumps: vec![Bump::None; ws.packages().len()],
        forced: vec![None; ws.packages().len()],
        reasons: vec![Vec::new(); ws.packages().len()],
    };

    direct_bumps(snapshot, cfg, &mut state)?;
    normalize_groups(ws, cfg, &mut state);
    cascade(ws, &graph, &order, cfg, &mut state);

    let (targets, tag_only_skips) = materialize_versions(ws, snapshot, cfg, &state);
    let targets = promote_pending_stable(ws, snapshot, cfg, &targets);
    let targets = channelize_targets(ws, snapshot, cfg, &state, &targets);
    let targets = enforce_maintenance_line(ws, cfg, &state, targets)?;
    let rewrites = collect_rewrites(ws, cfg, &targets)?;

    let mut releases: Vec<PlanRelease> = Vec::new();
    let mut edits: BTreeMap<(String, u32, u32), String> = BTreeMap::new();
    let mut skipped: Vec<PlanSkip> = Vec::new();

    for pkg in ws.packages() {
        let name = pkg_name(ws, pkg);
        let Some(target) = targets.get(&pkg.id) else {
            skipped.push(PlanSkip {
                package: name.to_owned(),
                reason: skip_reason(pkg, cfg, ws, tag_only_skips.get(&pkg.id).copied()),
            });
            continue;
        };
        let (row, version_edit) = release_row(ws, pkg, target, &state, digest, cfg, snapshot);
        releases.push(row);
        // Own version site (inherited sites dedupe through the keyed map).
        if let Some((site, text)) = version_edit {
            add_edit(&mut edits, &site, text);
        }
    }
    for (site, text) in rewrites {
        add_edit(&mut edits, &site, text);
    }

    let plan_edits: Vec<PlanEdit> = group_edits(edits);

    // Publish order: dependencies first (§5.8 `order`), releasing only.
    let released_names: BTreeSet<&str> = targets
        .keys()
        .filter_map(|id| ws.package(*id).map(|p| pkg_name(ws, p)))
        .collect();
    let order_out: Vec<String> = order
        .iter()
        .filter(|id| {
            ws.package(**id)
                .is_some_and(|p| released_names.contains(pkg_name(ws, p)))
        })
        .filter_map(|id| ws.package(*id).map(|p| pkg_name(ws, p).to_owned()))
        .collect();

    let mut doc = Plan {
        schema: PLAN_SCHEMA_V1.to_owned(),
        base_branch: cfg.base_branch.clone().unwrap_or_else(|| "HEAD".to_owned()),
        head: snapshot
            .history
            .head()
            .map(|oid| oid.to_string())
            .unwrap_or_default(),
        plan_hash: String::new(),
        releases,
        edits: plan_edits,
        order: order_out,
        skipped,
    };
    doc.plan_hash = crate::canonical::plan_hash(&doc);
    Ok(doc)
}

/// Assembles one release row (§5.8): version pair, bump, tag, wire
/// reasons, the merged changelog preview (the package's change-file
/// notes included) and the publish decision — plus the package's own
/// version-site edit, returned separately so the caller keeps the edit
/// map (inherited sites dedupe through the keyed map).
fn release_row(
    ws: &Workspace,
    pkg: &Package,
    target: &Version,
    state: &State,
    digest: &HistoryDigest,
    cfg: &PlanConfig,
    snapshot: &Snapshot<'_>,
) -> (PlanRelease, Option<(ManifestSite, String)>) {
    let name = pkg_name(ws, pkg);
    // The row's `from` is the SAME version the target algebra consumed
    // (`effective_from`): for a tag-only package that is the release
    // base tag's version (§8.2), never the zero fallback — the pair
    // `from → to` must be internally consistent (I3).
    let from = effective_from(ws, snapshot, pkg);
    let bump = release_bump(&from, target, state.bump(pkg.id));
    let tag =
        effective_template(ws, pkg, cfg).render(name, target, pkg.ecosystem.resolve(ws.interner()));
    let raw_reasons = state.reasons(pkg.id);
    let reasons: Vec<PlanReason> = raw_reasons
        .iter()
        .map(|reason| wire_reason(reason, ws))
        .collect();
    let changelog = Some(release_changelog(
        pkg,
        target,
        raw_reasons,
        digest,
        cfg,
        snapshot.change_files.notes_for(name),
    ));
    // The npm publish options ride the npm contract only (ADR-0033): a
    // cargo release never carries them, and an unset npm option stays
    // `None` (the spawned tool's own posture governs — no flag is
    // invented). PyPI releases ride the default index (uv's own
    // posture; M8 step 1, ADR-0056) and never carry the npm options —
    // custom-index routing is the recorded-open config-surface
    // remainder of the M8 row. A tag-only release (M8 Go, ADR-0057)
    // carries **no** publish decision at all (`publish: None`): the
    // release tag is the artifact, no registry exists, and the publish
    // engine's documented skip keeps the row out of every forecast.
    let publish = if pkg.ecosystem.is_tag_only() {
        None
    } else {
        let (registry, npm_access, npm_provenance) = match pkg.ecosystem {
            Ecosystem::Cargo => (cfg.publish_cargo_registry.clone(), None, None),
            Ecosystem::PyPi => ("pypi".to_owned(), None, None),
            // Maven rows ride the engine's disabled shape until the
            // `mvn` argv arm lands (ADR-0058 D5): `publishable = false`
            // at discovery keeps `enabled` false — the registry name is
            // truthful the day the argv arm flips it.
            Ecosystem::Maven => ("maven-central".to_owned(), None, None),
            _ => (
                cfg.publish_npm_registry.clone(),
                cfg.publish_npm_access.clone(),
                cfg.publish_npm_provenance,
            ),
        };
        Some(PlanPublish {
            registry,
            enabled: pkg.publishable && cfg.publish_enabled,
            access: npm_access,
            provenance: npm_provenance,
        })
    };
    let row = PlanRelease {
        package: name.to_owned(),
        ecosystem: pkg.ecosystem.resolve(ws.interner()).to_owned(),
        from: from.to_string(),
        to: target.to_string(),
        bump,
        tag,
        reasons,
        changelog,
        publish,
    };
    let edit = pkg
        .version_site
        .clone()
        .map(|site| (site, target.to_string()));
    (row, edit)
}

/// Per-package planning state (bumps, forced versions, reason chains).
/// All slots are addressed by [`PkgId`] through checked accessors — the
/// vectors are sized to the package count at construction, so an
/// out-of-range id is impossible, but access stays bounds-checked (§15.3
/// forbids direct indexing).
struct State {
    /// Strongest bump per package so far (monotone during the fixpoint).
    bumps: Vec<Bump>,
    /// `Release-As` forced target per package.
    forced: Vec<Option<Version>>,
    /// Reason chain per package, in emit order (§5.6 — never sorted).
    reasons: Vec<Vec<Reason>>,
}

impl State {
    /// The strongest bump recorded for `id` (`None` for a foreign id).
    fn bump(&self, id: PkgId) -> Bump {
        self.bumps.get(id.to_usize()).copied().unwrap_or(Bump::None)
    }

    /// Raises the bump of `id` to `b` (monotone: only increases).
    fn raise(&mut self, id: PkgId, b: Bump) {
        if let Some(slot) = self.bumps.get_mut(id.to_usize())
            && *slot < b
        {
            *slot = b;
        }
    }

    /// The forced version of `id`, if any.
    fn forced(&self, id: PkgId) -> Option<&Version> {
        self.forced.get(id.to_usize()).and_then(|f| f.as_ref())
    }

    /// Forces the target version of `id`.
    fn set_forced(&mut self, id: PkgId, version: Version) {
        if let Some(slot) = self.forced.get_mut(id.to_usize()) {
            *slot = Some(version);
        }
    }

    /// Overwrites the bump of `id` with the change-file explicit level
    /// (§9.4, ADR-0028) — a replacement, not a raise: the explicit
    /// input wins over the commit-derived level in both directions.
    fn set_explicit(&mut self, id: PkgId, bump: Bump) {
        if let Some(slot) = self.bumps.get_mut(id.to_usize()) {
            *slot = bump;
        }
    }

    /// The reason chain of `id` (empty for a foreign id).
    fn reasons(&self, id: PkgId) -> &[Reason] {
        self.reasons.get(id.to_usize()).map_or(&[], Vec::as_slice)
    }

    /// Appends a reason to `id`'s chain (emit order preserved).
    fn push_reason(&mut self, id: PkgId, reason: Reason) {
        if let Some(chain) = self.reasons.get_mut(id.to_usize()) {
            chain.push(reason);
        }
    }
}

/// Step 1: commit-derived bumps and `Release-As` footers.
fn direct_bumps(
    snapshot: &Snapshot<'_>,
    cfg: &PlanConfig,
    state: &mut State,
) -> Result<(), PlanError> {
    let ws = snapshot.workspace;
    let digest = snapshot.history.digest();
    for pkg in ws.packages() {
        let id = pkg.id;
        let name = pkg_name(ws, pkg);
        if cfg.release_disabled.contains(name) {
            continue;
        }
        let initial = snapshot.refs.base_of(pkg.id).is_none();
        let mut released_commits = 0usize;
        for &ci in snapshot.history.commits_for(pkg.id) {
            let Some(commit) = digest.commits.get(ci) else {
                continue;
            };
            released_commits += 1;
            let release_as = commit.trailers.iter().find(|t| {
                std::str::from_utf8(&t.key).is_ok_and(|k| k.eq_ignore_ascii_case("Release-As"))
            });
            if let Some(trailer) = release_as {
                let raw = String::from_utf8_lossy(&trailer.value).trim().to_owned();
                let manifest_version = pkg.version.clone().unwrap_or_else(zero_version);
                match Version::parse(&raw) {
                    // A `Release-As` equal to the manifest version is a
                    // no-op (I1: idempotence) — recorded, forces nothing.
                    Ok(v) if v > manifest_version => {
                        state.set_forced(id, v.clone());
                        state.push_reason(
                            id,
                            Reason::ReleaseAs {
                                oid: commit.oid.clone(),
                                version: v,
                            },
                        );
                    }
                    Ok(v) if v == manifest_version => {}
                    _ => {
                        return Err(PlanError::ReleaseAsVersion {
                            oid: commit.oid.to_string(),
                            raw,
                        });
                    }
                }
                continue;
            }
            let kind = std::str::from_utf8(&commit.parsed.kind).unwrap_or_default();
            let bump = if commit.parsed.breaking {
                Bump::Major
            } else {
                cfg.commit_types.get(kind).copied().unwrap_or(Bump::None)
            };
            if !bump.is_none() {
                state.raise(id, bump);
                state.push_reason(
                    id,
                    Reason::Commit {
                        oid: commit.oid.clone(),
                        kind: commit.parsed.kind.clone(),
                        breaking: commit.parsed.breaking,
                    },
                );
            }
        }
        if initial && released_commits > 0 {
            state.push_reason(id, Reason::InitialRelease);
        }
        // The §9.4 change-file explicit bump (ADR-0028) replaces the
        // commit-derived level outright (the precedence: `Release-As` >
        // change file > conventional commits > cascade) — the escape
        // hatch for squash-merge repos whose titles carry no signal.
        // The reason stays in the chain (it documents the explicit
        // input); the cascade exempts the package on its own (the
        // monotone guard reads the replaced slot, and every explicit
        // level is at least `Patch`).
        if let Some(entry) = snapshot.change_files.bump_of(name) {
            state.set_explicit(id, entry.bump);
            state.push_reason(
                id,
                Reason::Changeset {
                    file: entry.file.clone().into(),
                },
            );
        }
    }
    Ok(())
}

/// §9.4 input validation (ADR-0028): every package a change file names
/// must exist in the workspace — the strict-input discipline of
/// `E-CFG-UNKNOWN-KEY` applied to the change-file layer. Known but
/// release-disabled names skip silently (the skip wins over every bump
/// source); only a name the workspace never heard of refuses.
fn validate_change_files(ws: &Workspace, snapshot: &Snapshot<'_>) -> Result<(), PlanError> {
    let names: BTreeSet<&str> = ws.packages().iter().map(|p| pkg_name(ws, p)).collect();
    for name in snapshot.change_files.names() {
        if names.contains(name) {
            continue;
        }
        let file = snapshot
            .change_files
            .bump_of(name)
            .map_or(String::new(), |entry| entry.file.clone());
        return Err(PlanError::ChangeFilePackage {
            file,
            package: name.to_owned(),
        });
    }
    Ok(())
}

/// Step 2: group + lockstep-link normalization (A.1 "groups normalize").
/// Every member rises to the strongest bump in its union; raised members
/// gain a [`CascadeWhy::LockstepGroup`] cascade reason.
fn normalize_groups(ws: &Workspace, cfg: &PlanConfig, state: &mut State) {
    let mut pairs: Vec<(PkgId, PkgId)> = Vec::new();
    let mut single_union: Option<PkgId> = None;
    if cfg.group_mode == GroupMode::Independent {
        // Discovery-level groups (Fixed/Linked), when present.
        for group in ws.groups() {
            if group.policy == GroupPolicy::Independent {
                continue;
            }
            let mut members = group.members.iter().copied();
            if let Some(first) = members.next() {
                for other in members {
                    pairs.push((first, other));
                }
            }
        }
    } else {
        // Repo-wide Fixed/Linked policy: one union over every package.
        single_union = ws.packages().first().map(|p| p.id);
        for pkg in ws.packages().iter().skip(1) {
            if let Some(first) = single_union {
                pairs.push((first, pkg.id));
            }
        }
    }
    for link in ws.links() {
        if matches!(link.mode, LinkMode::Lockstep) {
            pairs.push((link.from, link.to));
        }
    }
    if pairs.is_empty() && single_union.is_none() {
        return;
    }
    let parent = union_find(ws.packages().len(), &pairs);
    // Strongest bump + its source package per root.
    let mut roots: BTreeMap<usize, (Bump, PkgId)> = BTreeMap::new();
    for pkg in ws.packages() {
        let root = find_root(&parent, pkg.id.to_usize());
        let bump = state.bump(pkg.id);
        let entry = roots.entry(root).or_insert((bump, pkg.id));
        if bump > entry.0 {
            entry.0 = bump;
            entry.1 = pkg.id;
        }
    }
    for pkg in ws.packages() {
        let id = pkg.id;
        let root = find_root(&parent, id.to_usize());
        let Some((strongest, source)) = roots.get(&root) else {
            continue;
        };
        if state.bump(id) < *strongest {
            state.raise(id, *strongest);
            state.push_reason(
                id,
                Reason::Cascade {
                    via: *source,
                    edge: DepKind::Normal,
                    why: CascadeWhy::LockstepGroup,
                },
            );
        }
    }
}

/// Union-find over package ids (deterministic: the smaller root wins).
fn union_find(count: usize, pairs: &[(PkgId, PkgId)]) -> Vec<usize> {
    let mut parent: Vec<usize> = (0..count).collect();
    for (a, b) in pairs {
        union(&mut parent, a.to_usize(), b.to_usize());
    }
    parent
}

fn union(parent: &mut [usize], a: usize, b: usize) {
    let (ra, rb) = (find_root(parent, a), find_root(parent, b));
    if ra != rb {
        let (lo, hi) = if ra <= rb { (ra, rb) } else { (rb, ra) };
        if let Some(slot) = parent.get_mut(hi) {
            *slot = lo;
        }
    }
}

fn find_root(parent: &[usize], mut node: usize) -> usize {
    while let Some(&p) = parent.get(node) {
        if p == node {
            return node;
        }
        node = p;
    }
    node
}

/// Step 3: the cascade fixpoint (§10.4). Iterates the dependency-first
/// topo order so each upstream's bump is final before its dependents are
/// examined; repeats while something changed, bounded by `|V| + 1` rounds.
/// Monotone (bumps only rise to `Patch`) ⇒ terminating.
fn cascade(
    ws: &Workspace,
    graph: &WorkspaceGraph,
    order: &[PkgId],
    cfg: &PlanConfig,
    state: &mut State,
) {
    if cfg.cascade == CascadeMode::Never {
        return;
    }
    let bound = ws.packages().len() + 1;
    for _round in 0..bound {
        let mut changed = false;
        for upstream in order {
            let u_bump = state.bump(*upstream);
            if u_bump.is_none() {
                continue;
            }
            let Some(u_pkg) = ws.package(*upstream) else {
                continue;
            };
            let Some(u_from) = u_pkg.version.clone() else {
                continue;
            };
            let u_new = apply_bump(&u_from, u_bump, &cfg.pre_major);
            let Some(dependents) = graph.in_neighbors(*upstream) else {
                continue;
            };
            for dependent in dependents {
                let d_id = *dependent;
                if state.bump(d_id) >= Bump::Patch {
                    continue;
                }
                let Some(d_pkg) = ws.package(d_id) else {
                    continue;
                };
                let mut fired: Option<(DepKind, CascadeWhy)> = None;
                for dep in &d_pkg.deps {
                    if dep.to != *upstream {
                        continue;
                    }
                    if let Some(why) = cascade_reason(cfg, dep, d_pkg, &u_new) {
                        fired = Some((dep.kind, why));
                        break;
                    }
                }
                if let Some((edge, why)) = fired {
                    state.raise(d_id, Bump::Patch);
                    state.push_reason(
                        d_id,
                        Reason::Cascade {
                            via: *upstream,
                            edge,
                            why,
                        },
                    );
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
}

/// Decides whether one dep edge fires a cascade (§10.4 a/b): dev edges
/// never fire (§6.3); an unsatisfied version requirement always fires;
/// `cascade = "always"` fires for normal edges of publishable dependents.
fn cascade_reason(
    cfg: &PlanConfig,
    dep: &DepEdge,
    dependent: &Package,
    u_new: &Version,
) -> Option<CascadeWhy> {
    if dep.kind == DepKind::Dev {
        return None;
    }
    match &dep.req {
        ReqSpec::VersionReq(req) => {
            if !req.matches(u_new) {
                Some(CascadeWhy::RequirementUnsatisfied)
            } else if cfg.cascade == CascadeMode::Always
                && dep.kind == DepKind::Normal
                && dependent.publishable
            {
                Some(CascadeWhy::PolicyAlways)
            } else {
                None
            }
        }
        // Protocols resolve at publish time; foreign grammars (ADR-0006)
        // and path-only deps never "mismatch" — only (b) applies.
        _ => {
            if cfg.cascade == CascadeMode::Always
                && dep.kind == DepKind::Normal
                && dependent.publishable
            {
                Some(CascadeWhy::PolicyAlways)
            } else {
                None
            }
        }
    }
}

/// Step 4a: the next version per releasing package (id → target). This
/// step cannot fail, so it returns the map directly.
///
/// With a channel configured (§10.3, ADR-0025) and an **active train**
/// (the release base tag itself is a prerelease), a non-breaking step is
/// absorbed into the train's committed triple — the manifest's stable
/// part, verbatim; the channel counter advances instead (`1.3.0-rc.1` +
/// one `fix` → `1.3.0-rc.2`). A breaking step moves the triple through
/// [`apply_bump`] (a fresh `2.0.0` base, counter restarted). Without a
/// channel the derivation is exactly the stable planner's.
/// The version a package plans FROM: the manifest version — except the
/// **tag-only** ecosystems (M8 ADR-0057), whose manifest carries no
/// version row: their `from` is the release base tag's version (§8.2,
/// parsed through the package's own tag template), or the zero version
/// for an initial release. Shared by the target algebra and the release
/// row so the `from → to` pair stays internally consistent.
fn effective_from(ws: &Workspace, snapshot: &Snapshot<'_>, pkg: &Package) -> Version {
    if pkg.ecosystem.is_tag_only() {
        return pkg
            .version
            .clone()
            .or_else(|| base_version_of(ws, snapshot, pkg))
            .unwrap_or_else(zero_version);
    }
    pkg.version.clone().unwrap_or_else(zero_version)
}

/// Materializes the per-package target versions (§4 step 4): the bump
/// algebra over the manifest version — except the **tag-only** ecosystems
/// (M8 ADR-0057), whose manifest carries no version row: their `from` is
/// the release base tag's version (§8.2 step 3, parsed through the
/// package's own subdir template), or the zero version for an initial
/// release. Returns the targets plus the tag-only packages whose target
/// the major-version-suffix rule refused (a `/vN` module path must
/// release `vN.0.0`-shaped versions — the skip is reported, never
/// silently re-scoped).
fn materialize_versions(
    ws: &Workspace,
    snapshot: &Snapshot<'_>,
    cfg: &PlanConfig,
    state: &State,
) -> (BTreeMap<PkgId, Version>, BTreeMap<PkgId, &'static str>) {
    let mut targets = BTreeMap::new();
    let mut tag_only_skips = BTreeMap::new();
    for pkg in ws.packages() {
        let id = pkg.id;
        let name = pkg_name(ws, pkg);
        if cfg.release_disabled.contains(name) {
            continue;
        }
        let from = if pkg.ecosystem.is_tag_only() {
            // Tag-only (ADR-0057): the release base tag carries the
            // version (§8.2 step 3 parses it through the package's own
            // template); an initial release starts from the zero
            // version.
            effective_from(ws, snapshot, pkg)
        } else {
            let Some(from) = pkg.version.clone() else {
                continue;
            };
            from
        };
        let target = if let Some(v) = state.forced(id) {
            v.clone()
        } else {
            let bump = state.bump(id);
            if bump.is_none() {
                continue;
            }
            let active_train = cfg.channel_for_package(name).is_some()
                && base_version_of(ws, snapshot, pkg).is_some_and(|base| !base.pre.is_empty());
            if active_train && !matches!(bump, Bump::Major) {
                // The train's triple is its committed progress.
                promote_to_stable(&from)
            } else {
                apply_bump(&from, bump, &cfg.pre_major)
            }
        };
        if target <= from {
            // Forced targets are validated on ingest; this guard keeps a
            // pathological 0.x remap from ever emitting a regression.
            continue;
        }
        // The major-version-suffix rule (go.dev/ref/mod): a module path
        // ending in /vN must release vN-shaped versions; a target below
        // the declared major is refused and reported.
        if let Some(major) = tag_only_major(ws, pkg)
            && target.major < major
        {
            tag_only_skips.insert(id, "major_version_suffix");
            continue;
        }
        targets.insert(id, target);
    }
    (targets, tag_only_skips)
}

/// The declared major version of a tag-only package's name (the final
/// `/vN` of a Go module path); `None` for other ecosystems and for
/// suffix-less paths.
fn tag_only_major(ws: &Workspace, pkg: &Package) -> Option<u64> {
    if !pkg.ecosystem.is_tag_only() {
        return None;
    }
    crate::package::major_version_suffix(pkg_name(ws, pkg))
}

/// §10.3 maintenance-line guard (ADR-0027): with a branch-resolved
/// line pinned, every non-forced target must stay on the line it names —
/// the guard runs over the FINAL targets, so a channelized train triple
/// is what is checked. Forced (`Release-As`) targets are the user's
/// explicit version and stay exempt.
fn enforce_maintenance_line(
    ws: &Workspace,
    cfg: &PlanConfig,
    state: &State,
    targets: BTreeMap<PkgId, Version>,
) -> Result<BTreeMap<PkgId, Version>, PlanError> {
    let Some(line) = &cfg.maintenance_line else {
        return Ok(targets);
    };
    // Refuse the FIRST out-of-line target in package order — the error
    // names the package, the line and the offending version.
    for pkg in ws.packages() {
        let Some(target) = targets.get(&pkg.id) else {
            continue;
        };
        if state.forced(pkg.id).is_some() || line.matches(target) {
            continue;
        }
        return Err(PlanError::MaintenanceLine {
            package: pkg_name(ws, pkg).to_owned(),
            line: line.to_string(),
            target: target.to_string(),
        });
    }
    Ok(targets)
}

/// §10.3 promotion (ADR-0025): with no channel configured, a manifest
/// that still carries a prerelease releases its own stable triple — the
/// train's `rc.N` collapses into `{triple}` even when no commits are
/// left (removing the channel *is* the promotion act; keeping it
/// configured continues the train). Skipped when the manifest is behind
/// the latest release tag (the reconciliation reports that state and
/// never tags downward) and when a commit-derived target already exists.
/// The channel decision is per package (ADR-0026): a package running
/// its own `[[package]] channel` train continues it even when the
/// repo-wide default is unset.
fn promote_pending_stable(
    ws: &Workspace,
    snapshot: &Snapshot<'_>,
    cfg: &PlanConfig,
    targets: &BTreeMap<PkgId, Version>,
) -> BTreeMap<PkgId, Version> {
    let mut out = targets.clone();
    for pkg in ws.packages() {
        let name = pkg_name(ws, pkg);
        if cfg.release_disabled.contains(name)
            || cfg.channel_for_package(name).is_some()
            || out.contains_key(&pkg.id)
        {
            continue;
        }
        let Some(from) = &pkg.version else {
            continue;
        };
        if from.pre.is_empty() {
            continue;
        }
        // The manifest must not be behind the latest release tag.
        let base_version = base_version_of(ws, snapshot, pkg);
        if base_version.is_some_and(|base| base > *from) {
            continue;
        }
        out.insert(pkg.id, promote_to_stable(from));
    }
    out
}

/// §10.3 channel application (ADR-0025): every non-forced target becomes
/// `{target}-{channel}.{n}`. The counter comes from the release base's
/// own tag and the manifest version (`advance_prerelease` picks the
/// highest matching counter — no state file); the manifest witnesses the
/// applied-but-untagged case where the manifest is ahead of the base
/// tag. Forced targets are the user's explicit version and are exempt.
/// The channel is resolved per package (ADR-0026): a `[[package]]`
/// override names that package's train, the repo-wide default fills the
/// rest, and an un-channelized package keeps its stable target.
fn channelize_targets(
    ws: &Workspace,
    snapshot: &Snapshot<'_>,
    cfg: &PlanConfig,
    state: &State,
    targets: &BTreeMap<PkgId, Version>,
) -> BTreeMap<PkgId, Version> {
    let mut out = BTreeMap::new();
    for pkg in ws.packages() {
        let Some(stable) = targets.get(&pkg.id) else {
            continue;
        };
        if state.forced(pkg.id).is_some() {
            out.insert(pkg.id, stable.clone());
            continue;
        }
        let name = pkg_name(ws, pkg);
        let Some(channel) = cfg.channel_for_package(name) else {
            out.insert(pkg.id, stable.clone());
            continue;
        };
        let from = pkg.version.clone().unwrap_or_else(zero_version);
        let mut witnesses = Vec::new();
        if let Some(base) = base_version_of(ws, snapshot, pkg) {
            witnesses.push(base);
        }
        witnesses.push(from.clone());
        let channelized = advance_prerelease(stable, channel, &witnesses);
        if channelized == from {
            // The manifest already carries this exact counter step
            // (applied but untagged, with no newer base tag): nothing new
            // is owed this run — the same skip `materialize_versions`
            // applies to a stable manifest the plan cannot move. Note the
            // guard is equality, not `<=`: a channel switch can emit a
            // version semver-below the old prerelease tier (`rc.1` →
            // `beta.1`) — channels are parallel tracks, not an order.
            continue;
        }
        out.insert(pkg.id, channelized);
    }
    out
}

/// The version parsed from the package's release base tag, if any
/// (§8.2): `None` for an initial release or a tag the template cannot
/// parse.
fn base_version_of(ws: &Workspace, snapshot: &Snapshot<'_>, pkg: &Package) -> Option<Version> {
    let base = snapshot.refs.base_of(pkg.id)?;
    let name = pkg_name(ws, pkg);
    let eco = pkg.ecosystem.resolve(ws.interner());
    pkg.tag_template
        .extract_version(base.tag.to_string().as_str(), name, eco)
}

/// Step 4b: requirement-rewrite patches for every edge whose requirement
/// no longer admits the upstream's new version (appendix A.2, §10.4).
fn collect_rewrites(
    ws: &Workspace,
    cfg: &PlanConfig,
    targets: &BTreeMap<PkgId, Version>,
) -> Result<Vec<(ManifestSite, String)>, PlanError> {
    let mut out: Vec<(ManifestSite, String)> = Vec::new();
    for pkg in ws.packages() {
        for dep in &pkg.deps {
            let Some(new_version) = targets.get(&dep.to) else {
                continue;
            };
            let text = match &dep.req {
                ReqSpec::VersionReq(_) => String::from_utf8_lossy(&dep.raw).to_string(),
                // Protocols resolve at publish time; foreign grammars are
                // carried verbatim (ADR-0006) and never rewritten.
                _ => continue,
            };
            if text.is_empty() {
                continue;
            }
            match rewrite_requirement(&text, new_version, cfg.upper_bound_policy) {
                Ok(RewriteOutcome::Rewritten(next)) => out.push((dep.site.clone(), next)),
                Ok(RewriteOutcome::Unchanged { .. }) => {}
                Err(detail) => {
                    return Err(PlanError::Unrewritable {
                        detail,
                        file: dep.site.file.to_string(),
                        package: pkg_name(ws, pkg).to_owned(),
                    });
                }
            }
        }
    }
    // Deterministic output regardless of edge order: sort by site.
    out.sort_by_key(|item| site_key(&item.0));
    out.dedup_by(|a, b| site_key(&a.0) == site_key(&b.0) && a.1 == b.1);
    Ok(out)
}

/// Sort/dedup key of an edit site: file, then first span.
fn site_key(site: &ManifestSite) -> (String, u32, u32) {
    (
        site.file.to_string(),
        site.spans.first().map_or(u32::MAX, |s| s.start),
        site.spans.first().map_or(u32::MAX, |s| s.end),
    )
}

/// Groups the flat patch list per file, spans sorted ascending.
fn group_edits(edits: BTreeMap<(String, u32, u32), String>) -> Vec<PlanEdit> {
    let mut files: BTreeMap<String, Vec<PlanPatch>> = BTreeMap::new();
    for ((file, start, end), text) in edits {
        files
            .entry(file)
            .or_default()
            .push(PlanPatch { start, end, text });
    }
    files
        .into_iter()
        .map(|(file, patches)| PlanEdit { file, patches })
        .collect()
}

/// Package name as `&str` (interned; `"?"` for a corrupt interner, which
/// discovery never produces).
fn pkg_name<'a>(ws: &'a Workspace, pkg: &Package) -> &'a str {
    ws.interner().get(pkg.name).unwrap_or("?")
}

/// Adds one edit patch, deduplicating by (file, start, end).
fn add_edit(edits: &mut BTreeMap<(String, u32, u32), String>, site: &ManifestSite, text: String) {
    let Some(span) = site.spans.first() else {
        return;
    };
    edits.insert((site.file.to_string(), span.start, span.end), text);
}

/// Skipped-list reason for a non-releasing package (§5.8).
fn skip_reason(
    pkg: &Package,
    cfg: &PlanConfig,
    ws: &Workspace,
    tag_only_skip: Option<&'static str>,
) -> String {
    let name = pkg_name(ws, pkg);
    if let Some(reason) = tag_only_skip {
        // A tag-only package the major-version-suffix rule refused (M8
        // ADR-0057) — the recorded reason, not a silent re-scope.
        reason.to_owned()
    } else if pkg.version.is_none() && !pkg.ecosystem.is_tag_only() {
        // Tag-only packages legitimately carry no manifest version (the
        // release tag does), so a missing version is not their skip
        // reason.
        "no_version".to_owned()
    } else if cfg.release_disabled.contains(name) {
        "release_disabled".to_owned()
    } else {
        "no_changes".to_owned()
    }
}

/// Minimal bump taking `from` to `to` (used for forced releases).
/// The release row's reported bump: a channel advance that keeps the
/// stable triple reports the bump the commits derived — an owed release
/// never renders as `none` — while stable targets (and promotions, whose
/// commits are already consumed) keep the minimal from→to reading.
fn release_bump(from: &Version, target: &Version, derived: Bump) -> Bump {
    if !target.pre.is_empty() && same_triple(from, target) {
        derived
    } else {
        minimal_bump(from, target)
    }
}

fn minimal_bump(from: &Version, to: &Version) -> Bump {
    if to.major != from.major {
        Bump::Major
    } else if to.minor != from.minor {
        Bump::Minor
    } else if to.patch != from.patch {
        Bump::Patch
    } else {
        Bump::None
    }
}

/// The tag template for one package (appendix A.3): the config override
/// wins, then the single-package `v{version}` default, then the
/// discovery-resolved template. Shared with the §11.1 step-2
/// reconciliation ([`crate::release_state`]) so the planner and the tag
/// phase can never disagree about a tag's name.
pub fn effective_template(ws: &Workspace, pkg: &Package, cfg: &PlanConfig) -> TagTemplate {
    if let Some(template) = &cfg.tag_template {
        return template.clone();
    }
    if ws.packages().len() == 1
        && let Ok(single) = TagTemplate::parse("v{version}")
    {
        return single;
    }
    pkg.tag_template.clone()
}

/// Maps an in-memory [`Reason`] onto its wire form (§5.6 → §5.8).
fn wire_reason(reason: &Reason, ws: &Workspace) -> PlanReason {
    match reason {
        Reason::Commit {
            oid,
            kind,
            breaking,
        } => PlanReason::Commit {
            oid: oid.to_string(),
            kind: String::from_utf8_lossy(kind).to_string(),
            breaking: *breaking,
        },
        Reason::ReleaseAs { oid, version } => PlanReason::ReleaseAs {
            oid: oid.to_string(),
            version: version.to_string(),
        },
        Reason::Changeset { file } => PlanReason::Changeset {
            file: String::from_utf8_lossy(file).to_string(),
        },
        Reason::Cascade { via, edge, why } => PlanReason::Cascade {
            via: ws
                .package(*via)
                .map(|p| pkg_name(ws, p).to_owned())
                .unwrap_or_default(),
            edge: edge.wire_key().to_owned(),
            why: why.wire_key().to_owned(),
        },
        Reason::InitialRelease => PlanReason::InitialRelease,
    }
}

/// The zero-config changelog path (§6.1 single-package mode: "changelog
/// at root"): `<package root>/CHANGELOG.md`, with root-only packages at
/// the repo root. Pure derivation from the package data — the planner
/// never reads the filesystem (§4).
fn default_changelog_file(pkg: &Package) -> String {
    let root = pkg.root.as_str();
    if root.is_empty() {
        "CHANGELOG.md".to_owned()
    } else {
        format!("{root}/CHANGELOG.md")
    }
}

/// One release's changelog contract (§5.8): the configured file, or the
/// §6.1 single-package default — the adapters leave `changelog` unset
/// today, so the default keeps the §9.4 chain fed on zero-config repos.
/// The package's change-file notes (ADR-0028) merge into the preview so
/// every downstream consumer (the §11.3 PR body, the CHANGELOG.md write,
/// the §11.1 `create_release` payload through
/// `render_release_notes_for_version`) sees the same merged text.
fn release_changelog(
    pkg: &Package,
    target: &Version,
    reasons: &[Reason],
    digest: &HistoryDigest,
    cfg: &PlanConfig,
    notes: &[ChangeNote],
) -> PlanChangelog {
    PlanChangelog {
        file: pkg
            .changelog
            .as_ref()
            .map_or_else(|| default_changelog_file(pkg), ToString::to_string),
        entry_md: render_entry(target, reasons, digest, cfg, notes),
    }
}

/// Renders one release's changelog preview (§9.4 keep-a-changelog subset):
/// breaking changes first, then features, fixes and performance notes,
/// then the change-file notes (ADR-0028) verbatim — one blank-line
/// separated block per note, in path order, no invented headings (no
/// fabrication). Hidden types (per config) are dropped; output is
/// deterministic (§5.6 reason order is the emit order, notes are the
/// record order).
fn render_entry(
    version: &Version,
    reasons: &[Reason],
    digest: &HistoryDigest,
    cfg: &PlanConfig,
    notes: &[ChangeNote],
) -> String {
    let by_oid: BTreeMap<&str, usize> = digest
        .commits
        .iter()
        .enumerate()
        .map(|(i, c)| (c.oid.as_str(), i))
        .collect();
    let mut breaking: Vec<String> = Vec::new();
    let mut features: Vec<String> = Vec::new();
    let mut fixes: Vec<String> = Vec::new();
    let mut perf: Vec<String> = Vec::new();
    for reason in reasons {
        let Reason::Commit {
            oid,
            kind,
            breaking: is_breaking,
        } = reason
        else {
            continue;
        };
        let kind_str = String::from_utf8_lossy(kind);
        if cfg.changelog_hidden_types.contains(kind_str.as_ref()) {
            continue;
        }
        let Some(commit) = by_oid
            .get(oid.as_str())
            .and_then(|&ci| digest.commits.get(ci))
        else {
            continue;
        };
        let description = String::from_utf8_lossy(&commit.parsed.description).to_string();
        let mut line = format!("- {description}");
        if let Some(pr) = commit.pr {
            let _ = write!(line, " (#{pr})");
        }
        if *is_breaking {
            breaking.push(line);
        } else {
            match kind_str.as_ref() {
                "feat" => features.push(line),
                "fix" => fixes.push(line),
                "perf" => perf.push(line),
                _ => {}
            }
        }
    }
    let mut out = format!("## {version}\n");
    for (title, lines) in [
        ("⚠ BREAKING CHANGES", &breaking),
        ("Features", &features),
        ("Bug Fixes", &fixes),
        ("Performance", &perf),
    ] {
        if lines.is_empty() {
            continue;
        }
        let _ = write!(out, "\n### {title}\n\n");
        for line in lines {
            out.push_str(line);
            out.push('\n');
        }
    }
    for note in notes {
        out.push('\n');
        out.push_str(&note.body);
        out.push('\n');
    }
    out
}

/// `0.0.0` for packages without a version (the wire format's documented
/// placeholder, §5.8 `from`).
fn zero_version() -> Version {
    Version::new(0, 0, 0)
}

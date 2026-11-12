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
//!    0.x remap via [`apply_bump`](crate::bump::apply_bump)). A
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
//!    rewritten through [`rewrite_requirement`](crate::rewrite::rewrite_requirement)
//!    (appendix A.2), then versions, tags, changelog previews, byte edits,
//!    publish decisions and the skipped list are emitted; the document is
//!    hashed ([`plan_hash`](crate::canonical::plan_hash)).

use std::collections::{BTreeMap, BTreeSet};

use semver::Version;

use crate::bump::{apply_bump, PreMajorPolicy};
use crate::graph::WorkspaceGraph;
use crate::history::{HistoryDigest, HistoryProvider, ReleaseRefs};
use crate::id::PkgId;
use crate::link::LinkMode;
use crate::package::{
    DepEdge, DepKind, Ecosystem, GroupPolicy, ManifestSite, Package, ReqSpec, TagTemplate,
    Workspace,
};
use crate::plan::{
    Bump, Plan, PlanChangelog, PlanEdit, PlanPatch, PlanPublish, PlanReason, PlanRelease, PlanSkip,
    PLAN_SCHEMA_V1,
};
use crate::reason::{CascadeWhy, Reason};
use crate::rewrite::{rewrite_requirement, RewriteOutcome, UpperBoundPolicy};

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
    /// Packages that must not be released (`[[package]] release = false`).
    pub release_disabled: BTreeSet<String>,
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
            release_disabled: BTreeSet::new(),
        }
    }
}

impl Default for PlanConfig {
    fn default() -> Self {
        Self::default_values()
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
}

impl PlanError {
    /// Stable §15.4 code of this error.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Cycle { .. } => "E-PLAN-CYCLE",
            Self::ReleaseAsVersion { .. } => "E-PLAN-RELEASE-AS",
            Self::Unrewritable { .. } => "E-PLAN-UNREWRITABLE",
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
        }
    }
}

impl std::error::Error for PlanError {}

/// The planner's immutable input (§4): discovered workspace, the
/// materialized history between HEAD and every package's release base,
/// and the resolved per-package release refs.
#[derive(Clone, Copy)]
pub struct Snapshot<'a> {
    /// The discovered workspace (packages, groups, links).
    pub workspace: &'a Workspace,
    /// Pre-materialized history digest (§5.7) — built by the shell, read
    /// purely by the planner (I6).
    pub history: &'a dyn HistoryProvider,
    /// Per-package release bases (§8.2); `None` = initial release.
    pub refs: &'a ReleaseRefs,
}

/// Plans one release (§4, appendix A.1).
///
/// # Errors
///
/// [`PlanError::Cycle`] when the workspace has a non-dev dependency
/// cycle, [`PlanError::ReleaseAsVersion`] for unusable `Release-As`
/// footers, [`PlanError::Unrewritable`] when a crossed requirement
/// cannot be rewritten under the configured policy.
pub fn plan(snapshot: &Snapshot<'_>, cfg: &PlanConfig) -> Result<Plan, PlanError> {
    let ws = snapshot.workspace;
    let digest = snapshot.history.digest();
    let graph = WorkspaceGraph::build(ws);
    let order = graph.topo_order().map_err(|e| match e {
        crate::graph::GraphError::Cycle { rendered, .. } => PlanError::Cycle { rendered },
    })?;

    let mut state = State {
        bumps: vec![Bump::None; ws.packages().len()],
        forced: vec![None; ws.packages().len()],
        reasons: vec![Vec::new(); ws.packages().len()],
    };

    direct_bumps(snapshot, cfg, &mut state)?;
    normalize_groups(ws, cfg, &mut state);
    cascade(ws, &graph, &order, cfg, &mut state);

    let targets = materialize_versions(ws, cfg, &state)?;
    let rewrites = collect_rewrites(ws, cfg, &targets)?;

    let mut releases: Vec<PlanRelease> = Vec::new();
    let mut edits: BTreeMap<(String, u32, u32), String> = BTreeMap::new();
    let mut skipped: Vec<PlanSkip> = Vec::new();

    for pkg in ws.packages() {
        let name = pkg_name(ws, pkg);
        let Some(target) = targets.get(&pkg.id) else {
            skipped.push(PlanSkip {
                package: name.to_owned(),
                reason: skip_reason(pkg, cfg, ws),
            });
            continue;
        };
        let from = pkg.version.clone().unwrap_or_else(zero_version);
        let bump = minimal_bump(&from, target);
        let tag = effective_template(ws, pkg, cfg)
            .render(name, target, pkg.ecosystem.resolve(ws.interner()));
        let reasons: Vec<PlanReason> = state
            .reasons(pkg.id)
            .iter()
            .map(|reason| wire_reason(reason, ws))
            .collect();
        let changelog = pkg.changelog.as_ref().map(|file| PlanChangelog {
            file: file.to_string(),
            entry_md: render_entry(target, state.reasons(pkg.id), digest, cfg),
        });
        let registry = match pkg.ecosystem {
            Ecosystem::Cargo => cfg.publish_cargo_registry.clone(),
            _ => cfg.publish_npm_registry.clone(),
        };
        releases.push(PlanRelease {
            package: name.to_owned(),
            ecosystem: pkg.ecosystem.resolve(ws.interner()).to_owned(),
            from: from.to_string(),
            to: target.to_string(),
            bump,
            tag,
            reasons,
            changelog,
            publish: Some(PlanPublish {
                registry,
                enabled: pkg.publishable && cfg.publish_enabled,
            }),
        });
        // Own version site (inherited sites dedupe through the keyed map).
        if let Some(site) = &pkg.version_site {
            add_edit(&mut edits, site, target.to_string());
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
        if let Some(slot) = self.bumps.get_mut(id.to_usize()) {
            if *slot < b {
                *slot = b;
            }
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
fn direct_bumps(snapshot: &Snapshot<'_>, cfg: &PlanConfig, state: &mut State) -> Result<(), PlanError> {
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
                std::str::from_utf8(&t.key)
                    .map(|k| k.eq_ignore_ascii_case("Release-As"))
                    .unwrap_or(false)
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
                        })
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
    }
    Ok(())
}

/// Step 2: group + lockstep-link normalization (A.1 "groups normalize").
/// Every member rises to the strongest bump in its union; raised members
/// gain a [`CascadeWhy::LockstepGroup`] cascade reason.
fn normalize_groups(ws: &Workspace, cfg: &PlanConfig, state: &mut State) {
    let mut pairs: Vec<(PkgId, PkgId)> = Vec::new();
    let mut single_union: Option<PkgId> = None;
    if cfg.group_mode != GroupMode::Independent {
        // Repo-wide Fixed/Linked policy: one union over every package.
        single_union = ws.packages().first().map(|p| p.id);
        for pkg in ws.packages().iter().skip(1) {
            if let Some(first) = single_union {
                pairs.push((first, pkg.id));
            }
        }
    } else {
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

/// Step 4a: the next version per releasing package (id → target).
fn materialize_versions(
    ws: &Workspace,
    cfg: &PlanConfig,
    state: &State,
) -> Result<BTreeMap<PkgId, Version>, PlanError> {
    let mut targets = BTreeMap::new();
    for pkg in ws.packages() {
        let id = pkg.id;
        let name = pkg_name(ws, pkg);
        if cfg.release_disabled.contains(name) {
            continue;
        }
        let Some(from) = pkg.version.clone() else {
            continue;
        };
        let target = match state.forced(id) {
            Some(v) => v.clone(),
            None => {
                let bump = state.bump(id);
                if bump.is_none() {
                    continue;
                }
                apply_bump(&from, bump, &cfg.pre_major)
            }
        };
        if target <= from {
            // Forced targets are validated on ingest; this guard keeps a
            // pathological 0.x remap from ever emitting a regression.
            continue;
        }
        targets.insert(id, target);
    }
    Ok(targets)
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
                    })
                }
            }
        }
    }
    // Deterministic output regardless of edge order: sort by site.
    out.sort_by(|a, b| {
        site_key(&a.0).cmp(&site_key(&b.0))
    });
    out.dedup_by(|a, b| site_key(&a.0) == site_key(&b.0) && a.1 == b.1);
    Ok(out)
}

/// Sort/dedup key of an edit site: file, then first span.
fn site_key(site: &ManifestSite) -> (String, u32, u32) {
    (
        site.file.to_string(),
        site.spans.first().map(|s| s.start).unwrap_or(u32::MAX),
        site.spans.first().map(|s| s.end).unwrap_or(u32::MAX),
    )
}

/// Groups the flat patch list per file, spans sorted ascending.
fn group_edits(edits: BTreeMap<(String, u32, u32), String>) -> Vec<PlanEdit> {
    let mut files: BTreeMap<String, Vec<PlanPatch>> = BTreeMap::new();
    for ((file, start, end), text) in edits {
        files.entry(file).or_default().push(PlanPatch {
            start,
            end,
            text,
        });
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
fn skip_reason(pkg: &Package, cfg: &PlanConfig, ws: &Workspace) -> String {
    let name = pkg_name(ws, pkg);
    if pkg.version.is_none() {
        "no_version".to_owned()
    } else if cfg.release_disabled.contains(name) {
        "release_disabled".to_owned()
    } else {
        "no_changes".to_owned()
    }
}

/// Minimal bump taking `from` to `to` (used for forced releases).
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
/// discovery-resolved template.
fn effective_template(ws: &Workspace, pkg: &Package, cfg: &PlanConfig) -> TagTemplate {
    if let Some(template) = &cfg.tag_template {
        return template.clone();
    }
    if ws.packages().len() == 1 {
        if let Ok(single) = TagTemplate::parse("v{version}") {
            return single;
        }
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

/// Renders one release's changelog preview (§9.4 keep-a-changelog subset):
/// breaking changes first, then features, fixes and performance notes.
/// Hidden types (per config) are dropped; output is deterministic (§5.6
/// reason order is the emit order).
fn render_entry(
    version: &Version,
    reasons: &[Reason],
    digest: &HistoryDigest,
    cfg: &PlanConfig,
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
        let Some(commit) = by_oid.get(oid.as_str()).and_then(|&ci| digest.commits.get(ci)) else {
            continue;
        };
        let description = String::from_utf8_lossy(&commit.parsed.description).to_string();
        let mut line = format!("- {description}");
        if let Some(pr) = commit.pr {
            line.push_str(&format!(" (#{pr})"));
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
        out.push_str(&format!("\n### {title}\n\n"));
        for line in lines {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// `0.0.0` for packages without a version (the wire format's documented
/// placeholder, §5.8 `from`).
fn zero_version() -> Version {
    Version::new(0, 0, 0)
}

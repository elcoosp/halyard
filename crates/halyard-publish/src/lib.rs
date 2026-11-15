//! The publish engine (M5.2, §13) — the orchestration half of
//! `publish`: what order packages publish in, what is already on the
//! registry, and what a dependent may safely publish after.
//!
//! The engine consumes a **verified plan** ([`plan_publication`]):
//! the plan's self-consistent `plan_hash` is recomputed before anything
//! happens (I8 — "never publish what wasn't planned"), every
//! publish-enabled release is joined against the workspace (unknown
//! package, unpublishable member, or a target outside `plan.order` is a
//! fail-closed `E-PUB-PLAN`), and the targets are layered over the
//! workspace graph's non-dev edges — dependencies first (§13 step 1). A
//! cycle among the targets is `E-PUB-ORDER`; dev-only cycles are legal
//! (§6.6) and never constrain the order.
//!
//! The run ([`run_publication`]) is the §13 resume loop (I2):
//!
//! 1. **Resume probe** (§13 step 2) — one `published()` read per
//!    target; an already-visible version is an effect-free no-op
//!    (`AlreadyPublished`, I1 — the re-run after an interruption never
//!    duplicates an artifact).
//! 2. **Publish effect** — the [`PublishEffect`] seam (the tool spawn;
//!    the scripted [`MemoryPublishEffect`] offline, the real
//!    `cargo publish`/`pnpm publish` spawn with the CLI shell). A
//!    failure is a **row** (`Failed`), never a run abort: the §13.7
//!    failure model persists the outcome and the re-run continues from
//!    the first failure.
//! 3. **Visibility wait** (§13 step 3) — after a successful publish the
//!    engine polls [`Registry::published`] through the registry crate's
//!    deterministic doubling-capped backoff before any dependent
//!    publishes. Exhaustion is honest: the row is `Published` with
//!    `visibility_confirmed: false` and dependents are skipped
//!    (`upstream_invisible:<pkg>`) — never guessed.
//!
//! A failed or skipped upstream gates its dependents
//! (`upstream_failed:` / `upstream_skipped:`) — publishing a dependent
//! of an artifact the registry does not carry would ship a broken
//! release. The report ([`PublishReport`]) is deterministic (I3): rows
//! in layer order, name-sorted inside a layer, so even the bounded
//! within-layer parallelism ([`PublishPolicy::max_parallel`]) cannot
//! reorder the output.
//!
//! Errors (§15.4 `E-PUB-*`): [`PublishError::Plan`] (`E-PUB-PLAN`) for
//! an unusable plan, [`PublishError::Order`] (`E-PUB-ORDER`) for a
//! publish cycle, [`PublishError::Registry`] (`E-PUB-REGISTRY`) when no
//! client is configured for a target's registry, [`PublishError::
//! Effect`] (`E-PUB-EFFECT`) for tool failures (also the row-level
//! shape) and [`PublishError::Auth`] (`E-PUB-AUTH`, M5.3 step 1) for a
//! present-but-empty publish token — the auth seam's fail-closed
//! misconfiguration, refused before any spawn. Registry probe failures
//! ride their own `E-REG-*` codes — the row carries the probe's stable
//! code, not a wrapper's.
//!
//! Layer note: `publish → [core, registry]` (§7.2; the registry rides
//! the forge transport). The engine owns no I/O: registries, the effect
//! and the clock are injected seams — and the auth plumbing ([`mod@
//! auth`]) resolves through the shell-owned [`PublishAuth`] seam (the
//! production [`EnvPublishAuth`] captures the process environment once;
//! ADR-0019), with [`scrub_secrets`] keeping resolved tokens out of
//! surfaced tool output.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::PoisonError;
use std::thread;
use std::time::Duration;

use halyard_core::graph::WorkspaceGraph;
use halyard_core::package::{DepKind, Workspace};
use halyard_core::plan::Plan;
use halyard_core::plan_hash;
use halyard_registry::{
    Registry, RegistryError, RegistrySleeper, VisibilityPolicy, wait_until_visible,
};

pub mod auth;

pub use auth::{
    AuthOutcome, CredentialEnv, EnvPublishAuth, PublishAuth, auth_summary, scrub_secrets,
    variables_for,
};

// ---------------------------------------------------------------------------
// Error taxonomy (§15.4 E-PUB-*)
// ---------------------------------------------------------------------------

/// Publish-engine failures (§15.4 `E-PUB-*`; each variant has a page
/// under `docs/errors/`). Run-level variants abort the run before any
/// effect; tool failures surface as report rows (§13.7), not errors.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PublishError {
    /// The plan is unusable for publishing (`E-PUB-PLAN`): the embedded
    /// hash does not match the content (I8), a publish-enabled release
    /// names no workspace package or an unpublishable member, a target
    /// is missing from `plan.order`, or a release name is duplicated.
    Plan {
        /// What is wrong and what to do instead.
        message: String,
    },
    /// The publish targets contain a dependency cycle over non-dev
    /// edges (`E-PUB-ORDER`): topological layers are undefined. The
    /// message renders the cycle path; dev-only cycles never reach this
    /// variant (§6.6).
    Order {
        /// The cycle path and the recovery hint.
        message: String,
    },
    /// No registry client is configured for a target's registry name
    /// (`E-PUB-REGISTRY`): the shell owns the routing map; a missing
    /// entry is a configuration gap, refused before any call.
    Registry {
        /// The registry name the engine could not route.
        registry: String,
        /// What the router reported.
        message: String,
    },
    /// The publish tool failed (`E-PUB-EFFECT`): the spawned
    /// `cargo publish`/`pnpm publish` (or the scripted stand-in) errored.
    /// Row-level by design (§13.7); `retryable` classifies the failure
    /// for the re-run policy.
    Effect {
        /// The ecosystem whose tool failed (`cargo`, `npm`, …).
        ecosystem: String,
        /// What the tool reported.
        message: String,
        /// Whether a re-run may plausibly succeed (transport-class
        /// failure) or the request itself is broken.
        retryable: bool,
    },
    /// A publish token is present but empty (`E-PUB-AUTH`, M5.3 step
    /// 1): with a `${VAR}` auth-file interpolation the empty value
    /// becomes an empty bearer token and a confusing 401. The auth
    /// seam's fail-closed misconfiguration — refused before any spawn,
    /// naming the variable. Structural: a re-run cannot fix the
    /// environment.
    Auth {
        /// The registry the credentials were resolved for (or `env`
        /// when the capture-level scan found the misconfiguration).
        registry: String,
        /// The variable name and the fix.
        message: String,
    },
}

impl PublishError {
    /// Stable §15.4 code of this error.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Plan { .. } => "E-PUB-PLAN",
            Self::Order { .. } => "E-PUB-ORDER",
            Self::Registry { .. } => "E-PUB-REGISTRY",
            Self::Effect { .. } => "E-PUB-EFFECT",
            Self::Auth { .. } => "E-PUB-AUTH",
        }
    }

    /// Whether a re-run may plausibly succeed. Only the row-level
    /// [`PublishError::Effect`] carries the classification; the
    /// structural variants are deterministic failures of their input.
    #[must_use]
    pub fn retryable(&self) -> bool {
        match self {
            Self::Effect { retryable, .. } => *retryable,
            Self::Plan { .. } | Self::Order { .. } | Self::Registry { .. } | Self::Auth { .. } => {
                false
            }
        }
    }
}

impl std::fmt::Display for PublishError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Plan { message } => write!(f, "[E-PUB-PLAN] {message}"),
            Self::Order { message } => write!(f, "[E-PUB-ORDER] {message}"),
            Self::Registry { registry, message } => {
                write!(f, "[E-PUB-REGISTRY] {registry}: {message}")
            }
            Self::Effect {
                ecosystem, message, ..
            } => write!(f, "[E-PUB-EFFECT] {ecosystem}: {message}"),
            Self::Auth { registry, message } => write!(f, "[E-PUB-AUTH] {registry}: {message}"),
        }
    }
}

impl std::error::Error for PublishError {}

// ---------------------------------------------------------------------------
// Plan → targets + layers (§13 step 1, pure)
// ---------------------------------------------------------------------------

/// One package the engine will publish: the plan's release row joined
/// with its workspace package. Plain wire-shape data (§5.8).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublishTarget {
    /// Registry package name (crate name or npm name).
    pub package: String,
    /// The version being published — the plan release's `to` (§5.8);
    /// what the resume probe looks for.
    pub version: String,
    /// Ecosystem identifier (`cargo`, `npm`, …; open set, §5.2).
    pub ecosystem: String,
    /// Registry identifier from the plan's publish contract
    /// (`crates.io`, `npm`, …; open set).
    pub registry: String,
    /// Package root directory relative to the repo root (empty = repo
    /// root, §6.1) — where the publish tool runs (the npm cwd).
    pub manifest_dir: String,
    /// Manifest file path relative to the repo root (e.g.
    /// `crates/a/Cargo.toml`) — the cargo `--manifest-path` argument
    /// (M5.2 step 2: the spawn effect needs the file, not only the dir).
    pub manifest: String,
}

/// A release the engine will not publish, reported for observability
/// (§16.3: "always shows what was ignored and why").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkippedRelease {
    /// Package name.
    pub package: String,
    /// The release's `to` version (context for the human renderer).
    pub version: String,
    /// Why it is not a target (`publish_disabled`, `no_publish_contract`).
    pub reason: String,
}

/// The pure publish plan (§13 step 1): validated targets, their
/// topological layers (dependencies first) and the not-target releases.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Publication {
    /// Publish targets, sorted by package name (deterministic, I3).
    pub targets: Vec<PublishTarget>,
    /// Layer `k` lists the package names that may publish in parallel
    /// once every earlier layer completed; dependencies first (§13
    /// step 1). Each layer is name-sorted.
    pub layers: Vec<Vec<String>>,
    /// For each target name: its target-upstream names over non-dev
    /// edges, sorted — the gating inputs of the resume loop.
    pub upstreams: BTreeMap<String, Vec<String>>,
    /// Releases with a disabled or absent publish contract.
    pub not_targets: Vec<SkippedRelease>,
}

/// Plans the publication from a verified plan and its workspace (§13
/// step 1; I8 hash gate; pure — no I/O, no network).
///
/// # Errors
///
/// [`PublishError::Plan`] when the plan is unusable (hash mismatch,
/// unknown package, unpublishable member, target outside `plan.order`,
/// duplicate release names) and [`PublishError::Order`] when the
/// targets form a dependency cycle over non-dev edges.
pub fn plan_publication(plan: &Plan, ws: &Workspace) -> Result<Publication, PublishError> {
    // I8: publish consumes a verified plan only.
    if plan_hash(plan) != plan.plan_hash {
        return Err(PublishError::Plan {
            message: "the plan hash does not match its content — publish consumes a verified \
                      plan only (I8); recompute the plan at HEAD instead of editing it"
                .to_owned(),
        });
    }

    // Duplicate release names would make layering and gating ambiguous.
    let mut seen = BTreeSet::new();
    for release in &plan.releases {
        if !seen.insert(release.package.as_str()) {
            return Err(PublishError::Plan {
                message: format!(
                    "the plan releases {} twice — one release per package (I3); recompute the \
                     plan at HEAD",
                    release.package
                ),
            });
        }
    }

    let (targets, not_targets) = join_targets(plan, ws)?;
    let layering = layer_targets(ws, &targets)?;
    Ok(Publication {
        targets,
        layers: layering.layers,
        upstreams: layering.upstreams,
        not_targets,
    })
}

/// Joins the plan's releases with the workspace packages (§13 step 1):
/// publish-enabled releases become [`PublishTarget`]s (validated: known
/// package, publishable, present in `plan.order`), the rest land in the
/// not-targets list. Name-sorted (deterministic, I3).
///
/// # Errors
///
/// [`PublishError::Plan`] for the contradictions listed on
/// [`plan_publication`].
fn join_targets(
    plan: &Plan,
    ws: &Workspace,
) -> Result<(Vec<PublishTarget>, Vec<SkippedRelease>), PublishError> {
    // Join releases × workspace; route by the publish contract.
    let mut targets: Vec<PublishTarget> = Vec::new();
    let mut not_targets: Vec<SkippedRelease> = Vec::new();
    for release in &plan.releases {
        let publish = match &release.publish {
            Some(publish) if publish.enabled => publish,
            Some(_) => {
                not_targets.push(SkippedRelease {
                    package: release.package.clone(),
                    version: release.to.clone(),
                    reason: "publish_disabled".to_owned(),
                });
                continue;
            }
            None => {
                not_targets.push(SkippedRelease {
                    package: release.package.clone(),
                    version: release.to.clone(),
                    reason: "no_publish_contract".to_owned(),
                });
                continue;
            }
        };
        let Some(id) = ws.find_by_name(&release.package) else {
            return Err(PublishError::Plan {
                message: format!(
                    "release {} names no workspace package — publish consumes the plan only \
                     with the workspace it was planned from (I8)",
                    release.package
                ),
            });
        };
        let Some(package) = ws.package(id) else {
            unreachable!("find_by_name returned an in-range id")
        };
        if !package.publishable {
            return Err(PublishError::Plan {
                message: format!(
                    "release {} is publish-enabled but the workspace marks the package not \
                     publishable (publish = false / private: true, §6.3/§6.4) — a \
                     contradictory plan is refused, never half-honored",
                    release.package
                ),
            });
        }
        if !plan.order.iter().any(|name| name == &release.package) {
            return Err(PublishError::Plan {
                message: format!(
                    "release {} is a publish target but is absent from the plan's topological \
                     `order` — an internally inconsistent plan is refused (§5.8)",
                    release.package
                ),
            });
        }
        targets.push(PublishTarget {
            package: release.package.clone(),
            version: release.to.clone(),
            ecosystem: release.ecosystem.clone(),
            registry: publish.registry.clone(),
            manifest_dir: package.root.as_str().to_owned(),
            manifest: package.manifest.as_str().to_owned(),
        });
    }
    targets.sort_unstable_by(|a, b| a.package.cmp(&b.package));
    not_targets.sort_unstable_by(|a, b| a.package.cmp(&b.package));
    Ok((targets, not_targets))
}

/// The layering result: the name-sorted layers plus the per-target
/// upstream map (the run's gating inputs).
struct Layering {
    /// Layer `k` lists the names that may publish after every earlier
    /// layer completed; each layer is name-sorted.
    layers: Vec<Vec<String>>,
    /// Target name → its target-upstream names over non-dev edges
    /// (sorted).
    upstreams: BTreeMap<String, Vec<String>>,
}

/// Layers the targets over the workspace graph's non-dev edges,
/// restricted to the target-induced subgraph (§13 step 1; dev cycles
/// are legal, §6.6, and never constrain publishing). Returns the
/// name-sorted layers and the per-target upstream map (the run's
/// gating inputs). Dependencies first — a package publishes only after
/// everything it depends on.
///
/// # Errors
///
/// [`PublishError::Order`] when the targets form a dependency cycle
/// over non-dev edges.
fn layer_targets(ws: &Workspace, targets: &[PublishTarget]) -> Result<Layering, PublishError> {
    let graph = WorkspaceGraph::build(ws);
    let mut names_by_id: BTreeMap<u32, &str> = BTreeMap::new();
    for target in targets {
        let Some(id) = ws.find_by_name(&target.package) else {
            unreachable!("validated target name")
        };
        names_by_id.insert(id.to_u32(), target.package.as_str());
    }

    let mut upstreams: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    let mut pending_count: BTreeMap<&str, usize> = BTreeMap::new();
    for target in targets {
        let Some(id) = ws.find_by_name(&target.package) else {
            unreachable!("validated target name")
        };
        let mut ups = BTreeSet::new();
        if let Some(edges) = graph.out_edges(id) {
            for (to, kind) in edges {
                if kind == DepKind::Dev {
                    continue;
                }
                if let Some(upstream) = names_by_id.get(&to.to_u32()) {
                    ups.insert(*upstream);
                }
            }
        }
        pending_count.insert(target.package.as_str(), ups.len());
        upstreams.insert(target.package.as_str(), ups);
    }

    // Kahn layering, name-ascending emission (deterministic, I3):
    // `ready` is kept name-descending so `pop` yields ascending order.
    let mut layer_of: BTreeMap<&str, usize> = BTreeMap::new();
    let mut ready: Vec<&str> = targets
        .iter()
        .map(|t| t.package.as_str())
        .filter(|name| upstreams.get(*name).is_some_and(BTreeSet::is_empty))
        .collect();
    ready.sort_unstable_by(|a, b| b.cmp(a));
    while let Some(name) = ready.pop() {
        let own = upstreams
            .get(name)
            .and_then(|ups| {
                ups.iter()
                    .filter_map(|upstream| layer_of.get(*upstream))
                    .max()
            })
            .map_or(0, |max| *max + 1);
        layer_of.insert(name, own);
        for (dependent, ups) in &upstreams {
            if layer_of.contains_key(*dependent) || !ups.contains(name) {
                continue;
            }
            let Some(remaining) = pending_count.get_mut(*dependent) else {
                unreachable!("counted upstream")
            };
            *remaining -= 1;
            if *remaining == 0 {
                ready.push(*dependent);
                ready.sort_unstable_by(|a, b| b.cmp(a));
            }
        }
    }

    if layer_of.len() != targets.len() {
        return Err(PublishError::Order {
            message: format!(
                "the publish targets contain a dependency cycle over non-dev edges ({}) — \
                 publish order is undefined; break the cycle or exclude one member from \
                 publishing",
                render_cycle(&upstreams, targets)
            ),
        });
    }

    let depth = layer_of.values().copied().max().map_or(0, |max| max + 1);
    let mut layers: Vec<Vec<String>> = vec![Vec::new(); depth];
    for (name, layer) in &layer_of {
        let Some(slot) = layers.get_mut(*layer) else {
            unreachable!("layer index is below the computed depth")
        };
        slot.push((*name).to_owned());
    }
    for layer in &mut layers {
        layer.sort_unstable();
    }

    let upstream_names: BTreeMap<String, Vec<String>> = upstreams
        .into_iter()
        .map(|(name, ups)| {
            (
                name.to_owned(),
                ups.into_iter().map(ToOwned::to_owned).collect(),
            )
        })
        .collect();

    Ok(Layering {
        layers,
        upstreams: upstream_names,
    })
}

/// Renders one cycle path among the un-layered targets (deterministic
/// DFS in name order; the caller only invokes this after a Kahn
/// deficit, so a cycle exists and a path is found).
fn render_cycle(upstreams: &BTreeMap<&str, BTreeSet<&str>>, targets: &[PublishTarget]) -> String {
    let mut visited: BTreeSet<&str> = BTreeSet::new();
    for target in targets {
        if visited.contains(target.package.as_str()) {
            continue;
        }
        let mut path: Vec<&str> = Vec::new();
        let mut on_path: BTreeSet<&str> = BTreeSet::new();
        if let Some(cycle) = dfs_cycle(
            target.package.as_str(),
            upstreams,
            &mut visited,
            &mut on_path,
            &mut path,
        ) {
            return cycle;
        }
    }
    String::new()
}

/// Depth-first cycle search over the target upstream map; `path`
/// carries the current trail, `on_path` its membership set. Returns
/// the rendered cycle (closed by repeating the entry node).
fn dfs_cycle<'a>(
    node: &'a str,
    upstreams: &BTreeMap<&'a str, BTreeSet<&'a str>>,
    visited: &mut BTreeSet<&'a str>,
    on_path: &mut BTreeSet<&'a str>,
    path: &mut Vec<&'a str>,
) -> Option<String> {
    visited.insert(node);
    on_path.insert(node);
    path.push(node);
    if let Some(ups) = upstreams.get(node) {
        for next in ups {
            if on_path.contains(*next) {
                let position = path
                    .iter()
                    .position(|step| *step == *next)
                    .unwrap_or_else(|| unreachable!("on_path ⇒ in path"));
                let mut cycle = path
                    .get(position..)
                    .unwrap_or_else(|| unreachable!("position came from the path itself"))
                    .to_vec();
                cycle.push(*next);
                return Some(cycle.join(" -> "));
            }
            if !visited.contains(*next)
                && let Some(cycle) = dfs_cycle(next, upstreams, visited, on_path, path)
            {
                return Some(cycle);
            }
        }
    }
    path.pop();
    on_path.remove(node);
    None
}

// ---------------------------------------------------------------------------
// Seams: registry routing + publish effect
// ---------------------------------------------------------------------------

/// The registry-routing seam: which [`Registry`] probes a target's
/// registry name. The shell owns the map (live `crates.io`/`npm`
/// clients over the blocking transport); tests inject scripted ones.
pub trait RegistryRouter: Send + Sync {
    /// The existence-probe client for `name` (`crates.io`, `npm`, …).
    ///
    /// # Errors
    ///
    /// [`PublishError::Registry`] when no client is configured — the
    /// run refuses before touching any registry or tool.
    fn registry(&self, name: &str) -> Result<&dyn Registry, PublishError>;
}

/// The scripted [`RegistryRouter`] — a name→client map (the routing
/// analogue of the forge `MemoryTransport`'s FIFO). Clones share the
/// map (an `Arc` inner, the house fake convention).
#[derive(Clone, Default)]
pub struct MapRouter {
    map: Arc<BTreeMap<String, Arc<dyn Registry>>>,
}

impl std::fmt::Debug for MapRouter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The clients are trait objects (no Debug); the routed names
        // are the observable shape.
        let names: Vec<&String> = self.map.keys().collect();
        f.debug_struct("MapRouter").field("routes", &names).finish()
    }
}

impl MapRouter {
    /// An empty router (every target is unroutable until `with`).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds (or replaces) the client for `name`; builder style.
    #[must_use]
    pub fn with(mut self, name: &str, registry: Arc<dyn Registry>) -> Self {
        let owned = Arc::try_unwrap(self.map).unwrap_or_else(|shared| (*shared).clone());
        let mut map = owned;
        map.insert(name.to_owned(), registry);
        self.map = Arc::new(map);
        self
    }
}

impl RegistryRouter for MapRouter {
    fn registry(&self, name: &str) -> Result<&dyn Registry, PublishError> {
        self.map.get(name).map_or_else(
            || {
                Err(PublishError::Registry {
                    registry: name.to_owned(),
                    message: "no registry client is configured for this name — the shell owns \
                              the routing map; configure the registry or disable publishing \
                              for its packages"
                        .to_owned(),
                })
            },
            |registry| Ok(registry.as_ref()),
        )
    }
}

/// The publish effect seam (§13 step 4): "shell out to `cargo publish`
/// / `pnpm publish` — they own packaging semantics; our job: ordering,
/// resume, auth plumbing". One call per target; the implementation owns
/// the spawn (argv arrays only, I7) or scripts it.
pub trait PublishEffect: Send + Sync {
    /// Publishes `target`'s exact `version` from its `manifest_dir`.
    ///
    /// # Errors
    ///
    /// [`PublishError::Effect`] when the tool fails; `retryable`
    /// classifies the failure for the re-run policy (§13.7).
    fn publish(&self, target: &PublishTarget) -> Result<(), PublishError>;
}

/// The scripted [`PublishEffect`] — the tool analogue of the forge's
/// `MemoryForge`: succeeds unless a failure is scripted for the
/// package, records every request (even failing ones — a dropped call
/// is an unobservable one, §11.5) and, when coupled with a
/// [`MemoryRegistry`], stages successful uploads into it so the
/// visibility wait observes a realistic index lag.
#[derive(Clone, Debug, Default)]
pub struct MemoryPublishEffect {
    inner: Arc<EffectInner>,
}

#[derive(Debug, Default)]
struct EffectInner {
    failures: Mutex<BTreeMap<String, VecDeque<PublishError>>>,
    requests: Mutex<Vec<PublishTarget>>,
    successes: Mutex<Vec<String>>,
    registry: Mutex<Option<MemoryRegistry>>,
}

impl MemoryPublishEffect {
    /// A standalone scripted effect (nothing lands anywhere).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Couples the effect with `registry`: every scripted-success
    /// publish stages its version there (the modelled upload), and the
    /// registry's index lag gates the visibility wait.
    pub fn publishes_into(&self, registry: &MemoryRegistry) {
        *self
            .inner
            .registry
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(registry.clone());
    }

    /// Scripts one failure for `package` — consumed by its next
    /// `publish` call (the §15.2 failure injector at the effect layer).
    pub fn fail(&self, package: &str, error: PublishError) {
        self.inner
            .failures
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(package.to_owned())
            .or_default()
            .push_back(error);
    }

    /// Every request seen so far, oldest first.
    #[must_use]
    pub fn requests(&self) -> Vec<PublishTarget> {
        self.inner
            .requests
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The packages whose publish succeeded so far (each exactly once
    /// — the zero-duplicates witness for the §15.2 matrix).
    #[must_use]
    pub fn successes(&self) -> Vec<String> {
        self.inner
            .successes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl PublishEffect for MemoryPublishEffect {
    fn publish(&self, target: &PublishTarget) -> Result<(), PublishError> {
        self.inner
            .requests
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(target.clone());
        let scripted = self
            .inner
            .failures
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get_mut(&target.package)
            .and_then(VecDeque::pop_front);
        if let Some(error) = scripted {
            return Err(error);
        }
        self.inner
            .successes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(target.package.clone());
        let coupled = self
            .inner
            .registry
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        if let Some(registry) = coupled {
            registry.stage(&target.package, &target.version);
        }
        Ok(())
    }
}

/// The scripted [`Registry`] — the registry analogue for the publish
/// engine: a name→visible-versions map plus the modelled index lag.
/// Successful effect uploads are *staged*: each subsequent probe
/// advances a per-entry counter, and after `lag` misses the version
/// becomes visible (the §13 step-3 poll sequence, deterministic, I3).
#[derive(Clone, Debug)]
pub struct MemoryRegistry {
    inner: Arc<RegistryInner>,
}

#[derive(Debug, Default)]
struct RegistryInner {
    visible: Mutex<BTreeMap<String, BTreeSet<String>>>,
    staged: Mutex<BTreeMap<String, (String, u32)>>,
    lag: u32,
    probe_log: Mutex<Vec<(String, String)>>,
    fail: Mutex<Option<RegistryError>>,
}

impl Default for MemoryRegistry {
    fn default() -> Self {
        Self::with_lag(1)
    }
}

impl MemoryRegistry {
    /// An empty registry with one-probe index lag (the realistic
    /// default: the version turns visible on the probe after the
    /// upload).
    #[must_use]
    pub fn new() -> Self {
        Self::with_lag(1)
    }

    /// An empty registry with a custom index lag (probes a staged
    /// version must miss before it turns visible; `u32::MAX` = never
    /// within any bounded wait — the visibility-timeout injector).
    #[must_use]
    pub fn with_lag(lag: u32) -> Self {
        Self {
            inner: Arc::new(RegistryInner {
                lag,
                ..RegistryInner::default()
            }),
        }
    }

    /// Makes `version` of `name` immediately visible (pre-existing
    /// state — the `AlreadyPublished` path).
    pub fn insert(&self, name: &str, version: &str) {
        self.inner
            .visible
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(name.to_owned())
            .or_default()
            .insert(version.to_owned());
    }

    /// Scripted probe failure: every subsequent probe returns `error`
    /// (the dead-registry injector; connection-level errors are what a
    /// real transport raises).
    pub fn fail_probes_with(&self, error: RegistryError) {
        *self
            .inner
            .fail
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(error);
    }

    /// Whether `version` of `name` is visible right now (test driver —
    /// the §15.2 final-state assertion).
    #[must_use]
    pub fn is_visible(&self, name: &str, version: &str) -> bool {
        self.inner
            .visible
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(name)
            .is_some_and(|versions| versions.contains(version))
    }

    /// The probe log, oldest first — `(name, version)` per probe.
    #[must_use]
    pub fn probe_log(&self) -> Vec<(String, String)> {
        self.inner
            .probe_log
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Stages a successful upload (called by the coupled
    /// [`MemoryPublishEffect`]); the version turns visible after `lag`
    /// further probes.
    fn stage(&self, name: &str, version: &str) {
        self.inner
            .staged
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(name.to_owned(), (version.to_owned(), 0));
    }

    /// One probe (the house `MemoryTransport` discipline: locks are
    /// brief, poisoning cannot corrupt a total map + log, so the guard
    /// is recovered and the panic propagates from the *original* site).
    fn probe(&self, name: &str, version: &str) -> Result<bool, RegistryError> {
        {
            let fail = self
                .inner
                .fail
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if let Some(error) = fail.as_ref() {
                return Err(error.clone());
            }
        }
        self.inner
            .probe_log
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((name.to_owned(), version.to_owned()));
        if self.is_visible(name, version) {
            return Ok(true);
        }
        let promote = {
            let mut staged = self
                .inner
                .staged
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            match staged.get_mut(name) {
                Some((staged_version, misses)) if staged_version == version => {
                    *misses += 1;
                    *misses > self.inner.lag
                }
                _ => false,
            }
        };
        if promote {
            self.insert(name, version);
            return Ok(true);
        }
        Ok(false)
    }
}

impl Registry for MemoryRegistry {
    fn published(&self, name: &str, version: &str) -> Result<bool, RegistryError> {
        self.probe(name, version)
    }
}

// ---------------------------------------------------------------------------
// Policy + run (§13 steps 2–3, the resume loop)
// ---------------------------------------------------------------------------

/// The run mode (§13.8): the live run resumes through the registry
/// probes; the dry run forecasts — it validates routing, runs the
/// effect (the tool's own `--dry-run`) and reports [`PublishOutcome::
/// Forecast`] rows without ever touching the registry (I5: the resume
/// probes are the live run's inputs, and an offline forecast cannot
/// have them — the same reasoning that leaves `fetch` without a
/// `--dry`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PublishMode {
    /// The real run: resume probes, publishes, visibility waits (§13
    /// steps 2–3).
    #[default]
    Live,
    /// The offline forecast (§13.8): no probes, no waits, the effect
    /// runs its dry form.
    Dry,
}

/// The run policy: the mode (§13.8), the visibility-wait bounds (§13
/// step 3, "max wait configurable") and the within-layer parallelism
/// bound (§13 step 1, "packages within a layer publish concurrently
/// (bounded)").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PublishPolicy {
    /// Live or dry (§13.8). Routing validation is identical in both —
    /// an unroutable registry refuses either run before any call.
    pub mode: PublishMode,
    /// The §13 step-3 visibility wait bounds. The default resolves to
    /// ≈78 s of polling (under the §16.4 `wait_for_index = "90s"`
    /// posture): 7 polls, 2 s base, 16 s cap.
    pub visibility: VisibilityPolicy,
    /// Maximum targets of one layer publishing concurrently. `0`
    /// normalizes to `1` (a run always progresses); `1` is the fully
    /// deterministic sequential run. The report is name-sorted either
    /// way (I3).
    pub max_parallel: usize,
}

impl PublishPolicy {
    /// A policy with explicit bounds (live mode); `max_parallel` of `0`
    /// normalizes to `1`.
    #[must_use]
    pub fn new(visibility: VisibilityPolicy, max_parallel: usize) -> Self {
        Self {
            mode: PublishMode::Live,
            visibility,
            max_parallel: max_parallel.max(1),
        }
    }

    /// Overrides the run mode (builder style — the CLI's `--dry`).
    #[must_use]
    pub fn in_mode(mut self, mode: PublishMode) -> Self {
        self.mode = mode;
        self
    }

    /// The §16.4-posture default visibility bounds (≈78 s of polling).
    #[must_use]
    pub fn default_visibility() -> VisibilityPolicy {
        VisibilityPolicy {
            max_polls: 7,
            base_delay: Duration::from_secs(2),
            max_delay: Duration::from_secs(16),
        }
    }
}

impl Default for PublishPolicy {
    fn default() -> Self {
        Self::new(Self::default_visibility(), 1)
    }
}

/// One target's outcome (§13.7: each package outcome ∈ `Published` /
/// `AlreadyPublished` / `Skipped` / `Failed`; the row shape stays
/// plain data for the renderers).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PublishOutcome {
    /// The tool accepted the upload. `visibility_confirmed` records the
    /// §13 step-3 wait: `false` = the index never showed the version
    /// within the policy bounds (dependents are skipped honestly).
    Published {
        /// Whether the registry confirmed visibility.
        visibility_confirmed: bool,
    },
    /// The resume probe found the exact version already visible — the
    /// I1 no-op (the re-run after an interruption).
    AlreadyPublished,
    /// The dry-run effect accepted the request (§13.8): nothing was
    /// published — the row is the offline forecast's witness. Dry mode
    /// only.
    Forecast,
    /// The target published nothing this run, with the deterministic
    /// reason (`upstream_failed:<pkgs>`, `upstream_invisible:<pkgs>`,
    /// `upstream_skipped:<pkgs>` — names sorted, comma-joined).
    Skipped {
        /// Why the target was passed over.
        reason: String,
    },
    /// The tool or the registry refused; the run continues (§13.7) and
    /// dependents are gated. `code` carries the stable §15.4 code of
    /// the underlying failure (`E-PUB-EFFECT` or the probe's
    /// `E-REG-*`).
    Failed {
        /// The stable code of the underlying failure.
        code: String,
        /// The failure message (tool stderr or probe report).
        message: String,
        /// Whether a re-run may plausibly succeed.
        retryable: bool,
    },
}

/// One row of the publish report: what ran, where, and what happened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublishRow {
    /// Registry package name.
    pub package: String,
    /// The version this row is about (the plan's `to`).
    pub version: String,
    /// Ecosystem identifier (§5.2).
    pub ecosystem: String,
    /// Registry identifier (§13).
    pub registry: String,
    /// Manifest file path relative to the repo root (the spawn
    /// provenance — the cargo `--manifest-path` argument).
    pub manifest: String,
    /// Package root directory relative to the repo root (the spawn
    /// cwd).
    pub manifest_dir: String,
    /// The layer the target executed in (0 = no target dependencies).
    pub layer: usize,
    /// What happened.
    pub outcome: PublishOutcome,
}

/// The deterministic publish report (§13.7): rows in layer order,
/// name-sorted within a layer; counters derived from the rows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PublishReport {
    /// One row per target, in execution order.
    pub rows: Vec<PublishRow>,
    /// Targets whose tool accepted the upload.
    pub published: usize,
    /// Targets found already visible by the resume probe (I1 no-ops).
    pub already_published: usize,
    /// Targets forecast by the dry run (§13.8) — never published.
    pub forecast: usize,
    /// Targets passed over (upstream gating).
    pub skipped: usize,
    /// Targets that failed (tool or probe).
    pub failed: usize,
}

/// The upstream-gating state of a target after its layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TargetState {
    /// Visible on the registry (published-and-confirmed, or found).
    Visible,
    /// Published but the index never confirmed it.
    Invisible,
    /// The tool or the probe failed.
    Failed,
    /// Passed over this run.
    Skipped,
}

/// The per-run context the executor reads (bundled so the executor
/// stays under the argument budget and the parallel path can share it
/// by reference). `states` rides outside: it is frozen for the duration
/// of a layer (read-only gating) and mutated between layers.
struct RunContext<'a> {
    by_name: BTreeMap<&'a str, &'a PublishTarget>,
    registries: BTreeMap<&'a str, &'a dyn Registry>,
    upstreams: &'a BTreeMap<String, Vec<String>>,
    effect: &'a dyn PublishEffect,
    policy: &'a PublishPolicy,
    sleeper: &'a dyn RegistrySleeper,
}

/// Runs the publication (§13 steps 2–3): resume probes, the effect,
/// the visibility wait, upstream gating and the deterministic report.
/// Registry routing is resolved **before** any call: an unroutable
/// registry fails the run closed with zero side effects.
///
/// # Errors
///
/// [`PublishError::Registry`] when a target's registry has no client.
/// Everything else is a report row (§13.7: a tool failure never aborts
/// the run).
pub fn run_publication(
    publication: &Publication,
    router: &dyn RegistryRouter,
    effect: &dyn PublishEffect,
    policy: &PublishPolicy,
    sleeper: &dyn RegistrySleeper,
) -> Result<PublishReport, PublishError> {
    // Route every target first: no probe, no spawn before the whole
    // run's routing is known-good (validate-then-act, §15.3).
    let by_name: BTreeMap<&str, &PublishTarget> = publication
        .targets
        .iter()
        .map(|target| (target.package.as_str(), target))
        .collect();
    let mut registries: BTreeMap<&str, &dyn Registry> = BTreeMap::new();
    for (name, target) in &by_name {
        let registry = router.registry(&target.registry)?;
        registries.insert(*name, registry);
    }

    let states: BTreeMap<String, TargetState> = BTreeMap::new();
    let ctx = RunContext {
        by_name,
        registries,
        upstreams: &publication.upstreams,
        effect,
        policy,
        sleeper,
    };

    let mut rows: Vec<PublishRow> = Vec::new();
    let mut states = states;
    for (index, layer) in publication.layers.iter().enumerate() {
        let mut names = layer.clone();
        names.sort_unstable();
        let layer_rows = Mutex::new(Vec::new());
        if ctx.policy.max_parallel == 1 {
            for name in &names {
                execute_target(&ctx, name, index, &states, &layer_rows);
            }
        } else {
            // Bounded within-layer parallelism (§13.1): independent
            // targets share a layer by construction, so scheduling
            // order cannot affect outcomes — only the name-sorted
            // report matters (I3).
            let next = std::sync::atomic::AtomicUsize::new(0);
            let ordered: Vec<&str> = names.iter().map(String::as_str).collect();
            thread::scope(|scope| {
                for _ in 0..ctx.policy.max_parallel.min(ordered.len()) {
                    let _handle = scope.spawn(|| {
                        loop {
                            let i = next.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                            let Some(name) = ordered.get(i) else {
                                break;
                            };
                            execute_target(&ctx, name, index, &states, &layer_rows);
                        }
                    });
                }
            });
        }
        let mut finished = layer_rows
            .into_inner()
            .unwrap_or_else(PoisonError::into_inner);
        finished.sort_unstable_by(|a, b| a.package.cmp(&b.package));
        for row in &finished {
            let state = match &row.outcome {
                PublishOutcome::Published {
                    visibility_confirmed,
                } => {
                    if *visibility_confirmed {
                        TargetState::Visible
                    } else {
                        TargetState::Invisible
                    }
                }
                // A found version and a dry-run forecast both make the
                // target publishable for the dependents' gate.
                PublishOutcome::AlreadyPublished | PublishOutcome::Forecast => TargetState::Visible,
                PublishOutcome::Skipped { .. } => TargetState::Skipped,
                PublishOutcome::Failed { .. } => TargetState::Failed,
            };
            states.insert(row.package.clone(), state);
        }
        rows.append(&mut finished);
    }

    let (published, already_published, forecast, skipped, failed) = tallies(&rows);
    Ok(PublishReport {
        rows,
        published,
        already_published,
        forecast,
        skipped,
        failed,
    })
}

/// Executes one target inside its layer and appends the row (shared by
/// the sequential and the parallel path). `states` is frozen for the
/// layer's duration — upstream rows come from earlier layers only.
fn execute_target(
    ctx: &RunContext<'_>,
    name: &str,
    layer: usize,
    states: &BTreeMap<String, TargetState>,
    rows: &Mutex<Vec<PublishRow>>,
) {
    let Some(target) = ctx.by_name.get(name).copied() else {
        unreachable!("layer names come from the publication's own targets");
    };
    let Some(registry) = ctx.registries.get(name).copied() else {
        unreachable!("routing was resolved for every target before the run");
    };
    // Upstream gating: dependents of a failed / invisible / skipped
    // upstream publish nothing this run (the released artifact would
    // reference a version the registry does not carry).
    let outcome = if let Some(gate) = gate_upstreams(name, ctx.upstreams, states) {
        PublishOutcome::Skipped { reason: gate }
    } else if ctx.policy.mode == PublishMode::Dry {
        // §13.8: the dry forecast runs the effect's dry form (the
        // shell's effect decides the argv) and never probes — the
        // resume probes and the visibility wait are the live run's
        // registry inputs, which an offline forecast cannot have (I5).
        // Routing was validated up front, so `registry` is unused here
        // by design.
        match ctx.effect.publish(target) {
            Ok(()) => PublishOutcome::Forecast,
            Err(error) => PublishOutcome::Failed {
                code: error.code().to_owned(),
                message: error.to_string(),
                retryable: error.retryable(),
            },
        }
    } else {
        // §13 step 2: the resume probe decides between the I1 no-op
        // and the publish.
        match registry.published(&target.package, &target.version) {
            Ok(true) => PublishOutcome::AlreadyPublished,
            Ok(false) => match ctx.effect.publish(target) {
                Ok(()) => match wait_until_visible(
                    registry,
                    &target.package,
                    &target.version,
                    &ctx.policy.visibility,
                    ctx.sleeper,
                ) {
                    Ok(true) => PublishOutcome::Published {
                        visibility_confirmed: true,
                    },
                    Ok(false) => PublishOutcome::Published {
                        visibility_confirmed: false,
                    },
                    Err(error) => PublishOutcome::Failed {
                        code: error.code().to_owned(),
                        message: format!(
                            "the visibility probe for {}@{} failed: {error}",
                            target.package, target.version
                        ),
                        retryable: retryable_registry(&error),
                    },
                },
                Err(error) => PublishOutcome::Failed {
                    code: error.code().to_owned(),
                    message: error.to_string(),
                    retryable: error.retryable(),
                },
            },
            Err(error) => PublishOutcome::Failed {
                code: error.code().to_owned(),
                message: format!(
                    "the resume probe for {}@{} failed: {error}",
                    target.package, target.version
                ),
                retryable: retryable_registry(&error),
            },
        }
    };
    rows.lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(PublishRow {
            package: target.package.clone(),
            version: target.version.clone(),
            ecosystem: target.ecosystem.clone(),
            registry: target.registry.clone(),
            manifest: target.manifest.clone(),
            manifest_dir: target.manifest_dir.clone(),
            layer,
            outcome,
        });
}

/// The upstream gate for `name`, if any: the first failing class in
/// the fixed precedence `Failed → Invisible → Skipped`, with the
/// offending names sorted and comma-joined.
fn gate_upstreams(
    name: &str,
    upstreams: &BTreeMap<String, Vec<String>>,
    states: &BTreeMap<String, TargetState>,
) -> Option<String> {
    let ups = upstreams.get(name)?;
    let failed: Vec<&str> = ups
        .iter()
        .filter(|upstream| {
            states
                .get(*upstream)
                .is_some_and(|state| *state == TargetState::Failed)
        })
        .map(String::as_str)
        .collect();
    if !failed.is_empty() {
        return Some(format!("upstream_failed:{}", failed.join(",")));
    }
    let invisible: Vec<&str> = ups
        .iter()
        .filter(|upstream| {
            states
                .get(*upstream)
                .is_some_and(|state| *state == TargetState::Invisible)
        })
        .map(String::as_str)
        .collect();
    if !invisible.is_empty() {
        return Some(format!("upstream_invisible:{}", invisible.join(",")));
    }
    let skipped: Vec<&str> = ups
        .iter()
        .filter(|upstream| {
            states
                .get(*upstream)
                .is_some_and(|state| *state == TargetState::Skipped)
        })
        .map(String::as_str)
        .collect();
    if !skipped.is_empty() {
        return Some(format!("upstream_skipped:{}", skipped.join(",")));
    }
    None
}

/// The row-level retryability of a registry probe failure:
/// connection-class failures (`E-REG-IO`) may succeed on a re-run;
/// name and protocol errors are deterministic refusals.
fn retryable_registry(error: &RegistryError) -> bool {
    matches!(error, RegistryError::Io { .. })
}

/// Derives the report counters from the rows (the tallies are never
/// counted by hand — the house "report tallies derived from rows"
/// convention).
fn tallies(rows: &[PublishRow]) -> (usize, usize, usize, usize, usize) {
    let mut published = 0;
    let mut already = 0;
    let mut forecast = 0;
    let mut skipped = 0;
    let mut failed = 0;
    for row in rows {
        match row.outcome {
            PublishOutcome::Published { .. } => published += 1,
            PublishOutcome::AlreadyPublished => already += 1,
            PublishOutcome::Forecast => forecast += 1,
            PublishOutcome::Skipped { .. } => skipped += 1,
            PublishOutcome::Failed { .. } => failed += 1,
        }
    }
    (published, already, forecast, skipped, failed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_policy_is_sequential_and_matches_the_90s_posture() {
        let policy = PublishPolicy::default();
        assert_eq!(policy.max_parallel, 1);
        // 2 + 4 + 8 + 16×4 = 78 s of modelled waiting across 7 polls —
        // inside the §16.4 `wait_for_index = "90s"` posture.
        let mut total = Duration::ZERO;
        let mut delay = policy.visibility.base_delay;
        for _ in 0..policy.visibility.max_polls.saturating_sub(1) {
            total += delay;
            delay = delay.saturating_mul(2).min(policy.visibility.max_delay);
        }
        assert!(
            total <= Duration::from_secs(90),
            "default wait {total:?} exceeds the 90 s posture"
        );
    }
}

//! The publish-engine contract (M5.2, §13): targets join, layer planning,
//! resume probes, the visibility wait, the effect seam and the §15.2
//! failure-injection matrix — all offline over the scripted fakes
//! (`MemoryRegistry`, `MemoryPublishEffect`, `MapRouter`). Written first
//! (red on the unresolved `halyard_publish` API), implemented second.

use std::sync::Arc;
use std::time::Duration;

use halyard_core::package::{
    DepEdge, DepKind, Ecosystem, GroupId, ManifestSite, Package, PathFilter, RelPath, ReqSpec,
    Span, TagTemplate, VersionSource, Workspace,
};
use halyard_core::plan::{PLAN_SCHEMA_V1, Plan, PlanPublish, PlanRelease};
use halyard_core::{PkgId, plan_hash};
use halyard_registry::{RegistryError, RegistrySleeper, VisibilityPolicy};

use halyard_publish::{
    MapRouter, MemoryPublishEffect, MemoryRegistry, Publication, PublishError, PublishMode,
    PublishOutcome, PublishPolicy, PublishReport, plan_publication, run_publication,
};

/// Registers one package with sane defaults (the planner-test fixture
/// shape). `manifest` pins the manifest path; `root` the package root.
fn push_pkg(
    ws: &mut Workspace,
    name: &str,
    version: Option<&str>,
    ecosystem: Ecosystem,
    root: &str,
    manifest: &str,
    publishable: bool,
) -> Option<PkgId> {
    let parsed_version = version.and_then(|v| semver::Version::parse(v).ok());
    let pkg = Package {
        id: PkgId::FIRST,
        name: ws.intern(name),
        ecosystem,
        root: RelPath::new(root)?,
        manifest: RelPath::new(manifest)?,
        version: parsed_version,
        version_source: VersionSource::Local,
        publishable,
        deps: Vec::new(),
        tag_template: TagTemplate::parse("{name}-v{version}").ok()?,
        changelog: None,
        version_site: Some(ManifestSite {
            file: RelPath::new(manifest)?,
            spans: vec![Span { start: 40, end: 45 }],
        }),
        include: PathFilter::new(Vec::new(), Vec::new()),
        group: GroupId::new(0),
        facets: halyard_core::Facets::default(),
    };
    Some(ws.push_package(pkg))
}

/// Adds one dependency edge `from` → `to` (`from` depends on `to`).
fn add_dep(ws: &mut Workspace, from: PkgId, to: PkgId, kind: DepKind) {
    let req = ReqSpec::VersionReq(
        semver::VersionReq::parse(">=0.1.0")
            .unwrap_or_else(|e| unreachable!("fixed version req parses: {e}")),
    );
    let site_file = RelPath::new("Cargo.toml").unwrap_or_else(|| unreachable!("fixed site path"));
    if let Some(pkg) = ws.package_mut(from) {
        pkg.deps.push(DepEdge {
            to,
            kind,
            req,
            site: ManifestSite {
                file: site_file,
                spans: vec![Span { start: 10, end: 17 }],
            },
            raw: ">=0.1.0".into(),
        });
    }
}

/// One release row for a publish plan.
fn release(
    name: &str,
    ecosystem: &str,
    from: &str,
    to: &str,
    publish: Option<PlanPublish>,
) -> PlanRelease {
    PlanRelease {
        package: name.to_owned(),
        ecosystem: ecosystem.to_owned(),
        from: from.to_owned(),
        to: to.to_owned(),
        bump: halyard_core::plan::Bump::Minor,
        tag: format!("{name}-v{to}"),
        reasons: Vec::new(),
        changelog: None,
        publish,
    }
}

/// Assembles a plan whose `plan_hash` is self-consistent (I8) unless
/// `tamper` rewrites a version after hashing.
fn plan_for(releases: Vec<PlanRelease>, order: Vec<String>, tamper: Option<fn(&mut Plan)>) -> Plan {
    let mut plan = Plan {
        schema: PLAN_SCHEMA_V1.to_owned(),
        base_branch: "main".to_owned(),
        head: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned(),
        plan_hash: String::new(),
        releases,
        edits: Vec::new(),
        order,
        skipped: Vec::new(),
    };
    plan.plan_hash = plan_hash(&plan);
    if let Some(tamper) = tamper {
        tamper(&mut plan);
    }
    plan
}

/// Two-cargo-crate workspace: `a` and `b`, `b` depends on `a`.
fn ws_ab(kind: DepKind) -> Workspace {
    let mut ws = Workspace::new();
    let a = push_pkg(
        &mut ws,
        "a",
        Some("0.1.0"),
        Ecosystem::Cargo,
        "crates/a",
        "crates/a/Cargo.toml",
        true,
    )
    .unwrap_or_else(|| unreachable!("a registers"));
    let b = push_pkg(
        &mut ws,
        "b",
        Some("0.2.0"),
        Ecosystem::Cargo,
        "crates/b",
        "crates/b/Cargo.toml",
        true,
    )
    .unwrap_or_else(|| unreachable!("b registers"));
    if let (Some(_), Some(_)) = (Some(a), Some(b)) {
        add_dep(&mut ws, b, a, kind);
    }
    ws
}

/// Both releases enabled on crates.io.
fn plan_ab(order: Vec<String>) -> Plan {
    plan_for(
        vec![
            release(
                "a",
                "cargo",
                "0.1.0",
                "0.2.0",
                Some(PlanPublish {
                    registry: "crates.io".to_owned(),
                    enabled: true,
                }),
            ),
            release(
                "b",
                "cargo",
                "0.2.0",
                "0.3.0",
                Some(PlanPublish {
                    registry: "crates.io".to_owned(),
                    enabled: true,
                }),
            ),
        ],
        order,
        None,
    )
}

/// The fast test policy: three polls, tiny delays, sequential.
fn policy() -> PublishPolicy {
    PublishPolicy::new(
        VisibilityPolicy {
            max_polls: 3,
            base_delay: Duration::from_millis(10),
            max_delay: Duration::from_millis(40),
        },
        1,
    )
}

/// A sleeper that records delays (the observable backoff sequence).
#[derive(Default)]
struct RecordingSleeper {
    delays_ms: std::sync::Mutex<Vec<u128>>,
}

impl RegistrySleeper for RecordingSleeper {
    fn sleep(&self, delay: Duration) {
        self.delays_ms
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(delay.as_millis());
    }
}

/// Plan + workspace for the a→b chain, both targets enabled.
fn publication_ab() -> (Workspace, Publication) {
    let ws = ws_ab(DepKind::Normal);
    let plan = plan_ab(vec!["a".to_owned(), "b".to_owned()]);
    let publication =
        plan_publication(&plan, &ws).unwrap_or_else(|e| unreachable!("publication plans: {e}"));
    (ws, publication)
}

#[test]
fn publication_targets_join_the_plan_with_the_workspace() {
    let mut ws = Workspace::new();
    push_pkg(
        &mut ws,
        "a",
        Some("0.1.0"),
        Ecosystem::Cargo,
        "crates/a",
        "crates/a/Cargo.toml",
        true,
    );
    push_pkg(
        &mut ws,
        "private",
        Some("1.0.0"),
        Ecosystem::Npm,
        "",
        "package.json",
        false,
    );
    let plan = plan_for(
        vec![
            release(
                "a",
                "cargo",
                "0.1.0",
                "0.2.0",
                Some(PlanPublish {
                    registry: "crates.io".to_owned(),
                    enabled: true,
                }),
            ),
            release(
                "private",
                "npm",
                "1.0.0",
                "1.1.0",
                Some(PlanPublish {
                    registry: "npm".to_owned(),
                    enabled: false,
                }),
            ),
            release("c", "cargo", "0.3.0", "0.4.0", None),
        ],
        vec!["a".to_owned(), "private".to_owned(), "c".to_owned()],
        None,
    );
    let publication = plan_publication(&plan, &ws).unwrap_or_else(|e| unreachable!("plans: {e}"));

    let targets = &publication.targets;
    assert_eq!(targets.len(), 1, "only the enabled release is a target");
    let target = targets
        .first()
        .unwrap_or_else(|| unreachable!("one target"));
    assert_eq!(target.package, "a");
    assert_eq!(target.version, "0.2.0", "the plan's `to` is what publishes");
    assert_eq!(target.ecosystem, "cargo");
    assert_eq!(target.registry, "crates.io");
    assert_eq!(target.manifest_dir, "crates/a");

    let skipped = &publication.not_targets;
    assert_eq!(skipped.len(), 2, "disabled and contract-less releases");
    let private = skipped
        .iter()
        .find(|s| s.package == "private")
        .unwrap_or_else(|| unreachable!("private is reported"));
    assert_eq!(private.reason, "publish_disabled");
    let c = skipped
        .iter()
        .find(|s| s.package == "c")
        .unwrap_or_else(|| unreachable!("c is reported"));
    assert_eq!(c.reason, "no_publish_contract");
}

#[test]
fn plan_hash_mismatch_fails_closed() {
    let ws = ws_ab(DepKind::Normal);
    let plan = plan_ab(vec!["a".to_owned(), "b".to_owned()]);
    // Tamper after hashing: the document no longer hashes to its own
    // stamp — publish refuses (I8).
    let mut plan = plan;
    plan.releases
        .get_mut(0)
        .unwrap_or_else(|| unreachable!("row 0"))
        .to = "9.9.9".to_owned();
    let error = plan_publication(&plan, &ws).unwrap_err();
    assert_eq!(
        error,
        PublishError::Plan {
            message: "the plan hash does not match its content — publish consumes a verified \
                  plan only (I8); recompute the plan at HEAD instead of editing it"
                .to_owned(),
        }
    );
    assert_eq!(error.code(), "E-PUB-PLAN");
}

#[test]
fn unknown_release_package_fails_closed() {
    let ws = ws_ab(DepKind::Normal);
    let plan = plan_for(
        vec![release(
            "ghost",
            "cargo",
            "0.1.0",
            "0.2.0",
            Some(PlanPublish {
                registry: "crates.io".to_owned(),
                enabled: true,
            }),
        )],
        vec!["ghost".to_owned()],
        None,
    );
    let error = plan_publication(&plan, &ws).unwrap_err();
    assert!(matches!(error, PublishError::Plan { .. }), "got {error}");
    assert!(
        error.to_string().contains("ghost"),
        "names the package: {error}"
    );
    assert_eq!(error.code(), "E-PUB-PLAN");
}

#[test]
fn unpublishable_member_with_enabled_publish_fails_closed() {
    let mut ws = Workspace::new();
    push_pkg(
        &mut ws,
        "a",
        Some("0.1.0"),
        Ecosystem::Cargo,
        "crates/a",
        "crates/a/Cargo.toml",
        false,
    );
    let plan = plan_for(
        vec![release(
            "a",
            "cargo",
            "0.1.0",
            "0.2.0",
            Some(PlanPublish {
                registry: "crates.io".to_owned(),
                enabled: true,
            }),
        )],
        vec!["a".to_owned()],
        None,
    );
    let error = plan_publication(&plan, &ws).unwrap_err();
    assert_eq!(error.code(), "E-PUB-PLAN");
    assert!(
        error.to_string().contains("not publishable"),
        "states the contradiction: {error}"
    );
}

#[test]
fn target_outside_the_plan_order_fails_closed() {
    let ws = ws_ab(DepKind::Normal);
    let plan = plan_ab(vec!["a".to_owned()]);
    let error = plan_publication(&plan, &ws).unwrap_err();
    assert_eq!(error.code(), "E-PUB-PLAN");
    assert!(
        error.to_string().contains('b'),
        "names the target missing from `order`: {error}"
    );
}

#[test]
fn layers_follow_non_dev_edges_dependencies_first() {
    let mut ws = Workspace::new();
    let ids: Vec<PkgId> = ["a", "b", "c", "d"]
        .iter()
        .map(|name| {
            push_pkg(
                &mut ws,
                name,
                Some("0.1.0"),
                Ecosystem::Cargo,
                name,
                &format!("crates/{name}/Cargo.toml"),
                true,
            )
            .unwrap_or_else(|| unreachable!("{name} registers"))
        })
        .collect();
    let [a, b, c, d] = ids.as_slice() else {
        unreachable!("four packages registered")
    };
    // Diamond: b,c <- a; d <- b,c.
    add_dep(&mut ws, *b, *a, DepKind::Normal);
    add_dep(&mut ws, *c, *a, DepKind::Normal);
    add_dep(&mut ws, *d, *b, DepKind::Normal);
    add_dep(&mut ws, *d, *c, DepKind::Normal);
    let plan = plan_for(
        ["a", "b", "c", "d"]
            .iter()
            .map(|n| {
                release(
                    n,
                    "cargo",
                    "0.1.0",
                    "0.2.0",
                    Some(PlanPublish {
                        registry: "crates.io".to_owned(),
                        enabled: true,
                    }),
                )
            })
            .collect(),
        ["a", "b", "c", "d"]
            .iter()
            .map(|n| (*n).to_owned())
            .collect(),
        None,
    );
    let publication = plan_publication(&plan, &ws).unwrap_or_else(|e| unreachable!("plans: {e}"));
    let names: Vec<Vec<&str>> = publication
        .layers
        .iter()
        .map(|layer| layer.iter().map(String::as_str).collect())
        .collect();
    assert_eq!(names, vec![vec!["a"], vec!["b", "c"], vec!["d"]]);
}

#[test]
fn dev_edges_do_not_constrain_layers() {
    // `b` depends on `a` normally; `a` dev-depends on `b` (the legal
    // cargo dev cycle). The dev edge must not reorder or fail the
    // layering.
    let ws = {
        let mut ws = ws_ab(DepKind::Normal);
        let a = ws
            .find_by_name("a")
            .unwrap_or_else(|| unreachable!("a exists"));
        let b = ws
            .find_by_name("b")
            .unwrap_or_else(|| unreachable!("b exists"));
        add_dep(&mut ws, a, b, DepKind::Dev);
        ws
    };
    let plan = plan_ab(vec!["a".to_owned(), "b".to_owned()]);
    let publication = plan_publication(&plan, &ws).unwrap_or_else(|e| unreachable!("plans: {e}"));
    let names: Vec<Vec<&str>> = publication
        .layers
        .iter()
        .map(|layer| layer.iter().map(String::as_str).collect())
        .collect();
    assert_eq!(
        names,
        vec![vec!["a"], vec!["b"]],
        "dev cycle is not a publish cycle"
    );
}

#[test]
fn publish_cycle_fails_closed_with_the_path() {
    let mut ws = Workspace::new();
    let a = push_pkg(
        &mut ws,
        "a",
        Some("0.1.0"),
        Ecosystem::Cargo,
        "crates/a",
        "crates/a/Cargo.toml",
        true,
    )
    .unwrap_or_else(|| unreachable!("a registers"));
    let b = push_pkg(
        &mut ws,
        "b",
        Some("0.1.0"),
        Ecosystem::Cargo,
        "crates/b",
        "crates/b/Cargo.toml",
        true,
    )
    .unwrap_or_else(|| unreachable!("b registers"));
    add_dep(&mut ws, a, b, DepKind::Normal);
    add_dep(&mut ws, b, a, DepKind::Normal);
    let plan = plan_ab(vec!["a".to_owned(), "b".to_owned()]);
    let error = plan_publication(&plan, &ws).unwrap_err();
    assert_eq!(error.code(), "E-PUB-ORDER");
    assert!(matches!(error, PublishError::Order { .. }), "got {error}");
    assert!(error.to_string().contains('a'), "names the cycle: {error}");
    assert!(error.to_string().contains('b'), "names the cycle: {error}");
}

#[test]
fn empty_publication_runs_zero_calls() {
    let ws = ws_ab(DepKind::Normal);
    let plan = plan_for(
        vec![release("a", "cargo", "0.1.0", "0.2.0", None)],
        vec!["a".to_owned()],
        None,
    );
    let publication = plan_publication(&plan, &ws).unwrap_or_else(|e| unreachable!("plans: {e}"));
    let registry = MemoryRegistry::new();
    let effect = MemoryPublishEffect::new();
    let router = MapRouter::new().with("crates.io", Arc::new(registry.clone()));
    let report = run_publication(
        &publication,
        &router,
        &effect,
        &policy(),
        &RecordingSleeper::default(),
    )
    .unwrap_or_else(|e| unreachable!("empty run: {e}"));
    assert!(report.rows.is_empty(), "nothing to publish ⇒ no rows");
    assert_eq!(report.published, 0);
    assert_eq!(report.already_published, 0);
    assert_eq!(report.skipped, 0);
    assert_eq!(report.failed, 0);
    assert!(
        registry.probe_log().is_empty(),
        "I5/I1: nothing to publish, nothing probed"
    );
    assert!(effect.requests().is_empty(), "no effect calls");
}

#[test]
fn first_publish_reports_published_with_visibility() {
    let (_ws, publication) = publication_ab();
    let registry = MemoryRegistry::new();
    // The coupled fake: the effect's successful uploads stage into the
    // registry and the one-probe index lag lets the §13 step-3 wait
    // observe the version turning visible.
    let effect = MemoryPublishEffect::new();
    effect.publishes_into(&registry);
    let router = MapRouter::new().with("crates.io", Arc::new(registry.clone()));
    let report = run_publication(
        &publication,
        &router,
        &effect,
        &policy(),
        &RecordingSleeper::default(),
    )
    .unwrap_or_else(|e| unreachable!("run: {e}"));

    assert_eq!(report.published, 2);
    assert_eq!(report.already_published, 0);
    assert_eq!(report.failed, 0);
    assert_eq!(report.skipped, 0);
    for row in &report.rows {
        let PublishOutcome::Published {
            visibility_confirmed,
        } = &row.outcome
        else {
            unreachable!("expected Published, got {:?}", row.outcome);
        };
        assert!(
            visibility_confirmed,
            "visibility confirmed for {}",
            row.package
        );
    }
    // The effect saw exactly the two requests, dependencies first.
    let requests = effect.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(
        requests
            .first()
            .unwrap_or_else(|| unreachable!("first"))
            .package,
        "a"
    );
    assert_eq!(
        requests
            .first()
            .unwrap_or_else(|| unreachable!("first"))
            .version,
        "0.2.0"
    );
    assert_eq!(
        requests
            .first()
            .unwrap_or_else(|| unreachable!("first"))
            .manifest_dir,
        "crates/a"
    );
    assert_eq!(
        requests
            .get(1)
            .unwrap_or_else(|| unreachable!("second"))
            .package,
        "b"
    );
}

#[test]
fn already_published_is_an_effect_free_no_op() {
    let (_ws, publication) = publication_ab();
    let registry = MemoryRegistry::new();
    registry.insert("a", "0.2.0");
    registry.insert("b", "0.3.0");
    let effect = MemoryPublishEffect::new();
    let router = MapRouter::new().with("crates.io", Arc::new(registry.clone()));
    let report = run_publication(
        &publication,
        &router,
        &effect,
        &policy(),
        &RecordingSleeper::default(),
    )
    .unwrap_or_else(|e| unreachable!("run: {e}"));
    assert_eq!(report.already_published, 2);
    assert_eq!(report.published, 0);
    assert!(
        effect.requests().is_empty(),
        "I1: published work is never re-published"
    );
    for row in &report.rows {
        assert!(matches!(row.outcome, PublishOutcome::AlreadyPublished));
    }
}

#[test]
fn effect_failure_is_a_row_not_a_run_error() {
    let (_ws, publication) = publication_ab();
    // The effect is coupled so `a`'s upload lands and confirms: `b` is
    // then ungated and reaches its own (scripted) tool failure — the
    // row-level outcome §13.7 prescribes.
    let registry = MemoryRegistry::new();
    let effect = MemoryPublishEffect::new();
    effect.publishes_into(&registry);
    effect.fail(
        "b",
        PublishError::Effect {
            ecosystem: "cargo".to_owned(),
            message: "the registry rejected the upload (400)".to_owned(),
            retryable: false,
        },
    );
    let router = MapRouter::new().with("crates.io", Arc::new(registry.clone()));
    let report = run_publication(
        &publication,
        &router,
        &effect,
        &policy(),
        &RecordingSleeper::default(),
    )
    .unwrap_or_else(|e| unreachable!("a failing row is not a run error: {e}"));
    assert_eq!(report.published, 1, "a still publishes");
    assert_eq!(report.failed, 1);
    let row = report
        .rows
        .iter()
        .find(|r| r.package == "b")
        .unwrap_or_else(|| unreachable!("b row exists"));
    let PublishOutcome::Failed {
        code,
        message,
        retryable,
    } = &row.outcome
    else {
        unreachable!("expected Failed, got {:?}", row.outcome);
    };
    assert_eq!(code, "E-PUB-EFFECT");
    assert!(message.contains("400"));
    assert!(!retryable);
}

#[test]
fn visibility_timeout_skips_dependents_honestly() {
    let (_ws, publication) = publication_ab();
    // The index never catches up within any bounded wait (the staged
    // version turns visible only after u32::MAX misses).
    let registry = MemoryRegistry::with_lag(u32::MAX);
    let effect = MemoryPublishEffect::new();
    effect.publishes_into(&registry);
    let router = MapRouter::new().with("crates.io", Arc::new(registry.clone()));
    let report = run_publication(
        &publication,
        &router,
        &effect,
        &policy(),
        &RecordingSleeper::default(),
    )
    .unwrap_or_else(|e| unreachable!("run: {e}"));
    let a = report
        .rows
        .iter()
        .find(|r| r.package == "a")
        .unwrap_or_else(|| unreachable!("a row exists"));
    let PublishOutcome::Published {
        visibility_confirmed,
    } = &a.outcome
    else {
        unreachable!("expected Published for a, got {:?}", a.outcome);
    };
    assert!(!visibility_confirmed, "the index never confirmed a");
    let b = report
        .rows
        .iter()
        .find(|r| r.package == "b")
        .unwrap_or_else(|| unreachable!("b row exists"));
    let PublishOutcome::Skipped { reason } = &b.outcome else {
        unreachable!("expected Skipped for b, got {:?}", b.outcome);
    };
    assert_eq!(reason, "upstream_invisible:a");
    assert_eq!(report.skipped, 1);
}

#[test]
fn upstream_failure_skips_dependents() {
    let (_ws, publication) = publication_ab();
    let registry = MemoryRegistry::new();
    let effect = MemoryPublishEffect::new();
    effect.fail(
        "a",
        PublishError::Effect {
            ecosystem: "cargo".to_owned(),
            message: "cargo publish exited 101".to_owned(),
            retryable: true,
        },
    );
    let router = MapRouter::new().with("crates.io", Arc::new(registry.clone()));
    let report = run_publication(
        &publication,
        &router,
        &effect,
        &policy(),
        &RecordingSleeper::default(),
    )
    .unwrap_or_else(|e| unreachable!("run: {e}"));
    let a = report
        .rows
        .iter()
        .find(|r| r.package == "a")
        .unwrap_or_else(|| unreachable!("a row exists"));
    assert!(
        matches!(a.outcome, PublishOutcome::Failed { .. }),
        "got {:?}",
        a.outcome
    );
    let b = report
        .rows
        .iter()
        .find(|r| r.package == "b")
        .unwrap_or_else(|| unreachable!("b row exists"));
    let PublishOutcome::Skipped { reason } = &b.outcome else {
        unreachable!("expected Skipped for b, got {:?}", b.outcome);
    };
    assert_eq!(reason, "upstream_failed:a");
    assert_eq!(effect.requests().len(), 1, "the dependent never spawns");
}

#[test]
fn resume_rerun_reaches_the_uninterrupted_state() {
    // §15.2: run 1 fails `b`'s publish after `a` landed; run 2 resumes:
    // `a` is AlreadyPublished (the registry probe), `b` publishes. The
    // final registry state equals the uninterrupted run, zero duplicates.
    let (_ws, publication) = publication_ab();

    let registry = MemoryRegistry::new();
    let effect = MemoryPublishEffect::new();
    effect.publishes_into(&registry);
    effect.fail(
        "b",
        PublishError::Effect {
            ecosystem: "cargo".to_owned(),
            message: "interrupted after a landed".to_owned(),
            retryable: true,
        },
    );
    let router = MapRouter::new().with("crates.io", Arc::new(registry.clone()));
    let first = run_publication(
        &publication,
        &router,
        &effect,
        &policy(),
        &RecordingSleeper::default(),
    )
    .unwrap_or_else(|e| unreachable!("run 1: {e}"));
    assert_eq!(first.published, 1);
    assert_eq!(first.failed, 1);

    // Run 2 over the same registry (the state survived) and a fresh
    // script: nothing fails.
    let effect2 = MemoryPublishEffect::new();
    effect2.publishes_into(&registry);
    let second = run_publication(
        &publication,
        &router,
        &effect2,
        &policy(),
        &RecordingSleeper::default(),
    )
    .unwrap_or_else(|e| unreachable!("run 2: {e}"));
    assert_eq!(
        second.already_published, 1,
        "a is probed, never re-published"
    );
    assert_eq!(second.published, 1, "b completes");
    assert_eq!(second.failed, 0);
    assert!(
        registry.is_visible("a", "0.2.0") && registry.is_visible("b", "0.3.0"),
        "final state == uninterrupted"
    );
    // Zero duplicates: exactly one *successful* effect call per package
    // across both runs (b: one failed, one ok; a: one ok in run 1).
    let mut successes = effect.successes();
    successes.extend(effect2.successes());
    successes.sort();
    assert_eq!(
        successes,
        vec!["a".to_owned(), "b".to_owned()],
        "no package published twice"
    );
}

#[test]
fn report_is_deterministic_across_runs() {
    let (_ws, publication) = publication_ab();
    let run = || {
        let registry = MemoryRegistry::new();
        let effect = MemoryPublishEffect::new();
        effect.publishes_into(&registry);
        let router = MapRouter::new().with("crates.io", Arc::new(registry.clone()));
        run_publication(
            &publication,
            &router,
            &effect,
            &policy(),
            &RecordingSleeper::default(),
        )
        .unwrap_or_else(|e| unreachable!("run: {e}"))
    };
    let first: PublishReport = run();
    let second: PublishReport = run();
    assert_eq!(first, second, "I3: identical inputs ⇒ identical report");
}

#[test]
fn max_parallel_greater_than_one_preserves_the_report() {
    let mut ws = Workspace::new();
    let mut names = Vec::new();
    for name in ["p1", "p2", "p3", "p4"] {
        push_pkg(
            &mut ws,
            name,
            Some("0.1.0"),
            Ecosystem::Cargo,
            name,
            &format!("{name}/Cargo.toml"),
            true,
        );
        names.push(name.to_owned());
    }
    let plan = plan_for(
        names
            .iter()
            .map(|n| {
                release(
                    n,
                    "cargo",
                    "0.1.0",
                    "0.2.0",
                    Some(PlanPublish {
                        registry: "crates.io".to_owned(),
                        enabled: true,
                    }),
                )
            })
            .collect(),
        names.clone(),
        None,
    );
    let publication = plan_publication(&plan, &ws).unwrap_or_else(|e| unreachable!("plans: {e}"));
    assert_eq!(
        publication.layers.len(),
        1,
        "independent targets share one layer"
    );

    let registry = MemoryRegistry::new();
    let effect = MemoryPublishEffect::new();
    effect.publishes_into(&registry);
    let router = MapRouter::new().with("crates.io", Arc::new(registry.clone()));
    let wide = PublishPolicy::new(
        VisibilityPolicy {
            max_polls: 3,
            base_delay: Duration::from_millis(1),
            max_delay: Duration::from_millis(4),
        },
        4,
    );
    let report = run_publication(
        &publication,
        &router,
        &effect,
        &wide,
        &RecordingSleeper::default(),
    )
    .unwrap_or_else(|e| unreachable!("run: {e}"));
    assert_eq!(report.published, 4);
    let packages: Vec<&str> = report.rows.iter().map(|r| r.package.as_str()).collect();
    assert_eq!(
        packages,
        vec!["p1", "p2", "p3", "p4"],
        "rows sort deterministically regardless of thread completion order"
    );
}

#[test]
fn unrouteable_registry_fails_closed_before_any_call() {
    let (_ws, publication) = publication_ab();
    let registry = MemoryRegistry::new();
    let effect = MemoryPublishEffect::new();
    // The router knows no client for "crates.io": the run refuses before
    // touching the registry or the tool (validate-then-act).
    let router = MapRouter::new();
    let error = run_publication(
        &publication,
        &router,
        &effect,
        &policy(),
        &RecordingSleeper::default(),
    )
    .unwrap_err();
    assert!(
        matches!(error, PublishError::Registry { .. }),
        "got {error}"
    );
    assert_eq!(error.code(), "E-PUB-REGISTRY");
    assert!(
        error.to_string().contains("crates.io"),
        "names the registry: {error}"
    );
    assert!(effect.requests().is_empty(), "no targets ⇒ no effect calls");
    assert!(registry.probe_log().is_empty(), "no targets ⇒ no probes");
}

#[test]
fn registry_probe_failure_is_a_row_not_a_run_error() {
    let (_ws, publication) = publication_ab();
    let registry = MemoryRegistry::new();
    registry.fail_probes_with(RegistryError::Io {
        message: "connection refused".to_owned(),
    });
    let effect = MemoryPublishEffect::new();
    let router = MapRouter::new().with("crates.io", Arc::new(registry.clone()));
    let report = run_publication(
        &publication,
        &router,
        &effect,
        &policy(),
        &RecordingSleeper::default(),
    )
    .unwrap_or_else(|e| unreachable!("a failing probe is not a run error: {e}"));
    assert_eq!(report.failed, 1, "`a` fails on its resume probe");
    assert_eq!(report.skipped, 1, "`b` is gated by the failed upstream");
    let a = report
        .rows
        .iter()
        .find(|r| r.package == "a")
        .unwrap_or_else(|| unreachable!("a row exists"));
    let PublishOutcome::Failed {
        code, retryable, ..
    } = &a.outcome
    else {
        unreachable!("expected Failed for a, got {:?}", a.outcome);
    };
    assert_eq!(
        code, "E-REG-IO",
        "the row carries the probe's own stable code"
    );
    assert!(retryable, "a connection-level failure is retryable");
    let b = report
        .rows
        .iter()
        .find(|r| r.package == "b")
        .unwrap_or_else(|| unreachable!("b row exists"));
    let PublishOutcome::Skipped { reason } = &b.outcome else {
        unreachable!("expected Skipped for b, got {:?}", b.outcome);
    };
    assert_eq!(reason, "upstream_failed:a");
    assert!(
        effect.requests().is_empty(),
        "a dead registry never reaches the tool"
    );
}

#[test]
fn error_codes_and_display_are_pinned() {
    let plan = PublishError::Plan {
        message: "the plan is unusable".to_owned(),
    };
    assert_eq!(plan.code(), "E-PUB-PLAN");
    assert_eq!(plan.to_string(), "[E-PUB-PLAN] the plan is unusable");

    let order = PublishError::Order {
        message: "cycle: a -> b -> a".to_owned(),
    };
    assert_eq!(order.code(), "E-PUB-ORDER");
    assert_eq!(order.to_string(), "[E-PUB-ORDER] cycle: a -> b -> a");

    let registry = PublishError::Registry {
        registry: "crates.io".to_owned(),
        message: "no client configured".to_owned(),
    };
    assert_eq!(registry.code(), "E-PUB-REGISTRY");
    assert_eq!(
        registry.to_string(),
        "[E-PUB-REGISTRY] crates.io: no client configured"
    );

    let effect = PublishError::Effect {
        ecosystem: "cargo".to_owned(),
        message: "cargo publish exited 101".to_owned(),
        retryable: false,
    };
    assert_eq!(effect.code(), "E-PUB-EFFECT");
    assert_eq!(
        effect.to_string(),
        "[E-PUB-EFFECT] cargo: cargo publish exited 101"
    );
}

#[test]
fn zero_parallel_normalizes_to_one() {
    let policy = PublishPolicy::new(
        VisibilityPolicy {
            max_polls: 1,
            base_delay: Duration::from_millis(1),
            max_delay: Duration::from_millis(1),
        },
        0,
    );
    assert_eq!(
        policy.max_parallel, 1,
        "0 is normalized: a run always progresses"
    );
}

#[test]
fn skip_propagates_down_the_chain() {
    // a → b → c: a fails, so b is skipped (upstream_failed) and c is
    // skipped on b's skip — publishing c would embed a version the
    // registry does not carry.
    let mut ws = Workspace::new();
    let a = push_pkg(
        &mut ws,
        "a",
        Some("0.1.0"),
        Ecosystem::Cargo,
        "crates/a",
        "crates/a/Cargo.toml",
        true,
    )
    .unwrap_or_else(|| unreachable!("a registers"));
    let b = push_pkg(
        &mut ws,
        "b",
        Some("0.1.0"),
        Ecosystem::Cargo,
        "crates/b",
        "crates/b/Cargo.toml",
        true,
    )
    .unwrap_or_else(|| unreachable!("b registers"));
    let c = push_pkg(
        &mut ws,
        "c",
        Some("0.1.0"),
        Ecosystem::Cargo,
        "crates/c",
        "crates/c/Cargo.toml",
        true,
    )
    .unwrap_or_else(|| unreachable!("c registers"));
    add_dep(&mut ws, b, a, DepKind::Normal);
    add_dep(&mut ws, c, b, DepKind::Normal);
    let plan = plan_for(
        vec!["a", "b", "c"]
            .into_iter()
            .map(|n| {
                release(
                    n,
                    "cargo",
                    "0.1.0",
                    "0.2.0",
                    Some(PlanPublish {
                        registry: "crates.io".to_owned(),
                        enabled: true,
                    }),
                )
            })
            .collect(),
        vec!["a".to_owned(), "b".to_owned(), "c".to_owned()],
        None,
    );
    let publication = plan_publication(&plan, &ws).unwrap_or_else(|e| unreachable!("plans: {e}"));
    let registry = MemoryRegistry::new();
    let effect = MemoryPublishEffect::new();
    effect.publishes_into(&registry);
    effect.fail(
        "a",
        PublishError::Effect {
            ecosystem: "cargo".to_owned(),
            message: "the tool failed at the head of the chain".to_owned(),
            retryable: true,
        },
    );
    let router = MapRouter::new().with("crates.io", Arc::new(registry.clone()));
    let report = run_publication(
        &publication,
        &router,
        &effect,
        &policy(),
        &RecordingSleeper::default(),
    )
    .unwrap_or_else(|e| unreachable!("run: {e}"));
    assert_eq!(report.failed, 1);
    assert_eq!(report.skipped, 2);
    let reasons: Vec<&str> = report
        .rows
        .iter()
        .filter_map(|row| match &row.outcome {
            PublishOutcome::Skipped { reason } => Some(reason.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(reasons, vec!["upstream_failed:a", "upstream_skipped:b"]);
    assert_eq!(effect.successes(), Vec::<String>::new(), "nothing landed");
}

#[test]
fn failure_injected_at_every_step_converges() {
    // §15.2: for each target of the chain, inject a tool failure in run
    // 1 and assert run 2 reaches the uninterrupted state with zero
    // duplicates (each package published successfully exactly once).
    for broken in ["a", "b"] {
        let (_ws, publication) = publication_ab();
        let registry = MemoryRegistry::new();
        let effect = MemoryPublishEffect::new();
        effect.publishes_into(&registry);
        effect.fail(
            broken,
            PublishError::Effect {
                ecosystem: "cargo".to_owned(),
                message: format!("injected failure at {broken}"),
                retryable: true,
            },
        );
        let router = MapRouter::new().with("crates.io", Arc::new(registry.clone()));
        let first = run_publication(
            &publication,
            &router,
            &effect,
            &policy(),
            &RecordingSleeper::default(),
        )
        .unwrap_or_else(|e| unreachable!("run 1: {e}"));
        assert_eq!(first.failed, 1, "run 1 fails exactly the injected target");
        let effect2 = MemoryPublishEffect::new();
        effect2.publishes_into(&registry);
        let second = run_publication(
            &publication,
            &router,
            &effect2,
            &policy(),
            &RecordingSleeper::default(),
        )
        .unwrap_or_else(|e| unreachable!("run 2: {e}"));
        assert_eq!(second.failed, 0, "run 2 completes");
        assert!(
            registry.is_visible("a", "0.2.0") && registry.is_visible("b", "0.3.0"),
            "final state == uninterrupted for the {broken} injection"
        );
        let mut successes = effect.successes();
        successes.extend(effect2.successes());
        successes.sort();
        assert_eq!(
            successes,
            vec!["a".to_owned(), "b".to_owned()],
            "zero duplicates across the {broken} injection"
        );
    }
}

#[test]
fn dry_mode_forecasts_without_probes() {
    // The offline forecast (§13.8): the engine validates routing, runs
    // the effect and reports `Forecast` rows — the resume probes and
    // the visibility wait are the live run's inputs, so a dry run never
    // touches the registry (I5).
    let (_ws, publication) = publication_ab();
    let registry = MemoryRegistry::new();
    // Any probe would fail loudly — the dry run must make none.
    registry.fail_probes_with(RegistryError::Io {
        message: "no network in the forecast lane".to_owned(),
    });
    let effect = MemoryPublishEffect::new();
    let router = MapRouter::new().with("crates.io", Arc::new(registry.clone()));
    let policy = policy().in_mode(PublishMode::Dry);
    let report = run_publication(
        &publication,
        &router,
        &effect,
        &policy,
        &RecordingSleeper::default(),
    )
    .unwrap_or_else(|e| unreachable!("dry run: {e}"));
    assert_eq!(report.forecast, 2);
    assert_eq!(report.published, 0);
    assert_eq!(report.failed, 0);
    assert!(
        registry.probe_log().is_empty(),
        "I5: the forecast never probes"
    );
    assert_eq!(
        effect.requests().len(),
        2,
        "the effect ran (the tool's --dry-run)"
    );
    for row in &report.rows {
        assert!(matches!(row.outcome, PublishOutcome::Forecast));
    }
}

#[test]
fn dry_mode_keeps_the_upstream_gate() {
    // The forecast shows the same skip structure the live run would
    // hit: a broken tool skips its dependents even in dry mode.
    let mut ws = Workspace::new();
    let a = push_pkg(
        &mut ws,
        "a",
        Some("0.1.0"),
        Ecosystem::Cargo,
        "crates/a",
        "crates/a/Cargo.toml",
        true,
    )
    .unwrap_or_else(|| unreachable!("a registers"));
    let b = push_pkg(
        &mut ws,
        "b",
        Some("0.1.0"),
        Ecosystem::Cargo,
        "crates/b",
        "crates/b/Cargo.toml",
        true,
    )
    .unwrap_or_else(|| unreachable!("b registers"));
    add_dep(&mut ws, b, a, DepKind::Normal);
    let plan = plan_for(
        vec![
            release(
                "a",
                "cargo",
                "0.1.0",
                "0.2.0",
                Some(PlanPublish {
                    registry: "crates.io".to_owned(),
                    enabled: true,
                }),
            ),
            release(
                "b",
                "cargo",
                "0.1.0",
                "0.2.0",
                Some(PlanPublish {
                    registry: "crates.io".to_owned(),
                    enabled: true,
                }),
            ),
        ],
        vec!["a".to_owned(), "b".to_owned()],
        None,
    );
    let publication = plan_publication(&plan, &ws).unwrap_or_else(|e| unreachable!("plans: {e}"));
    let registry = MemoryRegistry::new();
    let effect = MemoryPublishEffect::new();
    effect.fail(
        "a",
        PublishError::Effect {
            ecosystem: "cargo".to_owned(),
            message: "the dry tool check failed".to_owned(),
            retryable: false,
        },
    );
    let router = MapRouter::new().with("crates.io", Arc::new(registry.clone()));
    let policy = policy().in_mode(PublishMode::Dry);
    let report = run_publication(
        &publication,
        &router,
        &effect,
        &policy,
        &RecordingSleeper::default(),
    )
    .unwrap_or_else(|e| unreachable!("dry run: {e}"));
    assert_eq!(report.forecast, 0);
    assert_eq!(report.failed, 1);
    assert_eq!(report.skipped, 1);
    let b = report
        .rows
        .iter()
        .find(|r| r.package == "b")
        .unwrap_or_else(|| unreachable!("b row exists"));
    let PublishOutcome::Skipped { reason } = &b.outcome else {
        unreachable!("expected Skipped for b, got {:?}", b.outcome);
    };
    assert_eq!(reason, "upstream_failed:a");
}

#[test]
fn dry_mode_still_fails_closed_on_a_routing_gap() {
    // Validate-then-act holds in dry mode too: an unroutable registry
    // refuses the forecast before any effect call.
    let (_ws, publication) = publication_ab();
    let registry = MemoryRegistry::new();
    let effect = MemoryPublishEffect::new();
    let router = MapRouter::new();
    let policy = policy().in_mode(PublishMode::Dry);
    let error = run_publication(
        &publication,
        &router,
        &effect,
        &policy,
        &RecordingSleeper::default(),
    )
    .unwrap_err();
    assert_eq!(error.code(), "E-PUB-REGISTRY");
    assert!(effect.requests().is_empty(), "no targets ⇒ no effect calls");
    let _ = registry;
}

#[test]
fn targets_carry_the_manifest_path() {
    // The spawn effect needs the manifest file (the cargo
    // `--manifest-path` argument), not only the package root.
    let (_ws, publication) = publication_ab();
    let a = publication
        .targets
        .iter()
        .find(|t| t.package == "a")
        .unwrap_or_else(|| unreachable!("a target"));
    assert_eq!(a.manifest, "crates/a/Cargo.toml");
    assert_eq!(a.manifest_dir, "crates/a");
}

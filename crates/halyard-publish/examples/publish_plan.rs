//! The publish engine (M5.2, §13) over the scripted fakes: layer
//! planning from a verified plan, the resume probe, the publish effect
//! and the §13 step-3 visibility wait — including an interruption and
//! the I2 re-run that completes it with zero duplicates. No network is
//! touched (I5): the `MemoryRegistry`/`MemoryPublishEffect` fakes stand
//! in for the registries and the publish tool; the live CLI run rides
//! the same engine over the blocking transport and the real spawn.

use std::sync::Arc;
use std::time::Duration;

use halyard_core::package::{
    DepEdge, DepKind, Ecosystem, GroupId, ManifestSite, Package, PathFilter, RelPath, ReqSpec,
    Span, TagTemplate, VersionSource, Workspace,
};
use halyard_core::plan::{PLAN_SCHEMA_V1, Plan, PlanPublish, PlanRelease};
use halyard_core::{PkgId, plan_hash};
use halyard_publish::{
    MapRouter, MemoryPublishEffect, MemoryRegistry, Publication, PublishError, PublishOutcome,
    PublishPolicy, PublishReport, plan_publication, run_publication,
};
use halyard_registry::{RegistrySleeper, VisibilityPolicy};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ws = build_chain_workspace()?;
    let plan = build_plan(&ws)?;

    // §13 step 1: validate + layer.
    let publication: Publication = plan_publication(&plan, &ws)?;
    println!("targets:");
    for target in &publication.targets {
        println!(
            "  {}@{} ({} via {}, root `{}`)",
            target.package, target.version, target.ecosystem, target.registry, target.manifest_dir
        );
    }
    println!("layers: {:?}", publication.layers);
    for skipped in &publication.not_targets {
        println!("not a target: {} ({})", skipped.package, skipped.reason);
    }

    // §13 steps 2–3: the run. The scripted fakes stand in for the
    // registries and the tool; the one-probe lag models the sparse
    // index catching up after an upload.
    let registry = MemoryRegistry::new();
    let effect = MemoryPublishEffect::new();
    effect.publishes_into(&registry);
    let router = MapRouter::new().with("crates.io", Arc::new(registry.clone()));
    let policy = forecast_policy();
    effect.fail(
        "b",
        PublishError::Effect {
            ecosystem: "cargo".to_owned(),
            message: "the upload timed out (injected interruption)".to_owned(),
            retryable: true,
        },
    );

    println!("\nrun 1 (interrupted at b):");
    let first = run_publication(
        &publication,
        &router,
        &effect,
        &policy,
        &RecordingSleeper::default(),
    )?;
    print_report(&first);

    // The I2 re-run: `a` is already visible (probe, no effect call),
    // `b` retries, `c` follows — the final state matches the
    // uninterrupted run with zero duplicates.
    println!("\nrun 2 (resume):");
    let effect2 = MemoryPublishEffect::new();
    effect2.publishes_into(&registry);
    let second = run_publication(
        &publication,
        &router,
        &effect2,
        &policy,
        &RecordingSleeper::default(),
    )?;
    print_report(&second);

    println!(
        "\nsuccessful publishes across both runs: {:?} + {:?} (each package exactly once)",
        effect.successes(),
        effect2.successes()
    );
    println!("probes: {:?}", registry.probe_log().len());
    Ok(())
}

/// The three-crate chain `a` ← b ← c (b depends on a, c depends on b):
/// the publish order is forced, a, then b, then c (§13 step 1).
fn build_chain_workspace() -> Result<Workspace, Box<dyn std::error::Error>> {
    let mut ws = Workspace::new();
    let mut ids: Vec<PkgId> = Vec::new();
    for name in ["a", "b", "c"] {
        let root = format!("crates/{name}");
        let manifest = format!("crates/{name}/Cargo.toml");
        let pkg = Package {
            id: PkgId::FIRST,
            name: ws.intern(name),
            ecosystem: Ecosystem::Cargo,
            root: RelPath::new(&root).ok_or("fixed root parses")?,
            manifest: RelPath::new(&manifest).ok_or("fixed manifest parses")?,
            version: Some("0.1.0".parse()?),
            version_source: VersionSource::Local,
            publishable: true,
            deps: Vec::new(),
            tag_template: TagTemplate::parse("{name}-v{version}")?,
            changelog: None,
            version_site: Some(ManifestSite {
                file: RelPath::new(&manifest).ok_or("fixed manifest parses")?,
                spans: vec![Span { start: 40, end: 45 }],
            }),
            include: PathFilter::new(Vec::new(), Vec::new()),
            group: GroupId::new(0),
            facets: halyard_core::Facets::default(),
        };
        ids.push(ws.push_package(pkg));
    }
    for (dependent, upstream) in [(1usize, 0usize), (2, 1)] {
        let name = chain_name(dependent);
        let manifest = format!("crates/{name}/Cargo.toml");
        let (Some(dependent_id), Some(upstream_id)) = (ids.get(dependent), ids.get(upstream))
        else {
            return Err("the fixed chain indexes in range".into());
        };
        if let Some(pkg) = ws.package_mut(*dependent_id) {
            pkg.deps.push(DepEdge {
                to: *upstream_id,
                kind: DepKind::Normal,
                req: ReqSpec::VersionReq(">=0.1.0".parse()?),
                site: ManifestSite {
                    file: RelPath::new(&manifest).ok_or("fixed manifest parses")?,
                    spans: vec![Span { start: 10, end: 17 }],
                },
                raw: ">=0.1.0".into(),
            });
        }
    }
    Ok(ws)
}

/// The chain's package names by position (no slice indexing — the
/// `indexing_slicing` gate).
fn chain_name(position: usize) -> &'static str {
    match position {
        0 => "a",
        1 => "b",
        _ => "c",
    }
}

/// The self-consistent plan (I8: publish consumes a verified plan
/// only), with `internal-docs` carrying no publish contract.
fn build_plan(ws: &Workspace) -> Result<Plan, Box<dyn std::error::Error>> {
    for name in ["a", "b", "c"] {
        if ws.find_by_name(name).is_none() {
            return Err(format!("workspace lacks {name}").into());
        }
    }
    let releases = vec![
        (
            "a",
            Some(PlanPublish {
                registry: "crates.io".to_owned(),
                enabled: true,
            }),
        ),
        (
            "b",
            Some(PlanPublish {
                registry: "crates.io".to_owned(),
                enabled: true,
            }),
        ),
        (
            "c",
            Some(PlanPublish {
                registry: "crates.io".to_owned(),
                enabled: true,
            }),
        ),
        ("internal-docs", None),
    ]
    .into_iter()
    .map(|(name, publish)| PlanRelease {
        package: name.to_owned(),
        ecosystem: "cargo".to_owned(),
        from: "0.1.0".to_owned(),
        to: "0.2.0".to_owned(),
        bump: halyard_core::plan::Bump::Minor,
        tag: format!("{name}-v0.2.0"),
        reasons: Vec::new(),
        changelog: None,
        publish,
    })
    .collect::<Vec<_>>();
    let order = vec!["a", "b", "c", "internal-docs"]
        .into_iter()
        .map(String::from)
        .collect();
    let mut plan = Plan {
        schema: PLAN_SCHEMA_V1.to_owned(),
        base_branch: "main".to_owned(),
        head: "a".repeat(40),
        plan_hash: String::new(),
        releases,
        edits: Vec::new(),
        order,
        skipped: Vec::new(),
    };
    plan.plan_hash = plan_hash(&plan);
    Ok(plan)
}

/// The demo policy: four visibility polls with tiny delays, sequential.
fn forecast_policy() -> PublishPolicy {
    PublishPolicy::new(
        VisibilityPolicy {
            max_polls: 4,
            base_delay: Duration::from_millis(10),
            max_delay: Duration::from_millis(40),
        },
        1,
    )
}

fn print_report(report: &PublishReport) {
    for row in &report.rows {
        let what = match &row.outcome {
            PublishOutcome::Published {
                visibility_confirmed,
            } => {
                if *visibility_confirmed {
                    "published (visibility confirmed)".to_owned()
                } else {
                    "published (index never confirmed — dependents skip)".to_owned()
                }
            }
            PublishOutcome::AlreadyPublished => "already published (I1 no-op)".to_owned(),
            PublishOutcome::Forecast => "forecast (dry run — nothing published)".to_owned(),
            PublishOutcome::Skipped { reason } => format!("skipped ({reason})"),
            PublishOutcome::Failed {
                code, retryable, ..
            } => format!("failed [{code}] (retryable: {retryable})"),
        };
        println!(
            "  layer {} {}@{}: {}",
            row.layer, row.package, row.version, what
        );
    }
    println!(
        "  tallies: {} published, {} already, {} forecast, {} skipped, {} failed",
        report.published, report.already_published, report.forecast, report.skipped, report.failed
    );
}

/// A sleeper that records the requested delays (the observable backoff).
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

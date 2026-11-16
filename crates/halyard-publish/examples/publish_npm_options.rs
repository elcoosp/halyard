//! The npm publish options (M5.3 step 6's npm half, ADR-0033) end to
//! end over the scripted fakes: the plan contract's `access`/`provenance`
//! ride the targets, a live `provenance = true` run **refuses** outside
//! a supported cloud runtime before any call (validate-then-act, §15.3),
//! the same run publishes once the runtime is detected, and the unset
//! shape never consults the gate (the tool's own trusted-publishing
//! posture governs). Offline — `MemoryRegistry`/`MemoryPublishEffect`,
//! no network, no tokens.
//!
//! Run: `cargo run -p halyard-publish --example publish_npm_options`

use std::sync::Arc;

use halyard_publish::{
    MapRouter, MemoryPublishEffect, MemoryRegistry, ProvenanceRuntime, Publication, PublishMode,
    PublishPolicy, PublishTarget, run_publication,
};
use halyard_registry::{RegistrySleeper, VisibilityPolicy};

/// A no-op sleeper (the visibility wait's delays are scripted policy
/// bounds; the example never actually waits on a real index).
struct NoopSleeper;

impl RegistrySleeper for NoopSleeper {
    fn sleep(&self, _delay: std::time::Duration) {}
}

/// One npm target with the contract's options baked in (the shape
/// `plan_publication` produces from an npm release's `PlanPublish`).
fn npm_target(
    package: &str,
    version: &str,
    access: Option<&str>,
    provenance: Option<bool>,
) -> PublishTarget {
    PublishTarget {
        package: package.to_owned(),
        version: version.to_owned(),
        ecosystem: "npm".to_owned(),
        registry: "npm".to_owned(),
        manifest_dir: format!("packages/{package}"),
        manifest: format!("packages/{package}/package.json"),
        access: access.map(str::to_owned),
        provenance,
    }
}

/// A one-target publication (no layering needed for the demo).
fn publication(targets: Vec<PublishTarget>) -> Publication {
    Publication {
        layers: vec![targets.iter().map(|t| t.package.clone()).collect()],
        upstreams: std::collections::BTreeMap::new(),
        targets,
        not_targets: Vec::new(),
    }
}

/// The fast offline policy (the publish test fixtures' shape).
fn policy(runtime: ProvenanceRuntime) -> PublishPolicy {
    PublishPolicy::new(
        VisibilityPolicy {
            max_polls: 3,
            base_delay: std::time::Duration::from_millis(10),
            max_delay: std::time::Duration::from_millis(40),
        },
        1,
    )
    .in_provenance_runtime(runtime)
}

fn main() {
    // 1. The refusal: an explicit `provenance = true` request, live, no
    //    supported cloud runtime — refused before any probe or spawn,
    //    naming the must and the escape hatch.
    let requesting = publication(vec![npm_target(
        "demo-pkg",
        "1.1.0",
        Some("public"),
        Some(true),
    )]);
    let registry = MemoryRegistry::new();
    let effect = MemoryPublishEffect::new();
    effect.publishes_into(&registry);
    let router = MapRouter::new().with("npm", Arc::new(registry.clone()));
    let refused = run_publication(
        &requesting,
        &router,
        &effect,
        &policy(ProvenanceRuntime::NotDetected),
        &NoopSleeper,
    );
    match refused {
        Err(error) => println!("laptop shape → refused ({}):\n  {error}", error.code()),
        Ok(_) => println!("unexpectedly ran"),
    }
    assert!(effect.requests().is_empty(), "zero side effects");
    assert!(registry.probe_log().is_empty(), "zero probes");

    // 2. The detected runtime: the same request runs (GHA `id-token:
    //    write` or the GitLab marker — the shell resolves the verdict).
    let registry = MemoryRegistry::new();
    let effect = MemoryPublishEffect::new();
    effect.publishes_into(&registry);
    let router = MapRouter::new().with("npm", Arc::new(registry.clone()));
    let report = run_publication(
        &requesting,
        &router,
        &effect,
        &policy(ProvenanceRuntime::CloudDetected),
        &NoopSleeper,
    )
    .unwrap_or_else(|error| unreachable!("detected runtime runs: {error}"));
    println!("cloud-detected shape → {} published", report.published);
    for row in &report.rows {
        println!(
            "  row {}@{}: access {:?}, provenance {:?}",
            row.package, row.version, row.access, row.provenance
        );
    }

    // 3. The unset shape: no option in the contract — the gate never
    //    consults the runtime, the argv stays bare (trusted publishing
    //    decides; the tool's own posture governs).
    let unset = publication(vec![npm_target("plain-pkg", "0.2.0", None, None)]);
    let registry = MemoryRegistry::new();
    let effect = MemoryPublishEffect::new();
    effect.publishes_into(&registry);
    let router = MapRouter::new().with("npm", Arc::new(registry.clone()));
    let report = run_publication(
        &unset,
        &router,
        &effect,
        &policy(ProvenanceRuntime::NotDetected),
        &NoopSleeper,
    )
    .unwrap_or_else(|error| unreachable!("unset runs: {error}"));
    println!(
        "unset shape → ran with no gate ({} published; argv would be bare)",
        report.published
    );
    let _ = PublishMode::Live; // the gate is live-mode only; dry forecasts never refuse
}

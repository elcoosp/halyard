//! The end-to-end §11.1 release-pipeline consumer demo: everything a
//! release run composes, against a real git fixture — discovery, the
//! reconciliation, the tag-target precision rule (the commit that
//! *introduced* each pending version), the git-side tag-phase forecast
//! (nothing written) and the forge-side release machine over the preview
//! adapter. This is the chain `halyard release --dry` drives; the live
//! shell (M4.2b) swaps the preview writers for real ones, not the chain.
//!
//! Run: `cargo run -p halyard-cli --example release_pipeline`

use std::error::Error;

use halyard_core::HistoryProvider;
use halyard_core::planner::PlanConfig;
use halyard_core::release_state::{pending_releases, reconcile_tags};
use halyard_discover::{DiscoverInput, discover_repo};
use halyard_forge::{BranchName, MachineInput, PreviewForge, ReleaseSpec, run_release_machine};
use halyard_git::GitHistory;
use halyard_git::GitHistoryOptions;
use halyard_git::tag::PreviewTagWriter;
use halyard_git::tag_target::{
    GixTagTargetResolver, ManifestVersionProbe, TagTargetSource, directives_for_pending_resolved,
};
use halyard_test_support::{CommitSpec, GitFixture};
use tempfile::TempDir;

/// The shell's manifest-version probe: the ecosystem adapters own the
/// grammars, the git engine never parses manifests.
struct PipelineProbe;

impl ManifestVersionProbe for PipelineProbe {
    fn manifest_version(
        &self,
        ecosystem: &str,
        _manifest_path: &str,
        manifest: &[u8],
    ) -> Option<String> {
        match ecosystem {
            "cargo" => halyard_eco_cargo::manifest_version(manifest),
            "npm" => halyard_eco_npm::manifest_version(manifest),
            _ => None,
        }
    }
}

/// The fixture repo: `app` released at v0.1.0, bumped to 0.2.0 one
/// commit later (the pending release whose tag should target the bump
/// commit, not HEAD); `lib` released and quiet. Returns the temp dir
/// (the fixture borrows its path), the fixture and the bump commit's hex.
fn fixture() -> Result<(TempDir, GitFixture, String), Box<dyn Error>> {
    let dir = TempDir::new()?;
    let mut fx = GitFixture::init_in(dir.path())?;
    fx.commit_with(&CommitSpec::files(
        vec![
            (
                "Cargo.toml".to_owned(),
                "[package]\nname = \"app\"\nversion = \"0.1.0\"\n".to_owned(),
            ),
            (
                "src/lib.rs".to_owned(),
                "pub const APP: usize = 1;\n".to_owned(),
            ),
            (
                "lib/Cargo.toml".to_owned(),
                "[package]\nname = \"lib\"\nversion = \"1.2.0\"\n".to_owned(),
            ),
        ],
        "feat: scaffold app and lib",
    ))?;
    fx.tag_annotated("v0.1.0", "app 0.1.0")?;
    fx.tag_annotated("lib-v1.2.0", "lib 1.2.0")?;
    let bump = fx.commit_with(&CommitSpec::files(
        vec![(
            "Cargo.toml".to_owned(),
            "[package]\nname = \"app\"\nversion = \"0.2.0\"\n".to_owned(),
        )],
        "chore: release app 0.2.0",
    ))?;
    fx.commit_with(&CommitSpec::files(
        vec![(
            "src/lib.rs".to_owned(),
            "pub const APP: usize = 2;\n".to_owned(),
        )],
        "feat: grow app",
    ))?;
    Ok((dir, fx, bump))
}

fn main() -> Result<(), Box<dyn Error>> {
    let (_dir, fx, bump) = fixture()?;

    // 1. Discovery (the §6 orchestrator over the adapters).
    let discovery = discover_repo(fx.root(), &DiscoverInput::default())?;
    println!(
        "discovered {} package(s)",
        discovery.workspace.packages().len()
    );

    // 2. History + tags (the engine; the reconciliation never re-scans
    //    refs itself).
    let history = GitHistory::build_with_options(
        fx.root(),
        &discovery.workspace,
        GitHistoryOptions::default(),
    )?;
    let tag_names: Vec<halyard_core::BString> =
        history.tags().iter().map(|tag| tag.name.clone()).collect();

    // 3. The reconciliation (§11.1 step 2, the pure decision).
    let cfg = PlanConfig::default_values();
    let rows = reconcile_tags(&discovery.workspace, &tag_names, &cfg);
    for row in &rows {
        println!("reconcile: {} {:?}", row.package, row.state);
    }
    let pending = pending_releases(&discovery.workspace, &tag_names, &cfg);
    let head = history.head_oid().ok_or("fixture without HEAD")?;

    // 4. The tag-target precision rule: where each tag lands — the
    //    commit that introduced the version, or the documented HEAD
    //    fallback when the walk cannot verify.
    let resolver = GixTagTargetResolver::open(fx.root(), Box::new(PipelineProbe))?;
    let queue = directives_for_pending_resolved(
        &pending,
        &discovery.workspace,
        history.digest(),
        &head,
        &resolver,
    );
    for rd in &queue {
        let source = match rd.source {
            TagTargetSource::ManifestPinpoint => "manifest pinpoint",
            TagTargetSource::HeadFallback => "HEAD fallback",
        };
        println!(
            "target: {} -> {} ({source})",
            rd.directive.name, rd.directive.target
        );
    }
    let app = queue
        .iter()
        .find(|rd| rd.directive.name == "v0.2.0")
        .ok_or("missing app directive")?;
    let bump_oid = halyard_git::gix::hash::ObjectId::from_hex(bump.as_bytes())
        .map_err(|e| format!("fixture produced a non-hex oid: {e}"))?;
    if app.directive.target != bump_oid {
        return Err("app's tag did not target its bump commit".into());
    }

    // 5. The git-side tag-phase forecast (probes real, writes nothing).
    let writer = PreviewTagWriter::open(fx.root())?;
    let directives: Vec<_> = queue.into_iter().map(|rd| rd.directive).collect();
    let report = halyard_git::tag::run_tag_phase(&writer, &directives);
    println!(
        "tag phase forecast: {} created, {} already tagged, {} failed",
        report.created, report.already_tagged, report.failed
    );

    // 6. The forge-side release machine over the preview adapter: the
    //    pending queue gates the machine (PROPAGATE — the tag phase
    //    runs, the Release PR is not prepared yet).
    let forge = PreviewForge::new();
    let branch = BranchName::new("halyard/release/main")?;
    let input = MachineInput {
        branch: branch.clone(),
        pending_releases: pending
            .iter()
            .map(|release| ReleaseSpec {
                tag: release.tag.clone(),
                notes: format!("## Features\n\n- {} {}", release.package, release.version),
            })
            .collect(),
        release_pr: None,
        content_commit: None,
    };
    let machine = run_release_machine(&forge, &input)?;
    println!(
        "release machine: propagated = {}, effects = {:?} ({} call(s))",
        machine.propagated, machine.effects, machine.calls
    );
    Ok(())
}

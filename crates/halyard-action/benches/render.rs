//! The §12 forecast renderer's cost (ADR-0037): `plan_outputs` over a
//! 40-release plan and the env-file entry grammar must stay free — the
//! action renders on every CI run, and a microsecond-scale projection
//! must never become the step's cost. The §12 `pr`/`release`/`check`/
//! `auto` mode renderers (ADR-0043/0046/0044/0047) ride the same budget.

use std::hint::black_box;
use std::path::Path;

use criterion::Criterion;
use halyard_core::{
    Bump, PLAN_SCHEMA_V1, Plan, PlanEdit, PlanPatch, PlanReason, PlanRelease, PlanSkip,
};

/// A 40-release plan: `pkg0000` … `pkg0039`, one edit site each plus a
/// shared root manifest (the dedupe work), one skip.
fn plan_with_releases(count: usize) -> Plan {
    let releases = (0..count)
        .map(|idx| {
            let name = format!("pkg{idx:04}");
            PlanRelease {
                package: name.clone(),
                ecosystem: "cargo".to_owned(),
                from: "0.1.0".to_owned(),
                to: "0.2.0".to_owned(),
                bump: Bump::Minor,
                tag: format!("{name}-v0.2.0"),
                reasons: vec![PlanReason::Commit {
                    oid: format!("ab12cd3{idx:02}"),
                    kind: "feat".to_owned(),
                    breaking: false,
                }],
                changelog: None,
                publish: None,
            }
        })
        .collect();
    let edits = (0..count)
        .map(|idx| PlanEdit {
            file: format!("crates/pkg{idx:04}/Cargo.toml"),
            patches: vec![PlanPatch {
                start: 123,
                end: 128,
                text: "0.2.0".to_owned(),
            }],
        })
        .chain(std::iter::once(PlanEdit {
            file: "Cargo.toml".to_owned(),
            patches: vec![PlanPatch {
                start: 6,
                end: 11,
                text: "0.2.0".to_owned(),
            }],
        }))
        .collect();
    Plan {
        schema: PLAN_SCHEMA_V1.to_owned(),
        base_branch: "main".to_owned(),
        head: "9f2ca67".to_owned(),
        plan_hash: "blake3:ab12".to_owned(),
        releases,
        edits,
        order: (0..count).map(|idx| format!("pkg{idx:04}")).collect(),
        skipped: vec![PlanSkip {
            package: "docs".to_owned(),
            reason: "no_changes".to_owned(),
        }],
    }
}

/// The fixed output name; a failure at registration skips the entry
/// benches honestly (the fixed literal always parses).
fn output_name() -> Option<halyard_action::OutputName> {
    halyard_action::OutputName::new("released").ok()
}

/// The §12 `release` mode's renderers (ADR-0046): the release pass view
/// over the same 40-release plan — the release run renders these on
/// every CI pass, so they ride the same microsecond-scale budget as
/// the plan forecast. Own function: the bench entries stay grouped by
/// mode, the way the `pr` and `check` entries are.
fn bench_release_render<M: criterion::measurement::Measurement>(
    group: &mut criterion::BenchmarkGroup<'_, M>,
    plan: &Plan,
) {
    let release_pass = halyard_action::ReleasePass {
        released: true,
        forecast: false,
        created_tags: (0..40).map(|idx| format!("pkg{idx:04}-v0.2.0")).collect(),
        release_urls: std::collections::BTreeMap::from([(
            "pkg0000-v0.2.0".to_owned(),
            "https://github.com/example/monorepo/releases/tag/pkg0000-v0.2.0".to_owned(),
        )]),
        forge: halyard_action::ForgePosture::Ran {
            repo: "example/monorepo".to_owned(),
            releases: 1,
        },
    };
    group.bench_function("release_outputs_40_releases", |b| {
        b.iter(|| {
            black_box(
                halyard_action::release_outputs(
                    black_box(plan),
                    Path::new("halyard-plan.json"),
                    black_box(&release_pass),
                )
                .unwrap_or_default(),
            )
        });
    });
    group.bench_function("release_summary_40_releases", |b| {
        b.iter(|| {
            black_box(halyard_action::release_summary(
                black_box(plan),
                black_box(&release_pass),
            ))
        });
    });
}

/// The §12 `auto` mode's renderers (ADR-0047): the composed pass view
/// over the same 40-release plan — the push-triggered run renders these
/// on every CI pass, so they ride the same microsecond-scale budget as
/// the plan forecast. The PROPAGATE shape (the tag phase owning the
/// pass) is the heavier line: created tags + mints. Own function: the
/// bench entries stay grouped by mode, the way the release entries are.
fn bench_auto_render<M: criterion::measurement::Measurement>(
    group: &mut criterion::BenchmarkGroup<'_, M>,
    plan: &Plan,
) {
    let auto_pass = halyard_action::AutoPass {
        released: true,
        propagated: true,
        forecast: false,
        created_tags: (0..40).map(|idx| format!("pkg{idx:04}-v0.2.0")).collect(),
        pr_effect: None,
        content_effect: None,
        release_urls: std::collections::BTreeMap::from([(
            "pkg0000-v0.2.0".to_owned(),
            "https://github.com/example/monorepo/releases/tag/pkg0000-v0.2.0".to_owned(),
        )]),
        forge: halyard_action::ForgePosture::Ran {
            repo: "example/monorepo".to_owned(),
            releases: 1,
        },
    };
    // The prepare-phase shape (§11.1 step 3): the content verdict rides
    // the pass line between the tags and the PR it enables — the same
    // per-push budget, rendered with the content piece in it.
    let prepare_pass = halyard_action::AutoPass {
        released: false,
        propagated: false,
        forecast: false,
        created_tags: Vec::new(),
        pr_effect: Some(halyard_action::PrEffect::Updated {
            number: 7,
            url: "https://github.com/example/monorepo/pull/7".to_owned(),
        }),
        content_effect: Some(halyard_action::ContentEffect::Matched {
            tree_oid: "3fa1b2c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a0".to_owned(),
        }),
        release_urls: std::collections::BTreeMap::new(),
        forge: halyard_action::ForgePosture::Ran {
            repo: "example/monorepo".to_owned(),
            releases: 0,
        },
    };
    group.bench_function("auto_outputs_40_releases", |b| {
        b.iter(|| {
            black_box(
                halyard_action::auto_outputs(
                    black_box(plan),
                    Path::new("halyard-plan.json"),
                    black_box(&auto_pass),
                )
                .unwrap_or_default(),
            )
        });
    });
    group.bench_function("auto_summary_40_releases", |b| {
        b.iter(|| {
            black_box(halyard_action::auto_summary(
                black_box(plan),
                black_box(&auto_pass),
            ))
        });
    });
    group.bench_function("auto_summary_prepare_content_matched", |b| {
        b.iter(|| {
            black_box(halyard_action::auto_summary(
                black_box(plan),
                black_box(&prepare_pass),
            ))
        });
    });
}

fn bench(c: &mut Criterion) {
    let plan = plan_with_releases(40);
    let mut group = c.benchmark_group("action_render");
    group.bench_function("plan_outputs_40_releases", |b| {
        b.iter(|| {
            black_box(
                halyard_action::plan_outputs(black_box(&plan), Path::new("halyard-plan.json"))
                    .unwrap_or_default(),
            )
        });
    });
    if let Some(name) = output_name() {
        group.bench_function("env_file_entry_single_line", |b| {
            b.iter(|| black_box(halyard_action::env_file_entry(black_box(&name), "false")));
        });
        group.bench_function("env_file_entry_heredoc", |b| {
            b.iter(|| {
                black_box(halyard_action::env_file_entry(
                    black_box(&name),
                    "crates/pkg0000/Cargo.toml\ncrates/pkg0001/Cargo.toml",
                ))
            });
        });
    }
    // The §12 `pr` mode's renderers (ADR-0043): the pass view over the
    // same 40-release plan — the release-PR run renders these on every
    // CI pass, so they ride the same microsecond-scale budget as the
    // plan forecast.
    let pass = halyard_action::PrPass {
        pr_effect: Some(halyard_action::PrEffect::Opened {
            number: 7,
            url: "https://github.com/example/monorepo/pull/7".to_owned(),
        }),
        propagated: false,
        released: false,
        release_urls: std::collections::BTreeMap::new(),
        branch: "halyard/release/main".to_owned(),
        forecast: false,
    };
    group.bench_function("pr_outputs_40_releases", |b| {
        b.iter(|| {
            black_box(
                halyard_action::pr_outputs(
                    black_box(&plan),
                    Path::new("halyard-plan.json"),
                    black_box(&pass),
                )
                .unwrap_or_default(),
            )
        });
    });
    group.bench_function("pr_summary_40_releases", |b| {
        b.iter(|| {
            black_box(halyard_action::pr_summary(
                black_box(&plan),
                black_box(&pass),
            ))
        });
    });
    bench_release_render(&mut group, &plan);
    bench_auto_render(&mut group, &plan);
    // The §12 `check` mode's renderer (ADR-0044): the verdict beside the
    // 40-release table — the pull_request-event render, same budget.
    let verdict = halyard_action::CheckVerdict {
        title: Some(halyard_action::TitleOutcome::Accepted {
            kind: "feat".to_owned(),
            level: "minor".to_owned(),
            predictions: (0..40)
                .map(|idx| halyard_action::CheckRow {
                    package: format!("pkg{idx:04}"),
                    from: "0.1.0".to_owned(),
                    bump: "patch".to_owned(),
                })
                .collect(),
        }),
    };
    group.bench_function("check_summary_40_releases", |b| {
        b.iter(|| {
            black_box(halyard_action::check_summary(
                black_box(&plan),
                black_box(&verdict),
            ));
        });
    });
    group.finish();
}

/// Criterion harness entry (the `criterion_group!` expansion, spelled out
/// so the generated `pub fn` does not trip `missing_docs`).
fn benches() {
    let mut criterion: Criterion = Criterion::default().configure_from_args();
    bench(&mut criterion);
}

fn main() {
    benches();
    Criterion::default().configure_from_args().final_summary();
}

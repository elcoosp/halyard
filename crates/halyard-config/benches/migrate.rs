//! Bench: foreign-config migration surveys and the §16.2 write step
//! (M4.7, ADR-0022/ADR-0023).
//!
//! Measures the release-please survey (JSON parse + key mapping +
//! per-package tag resolution + draft serialization + strict-loader
//! validation), the changesets survey over realistic configs, and the
//! write step's plan+run pair over the memory sink. The migration runs
//! once per `halyard migrate` invocation — millisecond scale at worst —
//! but the draft's loader round-trip is the part the gate must keep
//! honest: a draft that stops parsing is a broken deliverable, and the
//! bench pins that the validation stays cheap. The write decision is
//! pinned because it runs on the `--write` path: the plan must stay a
//! trivial scan over the report (the heavy work — validation — already
//! happened in the survey).

use criterion::Criterion;
use halyard_config::ForeignFiles;
use halyard_config::MemoryDraftSink;
use halyard_config::default_sources;
use halyard_config::migrate;
use halyard_config::plan_draft_write;
use halyard_config::run_draft_write;

/// A realistic release-please manifest config: two packages with
/// per-package options, the pre-major switches, and documented keys the
/// survey must report as unmapped.
fn release_please_files() -> ForeignFiles {
    let mut files = ForeignFiles::new();
    files.insert(
        "release-please-config.json",
        r#"{
  "$schema": "https://raw.githubusercontent.com/googleapis/release-please/main/schemas/config.json",
  "packages": {
    "crates/p1": { "package-name": "p1-crate", "changelog-path": "docs/CHANGELOG.md" },
    "crates/p2": {},
    "crates/p3": { "include-component-in-tag": false },
    "crates/p4": { "component": "p4" }
  },
  "bump-minor-pre-major": true,
  "bump-patch-for-minor-pre-major": true,
  "plugins": ["cargo-workspace"],
  "draft": true,
  "prerelease": false,
  "pull-request-title-pattern": "chore: release ${branch}",
  "extra-files": ["Cargo.lock"],
  "bootstrap-sha": "0d0a9da",
  "signoff": "Signed-off-by: release <rel@example.com>"
}"#,
    );
    files.insert(
        ".release-please-manifest.json",
        r#"{ "crates/p1": "1.2.3", "crates/p2": "0.5.0", "crates/p3": "2.0.0" }"#,
    );
    files
}

/// A realistic changesets config: the mapped keys plus the group and
/// advisory keys the survey must report.
fn changesets_files() -> ForeignFiles {
    let mut files = ForeignFiles::new();
    files.insert(
        ".changeset/config.json",
        r#"{
  "changelog": ["github", { "repo": "org/repo" }],
  "commit": false,
  "access": "public",
  "baseBranch": "develop",
  "ignore": ["@scope/internal-tool", "@scope/legacy"],
  "linked": [["@scope/a", "@scope/b"], ["@scope/c", "@scope/d"]],
  "updateInternalDependencies": "minor",
  "privatePackages": { "version": true, "tag": false }
}"#,
    );
    files
}

/// A realistic knope config: the multi-package shape with the
/// documented keys the survey must report as unmapped.
fn knope_files() -> ForeignFiles {
    let mut files = ForeignFiles::new();
    files.insert(
        "knope.toml",
        r#"
[packages."engine"]
versioned_files = ["crates/engine/Cargo.toml"]
changelog = "crates/engine/CHANGELOG.md"

[packages."cli"]
versioned_files = ["crates/cli/Cargo.toml"]
scopes = ["cli"]

[[workflows]]
name = "release"

[[workflows]]
name = "verify"
"#,
    );
    files
}

/// A realistic release-plz config: the `[workspace]` defaults plus two
/// `[[package]]` overrides (one release-disabled, one grouped).
fn release_plz_files() -> ForeignFiles {
    let mut files = ForeignFiles::new();
    files.insert(
        "release-plz.toml",
        r#"
[workspace]
changelog_update = true
git_release_enable = true
publish_timeout = "20m"
pr_branch_prefix = "release-plz-"
release_always = false

[[package]]
name = "engine"
git_tag_name = "engine-v{{ version }}"

[[package]]
name = "cli"
release = false
version_group = "bundle"
"#,
    );
    files
}

fn bench_migrate_surveys(c: &mut Criterion) {
    let sources = default_sources();
    let rp = release_please_files();
    let cs = changesets_files();
    let kn = knope_files();
    let plz = release_plz_files();
    let mut group = c.benchmark_group("migrate");
    group.throughput(criterion::Throughput::Elements(1));
    group.bench_function("release_please_survey_4_packages", |b| {
        b.iter(|| {
            let report = migrate(&rp, &sources);
            assert!(
                matches!(
                    &report,
                    Ok(report) if report.tools.iter().any(|t| t.tool == "release-please"
                        && t.configured
                        && matches!(&t.draft, Some(draft) if !draft.is_empty()))
                ),
                "release-please surveys with a validated draft: {report:?}"
            );
        });
    });
    group.bench_function("changesets_survey_2_ignored", |b| {
        b.iter(|| {
            let report = migrate(&cs, &sources);
            assert!(
                matches!(
                    &report,
                    Ok(report) if report.tools.iter().any(|t| t.tool == "changesets"
                        && t.configured)
                ),
                "changesets surveys: {report:?}"
            );
        });
    });
    group.bench_function("knope_survey_2_packages", |b| {
        b.iter(|| {
            let report = migrate(&kn, &sources);
            assert!(
                matches!(
                    &report,
                    Ok(report) if report.tools.iter().any(|t| t.tool == "knope"
                        && t.configured
                        && t.packages.len() == 2)
                ),
                "knope surveys with both packages: {report:?}"
            );
        });
    });
    group.bench_function("release_plz_survey_2_packages", |b| {
        b.iter(|| {
            let report = migrate(&plz, &sources);
            assert!(
                matches!(
                    &report,
                    Ok(report) if report.tools.iter().any(|t| t.tool == "release-plz"
                        && t.configured
                        && t.packages.len() == 2)
                ),
                "release-plz surveys with both packages: {report:?}"
            );
        });
    });
    group.bench_function("empty_tree_four_sources", |b| {
        let empty = ForeignFiles::new();
        b.iter(|| {
            let report = migrate(&empty, &sources);
            assert_eq!(
                report.as_ref().map(|r| r.tools.len()).ok(),
                Some(4),
                "absence surveys every source"
            );
        });
    });
}

/// The §16.2 write step over the memory sink: the plan (a scan over the
/// report) plus the run (the gate + one recorded write). The real
/// `AtomicDraftSink` does filesystem I/O — the decision cost is what
/// the bench keeps honest, the durability protocol is pinned by tests.
/// Setup failures exit honestly (benches have no `unwrap`/`expect`
/// surface outside test context).
fn bench_migrate_write(c: &mut Criterion) {
    let sources = default_sources();
    let rp = release_please_files();
    let report = migrate(&rp, &sources).unwrap_or_else(|error| {
        eprintln!("the release-please survey must parse for the write bench: {error}");
        std::process::exit(1);
    });
    let mut group = c.benchmark_group("migrate_write");
    group.throughput(criterion::Throughput::Elements(1));
    group.bench_function("plan_and_run_over_memory_sink", |b| {
        b.iter(|| {
            let planned = plan_draft_write(&report).unwrap_or_else(|error| {
                eprintln!("the single configured draft always plans: {error}");
                std::process::exit(1);
            });
            let plan = planned.unwrap_or_else(|| {
                eprintln!("a configured tool is a write candidate");
                std::process::exit(1);
            });
            let write =
                run_draft_write(Some(plan), &MemoryDraftSink::new()).unwrap_or_else(|error| {
                    eprintln!("a fresh sink never refuses: {error}");
                    std::process::exit(1);
                });
            assert!(
                write.bytes > 0 && write.tool == "release-please",
                "the write lands the validated draft: {write:?}"
            );
        });
    });
}

fn main() {
    let mut criterion = Criterion::default();
    bench_migrate_surveys(&mut criterion);
    bench_migrate_write(&mut criterion);
}

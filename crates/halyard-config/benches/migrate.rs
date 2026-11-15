//! Bench: foreign-config migration surveys (M4.7 step 1, §16.2).
//!
//! Measures the release-please survey (JSON parse + key mapping +
//! per-package tag resolution + draft serialization + strict-loader
//! validation) and the changesets survey over realistic configs. The
//! migration runs once per `halyard migrate` invocation — millisecond
//! scale at worst — but the draft's loader round-trip is the part the
//! gate must keep honest: a draft that stops parsing is a broken
//! deliverable, and the bench pins that the validation stays cheap.

use criterion::Criterion;
use halyard_config::ForeignFiles;
use halyard_config::default_sources;
use halyard_config::migrate;

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

fn bench_migrate_surveys(c: &mut Criterion) {
    let sources = default_sources();
    let rp = release_please_files();
    let cs = changesets_files();
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
    group.bench_function("empty_tree_two_sources", |b| {
        let empty = ForeignFiles::new();
        b.iter(|| {
            let report = migrate(&empty, &sources);
            assert_eq!(
                report.as_ref().map(|r| r.tools.len()).ok(),
                Some(2),
                "absence surveys every source"
            );
        });
    });
}

fn main() {
    let mut criterion = Criterion::default();
    bench_migrate_surveys(&mut criterion);
}

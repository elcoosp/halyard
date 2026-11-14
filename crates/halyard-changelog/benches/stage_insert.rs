//! Changelog-stage benchmark (M2, §9.4): the insertion machinery the
//! `apply` command's changelog stage drives — built-in style render +
//! marker-anchored insertion over a long-lived changelog, and the
//! user-template path (the compile cost rides every render by design:
//! `Environment::empty()` per call, documented in
//! `render_template_source`).

use std::sync::OnceLock;

use criterion::Criterion;
use halyard_changelog::{render_releases_into, render_releases_into_template};
use halyard_core::Bump;
use halyard_core::plan::{PlanChangelog, PlanRelease};

/// Sections per changelog fixture (a long-lived keep-a-changelog file).
const SECTIONS: usize = 64;
/// Bullets per section.
const BULLETS: usize = 8;

/// A deterministic changelog with `SECTIONS` release sections, newest
/// first (the shape the stage idempotently maintains).
fn changelog_text() -> &'static String {
    static TEXT: OnceLock<String> = OnceLock::new();
    TEXT.get_or_init(|| {
        let mut out = String::from("# Changelog\n\n## [Unreleased]\n\n<!-- halyard:insert -->\n");
        for section in 0..SECTIONS {
            let _ = std::fmt::Write::write_fmt(
                &mut out,
                format_args!("## [0.{section}.0]\n\n### Features\n\n"),
            );
            for bullet in 0..BULLETS {
                let _ = std::fmt::Write::write_fmt(
                    &mut out,
                    format_args!("- feat(pkg-{bullet}): add the thing ({section})\n"),
                );
            }
            out.push('\n');
        }
        out
    })
}

/// One planned release with a plausible entry preview (~8 bullets).
fn release(version: u8) -> PlanRelease {
    let mut entry = format!("## 0.{version}.0\n\n### Features\n\n");
    for bullet in 0..BULLETS {
        let _ = std::fmt::Write::write_fmt(
            &mut entry,
            format_args!("- feat(pkg-{bullet}): add the thing ({version})\n"),
        );
    }
    PlanRelease {
        package: "solo".to_owned(),
        ecosystem: "cargo".to_owned(),
        from: format!("0.{version}.0"),
        to: format!("0.{}.0", version + 1),
        bump: Bump::Minor,
        tag: format!("v0.{}.0", version + 1),
        reasons: Vec::new(),
        changelog: Some(PlanChangelog {
            file: "CHANGELOG.md".to_owned(),
            entry_md: entry,
        }),
        publish: None,
    }
}

/// The user-template fixture: a three-variable template mirroring the
/// documented environment (`version`, `tag`, `sections`).
const USER_TEMPLATE: &str = "## {{ version }} ({{ tag }})\n\
{% for section in sections %}\
### {{ section[0] }}\n\
{% for line in section[1] %}- {{ line }}\n{% endfor %}\
{% endfor %}";

/// Built-in keep-a-changelog insertion over the long-lived document —
/// the stage's steady-state write (anchored insertion + in-place
/// replacement across 64 existing sections).
fn bench_builtin_insert(c: &mut Criterion) {
    c.bench_function("changelog/insert/keep-a-changelog-64-sections", |b| {
        b.iter_batched(
            || {
                let release = release(SECTIONS as u8);
                let existing = changelog_text().clone();
                (existing, release)
            },
            |(existing, release)| {
                let releases = [&release];
                let rendered = render_releases_into(Some(&existing), &releases, "keep-a-changelog")
                    .unwrap_or_else(|error| unreachable!("fixture render: {error}"));
                debug_assert!(rendered.contains("## [65.0]"), "the section anchors");
            },
            criterion::BatchSize::SmallInput,
        )
    });
}

/// The user-template path: compile + render + insert (the §9.4
/// template-path style the M2 slice wires).
fn bench_user_template_insert(c: &mut Criterion) {
    c.bench_function("changelog/insert/user-template-64-sections", |b| {
        b.iter_batched(
            || {
                let release = release(SECTIONS as u8);
                let existing = changelog_text().clone();
                (existing, release)
            },
            |(existing, release)| {
                let releases = [&release];
                let rendered =
                    render_releases_into_template(Some(&existing), &releases, USER_TEMPLATE)
                        .unwrap_or_else(|error| unreachable!("fixture render: {error}"));
                debug_assert!(rendered.contains("## 65.0"), "the template renders");
            },
            criterion::BatchSize::SmallInput,
        )
    });
}

fn main() {
    let mut criterion = Criterion::default();
    bench_builtin_insert(&mut criterion);
    bench_user_template_insert(&mut criterion);
    criterion.final_summary();
}

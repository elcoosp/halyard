//! Release-notes pipeline benchmark (§9.4 `github-release-notes`): the
//! pure surfaces the shell exercises per run — the version-section
//! extraction from changelog text and the notes body render (the forge
//! machine's `ReleaseSpec.notes` payload).

use std::sync::OnceLock;

use criterion::Criterion;
use halyard_changelog::{NotesContext, render_release_notes_for_version};
use halyard_core::Bump;
use halyard_core::plan::{PlanChangelog, PlanRelease};

/// Sections per changelog fixture (a long-lived keep-a-changelog file).
const SECTIONS: usize = 32;
/// Bullets per section.
const BULLETS: usize = 8;

/// A deterministic changelog with `SECTIONS` release sections, newest
/// first (the shape `render_plan` idempotently maintains).
fn changelog_text() -> &'static String {
    static TEXT: OnceLock<String> = OnceLock::new();
    TEXT.get_or_init(|| {
        let mut out = String::from("# Changelog\n");
        for section in 0..SECTIONS {
            let _ = std::fmt::Write::write_fmt(
                &mut out,
                format_args!("\n## [0.{}.0]\n\n### Features\n\n", SECTIONS - section),
            );
            for bullet in 0..BULLETS {
                let _ = std::fmt::Write::write_fmt(
                    &mut out,
                    format_args!("- feature {bullet} of section {section}\n"),
                );
            }
        }
        out
    })
}

/// A plan release rendering through the entry-preview entry point (the
/// plan-driven shape).
fn sample_release() -> PlanRelease {
    let mut entry =
        String::from("## 0.2.0\n\n### ⚠ BREAKING CHANGES\n\n- reshape\n\n### Features\n\n");
    for bullet in 0..BULLETS {
        let _ = std::fmt::Write::write_fmt(&mut entry, format_args!("- feature {bullet}\n"));
    }
    PlanRelease {
        package: "app".to_owned(),
        ecosystem: "cargo".to_owned(),
        from: "0.1.0".to_owned(),
        to: "0.2.0".to_owned(),
        bump: Bump::Minor,
        tag: "v0.2.0".to_owned(),
        reasons: Vec::new(),
        changelog: Some(PlanChangelog {
            file: "CHANGELOG.md".to_owned(),
            entry_md: entry,
        }),
        publish: None,
    }
}

fn bench_release_notes(c: &mut Criterion) {
    let mut group = c.benchmark_group("release_notes");
    let context = NotesContext {
        compare: Some("https://github.com/o/r/compare/v0.1.0...v0.2.0".to_owned()),
    };
    group.bench_function("for_version_32_sections", |b| {
        b.iter(|| {
            let notes = render_release_notes_for_version(changelog_text(), "0.2.0", &context);
            std::hint::black_box(notes.len())
        });
    });
    let release = sample_release();
    group.bench_function("entry_preview_render", |b| {
        b.iter(|| {
            let notes = halyard_changelog::render_release_notes(&release, &context);
            std::hint::black_box(notes.is_ok())
        });
    });
    group.finish();
}

fn main() {
    bench_release_notes(&mut Criterion::default().configure_from_args());
}

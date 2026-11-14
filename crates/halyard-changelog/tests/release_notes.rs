//! The release-notes pipeline (§9.4 `github-release-notes` style + the
//! forge-context seam): compare-link derivation from the configured
//! remote (pure, offline) and the per-release notes renderer the forge
//! machine's `ReleaseSpec.notes` consumes (the §11.1 tag phase's release
//! half). Tests written first (red), against the missing API.

use halyard_changelog::{
    NotesContext, Style, compare_url, extract_version_section, render_release,
    render_release_notes, render_release_notes_for_version,
};
use halyard_core::Bump;
use halyard_core::plan::{PlanChangelog, PlanRelease};

type BoxError = Box<dyn std::error::Error>;

/// A release whose planner entry carries a breaking group and a feature
/// group (the §9.4 shape the planner's `entry_md` preview renders).
fn sample_release() -> PlanRelease {
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
            entry_md: "## 0.2.0\n\n### ⚠ BREAKING CHANGES\n\n- reshape contract\n\n### Features\n\n- grow surface\n".to_owned(),
        }),
        publish: None,
    }
}

#[test]
fn compare_url_normalizes_github_remotes() {
    // The four remote shapes GitHub repos actually carry.
    for remote in [
        "https://github.com/o/r.git",
        "https://github.com/o/r",
        "git@github.com:o/r.git",
        "ssh://git@github.com/o/r.git",
    ] {
        let url = compare_url(remote, Some("v0.1.0"), "v0.2.0");
        assert_eq!(
            url.as_deref(),
            Some("https://github.com/o/r/compare/v0.1.0...v0.2.0"),
            "remote {remote} normalizes to the compare URL"
        );
    }
}

#[test]
fn compare_url_declines_non_github_and_initial_releases() {
    // A foreign forge gets no link — the renderer never guesses a
    // GitLab/Gitea compare shape (VERIFY discipline, §0 rule 4).
    assert_eq!(
        compare_url("https://gitlab.com/o/r.git", Some("v1"), "v2"),
        None
    );
    assert_eq!(
        compare_url("git@gitlab.com:o/r.git", Some("v1"), "v2"),
        None
    );
    // An initial release has no previous tag to compare against.
    assert_eq!(
        compare_url("https://github.com/o/r.git", None, "v0.1.0"),
        None
    );
    assert_eq!(compare_url("", Some("v1"), "v2"), None);
}

#[test]
fn notes_render_the_entry_sections_breaking_first() -> Result<(), BoxError> {
    let notes = render_release_notes(&sample_release(), &NotesContext { compare: None })?;
    // No version heading: the forge release's title already carries the
    // tag; the body is the section groups with breaking first.
    assert!(!notes.contains("## 0.2.0"), "no heading: {notes}");
    let breaking = notes
        .find("### ⚠ BREAKING CHANGES")
        .ok_or("breaking section present")?;
    let features = notes
        .find("### Features")
        .ok_or("features section present")?;
    assert!(breaking < features, "breaking first: {notes}");
    assert!(
        notes.contains("- reshape contract"),
        "bullets verbatim: {notes}"
    );
    assert!(
        notes.contains("- grow surface"),
        "bullets verbatim: {notes}"
    );
    Ok(())
}

#[test]
fn notes_append_the_compare_link_when_context_provides_it() -> Result<(), BoxError> {
    let context = NotesContext {
        compare: Some("https://github.com/o/r/compare/v0.1.0...v0.2.0".to_owned()),
    };
    let notes = render_release_notes(&sample_release(), &context)?;
    assert!(
        notes.ends_with("**Full Changelog**: https://github.com/o/r/compare/v0.1.0...v0.2.0\n"),
        "the compare line closes the body: {notes:?}"
    );
    Ok(())
}

#[test]
fn empty_entry_renders_an_honest_minimal_body() -> Result<(), BoxError> {
    let mut release = sample_release();
    release.changelog = None;
    let bare = render_release_notes(&release, &NotesContext { compare: None })?;
    assert_eq!(
        bare, "",
        "no entry and no compare — no fabrication: {bare:?}"
    );
    let context = NotesContext {
        compare: Some("https://github.com/o/r/compare/v0.1.0...v0.2.0".to_owned()),
    };
    let linked = render_release_notes(&release, &context)?;
    assert_eq!(
        linked, "**Full Changelog**: https://github.com/o/r/compare/v0.1.0...v0.2.0\n",
        "the compare line is the whole body: {linked:?}"
    );
    Ok(())
}

#[test]
fn notes_are_deterministic_i3() -> Result<(), BoxError> {
    let context = NotesContext {
        compare: Some("https://github.com/o/r/compare/v0.1.0...v0.2.0".to_owned()),
    };
    let first = render_release_notes(&sample_release(), &context)?;
    let second = render_release_notes(&sample_release(), &context)?;
    assert_eq!(first, second);
    Ok(())
}

#[test]
fn github_release_notes_style_is_registered() -> Result<(), BoxError> {
    let style = Style::parse("github-release-notes").ok_or("style registered")?;
    assert_eq!(style.name(), "github-release-notes");
    // Context-free render (no compare line — the style registry has no
    // forge context; `render_release_notes` carries it).
    let out = render_release(&sample_release(), "github-release-notes")?;
    assert!(out.contains("### ⚠ BREAKING CHANGES"), "{out}");
    assert!(!out.contains("Full Changelog"), "no fabricated link: {out}");
    Ok(())
}

#[test]
fn extract_version_section_accepts_the_heading_shapes() {
    let text = "# Changelog\n\n## [0.1.1] - 2026-11-14\n\n### Features\n\n- grow\n\n## [0.1.0]\n\n- initial\n";
    let section = extract_version_section(text, "0.1.1")
        .unwrap_or_else(|| panic!("the dated bracketed heading names the release"));
    assert!(section.contains("- grow"), "{section:?}");
    assert!(
        !section.contains("initial"),
        "the section stops at the next heading: {section:?}"
    );
    // The older release is found under its plain bracketed heading.
    let older =
        extract_version_section(text, "0.1.0").unwrap_or_else(|| panic!("plain bracketed heading"));
    assert!(older.contains("- initial"), "{older:?}");
    // Plain and v-prefixed headings name the same release.
    assert!(extract_version_section("## 1.2.0\n\n- x\n", "1.2.0").is_some());
    assert!(extract_version_section("## v1.2.0\n\n- x\n", "1.2.0").is_some());
    assert!(extract_version_section("## [v1.2.0]\n\n- x\n", "1.2.0").is_some());
    // No section, no extraction.
    assert_eq!(extract_version_section("# Changelog\n", "9.9.9"), None);
}

#[test]
fn notes_for_version_render_the_changelog_section() {
    let text = "# Changelog\n\n## [0.1.1]\n\n### ⚠ BREAKING CHANGES\n\n- reshape\n\n### Features\n\n- grow\n";
    let context = NotesContext {
        compare: Some("https://github.com/o/r/compare/v0.1.0...v0.1.1".to_owned()),
    };
    let notes = render_release_notes_for_version(text, "0.1.1", &context);
    let breaking = notes
        .find("### ⚠ BREAKING CHANGES")
        .expect("breaking group");
    let features = notes.find("### Features").expect("features group");
    assert!(breaking < features, "breaking first: {notes:?}");
    assert!(
        notes.contains("- grow"),
        "section bullets verbatim: {notes:?}"
    );
    assert!(
        notes.ends_with("**Full Changelog**: https://github.com/o/r/compare/v0.1.0...v0.1.1\n")
    );
    // A version without a section renders nothing — no fabrication.
    assert_eq!(
        render_release_notes_for_version("# Changelog\n", "9.9.9", &context),
        ""
    );
}

//! The `changelog_style` consumer seam (M2, the §9.4 user-template
//! path): the resolved `halyard.toml` value is either a built-in style
//! name or a user minijinja template *path* — the classifier decides,
//! the shell reads the file (the renderer stays pure, §4). Unknown
//! names keep the `E-CHLOG-STYLE` caller bug; template paths are
//! recognized by shape (a path separator), never by extension guessing.

use halyard_changelog::{Style, StyleSource};
use halyard_core::plan::{Plan, PlanChangelog, PlanRelease};

/// The built-in names all resolve to their styles.
#[test]
fn builtin_names_resolve() {
    let cases = [
        ("keep-a-changelog", Style::KeepAChangelog),
        ("conventional", Style::Conventional),
        ("compact", Style::Compact),
        ("github-release-notes", Style::GithubReleaseNotes),
    ];
    for (name, expected) in cases {
        let source = StyleSource::from_config(name).expect("a shipped name");
        assert_eq!(
            source.builtin(),
            Some(expected),
            "{name} resolves to the built-in style"
        );
        assert!(source.template_path().is_none());
    }
}

/// A path-shaped value is a template path — forward slashes, explicit
/// relative anchors and Windows separators all classify as paths
/// (§16.4: "or path to minijinja template"; style names never carry
/// separators). A suffix without a separator (`template.md.j2`) is a
/// bare name — the classifier never guesses an extension, so it lands
/// in the caller-bug branch pinned below.
#[test]
fn path_shaped_values_classify_as_templates() {
    let cases = [
        "./templates/release.md",
        "templates/release.md",
        "/abs/template.jinja",
        "..\\up\\template.md",
        "C:\\cfg\\template.md",
    ];
    for path in cases {
        let source =
            StyleSource::from_config(path).expect("a path-shaped value is always recognized");
        assert_eq!(
            source.template_path(),
            Some(path),
            "{path} classifies as a template path"
        );
        assert!(source.builtin().is_none());
    }
}

/// A suffix-less filename is ambiguous with style names — it stays a
/// template path only when a separator anchors it; a bare name that is
/// not shipped is the E-CHLOG-STYLE caller bug (the classifier never
/// guesses an extension).
#[test]
fn bare_unknown_names_stay_caller_bugs() {
    let err = StyleSource::from_config("keepachangelog")
        .err()
        .expect("an unknown name is the documented error");
    assert_eq!(err.code(), "E-CHLOG-STYLE", "{err}");
    assert!(
        err.to_string().contains("keepachangelog"),
        "the error names the requested value: {err}"
    );
    let err = StyleSource::from_config("")
        .err()
        .expect("empty is not a path");
    assert_eq!(err.code(), "E-CHLOG-STYLE", "{err}");
}

/// The empty template is the caller's problem at render time, not a
/// classifier problem — a path that exists but compiles to nothing
/// surfaces `E-CHLOG-TEMPLATE` from the renderer (pinned here through
/// the grouped render entry point the CLI consumes).
#[test]
fn grouped_render_replaces_in_place_idempotently() -> Result<(), Box<dyn std::error::Error>> {
    let plan = sample_plan();
    let release = plan.releases.first().expect("one release");
    let first = halyard_changelog::render_releases_into(
        None,
        std::slice::from_ref(&release),
        "keep-a-changelog",
    )?;
    let rerendered = halyard_changelog::render_releases_into(
        Some(first.as_str()),
        std::slice::from_ref(&release),
        "keep-a-changelog",
    )?;
    assert_eq!(
        first, rerendered,
        "the re-render replaces the section in place (I1), byte-identical"
    );
    Ok(())
}

/// Two releases sharing one changelog file (a fixed group) render both
/// sections into that one document, newest first.
#[test]
fn grouped_render_covers_multiple_releases() -> Result<(), Box<dyn std::error::Error>> {
    let plan = two_release_plan();
    let releases: Vec<&PlanRelease> = plan.releases.iter().collect();
    let document = halyard_changelog::render_releases_into(None, &releases, "keep-a-changelog")?;
    assert!(
        document.contains("## [0.2.0]") && document.contains("## [0.1.1]"),
        "both sections land: {document}"
    );
    Ok(())
}

/// The same plan shape as the crate's template tests: one release with
/// a changelog entry preview.
fn sample_plan() -> Plan {
    Plan {
        schema: halyard_core::PLAN_SCHEMA_V1.to_owned(),
        base_branch: "main".to_owned(),
        head: "a".repeat(40),
        plan_hash: "blake3:aa".to_owned(),
        releases: vec![PlanRelease {
            package: "app".to_owned(),
            ecosystem: "cargo".to_owned(),
            from: "0.1.0".to_owned(),
            to: "0.2.0".to_owned(),
            bump: halyard_core::Bump::Minor,
            tag: "v0.2.0".to_owned(),
            reasons: Vec::new(),
            changelog: Some(PlanChangelog {
                file: "CHANGELOG.md".to_owned(),
                entry_md: "## 0.2.0\n\n### ⚠ BREAKING CHANGES\n\n- reshape contract\n\n### Features\n\n- grow surface\n".to_owned(),
            }),
            publish: None,
        }],
        edits: Vec::new(),
        order: vec!["app".to_owned()],
        skipped: Vec::new(),
    }
}

/// Two releases writing the same changelog file — the fixed-group shape.
fn two_release_plan() -> Plan {
    let mut plan = sample_plan();
    plan.releases.push(PlanRelease {
        package: "cli".to_owned(),
        ecosystem: "cargo".to_owned(),
        from: "0.1.0".to_owned(),
        to: "0.1.1".to_owned(),
        bump: halyard_core::Bump::Patch,
        tag: "cli-v0.1.1".to_owned(),
        reasons: Vec::new(),
        changelog: Some(PlanChangelog {
            file: "CHANGELOG.md".to_owned(),
            entry_md: "## 0.1.1\n\n### Fixes\n\n- patch the surface\n".to_owned(),
        }),
        publish: None,
    });
    plan
}

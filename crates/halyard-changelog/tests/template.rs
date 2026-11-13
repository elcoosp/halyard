//! The user-supplied template path (§9.4, §16.4 `changelog.style` as a
//! template path): the same insertion machinery as the built-in styles,
//! driven by a caller-provided minijinja template. The renderer stays
//! pure — the caller reads the file and passes the source text (§4).

use halyard_changelog::{render_plan_template, render_release_template};
use halyard_core::plan::{Plan, PlanChangelog, PlanRelease};

/// The same plan shape as the crate's `tests::sample_plan` fixture.
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

/// House-style render helper: `?`-propagating (no `unwrap` in tests;
/// impossible paths are `unreachable!`).
fn release_of(plan: &Plan) -> &PlanRelease {
    plan.releases
        .first()
        .unwrap_or_else(|| unreachable!("fixture has one release"))
}

#[test]
fn user_template_renders_a_custom_section_shape() {
    let plan = sample_plan();
    // A minimal-environment template: variables + loops only (no builtin
    // filters — the renderer's environment matches the built-in styles',
    // documented on the API and the E-CHLOG-TEMPLATE page).
    let template = "## {{ version }} ({{ tag }})\n\
                    {% for s in sections %}### {{ s.0 }}\n\
                    {% for line in s.1 %}* {{ line }}\n{% endfor %}\
                    {% endfor %}";
    let out = render_release_template(release_of(&plan), template)
        .unwrap_or_else(|e| unreachable!("valid template renders: {e}"));
    assert!(
        out.starts_with("## 0.2.0 (v0.2.0)"),
        "version and tag reach the template: {out}"
    );
    assert!(
        out.contains("### ⚠ BREAKING CHANGES"),
        "sections pass through: {out}"
    );
    assert!(
        out.contains("* reshape contract"),
        "bullets reachable: {out}"
    );
    assert!(
        out.contains("### Features"),
        "second section present: {out}"
    );
}

#[test]
fn template_syntax_errors_are_e_chlog_template() {
    let plan = sample_plan();
    let broken = "{% for s in sections %}- {{ s.0 }}"; // unclosed block
    let Err(err) = render_release_template(release_of(&plan), broken) else {
        unreachable!("an unclosed block must be a template error")
    };
    assert_eq!(err.code(), "E-CHLOG-TEMPLATE");
    assert!(
        err.to_string().starts_with("[E-CHLOG-TEMPLATE]"),
        "display leads with the code: {err}"
    );
}

#[test]
fn template_render_failure_does_not_mutate_the_document() {
    // Fail-closed on the whole-document path: a broken template yields an
    // error, never a partially inserted document.
    let plan = sample_plan();
    let existing = "# Changelog\n\n## [Unreleased]\n\n<!-- halyard:insert -->\n";
    let broken = "{% for s in sections %}";
    let Err(_) = render_plan_template(&plan, broken, Some(existing)) else {
        unreachable!("broken template must fail the document render")
    };
}

#[test]
fn plan_template_shares_the_insertion_machinery_and_is_idempotent() {
    let plan = sample_plan();
    let template = "## [{{ version }}]\n{% for s in sections %}\
                    {% for line in s.1 %}- {{ line }}\n{% endfor %}{% endfor %}";
    let existing = "# Changelog\n\nAll notable changes live here.\n\n## [Unreleased]\n\n<!-- halyard:insert -->\n";
    let first = render_plan_template(&plan, template, Some(existing))
        .unwrap_or_else(|e| unreachable!("valid template renders: {e}"));
    // Idempotence (I1/I3): the re-run replaces the release section in
    // place and is byte-identical.
    let second = render_plan_template(&plan, template, Some(first.as_str()))
        .unwrap_or_else(|e| unreachable!("re-render: {e}"));
    assert_eq!(first, second, "template re-run is byte-identical");
    assert!(first.contains("- reshape contract"));
    assert!(
        first.contains("# Changelog"),
        "the header survives verbatim"
    );
}

#[test]
fn fresh_document_from_a_template_matches_the_builtin_anchor_rules() {
    let plan = sample_plan();
    let template = "## [{{ version }}]\n- templated";
    let fresh = render_plan_template(&plan, template, None)
        .unwrap_or_else(|e| unreachable!("valid template renders: {e}"));
    assert!(
        fresh.starts_with("# Changelog\n"),
        "fresh documents keep the built-in header block: {fresh}"
    );
    assert!(
        fresh.contains("## [Unreleased]"),
        "marker/anchor block present"
    );
    assert!(
        fresh.contains("- templated"),
        "the template body is inserted"
    );
}

#[test]
fn builtin_styles_are_untouched_by_the_template_path() {
    // Regression: the built-in dispatch still renders after the
    // template-path refactor (the inline goldens pin the bytes).
    let plan = sample_plan();
    let out = halyard_changelog::render_plan(&plan, "conventional", None)
        .unwrap_or_else(|e| unreachable!("builtin style renders: {e}"));
    assert!(
        out.contains("### feat"),
        "conventional grouping intact: {out}"
    );
    let Err(err) = halyard_changelog::render_plan(&plan, "not-a-style", None) else {
        unreachable!("unknown style names still fail with E-CHLOG-STYLE")
    };
    assert_eq!(err.code(), "E-CHLOG-STYLE");
}

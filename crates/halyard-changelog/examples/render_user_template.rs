//! The user-template path demo (M2, §9.4/§16.4): the resolved
//! `changelog.style` is classified by [`halyard_changelog::StyleSource`]
//! — a built-in name or a template *path* whose source the shell reads
//! (the renderer stays pure, §4). The demo renders one release through
//! a user template into a fresh document and shows the idempotent
//! re-render (I1: the same plan's re-render is byte-identical).
//!
//! Run: `cargo run -p halyard-changelog --example render_user_template`

use halyard_changelog::{StyleSource, render_releases_into_template};
use halyard_core::Bump;
use halyard_core::plan::{PlanChangelog, PlanRelease};

/// The user template: the documented environment (`version`, `tag`,
/// `sections` — each section is `(title, bullet lines)`), loops and ifs
/// only, no builtin filters or globals.
const USER_TEMPLATE: &str = "## {{ version }} ({{ tag }})\n\
{% for section in sections %}\
### {{ section[0] }}\n\
{% for line in section[1] %}- {{ line }}\n{% endfor %}\
{% endfor %}";

/// One planned release with the planner's entry preview.
fn release() -> PlanRelease {
    PlanRelease {
        package: "core".to_owned(),
        ecosystem: "cargo".to_owned(),
        from: "0.4.1".to_owned(),
        to: "0.5.0".to_owned(),
        bump: Bump::Minor,
        tag: "core-v0.5.0".to_owned(),
        reasons: Vec::new(),
        changelog: Some(PlanChangelog {
            file: "crates/core/CHANGELOG.md".to_owned(),
            entry_md: "## 0.5.0\n\n### ⚠ BREAKING CHANGES\n\n- reshape the contract\n\n\
                       ### Features\n\n- grow the surface\n"
                .to_owned(),
        }),
        publish: None,
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // The classifier: names resolve to built-ins, separator-carrying
    // values are template paths, unknown names are the E-CHLOG-STYLE
    // caller bug.
    for setting in [
        "keep-a-changelog",
        "templates/release.md.jinja",
        "keepachangelog",
    ] {
        match StyleSource::from_config(setting) {
            Ok(source) => println!("{setting:?} → {source:?}"),
            Err(error) => println!("{setting:?} → {error}"),
        }
    }

    // The template path: the shell reads the file (here: the constant
    // stands in for the read) and hands over the source text.
    let source = StyleSource::from_config("templates/release.md.jinja")?;
    let template_source = source
        .template_path()
        .map(|_| USER_TEMPLATE)
        .unwrap_or_default();
    let release = release();
    let releases = [&release];

    // Fresh document: the §9.4 machinery writes the header + the
    // marker + the rendered section.
    let fresh = render_releases_into_template(None::<&str>, &releases, template_source)?;
    println!("--- fresh document ---\n{fresh}");

    // The re-render replaces the section in place — byte-identical (I1;
    // the CLI stage reports `unchanged` and writes nothing).
    let rerendered =
        render_releases_into_template(Some(fresh.as_str()), &releases, template_source)?;
    println!("re-render byte-identical: {}", fresh == rerendered);
    Ok(())
}

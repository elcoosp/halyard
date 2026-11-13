//! Changelog rendering (M3.5 + M3.5b, §9.4): the built-in styles over a
//! [`Plan`]. The renderer is pure — a [`Plan`] plus the existing file
//! text in, the new file text out — and idempotent: re-running against a
//! file that already carries the plan's release replaces only the
//! `Unreleased` section and is byte-identical to the first run (I1/I3).
//!
//! The built-in style set ([`Style`]): `keep-a-changelog` (M3.5),
//! `conventional` (grouped by conventional commit type, M3.5b) and
//! `compact` (one flat bullet list, M3.5b). Other §9.4 styles
//! (`github-release-notes`) need forge context (compare links, remote
//! URL) and stay unshipped; requesting them yields
//! [`ChangelogError::Style`] (`E-CHLOG-STYLE`).

#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::fmt::Write as _;

use halyard_core::plan::{Plan, PlanRelease};
use minijinja::{Environment, context};

/// The stable `<!-- halyard:insert -->` marker (§9.4) and the Unreleased
/// heading the renderer replaces.
pub const INSERT_MARKER: &str = "<!-- halyard:insert -->";
const UNRELEASED_HEADING: &str = "## [Unreleased]";

/// The built-in changelog styles (§9.4) and their config names — the
/// M3.5b style registry that replaced the `style != "keep-a-changelog"`
/// string check. `github-release-notes` stays unshipped (it needs forge
/// context: compare links, remote URL).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    /// `keep-a-changelog`: `## [version]` + `### Section` groups taken
    /// from the planner's entry preview (M3.5).
    KeepAChangelog,
    /// `conventional`: grouped by conventional commit type (`### feat`,
    /// `### fix`, …), `### ⚠ BREAKING CHANGES` first (M3.5b).
    Conventional,
    /// `compact`: one flat bullet list per release, breaking entries
    /// first, no section grouping (M3.5b).
    Compact,
}

impl Style {
    /// Parses a style name as written in `halyard.toml` (`changelog.style`)
    /// or an API argument.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "keep-a-changelog" => Some(Self::KeepAChangelog),
            "conventional" => Some(Self::Conventional),
            "compact" => Some(Self::Compact),
            _ => None,
        }
    }

    /// The style's config name.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::KeepAChangelog => "keep-a-changelog",
            Self::Conventional => "conventional",
            Self::Compact => "compact",
        }
    }
}

/// Changelog failures (§15.4 `E-CHLOG-*`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChangelogError {
    /// The requested style is not shipped (`E-CHLOG-STYLE`); the built-in
    /// v1 set is `keep-a-changelog` only.
    Style {
        /// The style name as requested.
        requested: String,
    },
    /// The existing changelog text cannot be updated (`E-CHLOG-FILE`):
    /// no `Unreleased` section, no marker and no header block to anchor
    /// the insertion.
    File {
        /// The repo-relative changelog path.
        file: String,
        /// What is missing.
        reason: String,
    },
}

impl ChangelogError {
    /// Stable §15.4 code.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Style { .. } => "E-CHLOG-STYLE",
            Self::File { .. } => "E-CHLOG-FILE",
        }
    }
}

impl std::fmt::Display for ChangelogError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Style { requested } => write!(
                f,
                "[E-CHLOG-STYLE] changelog style {requested:?} is not available; the built-in \
                 styles are \"keep-a-changelog\", \"conventional\" and \"compact\""
            ),
            Self::File { file, reason } => write!(
                f,
                "[E-CHLOG-FILE] cannot update changelog {file}: {reason} (add an \
                 `## [Unreleased]` section or a {INSERT_MARKER} marker)"
            ),
        }
    }
}

impl std::error::Error for ChangelogError {}

/// Renders one release's keep-a-changelog section from its plan entry:
/// `⚠ BREAKING CHANGES` first, then Added (feat), Fixed (fix) and
/// Performance (perf). Hidden types are already excluded by the planner's
/// entry preview; the section re-uses it verbatim so plan and changelog
/// can never disagree (I8).
///
/// # Errors
///
/// [`ChangelogError::Style`] for any style other than
/// `keep-a-changelog`.
pub fn render_release(release: &PlanRelease, style: &str) -> Result<String, ChangelogError> {
    let style = Style::parse(style).ok_or_else(|| ChangelogError::Style {
        requested: style.to_owned(),
    })?;
    render_release_styled(release, style)
}

/// Renders one release section in the parsed [`Style`] (the dispatch
/// target of [`render_release`]).
fn render_release_styled(release: &PlanRelease, style: Style) -> Result<String, ChangelogError> {
    // `empty()` (no globals/filters, no auto-escape) instead of the
    // deprecated-without-`serde` `new()`: the keep-a-changelog template is
    // plain Jinja over the plan's own values — no builtin filters or
    // globals — so behavior is identical and no `serde` feature is pulled
    // into the layer-restricted dependency set (§7.2).
    let mut env = Environment::empty();
    env.add_template("release", RELEASE_TEMPLATE)
        .map_err(|e| ChangelogError::Style {
            requested: format!("template error: {e}"),
        })?;
    let template = env
        .get_template("release")
        .map_err(|e| ChangelogError::Style {
            requested: format!("template error: {e}"),
        })?;
    let sections = sections_of(
        &release
            .changelog
            .as_ref()
            .map(|c| c.entry_md.clone())
            .unwrap_or_default(),
    );
    match style {
        Style::KeepAChangelog => {
            // Dateless heading: the release step (which owns the clock)
            // stamps dates; the pure renderer never reads one (§4
            // purity).
            template
                .render(context! {
                    version => release.to,
                    tag => release.tag,
                    sections => sections,
                })
                .map_err(|e| ChangelogError::Style {
                    requested: format!("render error: {e}"),
                })
        }
        Style::Conventional => Ok(conventional_body(&release.to, &sections)),
        Style::Compact => Ok(compact_body(&release.to, &sections)),
    }
}

/// Renders the full changelog document: every release in the plan, newest
/// first, inserted after the header block (or the marker) of `existing`
/// (`None` = a fresh document). Idempotent: an existing release section
/// for the same version is replaced in place.
///
/// # Errors
///
/// [`ChangelogError::Style`] / [`ChangelogError::File`].
pub fn render_plan(
    plan: &Plan,
    style: &str,
    existing: Option<&str>,
) -> Result<String, ChangelogError> {
    let style = Style::parse(style).ok_or_else(|| ChangelogError::Style {
        requested: style.to_owned(),
    })?;
    let mut sections: Vec<String> = Vec::new();
    for release in &plan.releases {
        sections.push(render_release_styled(release, style)?);
    }
    let body = sections.join("\n");
    let versions: Vec<&str> = plan.releases.iter().map(|r| r.to.as_str()).collect();
    let Some(existing) = existing else {
        return Ok(format!(
            "# Changelog\n\nAll notable changes to this project are documented here.\n\n\
             {UNRELEASED_HEADING}\n\n{INSERT_MARKER}\n\n{body}"
        ));
    };
    // Idempotent insertion (I1): any existing release section for one of
    // the plan's versions is removed first (the re-run replaces it in
    // place, §9.4), then the fresh body is inserted after the marker or
    // the `Unreleased` heading. Anchor-less files are an error, never a
    // silent rewrite.
    let cleaned = remove_release_sections(existing, &versions);
    if let Some(marker_pos) = cleaned.find(INSERT_MARKER) {
        let head = cleaned.get(..marker_pos).unwrap_or_default();
        let tail = cleaned
            .get(marker_pos + INSERT_MARKER.len()..)
            .unwrap_or_default()
            .trim_start_matches('\n');
        // With nothing after the marker the output must be byte-identical
        // to the fresh document (no dangling blank-line tail).
        if tail.is_empty() {
            return Ok(format!("{head}{INSERT_MARKER}\n\n{body}"));
        }
        return Ok(format!("{head}{INSERT_MARKER}\n\n{body}\n\n{tail}"));
    }
    let unreleased = cleaned
        .find(UNRELEASED_HEADING)
        .ok_or_else(|| ChangelogError::File {
            file: "CHANGELOG.md".to_owned(),
            reason: "no `[Unreleased]` section and no insertion marker".to_owned(),
        })?;
    let after_heading = unreleased + UNRELEASED_HEADING.len();
    let rest = cleaned.get(after_heading..).unwrap_or_default();
    let next_section = rest
        .find("\n## [")
        .map_or(cleaned.len(), |offset| after_heading + offset + 1);
    let head = cleaned.get(..after_heading).unwrap_or_default();
    let tail = cleaned.get(next_section..).unwrap_or_default();
    Ok(format!("{head}\n{body}\n\n{tail}"))
}

/// True when `line` is a `## [<version>]` heading whose version is one of
/// `versions` (a re-run's replace targets, §9.4).
fn is_release_heading(line: &str, versions: &[&str]) -> bool {
    let Some(inner) = line.strip_prefix("## [") else {
        return false;
    };
    let Some(end) = inner.find(']') else {
        return false;
    };
    versions.contains(&&inner[..end])
}

/// Drops every `## [<version>]` section block whose version matches one
/// of `versions`, keeping every other byte (blank lines between the
/// remaining sections are collapsed only where the removed block was).
/// Line-based over `split_inclusive`, so the untouched text (dates,
/// trailing newline shape) survives verbatim (I4).
fn remove_release_sections(text: &str, versions: &[&str]) -> String {
    if versions.is_empty() {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut skipping = false;
    for line in text.split_inclusive('\n') {
        let trimmed = line.strip_suffix('\n').unwrap_or(line);
        if !skipping && is_release_heading(trimmed, versions) {
            skipping = true;
            continue;
        }
        if skipping && trimmed.starts_with("## [") {
            skipping = is_release_heading(trimmed, versions);
            if !skipping {
                out.push_str(line);
            }
            continue;
        }
        if !skipping {
            out.push_str(line);
        }
    }
    out
}

/// Splits the planner's entry markdown (rendered `## <version>` block
/// with `### Section` groups) back into section name → bullet lines.
fn sections_of(entry_md: &str) -> Vec<(String, Vec<String>)> {
    let mut sections: Vec<(String, Vec<String>)> = Vec::new();
    let mut current: Option<(String, Vec<String>)> = None;
    for line in entry_md.lines() {
        if let Some(title) = line.strip_prefix("### ") {
            if let Some(previous) = current.take() {
                sections.push(previous);
            }
            current = Some((title.to_owned(), Vec::new()));
        } else if let Some(bullet) = line.strip_prefix("- ")
            && let Some((_, lines)) = current.as_mut()
        {
            lines.push(bullet.to_owned());
        }
    }
    if let Some(last) = current.take() {
        sections.push(last);
    }
    sections
}

/// Maps the planner's section titles onto conventional type names (the
/// documented M3.5b map; unknown titles pass through verbatim — never a
/// guess).
fn conventional_heading(title: &str) -> &str {
    match title {
        "Features" => "feat",
        "Bug Fixes" | "Fixed" => "fix",
        "Performance" => "perf",
        "Reverts" => "revert",
        _ => title,
    }
}

/// The parsed sections of one release: `(heading, bullet lines)` pairs.
type Sections = Vec<(String, Vec<String>)>;

/// Splits the parsed sections into the breaking group and the rest (both
/// styles put breaking entries first, §9.4).
fn split_breaking(sections: &Sections) -> (Sections, Sections) {
    let (breaking, rest): (Sections, Sections) = sections
        .iter()
        .cloned()
        .partition(|(title, _)| title == "⚠ BREAKING CHANGES");
    (breaking, rest)
}

/// The `conventional` release body: `### <type>` groups (breaking first,
/// then document order).
fn conventional_body(version: &str, sections: &Sections) -> String {
    let (breaking, rest) = split_breaking(sections);
    let mut out = format!("## [{version}]\n");
    for (title, lines) in breaking.iter().chain(rest.iter()) {
        let _ = writeln!(out, "\n### {}\n", conventional_heading(title));
        for line in lines {
            let _ = writeln!(out, "- {line}");
        }
    }
    out
}

/// The `compact` release body: one flat bullet list (breaking entries
/// first, then document order), no section grouping.
fn compact_body(version: &str, sections: &Sections) -> String {
    let (breaking, rest) = split_breaking(sections);
    let mut out = format!("## [{version}]\n");
    for (title, lines) in breaking.iter().chain(rest.iter()) {
        let _ = title;
        for line in lines {
            let _ = writeln!(out, "- {line}");
        }
    }
    out
}

/// The keep-a-changelog release template (minijinja, compiled per call —
/// releases are one-shot documents, not hot-loop text).
static RELEASE_TEMPLATE: &str = r"## [{{ version }}]
{% for section in sections %}
### {{ section.0 }}

{% for line in section.1 %}- {{ line }}
{% endfor %}
{% endfor %}";

/// Convenience: a map view of the plan's releases by version (used by the
/// idempotence tests to compare pre/post states).
pub fn releases_by_version(plan: &Plan) -> BTreeMap<&str, &PlanRelease> {
    plan.releases.iter().map(|r| (r.to.as_str(), r)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use halyard_core::plan::{Plan, PlanChangelog, PlanRelease};

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
    fn golden(plan: &Plan, existing: Option<&str>) -> Result<String, ChangelogError> {
        render_plan(plan, "keep-a-changelog", existing)
    }

    #[test]
    fn fresh_document_has_breaking_first() -> Result<(), Box<dyn std::error::Error>> {
        let plan = sample_plan();
        let out = golden(&plan, None)?;
        let breaking = out
            .find("⚠ BREAKING CHANGES")
            .ok_or("breaking section rendered")?;
        let features = out
            .find("### Features")
            .ok_or("features section rendered")?;
        assert!(breaking < features);
        assert!(out.contains("## [0.2.0]"));
        insta::assert_snapshot!(out);
        Ok(())
    }

    #[test]
    fn rerun_is_byte_identical() -> Result<(), Box<dyn std::error::Error>> {
        let plan = sample_plan();
        let first = golden(&plan, None)?;
        let second = golden(&plan, Some(&first))?;
        assert_eq!(first, second);
        Ok(())
    }

    #[test]
    fn unreleased_section_is_replaced_only() -> Result<(), Box<dyn std::error::Error>> {
        let plan = sample_plan();
        let existing = "# Changelog\n\n## [Unreleased]\n\n- old note\n\n## [0.1.0] - 2020-01-01\n\n### Added\n\n- initial\n";
        let out = golden(&plan, Some(existing))?;
        assert!(out.contains("## [0.2.0]"));
        assert!(!out.contains("old note"));
        assert!(out.contains("## [0.1.0] - 2020-01-01"));
        assert!(out.contains("### Added"));
        // Idempotent on the merged document too.
        let again = golden(&plan, Some(&out))?;
        assert_eq!(out, again);
        Ok(())
    }

    #[test]
    fn unknown_style_is_an_error() {
        match render_plan(&sample_plan(), "github-release-notes", None) {
            Err(err) => assert_eq!(err.code(), "E-CHLOG-STYLE"),
            Ok(out) => unreachable!("unknown style must error, got {out:?}"),
        }
    }
    // --- M3.5b style registry + compact/conventional styles (tests
    // written first; the renderers land in the green step) ---

    #[test]
    fn style_parse_and_names_round_trip() {
        for (name, expected) in [
            ("keep-a-changelog", Style::KeepAChangelog),
            ("conventional", Style::Conventional),
            ("compact", Style::Compact),
        ] {
            let parsed = Style::parse(name).unwrap_or_else(|| unreachable!("{name} parses"));
            assert_eq!(parsed, expected);
            assert_eq!(parsed.name(), name);
        }
        assert!(Style::parse("keepachangelog").is_none());
        assert!(Style::parse("").is_none());
    }

    #[test]
    fn compact_style_flattens_bullets_breaking_first() -> Result<(), Box<dyn std::error::Error>> {
        let plan = sample_plan();
        let out = render_release(plan.releases.first().ok_or("release")?, "compact")?;
        // No section headings in the compact body.
        assert!(!out.contains("### "), "{out}");
        assert!(out.contains("## [0.2.0]"), "{out}");
        // Breaking entries come first, then the remaining bullets.
        let breaking = out.find("- reshape contract").ok_or("breaking bullet")?;
        let feat = out.find("- grow surface").ok_or("feature bullet")?;
        assert!(breaking < feat, "breaking first: {out}");
        insta::assert_snapshot!(out);
        Ok(())
    }

    #[test]
    fn conventional_style_groups_by_type_breaking_first() -> Result<(), Box<dyn std::error::Error>>
    {
        let plan = sample_plan();
        let out = render_release(plan.releases.first().ok_or("release")?, "conventional")?;
        // The planner's section titles map onto conventional type names;
        // the breaking group keeps its heading and stays first.
        let breaking = out.find("### ⚠ BREAKING CHANGES").ok_or("breaking group")?;
        let feat = out.find("### feat").ok_or("feat group")?;
        assert!(breaking < feat, "breaking first: {out}");
        assert!(out.contains("- reshape contract"), "{out}");
        assert!(out.contains("- grow surface"), "{out}");
        insta::assert_snapshot!(out);
        Ok(())
    }

    #[test]
    fn conventional_maps_the_documented_section_titles() -> Result<(), Box<dyn std::error::Error>> {
        let release = PlanRelease {
            changelog: Some(PlanChangelog {
                file: "CHANGELOG.md".to_owned(),
                entry_md: concat!(
                    "## 1.0.0\n\n### Bug Fixes\n\n- patch span drift\n\n",
                    "### Performance\n\n- parse faster\n"
                )
                .to_owned(),
            }),
            ..sample_plan().releases.first().ok_or("release")?.clone()
        };
        let out = render_release(&release, "conventional")?;
        assert!(out.contains("### fix"), "{out}");
        assert!(out.contains("### perf"), "{out}");
        assert!(!out.contains("### Bug Fixes"), "titles are mapped: {out}");
        assert!(!out.contains("### Performance"), "titles are mapped: {out}");
        Ok(())
    }

    #[test]
    fn compact_and_conventional_reruns_are_byte_identical() -> Result<(), Box<dyn std::error::Error>>
    {
        let plan = sample_plan();
        for style in ["compact", "conventional"] {
            let first = render_plan(&plan, style, None)?;
            let second = render_plan(&plan, style, Some(&first))?;
            assert_eq!(first, second, "{style} re-run is byte-identical");
        }
        Ok(())
    }

    #[test]
    fn unknown_style_error_names_the_built_ins() {
        match render_plan(&sample_plan(), "github-release-notes", None) {
            Err(err) => {
                assert_eq!(err.code(), "E-CHLOG-STYLE");
                let message = err.to_string();
                assert!(
                    message.contains("keep-a-changelog")
                        && message.contains("conventional")
                        && message.contains("compact"),
                    "message names the shipped styles: {message}"
                );
            }
            Ok(out) => unreachable!("unknown style must error, got {out:?}"),
        }
    }
}

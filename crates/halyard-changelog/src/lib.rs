//! Changelog rendering (M3.5, §9.4): the `keep-a-changelog` style over a
//! [`Plan`]. The renderer is pure — a [`Plan`] plus the existing file
//! text in, the new file text out — and idempotent: re-running against a
//! file that already carries the plan's release replaces only the
//! `Unreleased` section and is byte-identical to the first run (I1/I3).
//!
//! Other §9.4 styles (`conventional`, `compact`, `github-release-notes`)
//! are deferred to M3.5b; requesting them yields [`ChangelogError::Style`]
//! (`E-CHLOG-STYLE`).

#![forbid(unsafe_code)]

use std::collections::BTreeMap;

use halyard_core::plan::{Plan, PlanRelease};
use minijinja::{Environment, context};

/// The stable `<!-- halyard:insert -->` marker (§9.4) and the Unreleased
/// heading the renderer replaces.
pub const INSERT_MARKER: &str = "<!-- halyard:insert -->";
const UNRELEASED_HEADING: &str = "## [Unreleased]";

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
                "[E-CHLOG-STYLE] changelog style {requested:?} is not available in v1; the \
                 built-in style is \"keep-a-changelog\" (a minijinja template path arrives with \
                 M3.5b)"
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
    if style != "keep-a-changelog" {
        return Err(ChangelogError::Style {
            requested: style.to_owned(),
        });
    }
    let mut env = Environment::new();
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
    // Dateless heading: the release step (which owns the clock) stamps
    // dates; the pure renderer never reads one (§4 purity).
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
    if style != "keep-a-changelog" {
        return Err(ChangelogError::Style {
            requested: style.to_owned(),
        });
    }
    let mut sections: Vec<String> = Vec::new();
    for release in &plan.releases {
        sections.push(render_release(release, style)?);
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

    #[test]
    fn fresh_document_has_breaking_first() {
        let plan = sample_plan();
        let out = render_plan(&plan, "keep-a-changelog", None).unwrap();
        let breaking = out.find("⚠ BREAKING CHANGES").unwrap();
        let features = out.find("### Features").unwrap();
        assert!(breaking < features);
        assert!(out.contains("## [0.2.0]"));
        insta::assert_snapshot!(out);
    }

    #[test]
    fn rerun_is_byte_identical() {
        let plan = sample_plan();
        let first = render_plan(&plan, "keep-a-changelog", None).unwrap();
        let second = render_plan(&plan, "keep-a-changelog", Some(&first)).unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn unreleased_section_is_replaced_only() {
        let plan = sample_plan();
        let existing = "# Changelog\n\n## [Unreleased]\n\n- old note\n\n## [0.1.0] - 2020-01-01\n\n### Added\n\n- initial\n";
        let out = render_plan(&plan, "keep-a-changelog", Some(existing)).unwrap();
        assert!(out.contains("## [0.2.0]"));
        assert!(!out.contains("old note"));
        assert!(out.contains("## [0.1.0] - 2020-01-01"));
        assert!(out.contains("### Added"));
        // Idempotent on the merged document too.
        let again = render_plan(&plan, "keep-a-changelog", Some(&out)).unwrap();
        assert_eq!(out, again);
    }

    #[test]
    fn unknown_style_is_an_error() {
        let err = render_plan(&sample_plan(), "compact", None).unwrap_err();
        assert_eq!(err.code(), "E-CHLOG-STYLE");
    }
}

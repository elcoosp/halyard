//! The release-notes pipeline (§9.4 `github-release-notes`, the
//! forge-release body): the per-release entry point the §11.1 machine's
//! `ReleaseSpec.notes` consumes. The style registry is context-free —
//! the forge context (the compare link derived from the configured
//! remote) rides [`NotesContext`] in, so the renderer stays pure (§4):
//! strings in, deterministic markdown out (I3), no network, no clock.
//!
//! The compare-link derivation ([`compare_url`]) normalizes the remote
//! URL shapes GitHub repositories actually carry. A foreign forge gets
//! no link — the renderer never guesses a GitLab/Gitea compare shape
//! (the §0 rule 4 `VERIFY` discipline); an initial release has no
//! previous tag and likewise renders none.

use halyard_core::plan::PlanRelease;

use crate::ChangelogError;

/// The forge context of one release's notes: the compare URL the shell
/// derived from the configured remote and the previous tag (`None` when
/// there is none — a non-GitHub remote or an initial release; the body
/// renders without the `**Full Changelog**` line, never a guessed link).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotesContext {
    /// The compare URL (`{web-base}/compare/{from}...{to}`), when
    /// derivable.
    pub compare: Option<String>,
}

/// Renders one release's forge-release notes (§9.4): the planner's entry
/// preview re-grouped as the `github-release-notes` body (breaking
/// first), closed by the compare line when the context provides one. No
/// version heading — the forge release's title already carries the tag.
/// An empty entry with no compare link renders an empty body: no
/// fabrication.
///
/// This is the plan-driven entry (the release the plan would make); the
/// tag-phase notes — the release that was already built — render through
/// [`render_release_notes_for_version`] from the package changelog's
/// section.
///
/// # Errors
///
/// [`ChangelogError::Style`] is unreachable through this entry point
/// today (the body is built directly, not through the registry), but the
/// signature keeps the shared `Result` shape of the rendering seam.
pub fn render_release_notes(
    release: &PlanRelease,
    context: &NotesContext,
) -> Result<String, ChangelogError> {
    let sections = crate::sections_of(
        &release
            .changelog
            .as_ref()
            .map(|c| c.entry_md.clone())
            .unwrap_or_default(),
    );
    Ok(crate::github_notes_body(
        &release.to,
        &sections,
        context.compare.as_deref(),
        /* heading */ false,
    ))
}

/// Renders the forge-release notes for a version that is *already
/// built* (the §11.1 tag phase's `create_release` payload): the version's
/// section is extracted from the package changelog text the caller read
/// (the shell owns file I/O; the renderer stays pure, §4) and re-grouped
/// as the `github-release-notes` body. A version without a section
/// renders an empty body — the merged release PR writes the section, so
/// its absence means there is nothing to say, never something to invent.
#[must_use]
pub fn render_release_notes_for_version(
    changelog_text: &str,
    version: &str,
    context: &NotesContext,
) -> String {
    let Some(section) = extract_version_section(changelog_text, version) else {
        return String::new();
    };
    let sections = crate::sections_of(&section);
    crate::github_notes_body(
        version,
        &sections,
        context.compare.as_deref(),
        /* heading */ false,
    )
}

/// Extracts one release section from existing changelog text (§9.4): the
/// `## ` heading naming `version` — `## [1.2.0]`, `## [1.2.0] - date`,
/// `## [v1.2.0]`, `## 1.2.0` and `## v1.2.0` all name the same release —
/// plus its body until the next `## ` heading or the end of the text.
/// Deterministic; `None` when no heading names the version.
#[must_use]
pub fn extract_version_section(changelog_text: &str, version: &str) -> Option<String> {
    let names = |heading: &str| {
        heading == version
            || heading == format!("v{version}")
            || heading.strip_prefix('[').and_then(|h| h.strip_suffix(']')) == Some(version)
            || heading.strip_prefix('[').and_then(|h| h.strip_suffix(']')) == Some(&format!("v{version}"))
            // Dated keep-a-changelog headings: `1.2.0 - 2026-11-14`.
            || heading
                .split_once(" - ")
                .is_some_and(|(v, _)| v == version || v == format!("v{version}"))
            || heading
                .strip_prefix('[')
                .and_then(|h| h.split_once(']'))
                .is_some_and(|(v, _)| v == version || v == format!("v{version}"))
    };
    let mut in_section = false;
    let mut section = String::new();
    for line in changelog_text.lines() {
        if let Some(heading) = line.strip_prefix("## ") {
            if in_section {
                // The next release heading closes the section.
                break;
            }
            let heading = heading.trim();
            if names(heading) {
                in_section = true;
            }
            continue;
        }
        if in_section {
            section.push_str(line);
            section.push('\n');
        }
    }
    in_section.then_some(section)
}

/// Derives the GitHub compare URL from a configured remote URL and the
/// release pair (`from_tag` → `to_tag`). `None` for non-GitHub remotes,
/// an initial release (no previous tag) or an empty remote — the caller
/// renders no link instead of guessing a foreign forge's compare shape.
///
/// Normalized shapes (the four GitHub remotes carry):
/// `https://github.com/o/r(.git)`, `git@github.com:o/r(.git)`,
/// `ssh://git@github.com/o/r(.git)`.
#[must_use]
pub fn compare_url(remote_url: &str, from_tag: Option<&str>, to_tag: &str) -> Option<String> {
    let (Some(from), base) = (from_tag, web_base(remote_url)?) else {
        return None;
    };
    Some(format!("{base}/compare/{from}...{to_tag}"))
}

/// The `https://github.com/o/r` web base of a remote URL, when the
/// remote is GitHub-shaped.
fn web_base(remote_url: &str) -> Option<String> {
    const HOST: &str = "github.com";
    let path = if remote_url.starts_with("git@")
        && let Some(rest) = remote_url.strip_prefix("git@")
        && let Some(path) = rest.strip_prefix(HOST).and_then(|p| p.strip_prefix(':'))
    {
        path
    } else if remote_url.starts_with("https://")
        && let Some(path) = remote_url
            .strip_prefix("https://")
            .and_then(|p| p.strip_prefix(HOST))
    {
        path.strip_prefix('/').unwrap_or(path)
    } else if remote_url.starts_with("ssh://")
        && let Some(rest) = remote_url.strip_prefix("ssh://")
        && let Some(path) = rest
            .strip_prefix("git@")
            .and_then(|p| p.strip_prefix(HOST))
            .and_then(|p| p.strip_prefix(':').or_else(|| p.strip_prefix('/')))
    {
        path
    } else {
        return None;
    };
    let path = path.strip_suffix('/').unwrap_or(path);
    let path = path.strip_suffix(".git").unwrap_or(path);
    if path.is_empty() || path.contains(' ') {
        return None;
    }
    Some(format!("https://{HOST}/{path}"))
}

#[cfg(test)]
mod tests {
    use super::web_base;

    #[test]
    fn web_base_rejects_foreign_and_broken_shapes() {
        assert_eq!(web_base("https://gitlab.com/o/r.git"), None);
        assert_eq!(web_base("git@gitlab.com:o/r.git"), None);
        assert_eq!(web_base(""), None);
        assert_eq!(web_base("https://github.com/"), None);
        // A path is required; a bare host is not a repository.
        assert_eq!(web_base("git@github.com:"), None);
    }

    #[test]
    fn web_base_strips_one_trailing_git() {
        assert_eq!(
            web_base("https://github.com/o/r.git.git"),
            Some("https://github.com/o/r.git".to_owned()),
            "exactly one .git suffix is stripped — never a guess"
        );
    }
}

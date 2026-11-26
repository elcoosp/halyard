//! The Go release-tag grammar (M8 ADR-0057): subdir tag templates and the
//! major-version-suffix rule.
//!
//! Go publishes a module version by tagging the repository — the
//! publishing reference's own shape is `git tag v0.1.0` at the module
//! root (`docs/research/go-module-grammars.md` §3). A module rooted in a
//! subdirectory carries the directory as the tag prefix, and a module
//! path ending in `/vN` (N ≥ 2) declares its major version
//! (`go.dev/ref/mod`: *"Starting with major version 2, module paths must
//! have a major version suffix like /v2 that matches the major
//! version."*). Halyard renders Go tags through the shared template
//! grammar (appendix A.3): the per-package template is the module's
//! directory, `/v{version}` — the repo root renders `v{version}`. The
//! suffix rule itself lives with the core package model
//! ([`halyard_core::package::major_version_suffix`], re-exported here)
//! so the planner's release guard reads the same rule.

pub use halyard_core::package::major_version_suffix as major_suffix;

use halyard_core::package::TagTemplate;

/// The module path's major version suffix (the FINAL `/vN`, N ≥ 2).
/// `go.dev/ref/mod`'s path rules: a final element `/vN` where *"N must
/// not begin with a leading zero, must not be /v1, and must not contain
/// any dots"*; the suffix *"is not allowed at major versions v0 or v1"*.
/// The per-package tag template for a module rooted at `root` (the
/// repo-relative directory, empty = the repository root): `v{version}`
/// at the root, `{root}/v{version}` elsewhere — the subdir tag shape.
///
/// # Panics
///
/// Never: the built strings always carry one `{version}` placeholder and
/// no other braces, so [`TagTemplate::parse`] cannot fail. The guarantee
/// is pinned by the crate's contract tests.
#[must_use]
pub fn template_for(root: &str) -> TagTemplate {
    if root.is_empty() {
        return TagTemplate::parse("v{version}").unwrap_or_else(|_| unreachable_template());
    }
    let raw = format!("{root}/v{{version}}");
    TagTemplate::parse(&raw).unwrap_or_else(|_| unreachable_template())
}

/// The construction guarantee is total; this is the documented fallaway
/// if the guarantee were ever broken (it is pinned by tests).
fn unreachable_template() -> TagTemplate {
    TagTemplate::parse("v{version}").expect("the root fallback is a valid template")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suffixes_follow_the_path_rules() {
        assert_eq!(major_suffix("example.com/m/v2"), Some(2));
        assert_eq!(major_suffix("example.com/m/v10"), Some(10));
        assert_eq!(major_suffix("example.com/m"), None);
        assert_eq!(major_suffix("example.com/m/v1"), None);
        assert_eq!(major_suffix("example.com/m/v0"), None);
        assert_eq!(major_suffix("example.com/m/v02"), None);
        assert_eq!(major_suffix("example.com/m/v2.1"), None);
        assert_eq!(
            major_suffix("example.com/m/v2/x"),
            None,
            "final element only"
        );
        assert_eq!(major_suffix("/v2"), None);
        assert_eq!(major_suffix("example.com//v2"), None);
    }

    #[test]
    fn templates_render_both_shapes() {
        let root = template_for("");
        assert_eq!(
            root.render("example.com/m", &"1.2.3".parse().unwrap(), "go"),
            "v1.2.3"
        );
        let sub = template_for("libs/api");
        assert_eq!(
            sub.render("example.com/m/libs/api", &"2.0.0".parse().unwrap(), "go"),
            "libs/api/v2.0.0"
        );
    }
}

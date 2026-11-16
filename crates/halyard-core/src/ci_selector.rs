//! The CI-selector adapters (§16.3/M10 step 2, ADR-0034): the affected
//! selection rendered in a CI system's **own** grammar — the
//! machine-consumer surface beside the human/json/gha renderings (the
//! CLI's `affected --ci turbo|nx|bazel`).
//!
//! Each adapter is a pure projection over two name-sorted inputs (the
//! change roots and the closed selection, both as [`SelectedPackage`])
//! into the argv atoms or the one expression the CI system consumes —
//! never a guess: the grammars are VERIFIED per §0.4, fetched quotes in
//! `docs/research/ci-selector-grammars.md`:
//!
//! - **Turborepo** (turborepo.dev, "Running tasks"): `--filter=...pkg`
//!   runs "the package and its dependents" (the dependents close on
//!   turbo's side — the adapter emits one filter per change root), and
//!   "Multiple filters are combined as a union".
//! - **Nx** (nx.dev, "Nx CLI Command Reference"): `run-many --projects`
//!   is "comma/space delimited project names and/or patterns" — the
//!   affected closure rides the halyard selection; dependents-first
//!   scheduling is the tool's own default.
//! - **Bazel** (bazel.build, "Query language"): `rdeps(u, x)`
//!   "evaluates to the reverse dependencies of the argument set x
//!   within the transitive closure of the universe set u"; the argument
//!   set is the changed packages' recursive target patterns
//!   (`//pkg/root/...` — "all rules in every package recursively
//!   beneath the foo directory"), unioned with the documented `+`
//!   alias.
//!
//! An empty selection renders nothing — the honest no-op (the CI step
//! skips itself; the CLI prints nothing, exit 0, the existing
//! empty-selection contract). The surface is closed: an unknown system
//! has no adapter, and extending one is an ADR (the §16.2 convention).

/// One selected package as the adapters read it: the workspace package
/// name and its repo-relative root (the tool filters need the name;
/// the Bazel patterns need the root — one seam carries both).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectedPackage {
    /// The workspace package name (`halyard graph`'s node label).
    pub name: String,
    /// The package root directory, repo-relative (the Bazel package
    /// path).
    pub root: String,
}

/// The CI-selector adapter seam (M10 step 2, ADR-0034): renders the
/// affected selection in one CI system's own grammar. The inputs are
/// the same two lists the CLI resolves from a selection — the change
/// roots and the full affected closure, both name-sorted (deterministic,
/// I3) — and the output is the argv atoms / one expression the CI
/// system consumes, one line per atom.
pub trait CiSelectorAdapter: Send + Sync {
    /// The CI system this adapter speaks (`turbo`, `nx`, `bazel`).
    fn system(&self) -> &'static str;

    /// Renders the selection: `changed` are the packages with pending
    /// commits (the change roots), `affected` the full closed selection
    /// (roots included). Each returned line is one argv atom (I7) the
    /// consumer appends to its own invocation.
    fn render(&self, changed: &[SelectedPackage], affected: &[SelectedPackage]) -> Vec<String>;
}

/// The Turborepo adapter: one `--filter=...{name}` atom per change
/// root — the `...` prefix is the documented dependent filter, and the
/// per-root filters union on turbo's side, so the emitted set is the
/// exact dependent closure without re-encoding halyard's.
pub struct TurboSelector;

impl CiSelectorAdapter for TurboSelector {
    fn system(&self) -> &'static str {
        "turbo"
    }

    fn render(&self, changed: &[SelectedPackage], _affected: &[SelectedPackage]) -> Vec<String> {
        changed
            .iter()
            .map(|package| format!("--filter=...{}", package.name))
            .collect()
    }
}

/// The Nx adapter: one `--projects=` flag over the full affected
/// closure, comma-joined name-sorted — the list form of the documented
/// "comma/space delimited project names" grammar.
pub struct NxSelector;

impl CiSelectorAdapter for NxSelector {
    fn system(&self) -> &'static str {
        "nx"
    }

    fn render(&self, _changed: &[SelectedPackage], affected: &[SelectedPackage]) -> Vec<String> {
        if affected.is_empty() {
            return Vec::new();
        }
        let projects = affected
            .iter()
            .map(|package| package.name.as_str())
            .collect::<Vec<&str>>()
            .join(",");
        vec![format!("--projects={projects}")]
    }
}

/// The Bazel adapter: one `rdeps(//..., <patterns>)` query over the
/// changed packages' recursive target patterns, unioned with the `+`
/// alias — the documented reverse-dependency operator, the universe
/// being the whole repo (`//...`).
pub struct BazelSelector;

impl CiSelectorAdapter for BazelSelector {
    fn system(&self) -> &'static str {
        "bazel"
    }

    fn render(&self, changed: &[SelectedPackage], _affected: &[SelectedPackage]) -> Vec<String> {
        if changed.is_empty() {
            return Vec::new();
        }
        let patterns = changed
            .iter()
            .map(|package| format!("//{}/...", package.root))
            .collect::<Vec<String>>()
            .join(" + ");
        vec![format!("rdeps(//..., {patterns})")]
    }
}

/// The closed adapter lookup: `turbo`, `nx`, `bazel` — anything else is
/// `None` (an unknown system is a caller bug the CLI's value enum
/// refuses before this point).
#[must_use]
pub fn ci_selector_adapter(system: &str) -> Option<&'static dyn CiSelectorAdapter> {
    match system {
        "turbo" => Some(&TurboSelector),
        "nx" => Some(&NxSelector),
        "bazel" => Some(&BazelSelector),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_selection_is_plain_data() {
        // The seam's input is plain data (§5.8): name + root, both
        // owned, so adapters stay pure and testable without a
        // workspace.
        let package = SelectedPackage {
            name: "lib".to_owned(),
            root: "crates/lib".to_owned(),
        };
        assert_eq!(package.name, "lib");
        assert_eq!(package.root, "crates/lib");
    }
}

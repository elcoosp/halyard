//! The CI-selector adapters (§16.3/M10 step 2, ADR-0034): the affected
//! selection rendered in a CI system's **own** grammar — the
//! machine-consumer surface beside the human/json/gha renderings (the
//! CLI's `affected --ci
//! turbo|nx|bazel|gha|gitlab|azure|teamcity`).
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
//! - **GitHub Actions** (docs.github.com, "Running variations of jobs
//!   in a workflow"): "You can use the output from one job to define
//!   matrices for multiple jobs" — the documented output value is the
//!   JSON array (`colors=["red", "green", "blue"]` written to
//!   `$GITHUB_OUTPUT`, fed to `fromJSON(needs.…outputs.…)` inside the
//!   downstream `strategy.matrix`). Actions computes no dependency
//!   closure, so the array encodes the full affected closure; the
//!   consumer names the matrix dimension on its side.
//! - **GitLab** (docs.gitlab.com/ci/yaml, "parallel:matrix"): "Use
//!   parallel:matrix to run a job multiple times in parallel in a
//!   single pipeline, but with different variable values for each
//!   instance of the job" — "Supported values: An array of hashes of
//!   variables … The matrix identifiers, which become the variable
//!   names, can use only numbers, letters, and underscores". One
//!   `- HALYARD_PKG: <name>` hash per affected package — the
//!   halyard-owned identifier (namespaced so it cannot collide with
//!   the job's own variables) the consumer reads as `$HALYARD_PKG` in
//!   the script.
//! - **Azure Pipelines** (learn.microsoft.com, "Logging commands"):
//!   `##vso[task.setvariable variable=…;isOutput=true]…` — "Sets a
//!   variable in the variable service of taskcontext. The first task
//!   can set a variable, and following tasks are able to use the
//!   variable" — one line whose value is the affected closure,
//!   name-sorted and comma-joined: the documented consumption of a
//!   comma-joined variable is the `split(variables.environments, ',')`
//!   example iterated with the documented `${{ each }}` keyword, and
//!   the cross-job reference is
//!   `dependencies.<job-name>.outputs['<step-name>.<variable-name>']`.
//!   Pipelines computes no dependency closure, so the variable encodes
//!   the full affected closure; `HALYARD_PKGS` is the halyard-owned
//!   identifier (namespaced so it cannot collide with the pipeline's
//!   own variables).
//!
//! - **`TeamCity`** (jetbrains.com/help/teamcity, "Matrix Build"): the
//!   build feature "enables you to define a collection of builds by
//!   iterating over specified parameter values and generating a build
//!   for every combination" — the documented Kotlin DSL entry is
//!   `param("Browser", listOf(value("Chrome"), value("Firefox")))` and
//!   the documented sample iteration set carries an `env.*` parameter
//!   (`env.ShouldFail: true, false`). One
//!   `param("env.HALYARD_PKG", listOf(value(…), …))` line over the
//!   affected closure — the consumer pastes it inside its `matrix { }`
//!   block and each instance reads `$HALYARD_PKG` in the script
//!   (`TeamCity` computes no dependency closure, so the parameter
//!   encodes the full affected closure; `HALYARD_PKG` is the
//!   halyard-owned identifier, the ADR-0035 policy carried over).
//!
//! - **`CircleCI`** (circleci.com/docs, "Using dynamic configuration"):
//!   the continuation orb's `continue` command takes a `parameters`
//!   value — "The `parameters` parameter accepts a JSON object as a
//!   string. Note the single quote encapsulating the full JSON object"
//!   (the documented shape is `parameters: '{ "further": true }'`).
//!   One paste-ready `parameters: '{ "halyard_pkgs": [...] }'` line
//!   over the affected closure, name-sorted — the consumer pastes it
//!   into the setup workflow's `continuation/continue` step and
//!   declares the matching pipeline parameter in the continuation
//!   config (`circleci` computes no dependency closure, so the array
//!   encodes the full affected closure; `halyard_pkgs` is the
//!   halyard-owned identifier in the docs' own lower-snake
//!   pipeline-parameter spelling, so it cannot collide with the
//!   pipeline's own parameters).
//!
//!   The single-quoted JSON literal never needs escaping: neither
//!   package-name grammar the selection carries (cargo's
//!   `[A-Za-z0-9_-]`, npm's URL-safe set) contains a quote, so the
//!   values are emitted verbatim.
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
    /// The CI system this adapter speaks — one of the lookup's keys
    /// (`turbo`, `nx`, `bazel`, `gha`, `gitlab`, `azure`, `teamcity`;
    /// the closed surface [`ci_selector_adapter`] names).
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

/// The GitHub Actions adapter: one compact JSON array over the
/// affected closure, name-sorted — the documented matrix-output value
/// ("You can use the output from one job to define matrices for
/// multiple jobs"; the `colors=["red", "green", "blue"]` output fed to
/// `fromJSON(...)` inside the downstream `strategy.matrix`). Actions
/// computes no dependency closure of its own, so the array encodes the
/// full affected selection, and the consumer names the matrix
/// dimension on its side — no key policy is encoded here.
pub struct GhaSelector;

impl CiSelectorAdapter for GhaSelector {
    fn system(&self) -> &'static str {
        "gha"
    }

    fn render(&self, _changed: &[SelectedPackage], affected: &[SelectedPackage]) -> Vec<String> {
        if affected.is_empty() {
            return Vec::new();
        }
        let names = affected
            .iter()
            .map(|package| package.name.as_str())
            .collect::<Vec<&str>>();
        let array = serde_json::to_string(&names)
            .unwrap_or_else(|_| unreachable!("a string array serializes without error"));
        vec![array]
    }
}

/// The GitLab adapter: one `- HALYARD_PKG: <name>` hash per affected
/// package, name-sorted — the `parallel:matrix` "array of hashes of
/// variables" ("each instance of the job" runs "with different
/// variable values"). `HALYARD_PKG` is the halyard-owned matrix
/// identifier (identifiers "can use only numbers, letters, and
/// underscores"; the namespaced spelling cannot collide with the
/// job's own variables), and the consumer pastes the lines under
/// `parallel: matrix:` and reads `$HALYARD_PKG` in the script.
pub struct GitlabSelector;

impl CiSelectorAdapter for GitlabSelector {
    fn system(&self) -> &'static str {
        "gitlab"
    }

    fn render(&self, _changed: &[SelectedPackage], affected: &[SelectedPackage]) -> Vec<String> {
        affected
            .iter()
            .map(|package| format!("- HALYARD_PKG: {}", package.name))
            .collect()
    }
}

/// The Azure Pipelines adapter: one `##vso[task.setvariable
/// variable=HALYARD_PKGS;isOutput=true]` logging command over the
/// affected closure, name-sorted and comma-joined — the documented
/// variable-setting command ("Sets a variable in the variable service
/// of taskcontext. The first task can set a variable, and following
/// tasks are able to use the variable"). Pipelines computes no
/// dependency closure of its own, so the variable encodes the **full
/// affected closure**; the consumer references it across jobs as
/// `dependencies.<job-name>.outputs['<step-name>.<variable-name>']`
/// and iterates with the documented `split(…, ',')` + `${{ each }}`
/// shape. `HALYARD_PKGS` is the halyard-owned identifier (namespaced
/// so it cannot collide with the pipeline's own variables).
///
/// The command's escape sequences (`;`, `\n`, `\r`, `]`, `%`) never
/// fire: both package-name grammars the selection carries (cargo's
/// `[A-Za-z0-9_-]`, npm's URL-safe set) exclude every escapable
/// character, so the value is emitted verbatim.
pub struct AzureSelector;

impl CiSelectorAdapter for AzureSelector {
    fn system(&self) -> &'static str {
        "azure"
    }

    fn render(&self, _changed: &[SelectedPackage], affected: &[SelectedPackage]) -> Vec<String> {
        if affected.is_empty() {
            return Vec::new();
        }
        let names = affected
            .iter()
            .map(|package| package.name.as_str())
            .collect::<Vec<&str>>()
            .join(",");
        vec![format!(
            "##vso[task.setvariable variable=HALYARD_PKGS;isOutput=true]{names}"
        )]
    }
}

/// The `TeamCity` adapter: one `param("env.HALYARD_PKG", listOf(value(
/// …), …))` line over the affected closure, name-sorted — the
/// documented Matrix Build feature entry ("define a collection of
/// builds by iterating over specified parameter values and generating
/// a build for every combination"; the DSL block is
/// `param("Browser", listOf(value("Chrome"), value("Firefox")))`, and
/// the documented sample iteration set carries an `env.*` parameter).
/// `TeamCity` computes no dependency closure of its own, so the
/// parameter encodes the **full affected closure**; the consumer
/// pastes the line inside its `matrix { }` block and each matrix
/// instance reads `$HALYARD_PKG` in the script — the ADR-0035
/// identifier policy carried over.
///
/// The Kotlin string literal never needs escaping: neither package-name
/// grammar the selection carries (cargo's `[A-Za-z0-9_-]`, npm's
/// URL-safe set) contains a quote, so the values are emitted verbatim.
pub struct TeamcitySelector;

impl CiSelectorAdapter for TeamcitySelector {
    fn system(&self) -> &'static str {
        "teamcity"
    }

    fn render(&self, _changed: &[SelectedPackage], affected: &[SelectedPackage]) -> Vec<String> {
        if affected.is_empty() {
            return Vec::new();
        }
        let values = affected
            .iter()
            .map(|package| format!(r#"value("{}")"#, package.name))
            .collect::<Vec<String>>()
            .join(", ");
        vec![format!(r#"param("env.HALYARD_PKG", listOf({values}))"#)]
    }
}

/// The `CircleCI` adapter: one paste-ready
/// `parameters: '{ "halyard_pkgs": [...] }'` line over the affected
/// closure, name-sorted — the documented dynamic-configuration shape
/// (circleci.com/docs, VERIFIED 2026-11-17: "The `parameters`
/// parameter accepts a JSON object as a string. Note the single quote
/// encapsulating the full JSON object"; the documented continuation
/// step is `continuation/continue` with `parameters` +
/// `configuration_path`). `CircleCI` computes no dependency closure of
/// its own, so the array encodes the **full affected closure**; the
/// consumer pastes the line into the setup workflow's
/// `continuation/continue` step and declares the matching pipeline
/// parameter in the continuation config — `halyard_pkgs` is the
/// halyard-owned identifier in the docs' own lower-snake
/// pipeline-parameter spelling (the ADR-0035 policy carried over, the
/// parameter grammar's casing).
///
/// The single-quoted JSON literal never needs escaping: neither
/// package-name grammar the selection carries (cargo's
/// `[A-Za-z0-9_-]`, npm's URL-safe set) contains a quote, so the
/// values are emitted verbatim.
pub struct CircleciSelector;

impl CiSelectorAdapter for CircleciSelector {
    fn system(&self) -> &'static str {
        "circleci"
    }

    fn render(&self, _changed: &[SelectedPackage], affected: &[SelectedPackage]) -> Vec<String> {
        if affected.is_empty() {
            return Vec::new();
        }
        let names = affected
            .iter()
            .map(|package| format!(r#""{}""#, package.name))
            .collect::<Vec<String>>()
            .join(", ");
        vec![format!(r#"parameters: '{{ "halyard_pkgs": [{names}] }}'"#)]
    }
}

/// The closed adapter lookup: `turbo`, `nx`, `bazel`, `gha`, `gitlab`,
/// `azure`, `teamcity`, `circleci` — anything else is `None` (an
/// unknown system is a caller bug the CLI's value enum refuses before
/// this point).
#[must_use]
pub fn ci_selector_adapter(system: &str) -> Option<&'static dyn CiSelectorAdapter> {
    match system {
        "turbo" => Some(&TurboSelector),
        "nx" => Some(&NxSelector),
        "bazel" => Some(&BazelSelector),
        "gha" => Some(&GhaSelector),
        "gitlab" => Some(&GitlabSelector),
        "azure" => Some(&AzureSelector),
        "teamcity" => Some(&TeamcitySelector),
        "circleci" => Some(&CircleciSelector),
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

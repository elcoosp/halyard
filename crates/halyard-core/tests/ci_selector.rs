//! The CI-selector adapters (§16.3/M10 step 2, ADR-0034 + the ADR-0035
//! matrix adapters): the affected selection rendered in the CI systems'
//! own grammars — the `--filter` dependent filters (Turborepo), the
//! `run-many --projects` list (Nx), the `rdeps` query (Bazel), the
//! matrix-output JSON array (GitHub Actions), the `parallel:matrix`
//! hashes (GitLab) and the `task.setvariable` logging command (Azure
//! Pipelines). Grammars VERIFIED per §0.4, fetched quotes in
//! `docs/research/ci-selector-grammars.md`. Written first (red on the
//! unresolved `halyard_core` API), implemented second.

use halyard_core::ci_selector::{CiSelectorAdapter, SelectedPackage, ci_selector_adapter};

/// One selected package: the workspace name plus the repo-relative
/// root (the Bazel patterns need the root; the tool filters need the
/// name — one seam carries both).
fn pkg(name: &str, root: &str) -> SelectedPackage {
    SelectedPackage {
        name: name.to_owned(),
        root: root.to_owned(),
    }
}

/// The `lib ← app` fixture shape, name-sorted (`app` first).
fn lib_app() -> (Vec<SelectedPackage>, Vec<SelectedPackage>) {
    (
        vec![pkg("lib", "crates/lib")],
        vec![pkg("app", "crates/app"), pkg("lib", "crates/lib")],
    )
}

#[test]
fn turbo_renders_one_dependent_filter_per_changed_root() {
    // turborepo.dev (VERIFIED): "When you're working on a specific
    // package, you might want to run tasks for the package and its
    // dependents" — `--filter=...lib` closes over the dependents on
    // turbo's side, so the adapter emits one filter per change root.
    let (changed, affected) = lib_app();
    let adapter = ci_selector_adapter("turbo").expect("turbo is a known adapter");
    let rendered = adapter.render(&changed, &affected);
    assert_eq!(rendered, vec!["--filter=...lib".to_owned()]);
}

#[test]
fn turbo_filters_are_argv_atoms_in_input_order() {
    // Multiple filters union on turbo's side (VERIFIED: "Multiple
    // filters are combined as a union"); the atoms keep the caller's
    // name-sorted order so the CI command line is deterministic (I3).
    let changed = vec![pkg("app", "crates/app"), pkg("lib", "crates/lib")];
    let affected = vec![pkg("app", "crates/app"), pkg("lib", "crates/lib")];
    let adapter = ci_selector_adapter("turbo").expect("turbo is a known adapter");
    let rendered = adapter.render(&changed, &affected);
    assert_eq!(
        rendered,
        vec!["--filter=...app".to_owned(), "--filter=...lib".to_owned()]
    );
}

#[test]
fn nx_renders_one_projects_flag_over_the_affected_closure() {
    // nx.dev (VERIFIED): run-many's `--projects` is "comma/space
    // delimited project names and/or patterns" — the affected closure
    // rides the halyard selection (dependents-first scheduling is the
    // tool's own default), so the adapter emits one flag, name-sorted.
    let (changed, affected) = lib_app();
    let adapter = ci_selector_adapter("nx").expect("nx is a known adapter");
    let rendered = adapter.render(&changed, &affected);
    assert_eq!(rendered, vec!["--projects=app,lib".to_owned()]);
}

#[test]
fn bazel_renders_the_rdeps_query_over_the_changed_patterns() {
    // bazel.build (VERIFIED): `rdeps(u, x)` "evaluates to the reverse
    // dependencies of the argument set x within the transitive closure
    // of the universe set u"; the argument set is the changed packages'
    // recursive target patterns (`//crates/lib/...` — the pattern that
    // matches "all rules in every package recursively beneath the foo
    // directory"), unioned with the `+` alias.
    let (changed, affected) = lib_app();
    let adapter = ci_selector_adapter("bazel").expect("bazel is a known adapter");
    let rendered = adapter.render(&changed, &affected);
    assert_eq!(rendered, vec!["rdeps(//..., //crates/lib/...)".to_owned()]);
}

#[test]
fn bazel_unions_multiple_changed_roots() {
    let changed = vec![pkg("app", "crates/app"), pkg("lib", "crates/lib")];
    let affected = changed.clone();
    let adapter = ci_selector_adapter("bazel").expect("bazel is a known adapter");
    let rendered = adapter.render(&changed, &affected);
    assert_eq!(
        rendered,
        vec!["rdeps(//..., //crates/app/... + //crates/lib/...)".to_owned()]
    );
}

#[test]
fn an_empty_selection_renders_nothing() {
    // An honest no-op: no affected packages ⇒ no CI expression — the
    // CI step skips itself (the CLI prints nothing, exit 0, the
    // existing empty-selection contract).
    for system in ["turbo", "nx", "bazel", "gha", "gitlab", "azure", "teamcity"] {
        let adapter = ci_selector_adapter(system).expect("known adapter");
        assert!(
            adapter.render(&[], &[]).is_empty(),
            "{system} renders nothing for an empty selection"
        );
    }
}

#[test]
fn gha_renders_the_json_array_over_the_affected_closure() {
    // docs.github.com (VERIFIED): "You can use the output from one job
    // to define matrices for multiple jobs" — the documented output
    // value is the JSON array (`colors=["red", "green", "blue"]`) the
    // downstream job feeds to `fromJSON(needs.…outputs.…)` inside its
    // own `strategy.matrix`. Actions computes no dependency closure, so
    // the array encodes the full affected closure, name-sorted.
    let (changed, affected) = lib_app();
    let adapter = ci_selector_adapter("gha").expect("gha is a known adapter");
    let rendered = adapter.render(&changed, &affected);
    assert_eq!(rendered, vec![r#"["app","lib"]"#.to_owned()]);
}

#[test]
fn gha_renders_one_line_for_many_packages() {
    // One compact JSON array per render (the documented output value
    // shape); the consumer names the matrix dimension on its side, so
    // no key policy is encoded here.
    let changed = vec![pkg("lib", "crates/lib")];
    let affected = vec![
        pkg("app", "crates/app"),
        pkg("cli", "crates/cli"),
        pkg("lib", "crates/lib"),
    ];
    let adapter = ci_selector_adapter("gha").expect("gha is a known adapter");
    let rendered = adapter.render(&changed, &affected);
    assert_eq!(rendered, vec![r#"["app","cli","lib"]"#.to_owned()]);
}

#[test]
fn gitlab_renders_one_matrix_hash_per_affected_package() {
    // docs.gitlab.com/ci/yaml (VERIFIED): "Use parallel:matrix to run a
    // job multiple times in parallel in a single pipeline, but with
    // different variable values for each instance of the job" —
    // "Supported values: An array of hashes of variables ... The
    // matrix identifiers, which become the variable names, can use
    // only numbers, letters, and underscores". One hash per affected
    // package; `HALYARD_PKG` is the halyard-owned identifier (the
    // namespaced spelling the consumer reads as `$HALYARD_PKG` in the
    // script).
    let (changed, affected) = lib_app();
    let adapter = ci_selector_adapter("gitlab").expect("gitlab is a known adapter");
    let rendered = adapter.render(&changed, &affected);
    assert_eq!(
        rendered,
        vec![
            "- HALYARD_PKG: app".to_owned(),
            "- HALYARD_PKG: lib".to_owned()
        ]
    );
}

#[test]
fn azure_renders_the_setvariable_line_over_the_affected_closure() {
    // learn.microsoft.com (VERIFIED): the logging command is
    // `##vso[task.setvariable variable=shouldrun;isOutput=true]true` —
    // "Sets a variable in the variable service of taskcontext. The
    // first task can set a variable, and following tasks are able to
    // use the variable." The documented job-to-job reference is
    // `dependencies.<job-name>.outputs['<step-name>.<variable-name>']`,
    // and the documented consumption of a comma-joined variable is the
    // `split(variables.environments, ',')` + `${{ each }}` example —
    // so one `HALYARD_PKGS` output variable carries the affected
    // closure, name-sorted, comma-joined.
    let (changed, affected) = lib_app();
    let adapter = ci_selector_adapter("azure").expect("azure is a known adapter");
    let rendered = adapter.render(&changed, &affected);
    assert_eq!(
        rendered,
        vec!["##vso[task.setvariable variable=HALYARD_PKGS;isOutput=true]app,lib".to_owned()]
    );
}

#[test]
fn azure_renders_one_line_for_many_packages() {
    // One logging-command line per render (the documented setvariable
    // shape); the consumer names the job/step references on its side
    // and splits on the documented comma delimiter.
    let changed = vec![pkg("lib", "crates/lib")];
    let affected = vec![
        pkg("app", "crates/app"),
        pkg("cli", "crates/cli"),
        pkg("lib", "crates/lib"),
    ];
    let adapter = ci_selector_adapter("azure").expect("azure is a known adapter");
    let rendered = adapter.render(&changed, &affected);
    assert_eq!(
        rendered,
        vec!["##vso[task.setvariable variable=HALYARD_PKGS;isOutput=true]app,cli,lib".to_owned()]
    );
}

#[test]
fn teamcity_renders_the_matrix_param_line_over_the_affected_closure() {
    // jetbrains.com/help/teamcity (VERIFIED): the Matrix Build feature
    // "enables you to define a collection of builds by iterating over
    // specified parameter values and generating a build for every
    // combination" — the documented Kotlin DSL block is
    // `param("Browser", listOf(value("Chrome"), value("Firefox")))`,
    // and the documented sample iteration set includes an `env.*`
    // parameter (`env.ShouldFail: true, false`). One
    // `param("env.HALYARD_PKG", listOf(value(…), …))` line over the
    // affected closure — the consumer pastes it inside its `matrix { }`
    // block and each instance reads `%env.HALYARD_PKG%` / `$HALYARD_PKG`.
    let (changed, affected) = lib_app();
    let adapter = ci_selector_adapter("teamcity").expect("teamcity is a known adapter");
    let rendered = adapter.render(&changed, &affected);
    assert_eq!(
        rendered,
        vec![r#"param("env.HALYARD_PKG", listOf(value("app"), value("lib")))"#.to_owned()]
    );
}

#[test]
fn teamcity_renders_one_param_line_for_many_packages() {
    // One `param(...)` DSL line per render (the documented matrix
    // block entry); values are "a single line of text" — the package
    // names ride verbatim (the Kotlin string literal never needs
    // escaping: neither package-name grammar carries a quote).
    let changed = vec![pkg("lib", "crates/lib")];
    let affected = vec![
        pkg("app", "crates/app"),
        pkg("cli", "crates/cli"),
        pkg("lib", "crates/lib"),
    ];
    let adapter = ci_selector_adapter("teamcity").expect("teamcity is a known adapter");
    let rendered = adapter.render(&changed, &affected);
    assert_eq!(
        rendered,
        vec![
            r#"param("env.HALYARD_PKG", listOf(value("app"), value("cli"), value("lib")))"#
                .to_owned()
        ]
    );
}

#[test]
fn the_adapter_lookup_names_seven_systems_and_refuses_the_rest() {
    let systems: Vec<&'static str> = ["turbo", "nx", "bazel", "gha", "gitlab", "azure", "teamcity"]
        .iter()
        .filter_map(|system| ci_selector_adapter(system).map(CiSelectorAdapter::system))
        .collect();
    assert_eq!(
        systems,
        vec!["turbo", "nx", "bazel", "gha", "gitlab", "azure", "teamcity"]
    );
    assert!(
        ci_selector_adapter("circleci").is_none(),
        "an unknown system is None — the adapter surface is closed, extending one is an ADR"
    );
}

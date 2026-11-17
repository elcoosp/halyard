//! The CI-selector adapters (M10 step 2, ADR-0034; the matrix-jobs
//! adapters per ADR-0035, the azure/teamcity adapters per ADR-0036,
//! the circleci continuation adapter per ADR-0038, the buildkite
//! matrix adapter per ADR-0039, the jenkins axis adapter per
//! ADR-0040, the bitbucket parallel-steps adapter per ADR-0041, the
//! drone environment adapter per ADR-0045): the affected selection
//! rendered in the twelve CI systems' own grammars —
//! offline, over a hand-built `lib ← app` workspace (no git, no
//! network). Each adapter's output is the exact argv/one-line
//! expression the CI step consumes; an empty selection renders
//! nothing (the honest no-op).
//!
//! Run: `cargo run -p halyard-core --example affected_ci`

use halyard_core::ci_selector::{SelectedPackage, ci_selector_adapter};

fn main() {
    // The `lib ← app` shape: `lib` changed, `app` its dependent — the
    // selection the `affected` command computes over the real walk.
    let changed = vec![SelectedPackage {
        name: "lib".to_owned(),
        root: "crates/lib".to_owned(),
    }];
    let affected = vec![
        SelectedPackage {
            name: "app".to_owned(),
            root: "crates/app".to_owned(),
        },
        SelectedPackage {
            name: "lib".to_owned(),
            root: "crates/lib".to_owned(),
        },
    ];

    for system in [
        "turbo",
        "nx",
        "bazel",
        "gha",
        "gitlab",
        "azure",
        "teamcity",
        "circleci",
        "buildkite",
        "jenkins",
        "bitbucket",
        "drone",
    ] {
        if let Some(adapter) = ci_selector_adapter(system) {
            println!(
                "{} → {}",
                adapter.system(),
                adapter.render(&changed, &affected).join(" ")
            );
        }
    }

    // The honest no-op: nothing affected ⇒ nothing to run.
    let empty_render =
        ci_selector_adapter("turbo").is_none_or(|adapter| adapter.render(&[], &[]).is_empty());
    println!(
        "empty selection → renders nothing: {empty_render} (no output, exit 0 — the CI step skips itself)"
    );
}

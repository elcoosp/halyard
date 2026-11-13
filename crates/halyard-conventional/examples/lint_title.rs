//! PR-title lint demo (§11.6, M4.3 `check`): lints a handful of
//! real-shaped squash-merge PR titles against the §10.1 default mapping
//! and prints the verdict, the conveyed bump and any did-you-mean hint —
//! the exact verdicts `halyard check --pr-title` renders.
//!
//! Run: `cargo run -p halyard-conventional --example lint_title`

use halyard_conventional::{Level, TitleBumpRule, TitleVerdict, lint_title};

/// The §10.1 default mapping as a rule (the shape the CLI implements
/// over `PlanConfig::commit_types`).
struct Defaults;

impl TitleBumpRule for Defaults {
    fn bump_for_kind(&self, kind: &str) -> Level {
        match kind {
            "feat" => Level::Minor,
            "fix" | "perf" => Level::Patch,
            _ => Level::None,
        }
    }

    fn known_kinds(&self) -> Vec<String> {
        [
            "feat", "fix", "perf", "chore", "docs", "test", "refactor", "ci",
        ]
        .iter()
        .map(|k| (*k).to_owned())
        .collect()
    }
}

const TITLES: &[&str] = &[
    "feat(parser): accept spans",
    "feat(api)!: send an email to the customer when a product ships",
    "fix: correct minor typo (#412)",
    "chore: tidy the bench fixtures",
    "feet: adjust the parser fixtures",
    "fixup! feat: something earlier",
    "just a plain PR title without the conventional grammar",
];

fn main() {
    for title in TITLES {
        println!("title: {title:?}");
        match lint_title(title, &Defaults) {
            TitleVerdict::Accepted(lint) => {
                println!(
                    "  -> accepted: kind {} (scope {:?}), breaking {}, conveys {}",
                    lint.kind,
                    lint.scope.as_deref(),
                    lint.breaking,
                    lint.level
                );
                if let Some(closest) = &lint.closest_known {
                    println!("  -> hint: unmapped kind, did you mean {closest:?}?");
                }
            }
            TitleVerdict::NotConventional { reason } => {
                println!("  -> rejected: {reason}");
            }
            TitleVerdict::FixupOrSquash => {
                println!("  -> autosquash subject (fixup!/squash!), never released");
            }
        }
    }
}

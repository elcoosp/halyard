//! The `check` command's pure logic (§11.6): the shell's [`TitleBumpRule`]
//! over `PlanConfig::commit_types` and the per-package bump prediction for
//! squash-merge repositories.
//!
//! The prediction mirrors the planner exactly: the title's kind is looked
//! up as written, breaking titles are major (§10.1's fixed rule — the
//! engine applies it), and the conveyed bump is remapped through
//! [`halyard_core::bump::PreMajorPolicy`] per package for `0.y.z` versions (§10.2 — the same
//! `apply_bump` path `planner.rs` uses).

use halyard_conventional::{Level, TitleBumpRule, TitleLint, TitleVerdict, lint_title};
use halyard_core::Plan;
use halyard_core::plan::Bump;
use halyard_core::planner::PlanConfig;

/// The shell's policy adapter: `PlanConfig::commit_types` as a
/// [`TitleBumpRule`]. The lookup is exactly the planner's (`planner.rs`:
/// kind as written, unmapped → no bump), so a title lint can never
/// predict a bump the planner would not produce.
pub(crate) struct PlanTitleRule<'a> {
    config: &'a PlanConfig,
}

impl<'a> PlanTitleRule<'a> {
    /// Binds the resolved plan configuration.
    pub(crate) fn new(config: &'a PlanConfig) -> Self {
        Self { config }
    }
}

impl TitleBumpRule for PlanTitleRule<'_> {
    fn bump_for_kind(&self, kind: &str) -> Level {
        match self.config.commit_types.get(kind) {
            Some(Bump::Major) => Level::Major,
            Some(Bump::Minor) => Level::Minor,
            Some(Bump::Patch) => Level::Patch,
            Some(Bump::None) | None => Level::None,
        }
    }

    fn known_kinds(&self) -> Vec<String> {
        self.config.commit_types.keys().cloned().collect()
    }
}

/// One package's predicted release under a squash-merged title (§11.6's
/// "if merged, this will release" line).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Prediction {
    /// The package name (plan order).
    pub package: String,
    /// The version in the manifest (`PlanRelease::from`).
    pub from: String,
    /// The bump the title conveys for this package (after the §10.2
    /// pre-major remap where the package is `0.y.z`).
    pub bump: Bump,
    /// Whether the §10.2 pre-major policy changed the conveyed bump for
    /// this package (rendered so the prediction teaches its own cause).
    pub remapped: bool,
}

/// Predicts, per pending release, the bump `lint` conveys when the title
/// becomes the squash-merge commit subject (§11.6). Packages without a
/// pending release are untouched by definition (nothing to release).
pub(crate) fn predict(plan: &Plan, lint: &TitleLint, config: &PlanConfig) -> Vec<Prediction> {
    let conveyed = bump_of(lint.level);
    plan.releases
        .iter()
        .map(|release| {
            // §10.2: the planner remaps through PreMajorPolicy for 0.y.z
            // versions (`apply_bump`); versions that do not parse fall
            // through unmapped rather than guessing.
            let pre_major =
                semver::Version::parse(&release.from).is_ok_and(|version| version.major == 0);
            let bump = if pre_major {
                config.pre_major.remap(conveyed)
            } else {
                conveyed
            };
            Prediction {
                package: release.package.clone(),
                from: release.from.clone(),
                bump,
                remapped: pre_major && bump != conveyed,
            }
        })
        .collect()
}

/// Converts the engine's leaf level onto the plan contract's [`Bump`]
/// (the leaf mirrors core's enum without depending on it, ADR-0002).
pub(crate) fn bump_of(level: Level) -> Bump {
    match level {
        Level::None => Bump::None,
        Level::Patch => Bump::Patch,
        Level::Minor => Bump::Minor,
        Level::Major => Bump::Major,
    }
}

/// The lowercase wire name of a plan-contract [`Bump`] (the serde
/// rendering of `halyard-plan/v1`, reused by the human/GHA renderers —
/// `Bump` itself has no `Display`).
pub(crate) fn bump_str(bump: Bump) -> &'static str {
    match bump {
        Bump::None => "none",
        Bump::Patch => "patch",
        Bump::Minor => "minor",
        Bump::Major => "major",
    }
}

/// The accepted-title lint of one title against the resolved config
/// (the engine call with the shell's rule bound).
pub(crate) fn lint(title: &str, config: &PlanConfig) -> TitleVerdict {
    lint_title(title, &PlanTitleRule::new(config))
}

#[cfg(test)]
mod tests {
    use super::{Prediction, bump_of, lint, predict};
    use halyard_conventional::{Level, TitleVerdict};
    use halyard_core::Plan;
    use halyard_core::plan::{Bump, PlanReason, PlanRelease};
    use halyard_core::planner::PlanConfig;

    fn config() -> PlanConfig {
        PlanConfig::default_values()
    }

    /// Extracts the level of an accepted verdict (one-line assertions in
    /// the mapping test).
    fn accepted_level(verdict: &TitleVerdict) -> Level {
        match verdict {
            TitleVerdict::Accepted(lint) => lint.level,
            other => unreachable!("expected accepted, got {other:?}"),
        }
    }

    fn plan_with(packages: &[(&str, &str)]) -> Plan {
        let releases = packages
            .iter()
            .map(|(name, from)| PlanRelease {
                package: (*name).to_owned(),
                ecosystem: "cargo".to_owned(),
                from: (*from).to_owned(),
                to: "9.9.9".to_owned(),
                bump: Bump::Minor,
                tag: format!("{name}-v9.9.9"),
                reasons: vec![PlanReason::Commit {
                    oid: "ab12cd34ef567890ab12cd34ef567890ab12cd34".to_owned(),
                    kind: "feat".to_owned(),
                    breaking: false,
                }],
                changelog: None,
                publish: None,
            })
            .collect();
        Plan {
            schema: "halyard-plan/v1".to_owned(),
            base_branch: "main".to_owned(),
            head: "9f2ca67".to_owned(),
            plan_hash: String::new(),
            releases,
            edits: Vec::new(),
            order: Vec::new(),
            skipped: Vec::new(),
        }
    }

    #[test]
    fn rule_maps_the_default_config_like_the_planner() {
        let config = config();
        assert_eq!(accepted_level(&lint("feat: x", &config)), Level::Minor);
        assert_eq!(accepted_level(&lint("fix: x", &config)), Level::Patch);
        assert_eq!(accepted_level(&lint("chore: x", &config)), Level::None);
        assert_eq!(accepted_level(&lint("feat!: x", &config)), Level::Major);
    }

    #[test]
    fn prediction_remaps_for_pre_major_packages() {
        let config = config();
        let plan = plan_with(&[("p1", "0.4.1"), ("p2", "1.0.3")]);
        match lint("feat: the feature", &config) {
            TitleVerdict::Accepted(lint) => {
                let rows = predict(&plan, &lint, &config);
                assert_eq!(
                    rows,
                    vec![
                        Prediction {
                            package: "p1".to_owned(),
                            from: "0.4.1".to_owned(),
                            // §10.2 cargo-compatible: minor → patch on 0.y.z.
                            bump: Bump::Patch,
                            remapped: true,
                        },
                        Prediction {
                            package: "p2".to_owned(),
                            from: "1.0.3".to_owned(),
                            bump: Bump::Minor,
                            remapped: false,
                        },
                    ]
                );
            }
            other => unreachable!("expected accepted, got {other:?}"),
        }
    }

    #[test]
    fn prediction_of_a_breaking_title_remaps_major_to_the_policy() {
        let config = config();
        let plan = plan_with(&[("p1", "0.4.1")]);
        match lint("feat!: drop the wire format", &config) {
            TitleVerdict::Accepted(lint) => {
                let rows = predict(&plan, &lint, &config);
                let first = rows
                    .first()
                    .unwrap_or_else(|| unreachable!("one prediction row"));
                assert_eq!(first.bump, Bump::Minor, "breaking → minor on 0.y.z");
                assert!(first.remapped);
            }
            other => panic!("expected accepted, got {other:?}"),
        }
    }

    #[test]
    fn bump_of_covers_every_level() {
        assert_eq!(bump_of(Level::None), Bump::None);
        assert_eq!(bump_of(Level::Patch), Bump::Patch);
        assert_eq!(bump_of(Level::Minor), Bump::Minor);
        assert_eq!(bump_of(Level::Major), Bump::Major);
    }
}

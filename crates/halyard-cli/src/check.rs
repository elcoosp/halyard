//! The `check` command's pure logic (§11.6): the shell's [`TitleBumpRule`]
//! over `PlanConfig::commit_types`, the per-package bump prediction for
//! squash-merge repositories, and the commits lint (§16.2's "lint PR
//! title/commits" second half — the same machinery over the pending
//! history's own subjects; autosquash subjects never reach it, §8.5
//! drops them at the parser).
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

/// The title-rejection shapes the `E-CLI-TITLE` diagnosis and the §11.6
/// check comment share — single-sourced wording, so the job log and the
/// posted comment can never disagree about why a title was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TitleRejection {
    /// The title is not a conventional commit at all (`reason` is the
    /// engine's grammar refusal).
    NotConventional {
        /// The engine's grammar refusal text.
        reason: String,
    },
    /// The title is a `fixup!`/`squash!` autosquash subject (§8.5 —
    /// never released).
    FixupOrSquash,
    /// The title's type is not a known release type and a known type is
    /// within edit distance 2 (a likely typo — merging it would convey
    /// no bump).
    UnknownKind {
        /// The type as written.
        kind: String,
        /// The closest known type (the did-you-mean).
        closest: String,
    },
}

/// The `E-CLI-TITLE` diagnosis text for `rejection` (the error message
/// and the §11.6 comment's rejected-verdict reason — one source).
pub(crate) fn rejection_message(
    title: &str,
    rejection: &TitleRejection,
    config: &PlanConfig,
) -> String {
    match rejection {
        TitleRejection::NotConventional { reason } => format!(
            "PR title {title:?} is not a conventional commit: {reason}. Fix: squash-merge \
             repos derive the bump from the title (§11.6); use `type(scope): description` \
             (e.g. `feat(parser): accept spans`)"
        ),
        TitleRejection::FixupOrSquash => format!(
            "PR title {title:?} is a fixup!/squash! subject — autosquash commits are never \
             released (§8.5). Fix: reword the merge title to a conventional commit"
        ),
        TitleRejection::UnknownKind { kind, closest } => format!(
            "PR title type {kind:?} is not a known release type — did you mean {closest:?}? \
             Known types: {}. Fix: reword the merge title; unmapped types release nothing",
            known_types_csv(config)
        ),
    }
}

/// The config's known release types, sorted, comma-separated (the
/// `E-CLI-TITLE` diagnosis lists what the mapping accepts).
pub(crate) fn known_types_csv(config: &PlanConfig) -> String {
    config
        .commit_types
        .keys()
        .cloned()
        .collect::<Vec<_>>()
        .join(", ")
}

/// One pending commit's lint finding (§16.2's commits half): the same
/// title machinery over a commit subject — what the planner does with
/// the commit and whether its subject needs a fix.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CommitFinding {
    /// The kind maps to a bump and the commit drives the release (the
    /// engine's level, pre-major remap included, as the plan applies it).
    Drives {
        /// The bump the commit conveys.
        bump: Bump,
    },
    /// The kind is mapped to no bump (`docs:`, `chore:` …) — ignored by
    /// the configured mapping, no release effect, informational.
    MappedNone,
    /// The kind is unmapped and within edit distance 2 of a known type
    /// (the title gate's typo rule): a likely typo silently dropping a
    /// bump — the run refuses `E-CLI-COMMIT`.
    Typo {
        /// The type as written.
        kind: String,
        /// The closest known type (the did-you-mean).
        closest: String,
    },
    /// The subject is not a conventional commit at all. Unreachable
    /// from the digest path (the walk drops unparseable subjects —
    /// `halyard-git`'s `let Some(parsed) = … else { continue }`), kept
    /// because the classifier is total over any subject.
    NotConventional {
        /// The engine's grammar refusal text.
        reason: String,
    },
}

/// One pending commit's lint row: who it is, and what the planner would
/// do with it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CommitRow {
    /// The package the commit is attributed to (the digest's per-package
    /// attribution — one row per (package, commit) pair).
    pub package: String,
    /// Full commit object id (hex), matching the plan's reason chains.
    pub oid: String,
    /// The subject line (lossy UTF-8 of the digest's bytes).
    pub subject: String,
    /// The lint finding.
    pub finding: CommitFinding,
}

/// Builds the pending-history lint rows (§16.2's commits half): every
/// workspace package's pending commits (the digest's own attribution)
/// in discovery order, walk order inside a package — deterministic (I3)
/// without extra sorting. Packages with no release base (initial
/// releases) contribute no rows (the digest records nothing pending
/// for them).
pub(crate) fn pending_commit_rows(
    workspace: &halyard_core::Workspace,
    digest: &halyard_core::history::HistoryDigest,
    config: &PlanConfig,
) -> Vec<CommitRow> {
    let mut rows = Vec::new();
    for package in workspace.packages() {
        let Some(name) = workspace.interner().get(package.name) else {
            continue;
        };
        for index in digest.commits_for(package.id) {
            let Some(commit) = digest.commits.get(*index) else {
                continue;
            };
            let subject = String::from_utf8_lossy(&commit.subject).into_owned();
            let finding = classify_commit(&subject, config);
            rows.push(CommitRow {
                package: name.to_owned(),
                oid: commit.oid.to_string(),
                subject,
                finding,
            });
        }
    }
    rows
}

/// Lints one commit subject through the title machinery (`lint_title`
/// with the shell's rule — the exact gate the §11.6 title lint runs),
/// so a repo's commits and its squash titles can never disagree about
/// what a kind conveys.
pub(crate) fn classify_commit(subject: &str, config: &PlanConfig) -> CommitFinding {
    match lint(subject, config) {
        TitleVerdict::Accepted(lint) => {
            // The typo gate, verbatim from the title path: an unmapped
            // kind with a close known kind is a likely typo — fail
            // loudly instead of silently releasing nothing.
            if let Some(closest) = &lint.closest_known {
                CommitFinding::Typo {
                    kind: lint.kind.clone(),
                    closest: closest.clone(),
                }
            } else {
                match bump_of(lint.level) {
                    Bump::None => CommitFinding::MappedNone,
                    bump => CommitFinding::Drives { bump },
                }
            }
        }
        // Defensive totality: the digest never yields autosquash
        // subjects (§8.5 — the parser drops them before the walk), so
        // the arm maps onto the same honest "no effect" shape.
        TitleVerdict::FixupOrSquash => CommitFinding::MappedNone,
        TitleVerdict::NotConventional { reason } => CommitFinding::NotConventional { reason },
    }
}

/// The single-sourced `E-CLI-COMMIT` diagnosis for a typo'd pending
/// commit (the exit error's message — the same wording the renderers
/// surface on the typo row).
pub(crate) fn commit_typo_message(row: &CommitRow, config: &PlanConfig) -> String {
    let CommitFinding::Typo { kind, closest } = &row.finding else {
        unreachable!("commit_typo_message is only called on typo rows")
    };
    format!(
        "pending commit {} on {} — {} — type {kind:?} is not a known release type — did you          mean {closest:?}? Known types: {}. The planner ignores unmapped types, so this          commit releases nothing; reword the subject (`git rebase -i`) or map the type in          `[versioning].commit_types` (see E-CLI-COMMIT)",
        row.oid,
        row.package,
        row.subject,
        known_types_csv(config)
    )
}

/// The lint's outcome after the typo gate: the accepted lint (the
/// prediction input) or the rejection with its single-sourced
/// `E-CLI-TITLE` message — the shape both the exit path and the §11.6
/// comment consume.
#[derive(Debug)]
pub(crate) enum TitleClassification {
    /// The title passed the lint and the typo gate.
    Accepted(halyard_conventional::TitleLint),
    /// The title was rejected; the message is the diagnosis (the error
    /// text and the posted comment's reason — one source).
    Rejected(String),
}

/// Classifies one title: lint → typo gate → rejection wording (the
/// single-sourced `rejection_message`), so the command's exit path and
/// its posted comment can never disagree.
pub(crate) fn classify_title(title: &str, config: &PlanConfig) -> TitleClassification {
    match lint(title, config) {
        halyard_conventional::TitleVerdict::Accepted(lint) => {
            // §11.6's precise diagnosis: an unmapped kind with a close
            // known kind is a likely typo — fail loudly instead of
            // silently releasing nothing. A genuinely unmapped kind
            // (`chore:`, `docs:`) is a legitimate no-bump verdict.
            if let Some(closest) = &lint.closest_known {
                TitleClassification::Rejected(rejection_message(
                    title,
                    &TitleRejection::UnknownKind {
                        kind: lint.kind.clone(),
                        closest: closest.clone(),
                    },
                    config,
                ))
            } else {
                TitleClassification::Accepted(lint)
            }
        }
        halyard_conventional::TitleVerdict::NotConventional { reason } => {
            TitleClassification::Rejected(rejection_message(
                title,
                &TitleRejection::NotConventional { reason },
                config,
            ))
        }
        halyard_conventional::TitleVerdict::FixupOrSquash => TitleClassification::Rejected(
            rejection_message(title, &TitleRejection::FixupOrSquash, config),
        ),
    }
}

/// The §11.6 comment's inputs for `classification` (the renderer's
/// plain-data contract — the mapping stays beside the wording that
/// feeds it).
pub(crate) fn comment_input(
    title: &str,
    classification: &TitleClassification,
    predictions: &[Prediction],
) -> halyard_changelog::CheckComment {
    let verdict = match classification {
        TitleClassification::Accepted(lint) => halyard_changelog::CheckVerdict::Accepted {
            kind: lint.kind.clone(),
            breaking: lint.breaking,
        },
        TitleClassification::Rejected(reason) => halyard_changelog::CheckVerdict::Rejected {
            reason: reason.clone(),
        },
    };
    halyard_changelog::CheckComment {
        title: title.to_owned(),
        verdict,
        predictions: predictions
            .iter()
            .map(|row| halyard_changelog::CheckPrediction {
                package: row.package.clone(),
                from: row.from.clone(),
                bump: row.bump,
                remapped: row.remapped,
            })
            .collect(),
    }
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

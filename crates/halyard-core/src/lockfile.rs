//! The adapter lockfile seam (§7.3 lockfile subset, §9.2, M3.2): the
//! outcome, change and failure types shared by every
//! `EcosystemAdapter::patch_lockfile` implementation.
//!
//! The planner (§4) reduces a release to manifest byte patches (§5.8);
//! lockfiles are *derived* artifacts, so their rewrite is driven by the
//! plan's version moves (`releases`, §5.8) rather than by byte patches:
//! an adapter receives the [`LockfileVersionChange`] list and patches the
//! lockfile **semantically** — only the rows cargo itself would rewrite
//! (§9.2 rules 1–3), byte-in place (invariant I4).
//!
//! Fidelity is the contract: an implementation that cannot guarantee
//! cargo-identical bytes returns [`LockfileOutcome::NeedsNative`] and the
//! orchestrator falls back to the native refresh effect step
//! (`refresh_lockfile_native`, §9.2 rule 4). The default implementation
//! below *is* that fallback — an adapter without a lockfile patcher is
//! not a stub, it honestly declines and names the reason.
//!
//! Layer note (§7.2): this module is plain data; the effectful
//! filesystem/subprocess work stays in the shells (`halyard-edit`, CLI).

use crate::plan::PlanRelease;

/// One workspace member's planned version move (§9.2 rule 1): the input
/// vocabulary of [`crate::discover::EcosystemAdapter::patch_lockfile`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LockfileVersionChange {
    /// Package name as written in the lockfile (`[[package]].name`).
    pub name: String,
    /// Version the lockfile currently records (`from`; used to detect
    /// drift: a row neither at `from` nor at `to` is an unrecognized
    /// state).
    pub from: String,
    /// Version the plan moves the package to (`to`).
    pub to: String,
}

impl LockfileVersionChange {
    /// Derives the lockfile change for one plan release of a given
    /// ecosystem, and only for a real version move (`from != to`; a
    /// `Bump::None` release must not touch the lockfile). `None` for any
    /// other ecosystem or a no-move release.
    #[must_use]
    pub fn from_release_for_ecosystem(release: &PlanRelease, ecosystem: &str) -> Option<Self> {
        if release.ecosystem != ecosystem || release.from == release.to {
            return None;
        }
        Some(Self {
            name: release.package.clone(),
            from: release.from.clone(),
            to: release.to.clone(),
        })
    }

    /// Derives the Cargo.lock change for one plan release (M3.2, §9.2):
    /// cargo releases only, real moves only.
    #[must_use]
    pub fn from_release(release: &PlanRelease) -> Option<Self> {
        Self::from_release_for_ecosystem(release, "cargo")
    }
}

/// Result of one adapter lockfile pass (§7.3 lockfile subset, §9.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LockfileOutcome {
    /// The lockfile was patched in memory. Byte-identical to the input
    /// when nothing needed rewriting (invariant I4).
    Patched {
        /// The patched lockfile bytes.
        content: Vec<u8>,
        /// Rows/entries rewritten (member `version` rows and
        /// `"name version"` dependency disambiguation entries, §9.2
        /// rules 1–2).
        applied: usize,
        /// Rows/entries already at the target version (idempotent
        /// re-apply).
        skipped: usize,
    },
    /// The patcher cannot guarantee cargo-identical output for this
    /// lockfile (unknown format version, unrecognized row shape, drifted
    /// version, same-named registry package). Nothing was mutated; the
    /// orchestrator must fall back to the native refresh effect step
    /// (§9.2 rule 4: `cargo update --workspace --offline`).
    NeedsNative {
        /// Why the patcher declined (rendered in the apply summary).
        reason: String,
    },
}

/// Why a lockfile pass failed outright (§15.4 `E-LOCK-PARSE`; every
/// variant has a page under `docs/errors/`). A failure aborts the
/// lockfile edit — nothing is mutated (§9.1 apply protocol).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LockfileFailure {
    /// The lockfile is not valid UTF-8, does not parse as TOML, or a
    /// planned change is unusable (`E-LOCK-PARSE`); nothing was mutated.
    Parse {
        /// Lockfile path the pass targeted.
        file: String,
        /// What exactly went wrong.
        message: String,
    },
}

impl LockfileFailure {
    /// Stable code of [`LockfileFailure::Parse`].
    pub const PARSE_CODE: &'static str = "E-LOCK-PARSE";

    /// Stable §15.4 code of this error (rendered as the miette report
    /// code; each variant maps onto a `docs/errors/` page).
    pub fn code(&self) -> &'static str {
        match self {
            Self::Parse { .. } => Self::PARSE_CODE,
        }
    }
}

impl std::fmt::Display for LockfileFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parse { file, message } => {
                write!(f, "[{}] cannot patch {file}: {message}", Self::PARSE_CODE)
            }
        }
    }
}

impl std::error::Error for LockfileFailure {}

/// Why the native lockfile refresh effect step failed (§15.4
/// `E-LOCK-NATIVE`; the page covers the [`crate::discover::EcosystemAdapter::refresh_lockfile_native`]
/// surface). The refresh is the §9.2 rule-4 fallback when the in-memory
/// patcher declines; its failure is a hard error — the lockfile state is
/// cargo's business and the orchestrator must not guess.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NativeRefreshError {
    /// The adapter has no native refresh implementation (`E-LOCK-NATIVE`);
    /// only adapters with a sanctioned effect step implement this method.
    Unsupported {
        /// The adapter id that was asked to refresh.
        adapter: String,
    },
    /// Spawning the native tool failed (`E-LOCK-NATIVE`).
    Spawn {
        /// The exact argv of the tool (diagnostics only, never executed).
        command: String,
        /// Underlying OS error.
        source: String,
    },
    /// The native tool exited non-zero (`E-LOCK-NATIVE`); its stderr is
    /// surfaced verbatim (the tool owns the diagnosis).
    Failed {
        /// The exact argv of the tool.
        command: String,
        /// The tool's stderr (or stdout when stderr is empty).
        message: String,
    },
    /// The post-refresh verification failed (`E-LOCK-NATIVE`, §9.2 rule
    /// 4): the refresh changed rows beyond the workspace members (a
    /// resolution change) or produced an unrecognizable lockfile. The
    /// caller must treat the tree as drifted and re-plan.
    Verification {
        /// What exactly moved outside the members.
        reason: String,
    },
}

impl NativeRefreshError {
    /// Stable code of every variant (one `E-LOCK-NATIVE` page).
    pub const CODE: &'static str = "E-LOCK-NATIVE";

    /// Stable §15.4 code of this error.
    pub fn code(&self) -> &'static str {
        Self::CODE
    }
}

impl std::fmt::Display for NativeRefreshError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported { adapter } => write!(
                f,
                "[{}] adapter {adapter:?} has no native lockfile refresh; update the lockfile \
                 with the ecosystem's own tool",
                Self::CODE
            ),
            Self::Spawn { command, source } => {
                write!(f, "[{}] cannot spawn `{command}`: {source}", Self::CODE)
            }
            Self::Failed { command, message } => {
                write!(f, "[{}] `{command}` failed: {message}", Self::CODE)
            }
            Self::Verification { reason } => {
                write!(
                    f,
                    "[{}] native refresh verification failed, re-plan required: {reason}",
                    Self::CODE
                )
            }
        }
    }
}

impl std::error::Error for NativeRefreshError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discover::{DiscoverCtx, DiscoverError, EcosystemAdapter, RawPackage};
    use crate::package::RelPath;

    /// A minimal adapter that exercises only the trait defaults.
    struct BareAdapter;
    impl EcosystemAdapter for BareAdapter {
        fn id(&self) -> &'static str {
            "bare"
        }
        fn discover(&self, _ctx: &DiscoverCtx) -> Result<Vec<RawPackage>, DiscoverError> {
            Ok(Vec::new())
        }
        fn edit_versions(
            &self,
            _file: &RelPath,
            _content: &[u8],
            _patches: &[crate::plan::PlanPatch],
        ) -> Result<crate::edit::EditOutcome, crate::edit::EditFailure> {
            unreachable!("edit_versions is not exercised by the lockfile tests")
        }
    }

    #[test]
    fn default_patch_lockfile_declines_with_needs_native() {
        let file = RelPath::new("Cargo.lock").unwrap_or_else(|| unreachable!("valid rel path"));
        let changes = [LockfileVersionChange {
            name: "core".to_owned(),
            from: "0.4.1".to_owned(),
            to: "0.5.0".to_owned(),
        }];
        let outcome = BareAdapter
            .patch_lockfile(&file, b"version = 4\n", &changes)
            .unwrap_or_else(|f| unreachable!("default declines without error: {f}"));
        match outcome {
            LockfileOutcome::NeedsNative { reason } => {
                assert!(
                    reason.contains("bare"),
                    "reason names the adapter: {reason}"
                );
                assert!(
                    reason.contains("natively"),
                    "reason points at the native fallback: {reason}"
                );
            }
            other @ LockfileOutcome::Patched { .. } => {
                unreachable!("default impl must decline, got {other:?}")
            }
        }
    }

    #[test]
    fn from_release_keeps_only_cargo_version_moves() {
        let release = |ecosystem: &str, from: &str, to: &str| PlanRelease {
            package: "core".to_owned(),
            ecosystem: ecosystem.to_owned(),
            from: from.to_owned(),
            to: to.to_owned(),
            bump: crate::plan::Bump::Minor,
            tag: "core-v1".to_owned(),
            reasons: Vec::new(),
            changelog: None,
            publish: None,
        };
        assert_eq!(
            LockfileVersionChange::from_release(&release("cargo", "0.4.1", "0.5.0")),
            Some(LockfileVersionChange {
                name: "core".to_owned(),
                from: "0.4.1".to_owned(),
                to: "0.5.0".to_owned(),
            })
        );
        // Non-cargo ecosystems never touch a Cargo.lock.
        assert_eq!(
            LockfileVersionChange::from_release(&release("npm", "0.4.1", "0.5.0")),
            None
        );
        // A no-move release (bump None) must not touch the lockfile.
        assert_eq!(
            LockfileVersionChange::from_release(&release("cargo", "0.5.0", "0.5.0")),
            None
        );
    }

    #[test]
    fn default_refresh_lockfile_native_reports_unsupported() {
        let Err(err) = BareAdapter.refresh_lockfile_native(std::path::Path::new(".")) else {
            unreachable!("default impl must decline the native refresh")
        };
        assert_eq!(err.code(), "E-LOCK-NATIVE");
        match err {
            NativeRefreshError::Unsupported { adapter } => {
                assert_eq!(adapter, "bare");
            }
            other => unreachable!("default impl must be Unsupported, got {other:?}"),
        }
    }

    #[test]
    fn native_refresh_error_codes_and_display_are_stable() {
        let err = NativeRefreshError::Failed {
            command: "cargo update --workspace --offline".to_owned(),
            message: "no lockfile".to_owned(),
        };
        assert_eq!(err.code(), "E-LOCK-NATIVE");
        assert_eq!(
            err.to_string(),
            "[E-LOCK-NATIVE] `cargo update --workspace --offline` failed: no lockfile"
        );
        let err = NativeRefreshError::Verification {
            reason: "registry row moved".to_owned(),
        };
        assert!(err.to_string().contains("re-plan"));
    }

    #[test]
    fn failure_code_and_display_are_stable() {
        let f = LockfileFailure::Parse {
            file: "Cargo.lock".to_owned(),
            message: "not TOML".to_owned(),
        };
        assert_eq!(f.code(), "E-LOCK-PARSE");
        assert_eq!(
            f.to_string(),
            "[E-LOCK-PARSE] cannot patch Cargo.lock: not TOML"
        );
    }
}

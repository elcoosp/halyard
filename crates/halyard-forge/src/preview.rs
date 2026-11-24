//! The dry-run [`Forge`] adapter (M4.2b step 1): the forge behind
//! `halyard pr --dry`. Every probe answers honestly about an *empty,
//! unobservable* forge — offline mode cannot see the remote (invariant
//! I5: no unsolicited network), so [`PreviewForge::find_release_pr`] is
//! always `None` — and every mutation succeeds without persisting
//! anything: the run is a forecast, never an effect (I1 holds trivially,
//! because a dry run cannot mutate state at all).
//!
//! The adapter exists so the §11.1 machine can be driven end-to-end by
//! the shell in dry mode with the *same* code path a live run will take
//! (M4.1b swaps the adapter, not the machine): the
//! [`MachineReport`](crate::MachineReport) effects, the §11.5 call
//! budget and the rendered PR body are all observable before anything
//! touches a real forge.
//!
//! Preview results are distinguishable from real ones by shape: PR
//! number `0` and `preview://`-prefixed URLs — a real forge can never
//! produce either.
//!
//! ```
//! use halyard_forge::{BranchName, Forge, PrSpec, PreviewForge};
//!
//! let forge = PreviewForge::new();
//! let branch = BranchName::new("halyard/release/main").unwrap();
//! let pr = forge.upsert_pr(&PrSpec {
//!     branch,
//!     title: "chore: release p1-v0.1.1".to_owned(),
//!     body: "## 🚀 Release plan".to_owned(),
//! }).unwrap();
//! assert_eq!(pr.number, 0);
//! assert!(pr.url.starts_with("preview://"));
//! assert_eq!(forge.call_count("upsert_pr"), 1);
//! ```

use std::sync::Mutex;

use crate::Forge;
use crate::error::ForgeError;
use crate::memory::CallRecord;
use crate::spec::{
    ApiCommit, BranchName, CommentSpec, Pr, PrComment, PrSpec, RateInfo, Release, ReleaseSpec,
};

/// The dry-run [`Forge`]: every probe sees an empty forge, every
/// mutation is a forecast. Call records (§11.5) accumulate for the
/// whole lifetime — the CLI dry run reads them through
/// [`MachineReport::calls`](crate::MachineReport::calls).
#[derive(Debug, Default)]
pub struct PreviewForge {
    calls: Mutex<Vec<CallRecord>>,
}

impl PreviewForge {
    /// Creates an empty preview forge.
    pub fn new() -> Self {
        Self::default()
    }

    /// Records one operation in the §11.5 call log (mutations included:
    /// a forecasted call is still a call the live run would spend).
    fn record(&self, method: &'static str, target: &str) {
        if let Ok(mut calls) = self.calls.lock() {
            calls.push(CallRecord {
                method,
                target: target.to_owned(),
            });
        }
    }
}

impl Forge for PreviewForge {
    /// Always `None`: offline mode cannot observe the remote (I5), so
    /// the prepare-phase preview models the "no Release PR exists" path
    /// — the first run's forecast.
    fn find_release_pr(&self, branch: &BranchName) -> Result<Option<Pr>, ForgeError> {
        self.record("find_release_pr", branch.as_str());
        Ok(None)
    }

    /// Returns the preview PR (number `0`, `preview://` URL) carrying
    /// exactly the spec's title and body; nothing is stored, so the
    /// next probe still sees an empty forge.
    fn upsert_pr(&self, spec: &PrSpec) -> Result<Pr, ForgeError> {
        self.record("upsert_pr", spec.branch.as_str());
        Ok(Pr {
            number: 0,
            branch: spec.branch.as_str().to_owned(),
            title: spec.title.clone(),
            body: spec.body.clone(),
            url: format!("preview://{}", spec.branch.as_str()),
        })
    }

    /// Always `false`: the preview persists nothing, so no tag can have
    /// a release.
    fn release_exists(&self, tag: &str) -> Result<bool, ForgeError> {
        self.record("release_exists", tag);
        Ok(false)
    }

    /// Returns the preview release (`created: true` — the fresh path the
    /// live first run would take) with a `preview://` URL; nothing is
    /// stored.
    fn create_release(&self, spec: &ReleaseSpec) -> Result<Release, ForgeError> {
        self.record("create_release", &spec.tag);
        Ok(Release {
            tag: spec.tag.clone(),
            url: format!("preview://{}", spec.tag),
            created: true,
        })
    }

    /// Always `false` (§11.4: not-found is a value): nothing is stored,
    /// so there is no open PR to close.
    fn close_pr(&self, branch: &BranchName) -> Result<bool, ForgeError> {
        self.record("close_pr", branch.as_str());
        Ok(false)
    }

    /// The forecast comment (§11.6): the fresh path the live first run
    /// would take (`created: true`), a `preview://` URL distinguishable
    /// from a real forge's, nothing stored — the spec rides the shared
    /// validation contract so a forecast refuses an invalid comment
    /// exactly where the live run would.
    fn upsert_pr_comment(&self, spec: &CommentSpec) -> Result<PrComment, ForgeError> {
        self.record("upsert_pr_comment", &format!("pr/{}", spec.pr_number));
        spec.validate()?;
        Ok(PrComment {
            url: format!("preview://pr/{}/comment", spec.pr_number),
            created: true,
        })
    }

    /// The modeled budget (§11.5): the preview spends nothing real, so
    /// the full quota is reported as available.
    fn rate_limit(&self) -> RateInfo {
        self.record("rate_limit", "*");
        RateInfo::MODEL_DEFAULT
    }

    /// The recorded operations, oldest first — the observable call
    /// budget behind the dry-run report (§11.5).
    fn calls(&self) -> Vec<CallRecord> {
        self.calls
            .lock()
            .map(|calls| calls.clone())
            .unwrap_or_default()
    }

    /// Forecasts the content commit (the M4.2 caller: the machine's
    /// §11.1 step-3 prepare phase): the call is recorded, nothing is
    /// stored, and the returned oid is a deterministic blake3 digest of
    /// the commit's payloads — a valid OID shape so the report renders,
    /// never a real git object (the preview mints no objects; the
    /// 64-hex shape distinguishes it from the adapters' real commits the
    /// same way `preview://` distinguishes the URLs).
    fn commit_via_api(&self, spec: &ApiCommit) -> Result<halyard_core::reason::Oid, ForgeError> {
        self.record("commit_via_api", spec.branch.as_str());
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"halyard-preview-commit-v1");
        hasher.update(spec.branch.as_str().as_bytes());
        hasher.update(spec.message.as_bytes());
        for file in &spec.files {
            hasher.update(file.path.as_bytes());
            hasher.update(&file.content);
        }
        let hex = hasher.finalize().to_hex().to_string();
        let oid = halyard_core::reason::Oid::from_hex(hex.as_bytes()).ok_or_else(|| {
            ForgeError::Transport {
                reason: format!("the preview digest produced a non-hex oid: {hex:.200}"),
            }
        })?;
        Ok(oid)
    }
}

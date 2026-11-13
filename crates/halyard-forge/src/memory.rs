//! The [`MemoryForge`] reference adapter (§7.1 "mock forge", §15.1 E2E
//! lane): a deterministic, in-memory [`Forge`] that models the forge
//! semantics the M4.2 state machine relies on — find-by-branch upserts,
//! GET-by-tag-first release creation, synthetic signed commits — plus the
//! two observability surfaces the §11.5 budget work needs: the call log
//! (the "≤ 2 REST calls on the update path" assertion rides on it) and
//! the failpoint harness (§15.2 failure injection).
//!
//! Determinism (I3): no clock, no randomness — PR numbers and object ids
//! are assigned sequentially, URLs are synthetic `mem://` schemes, and
//! two equally-constructed forges produce identical outcomes.

use std::sync::Mutex;
use std::sync::MutexGuard;
use std::sync::PoisonError;

use halyard_core::reason::Oid;

use crate::Forge;
use crate::error::ForgeError;
use crate::spec::{ApiCommit, BranchName, Pr, PrSpec, RateInfo, Release, ReleaseSpec};

/// One recorded forge operation (the §11.5 "API usage summary" in
/// miniature). `target` is the branch or tag the call addressed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallRecord {
    /// The trait method invoked (`"find_release_pr"`, …).
    pub method: &'static str,
    /// The branch/tag the call targeted.
    pub target: String,
}

#[derive(Debug, Default)]
struct Inner {
    failure: Option<ForgeError>,
    prs: Vec<Pr>,
    next_pr_number: u64,
    releases: Vec<Release>,
    next_commit: u64,
    calls: Vec<CallRecord>,
}

/// The deterministic in-memory [`Forge`] — the reference implementation
/// of the seam and the §7.1 mock forge future E2E suites drive.
#[derive(Debug)]
pub struct MemoryForge {
    inner: Mutex<Inner>,
}

impl Default for MemoryForge {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryForge {
    /// A fresh forge: no PRs, no releases, no armed failpoint.
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Inner::default()),
        }
    }

    /// Arms (`Some`) or disarms (`None`) the failpoint: while armed, every
    /// forge operation fails with a clone of the given error (§15.2
    /// failure injection). Disarming restores normal operation; state is
    /// never corrupted by an injected failure.
    pub fn set_failure(&self, failure: Option<ForgeError>) {
        self.state().failure = failure;
    }

    /// Snapshot of the PRs the forge currently holds (open/closed state
    /// is not modeled — the seam's consumers re-find by branch).
    pub fn prs(&self) -> Vec<Pr> {
        self.state().prs.clone()
    }

    /// Snapshot of the releases the forge currently holds.
    pub fn releases(&self) -> Vec<Release> {
        self.state().releases.clone()
    }

    /// Locks the state; poisoning cannot corrupt the invariants (every
    /// mutation is a total replace or append), so the guard is recovered
    /// and the panic propagates from the *original* site.
    fn state(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Records one call (after the failpoint check, so armed failures are
    /// logged too — a dropped call is an unobservable one, §11.5).
    fn record(state: &mut Inner, method: &'static str, target: &str) {
        state.calls.push(CallRecord {
            method,
            target: target.to_owned(),
        });
    }
}

impl Forge for MemoryForge {
    fn find_release_pr(&self, branch: &BranchName) -> Result<Option<Pr>, ForgeError> {
        let mut state = self.state();
        Self::record(&mut state, "find_release_pr", branch.as_str());
        if let Some(failure) = state.failure.clone() {
            return Err(failure);
        }
        Ok(state
            .prs
            .iter()
            .find(|pr| pr.branch == branch.as_str())
            .cloned())
    }

    fn upsert_pr(&self, spec: &PrSpec) -> Result<Pr, ForgeError> {
        let mut state = self.state();
        Self::record(&mut state, "upsert_pr", spec.branch.as_str());
        if let Some(failure) = state.failure.clone() {
            return Err(failure);
        }
        if let Some(existing) = state
            .prs
            .iter_mut()
            .find(|pr| pr.branch == spec.branch.as_str())
        {
            existing.title = spec.title.clone();
            existing.body = spec.body.clone();
            return Ok(existing.clone());
        }
        state.next_pr_number += 1;
        let pr = Pr {
            number: state.next_pr_number,
            branch: spec.branch.as_str().to_owned(),
            title: spec.title.clone(),
            body: spec.body.clone(),
            url: format!("mem://pr/{}", state.next_pr_number),
        };
        state.prs.push(pr.clone());
        Ok(pr)
    }

    fn release_exists(&self, tag: &str) -> Result<bool, ForgeError> {
        let mut state = self.state();
        Self::record(&mut state, "release_exists", tag);
        if let Some(failure) = state.failure.clone() {
            return Err(failure);
        }
        Ok(state.releases.iter().any(|release| release.tag == tag))
    }

    fn create_release(&self, spec: &ReleaseSpec) -> Result<Release, ForgeError> {
        let mut state = self.state();
        Self::record(&mut state, "create_release", &spec.tag);
        if let Some(failure) = state.failure.clone() {
            return Err(failure);
        }
        if spec.tag.is_empty() {
            return Err(ForgeError::request("release tag is empty"));
        }
        // GET-by-tag-first idempotence (§11.1 step 2, I1): the re-run
        // finds the existing release and reports it as not-created.
        if let Some(existing) = state
            .releases
            .iter()
            .find(|release| release.tag == spec.tag)
        {
            let mut found = existing.clone();
            found.created = false;
            return Ok(found);
        }
        let release = Release {
            tag: spec.tag.clone(),
            url: format!("mem://release/{}", spec.tag),
            created: true,
        };
        state.releases.push(release.clone());
        Ok(release)
    }

    fn commit_via_api(&self, spec: &ApiCommit) -> Result<Oid, ForgeError> {
        let mut state = self.state();
        Self::record(&mut state, "commit_via_api", spec.branch.as_str());
        if let Some(failure) = state.failure.clone() {
            return Err(failure);
        }
        state.next_commit += 1;
        // Synthetic object id: lowercase hex, 40 digits, `fe`-prefixed so
        // it can never be mistaken for a real git object (a mock mints no
        // git objects). 38 hex digits hold a u64 counter without loss.
        let hex = format!("fe{:038x}", state.next_commit);
        let oid = Oid::from_hex(hex.as_bytes())
            .unwrap_or_else(|| unreachable!("counter-formatted oid is 40 hex digits"));
        Ok(oid)
    }

    fn rate_limit(&self) -> RateInfo {
        let mut state = self.state();
        Self::record(&mut state, "rate_limit", "*");
        RateInfo::MODEL_DEFAULT
    }

    fn calls(&self) -> Vec<CallRecord> {
        self.state().calls.clone()
    }
}

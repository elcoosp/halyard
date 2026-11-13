//! The [`Forge`] trait contract (§11.4, M4.1a): the semantics every
//! adapter — including the future GitHub REST adapter — must honour,
//! pinned against the [`MemoryForge`] reference implementation.
//!
//! The tests run through `&dyn Forge` so object safety is part of the
//! contract, and every §15.4 `E-FORGE-*` code has a real producer here
//! (the failpoint harness, §15.2 failure injection).

use halyard_forge::{
    ApiCommit, ApiFile, BranchName, Forge, ForgeError, MemoryForge, PrSpec, ReleaseSpec,
};

/// Runs `f` against the forge through a trait object (object-safety +
/// Send/Sync proof rides along in `forge_is_object_safe_and_send_sync`).
fn drive(f: impl FnOnce(&dyn Forge) -> Result<(), ForgeError>) {
    let forge: Box<dyn Forge> = Box::new(MemoryForge::new());
    f(forge.as_ref()).unwrap_or_else(|e| unreachable!("drive() sequence must succeed: {e}"));
}

/// A branch name shared by the test sequences (validated once, reused).
fn main_branch() -> BranchName {
    BranchName::new("halyard/release/main")
        .unwrap_or_else(|e| unreachable!("branch fixture must be valid: {e}"))
}

#[test]
fn upsert_pr_creates_then_updates_in_place() {
    drive(|forge| {
        let branch = main_branch();
        let spec = PrSpec {
            branch: branch.clone(),
            title: "chore: release v0.1.0".to_owned(),
            body: "## 🚀 Release plan".to_owned(),
        };
        let first = forge.upsert_pr(&spec)?;
        let second = forge.upsert_pr(&spec)?;
        assert_eq!(first, second, "idempotent upsert returns the same PR (I1)");
        let found = forge
            .find_release_pr(&branch)?
            .unwrap_or_else(|| unreachable!("upsert must make the PR findable"));
        assert_eq!(found.number, first.number);
        assert_eq!(found.title, spec.title);
        assert_eq!(found.body, spec.body);
        Ok(())
    });
}

#[test]
fn upsert_pr_rewrites_the_body_on_a_second_pass() {
    drive(|forge| {
        let branch = main_branch();
        let spec = PrSpec {
            branch: branch.clone(),
            title: "chore: release v0.1.0".to_owned(),
            body: "old plan".to_owned(),
        };
        let first = forge.upsert_pr(&spec)?;
        let updated = forge.upsert_pr(&PrSpec {
            branch: branch.clone(),
            title: spec.title,
            body: "## 🚀 Release plan\n\n| core | 0.1.0 | 0.2.0 |".to_owned(),
        })?;
        assert_eq!(first.number, updated.number, "no duplicate PR is opened");
        assert!(
            updated.body.contains("| core |"),
            "the body was replaced in place: {}",
            updated.body
        );
        Ok(())
    });
}

#[test]
fn find_release_pr_miss_is_none_never_an_error() {
    drive(|forge| {
        assert_eq!(
            forge.find_release_pr(&main_branch())?,
            None,
            "a missing PR is `None`, not an error (§11.4)"
        );
        Ok(())
    });
}

#[test]
fn create_release_is_idempotent_get_by_tag_first() {
    drive(|forge| {
        let spec = ReleaseSpec {
            tag: "core-v0.5.0".to_owned(),
            notes: "## Features\n\n- the parser".to_owned(),
        };
        let first = forge.create_release(&spec)?;
        assert!(first.created, "the first call creates");
        assert_eq!(first.tag, spec.tag);
        let second = forge.create_release(&spec)?;
        assert!(
            !second.created,
            "the re-run finds the release by tag first (§11.1 step 2, I1)"
        );
        assert_eq!(first.url, second.url, "no duplicate release artifact");
        assert!(forge.release_exists(&spec.tag)?, "the tag reports released");
        Ok(())
    });
}

#[test]
fn release_exists_miss_is_false_never_an_error() {
    drive(|forge| {
        assert!(
            !forge.release_exists("core-v9.9.9")?,
            "a missing tag is `false`, not an error (§11.4)"
        );
        Ok(())
    });
}

#[test]
fn update_path_stays_within_the_two_call_budget() {
    drive(|forge| {
        let branch = main_branch();
        let spec = PrSpec {
            branch: branch.clone(),
            title: "chore: release v0.1.0".to_owned(),
            body: "plan".to_owned(),
        };
        let _ = forge.upsert_pr(&spec)?;
        let _ = forge.find_release_pr(&branch)?;
        let _ = forge.upsert_pr(&spec)?;
        // §11.5: the update path is find (≤1 call) + upsert (1 call)
        // per pass; each trait invocation logs exactly one entry, and
        // the call log is the observable that lets the M4.1 REST
        // adapter assert the same budget against real HTTP.
        assert_eq!(
            forge.call_count("find_release_pr"),
            1,
            "each find is one call: {:?}",
            forge.calls()
        );
        assert_eq!(forge.call_count("upsert_pr"), 2);
        assert_eq!(forge.calls().len(), 3, "every operation is logged");
        Ok(())
    });
}

#[test]
fn commit_via_api_records_the_commit_and_returns_a_synthetic_oid() {
    drive(|forge| {
        let spec = ApiCommit {
            branch: main_branch(),
            message: "chore: apply the release plan".to_owned(),
            files: vec![ApiFile {
                path: "crates/core/Cargo.toml".to_owned(),
                content: b"version = \"0.5.0\"".to_vec(),
            }],
        };
        let oid = forge.commit_via_api(&spec)?;
        // 40 lowercase hex digits: syntactically a SHA-1, documented as
        // synthetic (a mock forge mints no real git objects).
        assert_eq!(oid.as_bytes().len(), 40);
        assert!(
            oid.as_str()
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()),
            "synthetic oid is lowercase hex: {}",
            oid.as_str()
        );
        let second = forge.commit_via_api(&spec)?;
        assert_ne!(oid, second, "each commit is a distinct object");
        Ok(())
    });
}

#[test]
fn commit_via_api_defaults_to_unsupported() {
    // The trait's default implementation is the documented honest
    // decline (mirrors `refresh_lockfile_native`'s default): forges
    // without an API-commit endpoint name themselves instead of lying.
    struct GitOnlyForge(MemoryForge);
    impl Forge for GitOnlyForge {
        fn find_release_pr(
            &self,
            branch: &BranchName,
        ) -> Result<Option<halyard_forge::Pr>, ForgeError> {
            self.0.find_release_pr(branch)
        }
        fn upsert_pr(&self, spec: &PrSpec) -> Result<halyard_forge::Pr, ForgeError> {
            self.0.upsert_pr(spec)
        }
        fn release_exists(&self, tag: &str) -> Result<bool, ForgeError> {
            self.0.release_exists(tag)
        }
        fn create_release(&self, spec: &ReleaseSpec) -> Result<halyard_forge::Release, ForgeError> {
            self.0.create_release(spec)
        }
        fn rate_limit(&self) -> halyard_forge::RateInfo {
            self.0.rate_limit()
        }
        fn calls(&self) -> Vec<halyard_forge::CallRecord> {
            self.0.calls()
        }
        // `commit_via_api` intentionally left at the trait default.
    }
    let forge = GitOnlyForge(MemoryForge::new());
    let spec = ApiCommit {
        branch: main_branch(),
        message: "chore: apply".to_owned(),
        files: Vec::new(),
    };
    let Err(err) = forge.commit_via_api(&spec) else {
        unreachable!("the default impl must decline the API commit")
    };
    assert_eq!(err.code(), "E-FORGE-UNSUPPORTED");
    assert!(
        err.to_string().contains("GitOnlyForge") || err.to_string().contains("adapter"),
        "the decline names the adapter: {err}"
    );
}

#[test]
fn close_pr_reports_false_when_no_open_pr_exists() {
    drive(|forge| {
        let branch = main_branch();
        assert!(
            !forge.close_pr(&branch)?,
            "a missing PR is `false`, not an error (§11.4 not-found convention)"
        );
        Ok(())
    });
}

#[test]
fn close_pr_closes_in_place_and_find_stops_returning_it() {
    drive(|forge| {
        let branch = main_branch();
        let spec = PrSpec {
            branch: branch.clone(),
            title: "chore: release v0.1.0".to_owned(),
            body: "stale plan".to_owned(),
        };
        let pr = forge.upsert_pr(&spec)?;
        assert!(forge.close_pr(&branch)?, "closing an open PR reports true");
        assert_eq!(
            forge.find_release_pr(&branch)?,
            None,
            "a closed PR is no longer found (the §11.1 re-run sees nothing stale)"
        );
        assert!(!forge.close_pr(&branch)?, "the re-close is a no-op (I1)");
        let _ = pr;
        Ok(())
    });
}

#[test]
fn upsert_after_close_opens_a_fresh_pr() {
    drive(|forge| {
        let branch = main_branch();
        let spec = PrSpec {
            branch: branch.clone(),
            title: "chore: release v0.1.0".to_owned(),
            body: "old plan".to_owned(),
        };
        let closed = forge.upsert_pr(&spec)?;
        assert!(forge.close_pr(&branch)?, "fixture: the PR is closed");
        let fresh = forge.upsert_pr(&spec)?;
        assert_ne!(
            closed.number, fresh.number,
            "a closed PR is never rewritten in place — a new PR is opened (real forges \
             cannot reopen a closed PR under the same number)"
        );
        Ok(())
    });
}

#[test]
fn close_pr_defaults_to_unsupported() {
    // The trait's default implementation is the documented honest
    // decline (mirrors `commit_via_api`): forges without a close
    // endpoint name themselves instead of lying.
    struct NoCloseForge(MemoryForge);
    impl Forge for NoCloseForge {
        fn find_release_pr(
            &self,
            branch: &BranchName,
        ) -> Result<Option<halyard_forge::Pr>, ForgeError> {
            self.0.find_release_pr(branch)
        }
        fn upsert_pr(&self, spec: &PrSpec) -> Result<halyard_forge::Pr, ForgeError> {
            self.0.upsert_pr(spec)
        }
        fn release_exists(&self, tag: &str) -> Result<bool, ForgeError> {
            self.0.release_exists(tag)
        }
        fn create_release(&self, spec: &ReleaseSpec) -> Result<halyard_forge::Release, ForgeError> {
            self.0.create_release(spec)
        }
        fn rate_limit(&self) -> halyard_forge::RateInfo {
            self.0.rate_limit()
        }
        fn calls(&self) -> Vec<halyard_forge::CallRecord> {
            self.0.calls()
        }
        // `close_pr` intentionally left at the trait default.
    }
    let forge = NoCloseForge(MemoryForge::new());
    let Err(err) = forge.close_pr(&main_branch()) else {
        unreachable!("the default impl must decline the close");
    };
    assert_eq!(err.code(), "E-FORGE-UNSUPPORTED");
    assert!(
        err.to_string().contains("NoCloseForge"),
        "the decline names the adapter: {err}"
    );
}

#[test]
fn rate_limit_reports_a_deterministic_budget() {
    drive(|forge| {
        let info = forge.rate_limit();
        assert!(info.remaining <= info.limit, "budget shape: {info:?}");
        assert_eq!(info, forge.rate_limit(), "deterministic within a run (I3)");
        Ok(())
    });
}

#[test]
fn branch_name_validation_fails_closed() {
    for bad in ["", "  ", "main\t", "feature\nbranch"] {
        let Err(err) = BranchName::new(bad) else {
            unreachable!("branch name {bad:?} must be rejected");
        };
        assert_eq!(err.code(), "E-FORGE-REQUEST", "bad name {bad:?}: {err}");
    }
    let _ = main_branch();
}

#[test]
fn empty_release_tag_fails_closed() {
    drive(|forge| {
        let spec = ReleaseSpec {
            tag: String::new(),
            notes: String::new(),
        };
        let Err(err) = forge.create_release(&spec) else {
            unreachable!("an empty tag must be rejected");
        };
        assert_eq!(err.code(), "E-FORGE-REQUEST");
        Ok(())
    });
}

#[test]
fn failure_injection_produces_every_error_code() {
    // §15.2: the failpoint harness injects each error so the codes have a
    // producer that the M4.2 failure-injection matrix reuses.
    let cases = [
        (ForgeError::auth("no token"), "E-FORGE-AUTH"),
        (ForgeError::rate_limited(42), "E-FORGE-RATE-LIMIT"),
        (
            ForgeError::conflict("expected oid 9f2c, found 1a2b"),
            "E-FORGE-CONFLICT",
        ),
        (
            ForgeError::request("branch name is empty"),
            "E-FORGE-REQUEST",
        ),
    ];
    for (failure, code) in cases {
        let forge = MemoryForge::new();
        forge.set_failure(Some(failure));
        let Err(err) = forge.find_release_pr(&main_branch()) else {
            unreachable!("armed failpoint must fire");
        };
        assert_eq!(err.code(), code, "injected error carries its code: {err}");
        forge.set_failure(None);
        let _ = forge
            .find_release_pr(&main_branch())
            .unwrap_or_else(|e| unreachable!("disarmed forge works again: {e}"));
    }
}

#[test]
fn error_codes_and_display_are_stable() {
    let err = ForgeError::auth("token lacks contents:write");
    assert_eq!(err.code(), "E-FORGE-AUTH");
    assert!(
        err.to_string().starts_with("[E-FORGE-AUTH]"),
        "display leads with the code: {err}"
    );
    let err = ForgeError::rate_limited(120);
    assert_eq!(err.code(), "E-FORGE-RATE-LIMIT");
    assert!(
        err.to_string().contains("120"),
        "retry hint surfaces: {err}"
    );
    let err = ForgeError::conflict("release branch moved under us");
    assert_eq!(err.code(), "E-FORGE-CONFLICT");
    assert!(
        err.to_string().contains("re-plan"),
        "conflict teaches the fix: {err}"
    );
    let err = ForgeError::request("branch name is empty");
    assert_eq!(err.code(), "E-FORGE-REQUEST");
}

#[test]
fn forge_is_object_safe_and_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<MemoryForge>();
    assert_send_sync::<Box<dyn Forge>>();
    let dyn_forge: Box<dyn Forge> = Box::new(MemoryForge::new());
    let _ = dyn_forge.rate_limit();
}

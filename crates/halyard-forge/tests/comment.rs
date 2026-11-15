//! The §11.6 check-comment seam contract (M4.5 step 1): the
//! `Forge::upsert_pr_comment` capability — the marker-based idempotent
//! comment upsert the `check --pr-title --comment` mode posts to the PR
//! under review. The assertions pin the seam values (I1 marker upsert,
//! fail-closed spec validation, the honest adapter default) and the
//! observability surfaces (call log, §11.5 governor integration) on the
//! deterministic [`MemoryForge`] reference adapter.

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::PoisonError;

use halyard_forge::error::ForgeError;
use halyard_forge::governor::{Governor, GovernorClock, GovernorPolicy, GovernorSleeper};
use halyard_forge::spec::{BranchName, Pr, PrSpec, RateInfo, Release, ReleaseSpec};
use halyard_forge::{CallRecord, CommentSpec, Forge, MemoryForge, PreviewForge};

type BoxError = Box<dyn std::error::Error>;

/// The marker the `check` mode posts with (the §11.6 comment's
/// idempotence key — the changelog renderer embeds it in the body).
const MARKER: &str = "<!-- halyard:check v1 -->";

fn spec(pr_number: u64, body: &str) -> CommentSpec {
    CommentSpec {
        pr_number,
        marker: MARKER.to_owned(),
        body: format!("{MARKER}\n\n{body}"),
    }
}

/// Opens one PR on the reference adapter (the comment target's setup).
fn open_pr(forge: &MemoryForge, branch: &str, number: u64) -> Result<(), BoxError> {
    let created = forge.upsert_pr(&PrSpec {
        branch: BranchName::new(branch)?,
        title: "feat: the change".to_owned(),
        body: "the change body".to_owned(),
    })?;
    assert_eq!(created.number, number, "numbers are sequential from 1");
    Ok(())
}

#[test]
fn comment_upsert_creates_then_rewrites_in_place() -> Result<(), BoxError> {
    let forge = MemoryForge::new();
    open_pr(&forge, "feature/one", 1)?;

    let first = forge.upsert_pr_comment(&spec(1, "core 0.1.0 → 0.1.1 (patch)"))?;
    assert!(first.created, "the first upsert creates the comment");
    assert!(
        first.url.starts_with("mem://"),
        "reference URLs are synthetic"
    );

    let second = forge.upsert_pr_comment(&spec(1, "core 0.1.0 → 0.1.2 (patch)"))?;
    assert!(!second.created, "the marker re-run rewrites in place (I1)");
    assert_eq!(second.url, first.url, "same marker ⇒ same comment");

    let bodies = forge.pr_comments(1);
    assert_eq!(bodies.len(), 1, "the re-run never duplicates (I1)");
    let body = bodies
        .first()
        .unwrap_or_else(|| unreachable!("one comment asserted above"));
    assert!(body.contains("0.1.2"), "the rewrite carried the new body");
    Ok(())
}

#[test]
fn comment_marker_distinguishes_multiple_bot_comments() -> Result<(), BoxError> {
    let forge = MemoryForge::new();
    open_pr(&forge, "feature/one", 1)?;

    let check = forge.upsert_pr_comment(&spec(1, "check verdict"))?;
    let other = forge.upsert_pr_comment(&CommentSpec {
        pr_number: 1,
        marker: "<!-- halyard:other v1 -->".to_owned(),
        body: "<!-- halyard:other v1 -->\n\nanother bot's comment".to_owned(),
    })?;
    assert!(other.created, "a foreign marker has no comment to rewrite");
    assert_ne!(
        check.url, other.url,
        "distinct markers are distinct comments"
    );
    assert_eq!(
        forge.pr_comments(1).len(),
        2,
        "both comments are stored under the PR"
    );
    Ok(())
}

#[test]
fn comment_on_a_closed_pr_still_works_like_the_real_forge() -> Result<(), BoxError> {
    let forge = MemoryForge::new();
    open_pr(&forge, "feature/one", 1)?;
    let closed = forge.close_pr(&BranchName::new("feature/one")?)?;
    assert!(closed, "the setup closes the PR");
    let comment = forge.upsert_pr_comment(&spec(1, "verdict"))?;
    assert!(
        comment.created,
        "GitHub comments work on closed PRs; so does the model"
    );
    Ok(())
}

#[test]
fn comment_specs_fail_closed_before_any_store_touch() -> Result<(), BoxError> {
    let forge = MemoryForge::new();
    open_pr(&forge, "feature/one", 1)?;

    let cases: Vec<CommentSpec> = vec![
        // The marker is the find key: an empty marker can never be found.
        CommentSpec {
            pr_number: 1,
            marker: String::new(),
            body: "body".to_owned(),
        },
        // A marker absent from the body makes the comment unfindable.
        CommentSpec {
            pr_number: 1,
            marker: MARKER.to_owned(),
            body: "body without the marker".to_owned(),
        },
        // An empty body has nothing to post.
        CommentSpec {
            pr_number: 1,
            marker: MARKER.to_owned(),
            body: String::new(),
        },
        // Forge PR numbers are 1-based; zero names no PR.
        CommentSpec {
            pr_number: 0,
            marker: MARKER.to_owned(),
            body: format!("{MARKER}\nb"),
        },
    ];
    for invalid in cases {
        let error = forge
            .upsert_pr_comment(&invalid)
            .err()
            .unwrap_or_else(|| unreachable!("the case is invalid by construction"));
        assert!(
            matches!(error, ForgeError::Request { .. }),
            "spec validation is `E-FORGE-REQUEST`, got {error}"
        );
    }
    assert_eq!(
        forge.pr_comments(1).len(),
        0,
        "validation happens before any store touch"
    );
    Ok(())
}

#[test]
fn comment_on_an_unknown_pr_is_refused_fail_closed() {
    let forge = MemoryForge::new();
    // The reference adapter creates the comment only for stored PRs;
    // an unknown PR number is a caller bug — `E-FORGE-REQUEST`.
    let error = forge
        .upsert_pr_comment(&spec(99, "verdict"))
        .err()
        .unwrap_or_else(|| unreachable!("PR 99 does not exist"));
    assert!(matches!(error, ForgeError::Request { .. }), "got {error}");
}

#[test]
fn comment_failpoint_is_recorded_and_never_corrupts() -> Result<(), BoxError> {
    let forge = MemoryForge::new();
    open_pr(&forge, "feature/one", 1)?;
    forge.set_failure(Some(ForgeError::rate_limited(3)));
    let error = forge
        .upsert_pr_comment(&spec(1, "verdict"))
        .err()
        .unwrap_or_else(|| unreachable!("the failpoint is armed"));
    assert!(
        matches!(error, ForgeError::RateLimited { .. }),
        "the injected failure surfaces verbatim, got {error}"
    );
    assert_eq!(forge.pr_comments(1).len(), 0, "no partial state");
    forge.set_failure(None);
    let comment = forge.upsert_pr_comment(&spec(1, "verdict"))?;
    assert!(comment.created, "disarming restores normal operation");
    Ok(())
}

#[test]
fn comment_call_log_records_the_operation() -> Result<(), BoxError> {
    let forge = MemoryForge::new();
    open_pr(&forge, "feature/one", 1)?;
    forge.upsert_pr_comment(&spec(1, "verdict"))?;
    let calls = forge.calls();
    let recorded: Vec<&CallRecord> = calls
        .iter()
        .filter(|call| call.method == "upsert_pr_comment")
        .collect();
    assert_eq!(recorded.len(), 1, "one call record for one upsert");
    let record = recorded
        .first()
        .unwrap_or_else(|| unreachable!("one record asserted above"));
    assert_eq!(record.target, "pr/1", "the record names the target PR");
    Ok(())
}

#[test]
fn preview_comment_is_a_forecast_never_a_state_change() -> Result<(), BoxError> {
    let forge = PreviewForge::new();
    let comment = forge.upsert_pr_comment(&spec(7, "verdict"))?;
    assert!(comment.created, "the forecast models the fresh path");
    assert!(
        comment.url.starts_with("preview://"),
        "preview URLs are distinguishable from real ones"
    );
    assert_eq!(
        forge.call_count("upsert_pr_comment"),
        1,
        "the forecasted call is logged"
    );
    Ok(())
}

/// A minimal adapter that keeps every trait default — the honest-decline
/// witness for [`Forge::upsert_pr_comment`].
struct DecliningForge;

impl Forge for DecliningForge {
    fn find_release_pr(&self, _branch: &BranchName) -> Result<Option<Pr>, ForgeError> {
        Ok(None)
    }
    fn upsert_pr(&self, _spec: &PrSpec) -> Result<Pr, ForgeError> {
        Err(ForgeError::unsupported("DecliningForge"))
    }
    fn release_exists(&self, _tag: &str) -> Result<bool, ForgeError> {
        Ok(false)
    }
    fn create_release(&self, _spec: &ReleaseSpec) -> Result<Release, ForgeError> {
        Err(ForgeError::unsupported("DecliningForge"))
    }
    fn rate_limit(&self) -> RateInfo {
        RateInfo::MODEL_DEFAULT
    }
    fn calls(&self) -> Vec<CallRecord> {
        Vec::new()
    }
}

#[test]
fn the_trait_default_is_the_documented_honest_decline() {
    let error = DecliningForge
        .upsert_pr_comment(&spec(1, "verdict"))
        .err()
        .unwrap_or_else(|| unreachable!("the default declines"));
    assert!(
        matches!(error, ForgeError::Unsupported { .. }),
        "the default is `E-FORGE-UNSUPPORTED`, got {error}"
    );
    assert!(
        error.to_string().contains("DecliningForge"),
        "the decline names the adapter"
    );
}

/// A scripted clock/sleeper pair: the clock is a mutable millisecond
/// counter; the sleeper advances it by the requested wait (the §11.5
/// convention — a wait schedule is assertable data).
fn scripted_time() -> (GovernorClock, GovernorSleeper, Arc<Mutex<Vec<u64>>>) {
    let now = Arc::new(Mutex::new(1_000_u64));
    let now_clock = Arc::clone(&now);
    let clock: GovernorClock =
        Arc::new(move || *now_clock.lock().unwrap_or_else(PoisonError::into_inner));
    let waits = Arc::new(Mutex::new(Vec::new()));
    let waits_sleeper = Arc::clone(&waits);
    let now_sleeper = Arc::clone(&now);
    let sleeper: GovernorSleeper = Arc::new(move |ms| {
        *now_sleeper.lock().unwrap_or_else(PoisonError::into_inner) += ms;
        waits_sleeper
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(ms);
    });
    (clock, sleeper, waits)
}

#[test]
fn the_governor_paces_and_budgets_the_comment_upsert() -> Result<(), BoxError> {
    let forge = MemoryForge::new();
    open_pr(&forge, "feature/one", 1)?;
    let policy = GovernorPolicy {
        min_spacing_ms: 50,
        max_calls: Some(2),
        ..GovernorPolicy::default()
    };
    let (clock, sleeper, waits) = scripted_time();
    let governor = Governor::new(&forge, policy, clock, sleeper);
    governor.upsert_pr_comment(&spec(1, "first"))?;
    governor.upsert_pr_comment(&spec(1, "second"))?;
    let logged = waits.lock().unwrap_or_else(PoisonError::into_inner).clone();
    assert!(
        logged.contains(&50),
        "the second content call waits the spacing, waits={logged:?}"
    );
    let third = governor
        .upsert_pr_comment(&spec(1, "third"))
        .err()
        .unwrap_or_else(|| unreachable!("the budget (2) is spent"));
    assert!(
        matches!(third, ForgeError::Budget { .. }),
        "the budget refuses before the call, got {third}"
    );
    assert_eq!(
        forge.call_count("upsert_pr_comment"),
        2,
        "the refused call never reached the forge"
    );
    Ok(())
}

/// The scripted-transport harness for the wire contract lives in the
/// adapter crate; the seam level pins only the shape of a *successful*
/// upsert value.
#[test]
fn the_upsert_value_is_plain_data() {
    let comment = halyard_forge::PrComment {
        url: "https://github.com/o/h/pull/1#issuecomment-9".to_owned(),
        created: true,
    };
    assert!(comment.created && comment.url.contains("issuecomment"));
}

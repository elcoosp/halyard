//! Registry existence-probe contract tests (§13 step 2, M5.1): the
//! `Registry` seam over the forge's `HttpTransport`, exercised offline
//! through the scripted `MemoryTransport`. Pins the three contracts the
//! publisher (M5.2) depends on:
//!
//! * **§13 step 2 idempotent resume** — `published(name, version)` is a
//!   pure probe: `404` ⇒ not published, a matching version row ⇒
//!   published, everything else fail-closed (`E-REG-*`);
//! * **the VERIFIED URL grammar** — the crates.io sparse-index prefix
//!   scheme (Cargo Book "Index files", verified live against
//!   index.crates.io) and the npm packument endpoint (verified live
//!   against registry.npmjs.org, including unencoded scoped names);
//! * **§13 step 3 visibility wait** — deterministic backoff (no jitter,
//!   the ADR-0015 I3-deviation convention), bounded by the policy, with
//!   the sleep sequence observable.

use std::sync::Arc;
use std::time::Duration;

use halyard_forge::transport::{HttpRequest, HttpResponse, MemoryTransport};
use halyard_registry::{
    CratesIo, Npm, PyPi, Registry, RegistrySleeper, VisibilityPolicy, wait_until_visible,
};

type BoxError = Box<dyn std::error::Error>;

fn response(status: u16, body: &str) -> HttpResponse {
    HttpResponse {
        status,
        headers: Vec::new(),
        body: body.to_owned(),
    }
}

/// A sleeper that records the requested delays (the observable the
/// backoff tests assert against).
#[derive(Default)]
struct RecordedSleeper {
    delays_ms: std::sync::Mutex<Vec<u128>>,
}

impl RecordedSleeper {
    fn recorded_ms(&self) -> Vec<u128> {
        self.delays_ms
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

impl RegistrySleeper for RecordedSleeper {
    fn sleep(&self, delay: Duration) {
        self.delays_ms
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(delay.as_millis());
    }
}

#[test]
fn sparse_index_paths_follow_the_verified_prefix_scheme() -> Result<(), BoxError> {
    // Cargo Book "Index files", verified live against index.crates.io
    // (serde served at se/rd/serde, log at 3/l/log, missing crate 404).
    let cases = [
        ("a", "1/a"),
        ("ab", "2/ab"),
        ("log", "3/l/log"),
        ("cargo", "ca/rg/cargo"),
        ("serde", "se/rd/serde"),
        ("halyard", "ha/ly/halyard"),
    ];
    for (name, path) in cases {
        assert_eq!(CratesIo::index_path(name)?, path, "prefix rule for {name}");
    }
    Ok(())
}

#[test]
fn sparse_index_paths_fail_closed_on_invalid_names() -> Result<(), BoxError> {
    let invalid = ["", "serde/", "se/r/d", "has space", "serde.crate"];
    for name in invalid {
        let error = CratesIo::index_path(name).expect_err("an invalid crate name must fail closed");
        assert_eq!(error.code(), "E-REG-NAME", "{name:?}");
    }
    // The index filename is lowercased (Cargo Book: "the index filenames
    // are in lowercase") — an uppercase name probes the lowercase path.
    assert_eq!(CratesIo::index_path("Serde")?, "se/rd/serde");
    Ok(())
}

#[test]
fn cratesio_probe_reads_the_version_lines() -> Result<(), BoxError> {
    let transport = MemoryTransport::new();
    transport.enqueue(response(
        200,
        concat!(
            r#"{"name":"serde","vers":"0.0.0","yanked":false}"#,
            "\n",
            r#"{"name":"serde","vers":"1.0.210","yanked":false}"#,
            "\n",
        ),
    ));
    let registry = CratesIo::with_index_base(Arc::new(transport.clone()), "https://index.test");
    assert!(registry.published("serde", "1.0.210")?);
    let sent: &[HttpRequest] = &transport.sent();
    assert_eq!(sent.len(), 1);
    let sent = sent.first().unwrap_or_else(|| unreachable!("len == 1"));
    assert_eq!(sent.method, "GET");
    assert_eq!(sent.url, "https://index.test/se/rd/serde");
    Ok(())
}

#[test]
fn cratesio_probe_404_means_not_published() -> Result<(), BoxError> {
    // §13 step 2: the sparse index 404s a package that was never
    // published (verified live) — the resume path reads it as "not
    // published yet", not an error.
    let transport = MemoryTransport::new();
    transport.enqueue(response(404, ""));
    let registry = CratesIo::with_index_base(Arc::new(transport), "https://index.test");
    assert!(!registry.published("zzzzz-not-a-crate", "0.1.0")?);
    Ok(())
}

#[test]
fn cratesio_probe_absent_version_row_is_not_published() -> Result<(), BoxError> {
    // The package exists but the requested version is not among the
    // rows: not published (the visibility-wait continuation state).
    let transport = MemoryTransport::new();
    transport.enqueue(response(200, r#"{"name":"serde","vers":"0.0.0"}"#));
    let registry = CratesIo::with_index_base(Arc::new(transport), "https://index.test");
    assert!(!registry.published("serde", "1.0.210")?);
    Ok(())
}

#[test]
fn cratesio_probe_garbage_lines_fail_closed() {
    // A foreign body (HTML error page, truncated JSON) is never read as
    // "not published" — that would double-publish (§13). Fail closed.
    let transport = MemoryTransport::new();
    transport.enqueue(response(200, "<html>registry maintenance</html>"));
    let registry = CratesIo::with_index_base(Arc::new(transport), "https://index.test");
    let error = registry
        .published("serde", "1.0.210")
        .expect_err("garbage body");
    assert_eq!(error.code(), "E-REG-PROTOCOL");
}

#[test]
fn cratesio_probe_transport_failure_is_io() {
    let transport = MemoryTransport::new();
    transport.enqueue_failure(halyard_forge::ForgeError::Transport {
        reason: "connection refused".to_owned(),
    });
    let registry = CratesIo::with_index_base(Arc::new(transport), "https://index.test");
    let error = registry
        .published("serde", "1.0.210")
        .expect_err("transport failure");
    assert_eq!(error.code(), "E-REG-IO");
}

#[test]
fn npm_probe_reads_the_packument_versions_map() -> Result<(), BoxError> {
    // The packument shape verified live: `versions` maps version
    // strings to objects; scoped names ride the URL unencoded.
    let transport = MemoryTransport::new();
    let packument = r#"{"_id":"@angular/common","dist-tags":{"latest":"19.0.0"},"versions":{"18.1.0":{"name":"@angular/common"},"19.0.0":{"name":"@angular/common"}}}"#;
    transport.enqueue(response(200, packument));
    transport.enqueue(response(200, packument));
    let registry = Npm::with_registry_base(Arc::new(transport.clone()), "https://registry.test");
    assert!(registry.published("@angular/common", "19.0.0")?);
    let sent: &[HttpRequest] = &transport.sent();
    assert_eq!(
        sent.first().map(|r| r.url.as_str()),
        Some("https://registry.test/@angular/common")
    );
    assert!(!registry.published("@angular/common", "20.0.0")?);
    Ok(())
}

#[test]
fn npm_probe_404_means_not_published_and_garbage_fails_closed() -> Result<(), BoxError> {
    let transport = MemoryTransport::new();
    transport.enqueue(response(404, "Not Found"));
    transport.enqueue(response(200, "gateway timeout body, not json"));
    let registry = Npm::with_registry_base(Arc::new(transport), "https://registry.test");
    assert!(!registry.published("definitely-not-a-package", "1.0.0")?);
    let error = registry
        .published("definitely-not-a-package", "1.0.0")
        .expect_err("unparseable packument");
    assert_eq!(error.code(), "E-REG-PROTOCOL");
    Ok(())
}

#[test]
fn npm_probe_rejects_invalid_names() {
    let registry =
        Npm::with_registry_base(Arc::new(MemoryTransport::new()), "https://registry.test");
    for name in ["", "@scope-only", "has space", "a/b/c"] {
        let error = registry
            .published(name, "1.0.0")
            .expect_err("invalid npm name");
        assert_eq!(error.code(), "E-REG-NAME", "{name:?}");
    }
}

#[test]
fn visibility_wait_polls_with_deterministic_backoff() -> Result<(), BoxError> {
    // §13 step 3: poll until the version appears; the backoff doubles
    // from the base delay, capped at the max — no jitter (I3, the
    // ADR-0015 deviation convention). The observed sequence:
    // miss → 20 ms → hit, so exactly one sleep happened.
    let transport = MemoryTransport::new();
    transport.enqueue(response(404, ""));
    transport.enqueue(response(200, r#"{"name":"log","vers":"0.4.0"}"#));
    let registry = CratesIo::with_index_base(Arc::new(transport), "https://index.test");
    let sleeper = RecordedSleeper::default();
    let policy = VisibilityPolicy {
        max_polls: 4,
        base_delay: Duration::from_millis(20),
        max_delay: Duration::from_millis(160),
    };
    assert!(wait_until_visible(
        &registry, "log", "0.4.0", &policy, &sleeper
    )?);
    assert_eq!(sleeper.recorded_ms(), vec![20]);
    Ok(())
}

#[test]
fn visibility_wait_exhaustion_reports_not_visible() -> Result<(), BoxError> {
    // The wait is bounded: max_polls misses (each sleeping the capped
    // backoff) report Ok(false) — the publisher (M5.2) renders the skip;
    // nothing panics, nothing loops forever.
    let transport = MemoryTransport::new();
    for _ in 0..3 {
        transport.enqueue(response(404, ""));
    }
    let registry = CratesIo::with_index_base(Arc::new(transport), "https://index.test");
    let sleeper = RecordedSleeper::default();
    let policy = VisibilityPolicy {
        max_polls: 3,
        base_delay: Duration::from_millis(20),
        max_delay: Duration::from_millis(40),
    };
    assert!(!wait_until_visible(
        &registry, "log", "0.4.0", &policy, &sleeper
    )?);
    // Three probes, two sleeps between them: base doubling, capped.
    assert_eq!(sleeper.recorded_ms(), vec![20, 40]);
    Ok(())
}

// ---------------------------------------------------------------------------
// M8 step 2 (ADR-0056): the PyPI JSON-API probe — the same §13 step-2
// idempotent-resume contract at the PyPI grammar (VERIFIED live:
// `GET pypi.org/pypi/{name}/json` answers a `releases` map keyed by
// version; the per-version endpoint answers 200/404).
// ---------------------------------------------------------------------------

#[test]
fn pypi_probe_404_is_not_published() -> Result<(), BoxError> {
    let transport = MemoryTransport::new();
    transport.enqueue(response(404, "Not Found"));
    let registry = PyPi::with_registry_base(Arc::new(transport), "https://pypi.test");
    assert!(!registry.published("halyard-nope", "1.0.0")?);
    Ok(())
}

#[test]
fn pypi_probe_release_row_is_published() -> Result<(), BoxError> {
    let transport = MemoryTransport::new();
    transport.enqueue(response(
        200,
        r#"{"info": {"name": "uv"}, "releases": {"0.13.9": [{"url": "..."}]}}"#,
    ));
    let registry = PyPi::with_registry_base(Arc::new(transport), "https://pypi.test");
    assert!(registry.published("uv", "0.13.9")?);
    Ok(())
}

#[test]
fn pypi_probe_missing_row_is_not_published() -> Result<(), BoxError> {
    let transport = MemoryTransport::new();
    transport.enqueue(response(
        200,
        r#"{"info": {"name": "uv"}, "releases": {"0.13.9": []}}"#,
    ));
    let registry = PyPi::with_registry_base(Arc::new(transport), "https://pypi.test");
    assert!(!registry.published("uv", "0.14.0")?);
    Ok(())
}

#[test]
fn pypi_probe_url_grammar_is_the_json_api() -> Result<(), BoxError> {
    let transport = MemoryTransport::new();
    let registry = PyPi::with_registry_base(Arc::new(transport), "https://pypi.test");
    assert_eq!(registry.probe_url("uv")?, "https://pypi.test/pypi/uv/json");
    // Normalized names ride the URL verbatim (the PyPA identity uv and
    // PyPI match on); a name outside the grammar never reaches the wire.
    let transport = MemoryTransport::new();
    let registry = PyPi::with_registry_base(Arc::new(transport), "https://pypi.test");
    let error = registry.probe_url("bad name").expect_err("invalid name");
    assert_eq!(error.code(), "E-REG-NAME");
    Ok(())
}

#[test]
fn pypi_probe_non_json_answer_fails_closed() {
    let transport = MemoryTransport::new();
    transport.enqueue(response(200, "<html>502</html>"));
    let registry = PyPi::with_registry_base(Arc::new(transport), "https://pypi.test");
    let error = registry.published("uv", "0.13.9").expect_err("must fail");
    assert_eq!(error.code(), "E-REG-PROTOCOL");
}

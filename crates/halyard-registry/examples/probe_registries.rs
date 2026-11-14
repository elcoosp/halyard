//! The registry existence-probe seam (M5.1, §13) over a scripted
//! transport: `published()` against a crates.io sparse-index file and
//! an npm packument, plus the §13 step-3 visibility wait with its
//! observable backoff. No network is touched (I5) — the scripted
//! `MemoryTransport` stands in for the registries; the live doctor
//! probe (`halyard doctor --registries`) rides the same seam over the
//! blocking transport.

use std::sync::Arc;
use std::time::Duration;

use halyard_forge::transport::{HttpResponse, MemoryTransport};
use halyard_registry::{
    CratesIo, Npm, Registry, RegistrySleeper, VisibilityPolicy, wait_until_visible,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // The crates.io sparse-index file for `serde` (one JSON line per
    // version, `vers` carries the SemVer — verified live, ADR-0017).
    // Two copies: each probe consumes one scripted response.
    let sparse_body = concat!(
        r#"{"name":"serde","vers":"0.0.0","yanked":false}"#,
        "\n",
        r#"{"name":"serde","vers":"1.0.210","yanked":false}"#,
        "\n",
    );
    let sparse = MemoryTransport::new();
    for _ in 0..2 {
        sparse.enqueue(HttpResponse {
            status: 200,
            headers: Vec::new(),
            body: sparse_body.to_owned(),
        });
    }
    let crates_io = CratesIo::with_index_base(Arc::new(sparse), "https://index.test");
    println!(
        "crates.io serde@1.0.210 published: {}",
        crates_io.published("serde", "1.0.210")?
    );
    println!(
        "crates.io serde@9.9.9 published: {}",
        crates_io.published("serde", "9.9.9")?
    );

    // The npm packument (versions map; scoped names ride unencoded —
    // verified live, ADR-0017).
    let npm_transport = MemoryTransport::new();
    npm_transport.enqueue(HttpResponse {
        status: 200,
        headers: Vec::new(),
        body: r#"{"_id":"@angular/common","versions":{"18.1.0":{},"19.0.0":{}}}"#.to_owned(),
    });
    let npm = Npm::with_registry_base(Arc::new(npm_transport), "https://registry.test");
    println!(
        "npm @angular/common@19.0.0 published: {}",
        npm.published("@angular/common", "19.0.0")?
    );

    // The §13 step-3 visibility wait: poll a scripted registry until
    // the version appears; the sleeper records the deterministic
    // doubling-capped backoff (no jitter — I3).
    let transport = MemoryTransport::new();
    transport.enqueue(HttpResponse {
        status: 404,
        headers: Vec::new(),
        body: String::new(),
    });
    transport.enqueue(HttpResponse {
        status: 200,
        headers: Vec::new(),
        body: r#"{"name":"log","vers":"0.4.0"}"#.to_owned(),
    });
    let registry = CratesIo::with_index_base(Arc::new(transport), "https://index.test");
    let sleeper = RecordingSleeper::default();
    let policy = VisibilityPolicy {
        max_polls: 4,
        base_delay: Duration::from_millis(20),
        max_delay: Duration::from_millis(160),
    };
    let visible = wait_until_visible(&registry, "log", "0.4.0", &policy, &sleeper)?;
    println!("visibility wait: log@0.4.0 visible after wait = {visible}");
    println!(
        "backoff sleeps (ms): {:?}",
        sleeper
            .delays_ms
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    );

    println!(
        "\nlive probe: halyard doctor --registries (network only by the flag, I5) — or \
         inject the UreqTransport from halyard-forge-github into CratesIo::new / Npm::new"
    );
    Ok(())
}

/// A sleeper that records the requested delays.
#[derive(Default)]
struct RecordingSleeper {
    delays_ms: std::sync::Mutex<Vec<u128>>,
}

impl RegistrySleeper for RecordingSleeper {
    fn sleep(&self, delay: Duration) {
        self.delays_ms
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(delay.as_millis());
    }
}

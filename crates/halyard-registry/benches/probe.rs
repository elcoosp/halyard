//! Registry existence-probe benchmark (M5.1, §13): the CPU cost of one
//! probe is the URL composition plus the body parse — the network round
//! trip dominates in production, so the CPU shapes must stay trivially
//! cheap. Benches the sparse-index line parse (the serde-shaped index
//! file, five versions) and the packument parse (the npm document with
//! a hundred versions) over the scripted transport.

use std::fmt::Write as _;
use std::sync::Arc;

use criterion::{BatchSize, Criterion};
use halyard_forge::transport::{HttpResponse, MemoryTransport};
use halyard_registry::{CratesIo, Npm, Registry};

/// Five serde-shaped index lines (the §13 step-2 response body shape).
const SPARSE: &str = concat!(
    r#"{"name":"serde","vers":"0.0.0","deps":[],"cksum":"d1bb","features":{},"yanked":false}"#,
    "\n",
    r#"{"name":"serde","vers":"0.2.0","deps":[],"cksum":"aaaa","features":{},"yanked":false}"#,
    "\n",
    r#"{"name":"serde","vers":"1.0.0","deps":[],"cksum":"bbbb","features":{},"yanked":false}"#,
    "\n",
    r#"{"name":"serde","vers":"1.0.100","deps":[],"cksum":"cccc","features":{},"yanked":false}"#,
    "\n",
    r#"{"name":"serde","vers":"1.0.210","deps":[],"cksum":"dddd","features":{},"yanked":false}"#,
    "\n",
);

/// Builds the hundred-version packument body (the §13 npm response
/// shape) once per bench setup.
fn packument_body() -> String {
    let mut versions = String::from("{\"_id\":\"react\",\"versions\":{");
    for minor in 0..100u32 {
        if minor > 0 {
            versions.push(',');
        }
        let _ = write!(versions, "\"0.{minor}.0\":{{\"name\":\"react\"}}");
    }
    versions.push_str("}}");
    versions
}

fn response(body: &str) -> HttpResponse {
    HttpResponse {
        status: 200,
        headers: Vec::new(),
        body: body.to_owned(),
    }
}

fn bench_registry_probes(c: &mut Criterion) {
    // One scripted response per iteration (`iter_batched` re-enqueues
    // in the setup): the bench measures probe + parse, with the
    // transport's request log growing in-place.
    let sparse = MemoryTransport::new();
    let crates_io = CratesIo::with_index_base(Arc::new(sparse.clone()), "https://index.test");
    c.bench_function("registry_cratesio_probe_parse", |b| {
        b.iter_batched(
            || sparse.enqueue(response(SPARSE)),
            |()| crates_io.published("serde", "1.0.210"),
            BatchSize::SmallInput,
        );
    });

    let packument = packument_body();
    let npm_transport = MemoryTransport::new();
    let npm = Npm::with_registry_base(Arc::new(npm_transport.clone()), "https://registry.test");
    c.bench_function("registry_npm_probe_parse", |b| {
        b.iter_batched(
            || npm_transport.enqueue(response(&packument)),
            |()| npm.published("react", "0.42.0"),
            BatchSize::SmallInput,
        );
    });
}

fn main() {
    let mut criterion = Criterion::default();
    bench_registry_probes(&mut criterion);
}

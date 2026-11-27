//! Bench: the docker adapter's discovery slice (M1 of the §17
//! `Docker image tags` cell, ADR-0063). Measures the Dockerfile lexer's
//! comment/continuation strip + the LABEL row scan over a canonical
//! image manifest — the discovery-path cost per Dockerfile (the planner
//! re-runs discovery on the same bytes at apply time, so the scan stays
//! pure and cheap).

use criterion::Criterion;
use halyard_core::EcosystemAdapter;
use halyard_core::discover::DiscoverCtx;
use halyard_eco_docker::DockerAdapter;

/// The Dockerfile fixture: the OCI pair set inside the realistic
/// instruction mix (comments, a multi-stage FROM, RUN rows, a multi-pair
/// LABEL with a continuation).
const DOCKERFILE: &str = concat!(
    "# syntax=docker/dockerfile:1\n",
    "FROM alpine:3.20 AS build\n",
    "RUN make all \\\n",
    "    --jobs 4\n",
    "FROM alpine:3.20\n",
    "COPY --from=build /app /app\n",
    "LABEL org.opencontainers.image.title=\"squid-image\" \\\n",
    "      org.opencontainers.image.version=\"1.4.2\" \\\n",
    "      org.opencontainers.image.source=\"https://example.dev/repo\"\n",
    "CMD [\"/app/squid\"]\n",
);

/// One scan of one Dockerfile (the discovery path's own unit).
/// The bench fixture failure exits loudly instead of panicking (the
/// sibling bench posture).
fn fail(reason: &str) -> ! {
    eprintln!("the fixture must {reason}");
    std::process::exit(101);
}

fn bench(c: &mut Criterion) {
    let mut ctx = DiscoverCtx::new();
    if !ctx.insert_file("Dockerfile", DOCKERFILE.as_bytes().to_vec()) {
        fail("carry the fixture file");
    }
    c.bench_function("docker_scan_dockerfile", |b| {
        b.iter(|| {
            let rows = DockerAdapter::new()
                .discover(&ctx)
                .unwrap_or_else(|_| fail("scan the fixture"));
            assert_eq!(rows.len(), 1);
        });
    });
}

/// The bench entry point (the criterion macros generate undocumented
/// items — the workspace's missing-docs gate forbids them; the sibling
/// benches call the group directly).
fn main() {
    bench(&mut Criterion::default().configure_from_args());
}

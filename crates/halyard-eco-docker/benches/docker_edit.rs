//! Bench: the docker adapter's edit half — the semantic in-place OCI
//! version-label patch (M2 of the §17 `Docker image tags` cell,
//! ADR-0063). Measures the re-scan + span-match + splice + read-back
//! verify over a canonical Dockerfile.

use criterion::Criterion;
use halyard_core::EcosystemAdapter;
use halyard_core::package::RelPath;
use halyard_core::plan::PlanPatch;
use halyard_eco_docker::DockerAdapter;

/// The Dockerfile fixture: the OCI pair set inside the realistic
/// instruction mix (the discovery bench's shape).
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

/// The version label's value span in the fixture (the discovery scan
/// pins it; the edit aims the patch there).
const VALUE_TEXT: &str = "1.4.2";

/// The bench fixture failure exits loudly instead of panicking (the
/// sibling bench posture).
fn fail(reason: &str) -> ! {
    eprintln!("the fixture must {reason}");
    std::process::exit(101);
}

fn bench(c: &mut Criterion) {
    // The patch aims at the label's value span (the span discovery
    // emits — located by scanning, pinned by the edit contract tests).
    let start = DOCKERFILE
        .find(VALUE_TEXT)
        .unwrap_or_else(|| fail("carry the version value"));
    let end = start + VALUE_TEXT.len();
    let file = RelPath::new("Dockerfile").unwrap_or_else(|| fail("carry the fixture path"));
    let patches = [PlanPatch {
        start: u32::try_from(start).unwrap_or_else(|_| fail("carry the span")),
        end: u32::try_from(end).unwrap_or_else(|_| fail("carry the span")),
        text: "1.5.0".to_owned(),
    }];
    let adapter = DockerAdapter::new();
    c.bench_function("docker_edit_version_label", |b| {
        b.iter(|| {
            let outcome = adapter
                .edit_versions(&file, DOCKERFILE.as_bytes(), &patches)
                .unwrap_or_else(|_| fail("apply the patch"));
            assert_eq!(outcome.applied, 1);
        });
    });
}

/// The bench entry point (the criterion macros generate undocumented
/// items — the workspace's missing-docs gate forbids them; the sibling
/// benches call the group directly).
fn main() {
    bench(&mut Criterion::default().configure_from_args());
}

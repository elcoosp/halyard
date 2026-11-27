//! Example: apply the docker adapter's semantic edit seam (M2 of the
//! §17 **`Docker image tags`** cell — ADR-0063).
//!
//! Run: `cargo run -p halyard-eco-docker --example bump_docker`
//!
//! The plan's own-version byte patches re-apply **semantically and in
//! place**: the patch span must land on the LAST
//! `org.opencontainers.image.version` label's value span (Docker's own
//! override rule — *"the most-recently-applied value overrides any
//! previously-set value"* — makes the last row the site; a drifted file
//! refuses `E-EDIT-PATCHES`), the label value's inner bytes swap and
//! every other byte survives verbatim (invariant I4), and a re-scan
//! read-back verifies the file now carries the target before the patch
//! is accepted. A patch whose text already equals the current value is
//! the idempotent re-run (I1).

use halyard_core::EcosystemAdapter;
use halyard_core::package::RelPath;
use halyard_core::plan::PlanPatch;
use halyard_eco_docker::DockerAdapter;

/// The Dockerfile fixture: the OCI pair set inside the realistic
/// instruction mix — the continuation, the sibling source label, the
/// multi-stage `FROM` rows and the comments are the bytes the swap must
/// preserve.
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

/// The fixture offset as a plan-contract span coordinate (loud,
/// panic-free failure for a constant fixture that cannot miss).
fn span_of(haystack: &str, needle: &str) -> (u32, u32) {
    let start = haystack
        .find(needle)
        .and_then(|at| u32::try_from(at).ok())
        .unwrap_or_else(|| {
            eprintln!("the fixture must carry {needle}");
            std::process::exit(101);
        });
    let length = u32::try_from(needle.len()).unwrap_or_else(|_| {
        eprintln!("the fixture needle must fit the span coordinate range");
        std::process::exit(101);
    });
    (start, start + length)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let file = RelPath::new("Dockerfile").ok_or("the fixture path is normalized")?;
    let adapter = DockerAdapter::new();

    // 1. The plan's patch: the discovery scan's span (the OCI version
    //    label's value), the target from the plan row.
    let (start, end) = span_of(DOCKERFILE, "1.4.2");
    let patches = [PlanPatch {
        start,
        end,
        text: "1.5.0".to_owned(),
    }];
    let outcome = adapter.edit_versions(&file, DOCKERFILE.as_bytes(), &patches)?;
    println!(
        "applied {} patch (skipped {}); the Dockerfile now carries:",
        outcome.applied, outcome.skipped
    );
    let bumped = String::from_utf8(outcome.content)?;
    for line in bumped.lines().filter(|line| line.contains("image.version")) {
        println!("  {line}");
    }

    // 2. The idempotent re-run (I1): the same patch aimed at the file
    //    that already carries the target — matched, skipped, verbatim.
    let (start, end) = span_of(&bumped, "1.5.0");
    let rerun = [PlanPatch {
        start,
        end,
        text: "1.5.0".to_owned(),
    }];
    let again = adapter.edit_versions(&file, bumped.as_bytes(), &rerun)?;
    println!(
        "idempotent re-run: applied {}, skipped {} — bytes verbatim: {}",
        again.applied,
        again.skipped,
        again.content == bumped.as_bytes()
    );

    // 3. The drifted span refuses (`E-EDIT-PATCHES`) — one byte right of
    //    the value is no editable row; the bytes survive untouched.
    let drifted = [PlanPatch {
        start: start + 1,
        end: end + 1,
        text: "1.6.0".to_owned(),
    }];
    let refusal = adapter.edit_versions(&file, bumped.as_bytes(), &drifted);
    match refusal {
        Err(failure) => println!("drifted span refuses: {failure}"),
        Ok(_) => println!("unexpected: the drifted span applied"),
    }
    Ok(())
}

//! The M8 `Docker image tags` cell's edit contract (ADR-0063's release
//! half): the plan's own-version byte patches re-apply **semantically
//! and in place** — each patch span must land on the LAST
//! `org.opencontainers.image.version` label's value span (the site the
//! discovery row emitted — the OCI annotations table's *"version of the
//! packaged software"*; Docker's own *"the most-recently-applied value
//! overrides any previously-set value"* makes the last row the site),
//! the value's inner bytes swap and every other byte survives verbatim
//! (invariant I4) — the quotes, the sibling labels, the `FROM` rows, the
//! comments — and a re-scan read-back verifies the file now carries the
//! target before the patch is accepted.
//!
//! A patch aimed at any other span refuses (`E-EDIT-PATCHES`); a
//! read-back that does not carry the target refuses (`E-EDIT-VERIFY`
//! posture); a patch whose text already equals the current value is the
//! idempotent re-run (I1): matched, skipped, bytes verbatim. A
//! Dockerfile with no version label (absent or non-canonical — the
//! fail-closed discipline) has no slot: a patch aimed at its manifest
//! refuses.

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

/// The fixture offset as a plan-contract span coordinate (`None` cannot
/// happen for a constant fixture, but the failure must stay loud and
/// panic-free — the sibling bench posture).
fn usize_to_u32(value: Option<usize>, needle: &str) -> u32 {
    if let Some(value) = value.and_then(|value| u32::try_from(value).ok()) {
        return value;
    }
    eprintln!("the fixture must carry {needle}");
    std::process::exit(101);
}

fn patch(haystack: &str, needle: &str, text: &str) -> PlanPatch {
    let start = usize_to_u32(haystack.find(needle), needle);
    let end = start + usize_to_u32(Some(needle.len()), needle);
    PlanPatch {
        start,
        end,
        text: text.to_owned(),
    }
}

/// The label value swaps in place and every other byte survives
/// verbatim (I4) — the quotes, the title/source labels, the continuation
/// and the `FROM` rows.
#[test]
fn swaps_the_label_value_and_preserves_the_markup() {
    let file = RelPath::new("Dockerfile").expect("fixture path");
    let adapter = DockerAdapter::new();
    let patches = [patch(DOCKERFILE, "1.4.2", "1.5.0")];
    let out = adapter
        .edit_versions(&file, DOCKERFILE.as_bytes(), &patches)
        .expect("applies");
    assert_eq!((out.applied, out.skipped), (1, 0));
    assert_eq!(
        String::from_utf8(out.content).expect("valid UTF-8"),
        DOCKERFILE.replace("1.4.2", "1.5.0"),
        "the markup around the label value survives verbatim (I4)"
    );
}

/// The LAST version label is the site (Docker's documented override
/// rule): with two rows the patch lands on the later one; an earlier
/// row's span refuses.
#[test]
fn the_last_version_label_is_the_edit_site() {
    let body = concat!(
        "FROM alpine\n",
        "LABEL org.opencontainers.image.version=\"1.0.0\"\n",
        "LABEL org.opencontainers.image.version=\"1.1.0\"\n",
    );
    let file = RelPath::new("Dockerfile").expect("fixture path");
    let adapter = DockerAdapter::new();
    // The last row's span: the patch applies.
    let last = patch(body, "1.1.0", "1.2.0");
    let out = adapter
        .edit_versions(&file, body.as_bytes(), &[last])
        .expect("the last row is the site");
    assert_eq!(
        String::from_utf8(out.content).expect("valid UTF-8"),
        body.replace("1.1.0", "1.2.0"),
    );
    // The superseded row's span: no editable row there.
    let superseded = patch(body, "1.0.0", "1.2.0");
    let error = adapter
        .edit_versions(&file, body.as_bytes(), &[superseded])
        .expect_err("the superseded row is not the site");
    assert_eq!(error.code(), "E-EDIT-PATCHES");
}

/// A patch whose span is not the version label's value span refuses
/// (`E-EDIT-PATCHES`) — the working tree drifted from the file the plan
/// was computed against; nothing is mutated.
#[test]
fn a_patch_off_the_label_span_refuses() {
    let file = RelPath::new("Dockerfile").expect("fixture path");
    let adapter = DockerAdapter::new();
    // One byte right of the value: no editable row there.
    let start = usize_to_u32(DOCKERFILE.find("1.4.2"), "1.4.2") + 1;
    let end = start + usize_to_u32(Some("1.4.2".len()), "1.4.2");
    let patches = [PlanPatch {
        start,
        end,
        text: "1.5.0".to_owned(),
    }];
    let error = adapter
        .edit_versions(&file, DOCKERFILE.as_bytes(), &patches)
        .expect_err("the drifted span refuses");
    assert_eq!(error.code(), "E-EDIT-PATCHES");
    assert!(
        DOCKERFILE.contains("1.4.2"),
        "nothing was mutated on refusal"
    );
}

/// A patch whose text already equals the current value is the
/// idempotent re-run (I1): matched, skipped, bytes verbatim.
#[test]
fn the_idempotent_rerun_skips_the_carried_target() {
    let file = RelPath::new("Dockerfile").expect("fixture path");
    let adapter = DockerAdapter::new();
    let patches = [patch(DOCKERFILE, "1.4.2", "1.4.2")];
    let out = adapter
        .edit_versions(&file, DOCKERFILE.as_bytes(), &patches)
        .expect("applies");
    assert_eq!((out.applied, out.skipped), (0, 1));
    assert_eq!(
        String::from_utf8(out.content).expect("valid UTF-8"),
        DOCKERFILE,
        "bytes verbatim on the idempotent re-run (I1)"
    );
}

/// The read-back verify: a target the Dockerfile's own shape cannot
/// carry refuses after the would-be swap — the pinned read-back re-scan
/// must re-find the label carrying the target; a target that breaks the
/// scanned shape (a quote ending the value early) is refused before
/// acceptance (`E-EDIT-VERIFY` posture).
#[test]
fn an_uncarrable_target_refuses_verify() {
    let file = RelPath::new("Dockerfile").expect("fixture path");
    let adapter = DockerAdapter::new();
    // The scanner's pin: the label pair survives the swap with its key
    // and value. A quote inside the target ends the value early — the
    // re-scanned label would carry `1.5.0`, not the injected text.
    let patches = [patch(DOCKERFILE, "1.4.2", "1.5.0\" extra=\"junk")];
    let error = adapter
        .edit_versions(&file, DOCKERFILE.as_bytes(), &patches)
        .expect_err("the shape-breaking target refuses");
    assert_eq!(error.code(), "E-EDIT-VERIFY");
    assert!(DOCKERFILE.contains("1.4.2"), "nothing was mutated");
}

/// A Dockerfile with no version label row (the row vanished — the
/// working tree drifted) refuses (`E-EDIT-PATCHES`); nothing is mutated.
#[test]
fn a_dockerfile_without_a_version_label_refuses() {
    let drifted = "FROM alpine\nCMD [\"/app\"]\n";
    let file = RelPath::new("Dockerfile").expect("fixture path");
    let adapter = DockerAdapter::new();
    let patches = [patch(DOCKERFILE, "1.4.2", "1.5.0")];
    let error = adapter
        .edit_versions(&file, drifted.as_bytes(), &patches)
        .expect_err("the drifted file refuses");
    assert_eq!(error.code(), "E-EDIT-PATCHES");
}

/// A non-UTF-8 Dockerfile refuses at the parse gate (`E-EDIT-VERIFY` —
/// the `Parse` variant's code); nothing is mutated.
#[test]
fn a_non_utf8_dockerfile_refuses() {
    let file = RelPath::new("Dockerfile").expect("fixture path");
    let adapter = DockerAdapter::new();
    let patches = [patch(DOCKERFILE, "1.4.2", "1.5.0")];
    let error = adapter
        .edit_versions(&file, &[0xff, 0xfe], &patches)
        .expect_err("non-UTF-8 refuses");
    assert_eq!(error.code(), "E-EDIT-VERIFY");
}

/// The legitimate empty pass (nothing to apply): bytes verbatim,
/// applied 0, skipped 0 — the sibling adapters' posture.
#[test]
fn an_empty_pass_returns_the_bytes_verbatim() {
    let file = RelPath::new("Dockerfile").expect("fixture path");
    let adapter = DockerAdapter::new();
    let out = adapter
        .edit_versions(&file, DOCKERFILE.as_bytes(), &[])
        .expect("applies");
    assert_eq!((out.applied, out.skipped), (0, 0));
    assert_eq!(out.content, DOCKERFILE.as_bytes());
}

/// A length-changing swap legitimately moves the span: the read-back
/// verify pins the **text**, never the offset (the sibling adapters'
/// posture — the re-scan re-locates the label's value).
#[test]
fn a_length_changing_swap_relocates_the_verify() {
    let file = RelPath::new("Dockerfile").expect("fixture path");
    let adapter = DockerAdapter::new();
    let patches = [patch(DOCKERFILE, "1.4.2", "1.5.0-rc.1+build.7")];
    let out = adapter
        .edit_versions(&file, DOCKERFILE.as_bytes(), &patches)
        .expect("applies");
    assert_eq!((out.applied, out.skipped), (1, 0));
    let swapped = String::from_utf8(out.content).expect("valid UTF-8");
    assert!(
        swapped.contains("version=\"1.5.0-rc.1+build.7\""),
        "the length-changing swap lands: {swapped}"
    );
    // The byte count moved exactly by the length delta — nothing else
    // shifted (I4).
    assert_eq!(
        swapped.len(),
        DOCKERFILE.len() + "1.5.0-rc.1+build.7".len() - "1.4.2".len()
    );
}

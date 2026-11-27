//! The M8 `Docker image tags` cell's discovery contract
//! (`docs/HALYARD_ARCHITECTURE.md` §17 YEAR 2, M8 row — *"More
//! ecosystems"*, the `Docker image tags` cell — the next listed cell
//! after the shipped `generic` half (ADR-0062), gate *"each:
//! discover/edit/publish/resume tests + replay on 2 real repos"*;
//! VERIFIED grammar in `docs/research/docker-image-tags.md`).
//!
//! This is the plan's acceptance criterion pinned as code: a `Dockerfile`
//! discovers as a docker row — the version site is the OCI-documented
//! version annotation (`org.opencontainers.image.version`, *"version of
//! the packaged software"*, *"version MAY be Semantic
//! versioning-compatible"* — only semver-parseable spellings are
//! canonical per ADR-0056's discipline), carried by the Dockerfile
//! reference's `LABEL` instruction (*"The LABEL instruction adds
//! metadata to an image. A LABEL is a key-value pair."* — several pairs
//! may ride one instruction). A `FROM` row *"initializes a new build
//! stage and sets the base image"* — its tag is the **base** image's
//! version, never this package's, so it is consumed unmodeled and never
//! an edit site. Docker's own override rule (*"If a label already exists
//! but with a different value, the most-recently-applied value overrides
//! any previously-set value"*) makes the LAST version label the row's
//! site; superseded rows warn. The identity chain (ADR-0063 D2) is the
//! OCI title label → the containing directory (the documented build
//! context) → the manifest path; the row carries `publishable = false`
//! (no `docker build`/`docker push` argv arm exists — the engine's
//! disabled shape) and no dependency grammar.

use halyard_core::EcosystemAdapter;
use halyard_core::discover::{DiscoverCtx, DiscoverError, ManifestFormat, RawVersionSource};
use halyard_core::package::Ecosystem;
use halyard_eco_docker::DockerAdapter;

/// Builds a context from `(path, body)` fixture files.
fn ctx_with(files: &[(&str, &str)]) -> DiscoverCtx {
    let mut ctx = DiscoverCtx::new();
    for (path, body) in files {
        assert!(ctx.insert_file(path, body.as_bytes().to_vec()), "{path}");
    }
    ctx
}

/// The canonical root Dockerfile: an OCI title naming the image and an
/// OCI version label carrying a semver — the documented shapes.
const ROOT_DOCKERFILE: &str = "# syntax=docker/dockerfile:1\n\
FROM alpine:3.20\n\
LABEL org.opencontainers.image.title=\"squid-image\"\n\
LABEL org.opencontainers.image.version=\"1.2.3\"\n\
CMD [\"/app/squid\"]\n";

/// The canonical root: one `Dockerfile` with the OCI pair set.
#[test]
fn a_root_dockerfile_discovers_as_one_docker_row() -> Result<(), DiscoverError> {
    let ctx = ctx_with(&[("Dockerfile", ROOT_DOCKERFILE)]);
    let rows = DockerAdapter::new().discover(&ctx)?;
    assert_eq!(rows.len(), 1, "one Dockerfile is one row");
    let row = rows.first().expect("one row");
    assert_eq!(row.name, "squid-image", "the OCI title is the identity");
    assert_eq!(row.ecosystem, Ecosystem::Docker);
    assert_eq!(row.manifest, "Dockerfile");
    assert_eq!(row.root, "", "the root Dockerfile's context is the root");
    assert_eq!(
        row.version.as_ref().expect("canonical version").to_string(),
        "1.2.3"
    );
    assert_eq!(row.version_source, RawVersionSource::Local);
    let site = row
        .version_site
        .as_ref()
        .expect("the version label is a site");
    assert_eq!(site.file.as_str(), "Dockerfile");
    let span = site.spans.first().copied().expect("one value span");
    // The span covers the label's quoted value's inner bytes.
    let value = &ctx
        .file("Dockerfile")
        .expect("fixture bytes")
        .get(span.start as usize..span.end as usize)
        .expect("the fixture span is in bounds");
    assert_eq!(value, b"1.2.3", "the span is the label value's inner span");
    assert!(
        !row.publishable,
        "no docker build/push argv arm exists — the disabled shape"
    );
    assert!(row.deps.is_empty(), "no dependency grammar");
    assert!(row.tag_template.is_none() && row.changelog.is_none());
    Ok(())
}

/// The identity chain: a Dockerfile with no OCI title falls back to the
/// containing directory's name (the documented build context), and the
/// root Dockerfile falls back to its manifest path.
#[test]
fn the_identity_chain_falls_through_directory_then_path() -> Result<(), DiscoverError> {
    let ctx = ctx_with(&[
        (
            "Dockerfile",
            "FROM alpine\nLABEL org.opencontainers.image.version=\"1.0.0\"\n",
        ),
        (
            "services/api/Dockerfile",
            "FROM alpine\nLABEL org.opencontainers.image.version=\"2.0.0\"\n",
        ),
    ]);
    let mut rows = DockerAdapter::new().discover(&ctx)?;
    rows.sort_by(|a, b| a.name.cmp(&b.name));
    assert_eq!(rows.len(), 2, "each Dockerfile is one row");
    assert_eq!(
        rows.first().expect("a row").name,
        "Dockerfile",
        "a nameless root Dockerfile rides the manifest path"
    );
    assert_eq!(
        rows.last().expect("a row").name,
        "api",
        "a nameless nested Dockerfile rides the containing directory"
    );
    Ok(())
}

/// A base-image tag is never the version site: `FROM app:1.2.3` names
/// the base image (the reference's *"sets the base image"*), the OCI
/// version label is the row's version and site.
#[test]
fn a_from_tag_is_never_the_version_site() -> Result<(), DiscoverError> {
    let body = "FROM registry.internal/base:9.9.9\n\
LABEL org.opencontainers.image.version=\"0.3.0\"\n";
    let ctx = ctx_with(&[("Dockerfile", body)]);
    let rows = DockerAdapter::new().discover(&ctx)?;
    let row = rows.first().expect("one row");
    assert_eq!(
        row.version.as_ref().expect("the label wins").to_string(),
        "0.3.0",
        "the FROM tag is the base image's version, never this package's"
    );
    let site = row.version_site.as_ref().expect("the label is the site");
    let span = site.spans.first().copied().expect("one span");
    let value = &ctx
        .file("Dockerfile")
        .expect("fixture bytes")
        .get(span.start as usize..span.end as usize)
        .expect("the fixture span is in bounds");
    assert_eq!(
        value, b"0.3.0",
        "the edit site aims at the label, not the FROM tag"
    );
    Ok(())
}

/// Docker's documented override rule: a label *"already exists but with
/// a different value"* → *"the most-recently-applied value overrides any
/// previously-set value"* — the LAST version label is the site, the
/// superseded rows warn.
#[test]
fn the_last_version_label_overrides_and_earlier_rows_warn() -> Result<(), DiscoverError> {
    let body = "FROM alpine\n\
LABEL org.opencontainers.image.version=\"1.0.0\"\n\
LABEL org.opencontainers.image.version=\"1.1.0\"\n";
    let ctx = ctx_with(&[("Dockerfile", body)]);
    let rows = DockerAdapter::new().discover(&ctx)?;
    let row = rows.first().expect("one row");
    assert_eq!(
        row.version.as_ref().expect("the override wins").to_string(),
        "1.1.0",
        "the most-recently-applied value overrides"
    );
    assert!(
        row.warnings
            .iter()
            .any(|w| w.code == "superseded-version-label"),
        "the superseded row is observable: {:?}",
        row.warnings
    );
    Ok(())
}

/// A non-semver label value is recognized-but-non-canonical (the
/// ADR-0056 discipline): warned, no version, no edit site, never
/// rewritten.
#[test]
fn a_non_semver_label_value_is_warned_and_never_a_site() -> Result<(), DiscoverError> {
    let body = "FROM alpine\n\
LABEL org.opencontainers.image.version=\"1.2\"\n";
    let ctx = ctx_with(&[("Dockerfile", body)]);
    let rows = DockerAdapter::new().discover(&ctx)?;
    let row = rows.first().expect("one row");
    assert!(
        row.version.is_none(),
        "a two-component spelling never pairs"
    );
    assert!(row.version_site.is_none(), "no site — never rewritten");
    assert!(
        row.warnings
            .iter()
            .any(|w| w.code == "non-canonical-version"),
        "the spelling is observable: {:?}",
        row.warnings
    );
    Ok(())
}

/// A Dockerfile with no version label warns `no-version` and carries no
/// site (the package is treated as unpublished).
#[test]
fn a_dockerfile_without_a_version_label_warns_no_version() -> Result<(), DiscoverError> {
    let ctx = ctx_with(&[("Dockerfile", "FROM alpine\nCMD [\"/app\"]\n")]);
    let rows = DockerAdapter::new().discover(&ctx)?;
    let row = rows.first().expect("one row");
    assert!(row.version.is_none() && row.version_site.is_none());
    assert!(row.warnings.iter().any(|w| w.code == "no-version"));
    Ok(())
}

/// The scanner rides the documented format rules: instructions are not
/// case-sensitive, comments are removed, and a LABEL may carry several
/// `key=value` pairs with quoted values over line continuations.
#[test]
fn the_lexer_rides_the_documented_format_rules() -> Result<(), DiscoverError> {
    let body = "# a comment line\n\
from alpine as build\n\
RUN make all \\\n\
    --jobs 4\n\
# another comment\n\
LABEL org.opencontainers.image.title = \"pooled\" \\\n\
      org.opencontainers.image.version = \"2.3.0\"\n";
    let ctx = ctx_with(&[("Dockerfile", body)]);
    let rows = DockerAdapter::new().discover(&ctx)?;
    let row = rows.first().expect("one row");
    assert_eq!(
        row.name, "pooled",
        "lowercase instructions and continuations parse"
    );
    assert_eq!(
        row.version
            .as_ref()
            .expect("the pooled label wins")
            .to_string(),
        "2.3.0",
        "the label's value rides the multi-pair continued instruction"
    );
    Ok(())
}

/// A non-UTF-8 Dockerfile refuses (`E-DISC-MANIFEST` at the
/// `Dockerfile` format).
#[test]
fn a_non_utf8_dockerfile_refuses_manifest_format_dockerfile() {
    let mut ctx = DiscoverCtx::new();
    assert!(ctx.insert_file("Dockerfile", vec![0xff, 0xfe]));
    let error = DockerAdapter::new()
        .discover(&ctx)
        .expect_err("non-UTF-8 refuses");
    match error {
        DiscoverError::Manifest {
            file,
            format,
            message,
            ..
        } => {
            assert_eq!(file, "Dockerfile");
            assert_eq!(format, ManifestFormat::Dockerfile);
            assert!(
                message.contains("UTF-8"),
                "the refusal names the grammar: {message}"
            );
        }
        other => panic!("a manifest refusal: {other:?}"),
    }
}

/// The adapter is a per-file row builder (§6.1): colliding fallback
/// identities both emit, and the orchestrator's per-ecosystem duplicate
/// check (`E-DISC-DUPLICATE`, §6.3) is the refusal — pinned end-to-end
/// by `halyard-cli/tests/cli_docker.rs`.
#[test]
fn colliding_fallback_identities_emit_per_file() -> Result<(), DiscoverError> {
    let ctx = ctx_with(&[
        ("services/a/Dockerfile", "FROM alpine\n"),
        ("workers/a/Dockerfile", "FROM alpine\n"),
    ]);
    let rows = DockerAdapter::new().discover(&ctx)?;
    assert_eq!(rows.len(), 2, "each Dockerfile emits");
    assert_eq!(
        rows.first().expect("a row").name,
        rows.last().expect("a row").name,
        "the fallback identity collides — the orchestrator refuses"
    );
    Ok(())
}

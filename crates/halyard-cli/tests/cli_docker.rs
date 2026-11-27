//! CLI e2e over the M8 `Docker image tags` cell (ADR-0063): a
//! repository whose only package is a root `Dockerfile` carrying the
//! OCI-documented version annotation. The plan row carries the engine's
//! **disabled publish shape** (`publish.registry = "docker.io"` — the
//! documented default registry host — with `enabled: false` until a
//! `docker build`/`docker push` argv grammar exists), the edit set aims
//! the version patch at the label's value span, and the release row's
//! `ecosystem` is the `docker` label the plan contract resolves.

use std::process::Command;

use halyard_test_support::{CommitSpec, GitFixture};

type BoxError = Box<dyn std::error::Error>;

fn fixture_dir(label: &str) -> Result<std::path::PathBuf, BoxError> {
    let dir =
        std::env::temp_dir().join(format!("halyard-cli-docker-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// A docker repository: one root `Dockerfile` with the OCI title +
/// version pair, a feat commit driving the plan row. No other manifest
/// grammar exists in the tree — the Dockerfile cell is the only way this
/// repository is plannable (the cell's reason to exist).
fn build_docker_workspace(label: &str) -> Result<GitFixture, BoxError> {
    let dir = fixture_dir(label)?;
    let mut fx = GitFixture::init_in(&dir)?;
    fx.commit_with(&CommitSpec::files(
        vec![(
            "Dockerfile".to_owned(),
            concat!(
                "# syntax=docker/dockerfile:1\n",
                "FROM alpine:3.20\n",
                "LABEL org.opencontainers.image.title=\"squid-image\"\n",
                "LABEL org.opencontainers.image.version=\"1.0.0\"\n",
                "CMD [\"/app/squid\"]\n",
            )
            .to_owned(),
        )],
        "chore: scaffold the docker repository",
    ))?;
    fx.commit_with(&CommitSpec::files(
        vec![("docs/readme.md".to_owned(), "the app changes\n".to_owned())],
        "feat: the app changes",
    ))?;
    Ok(fx)
}

fn halyard(
    root: &std::path::Path,
    args: &[&str],
) -> Result<(std::process::Output, String), BoxError> {
    let out = Command::new(env!("CARGO_BIN_EXE_halyard"))
        .arg("-C")
        .arg(root)
        .args(args)
        .env("NO_COLOR", "1")
        .output()?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    Ok((out, text))
}

/// The plan over a Dockerfile-only repository: the docker release row
/// exists (the walk feeds the Dockerfile through discovery), the
/// ecosystem label is `docker`, the tag rides the single-package
/// default, and the publish decision carries the engine's **disabled
/// shape** (the documented default registry host `docker.io`, `enabled`
/// false until an argv arm lands).
#[test]
fn a_dockerfile_repository_plans_end_to_end() -> Result<(), BoxError> {
    let fx = build_docker_workspace("docker-plan")?;
    let (out, text) = halyard(fx.root(), &["plan", "--format", "json"])?;
    assert!(out.status.success(), "the docker repository plans: {text}");
    let value: serde_json::Value = serde_json::from_str(
        text.trim_start_matches(char::is_whitespace)
            .trim_start_matches(|c: char| c != '{'),
    )
    .map_err(|e| format!("the report is JSON: {e}\n{text}"))?;
    let releases = value
        .get("releases")
        .and_then(serde_json::Value::as_array)
        .ok_or("releases is an array")?;
    let row = releases
        .iter()
        .find(|row| row.get("package").and_then(serde_json::Value::as_str) == Some("squid-image"))
        .ok_or("the docker package releases")?;
    assert_eq!(
        row.get("ecosystem").and_then(serde_json::Value::as_str),
        Some("docker"),
        "the ecosystem label: {text}"
    );
    assert_eq!(
        row.get("from").and_then(serde_json::Value::as_str),
        Some("1.0.0"),
        "the OCI version label is the `from` version"
    );
    assert_eq!(
        row.get("to").and_then(serde_json::Value::as_str),
        Some("1.1.0"),
        "a feat bump is minor"
    );
    let publish = row
        .get("publish")
        .and_then(serde_json::Value::as_object)
        .ok_or("the disabled publish decision exists")?;
    assert_eq!(
        publish.get("registry").and_then(serde_json::Value::as_str),
        Some("docker.io"),
        "the documented default registry host: {text}"
    );
    assert_eq!(
        publish.get("enabled").and_then(serde_json::Value::as_bool),
        Some(false),
        "the disabled shape until an argv arm lands: {text}"
    );
    // The edit set: the version patch aims at the Dockerfile's label.
    let edits = value
        .get("edits")
        .and_then(serde_json::Value::as_array)
        .ok_or("edits is an array")?;
    let edit = edits
        .iter()
        .find(|edit| edit.get("file").and_then(serde_json::Value::as_str) == Some("Dockerfile"))
        .ok_or("the Dockerfile edit is planned")?;
    assert!(
        edit.get("patches")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|p| !p.is_empty()),
        "the label-value patch is planned: {text}"
    );
    Ok(())
}

/// `plan --dry`-shaped commands never touch the working tree (I5) — the
/// Dockerfile keeps its bytes.
#[test]
fn planning_never_mutates_the_dockerfile() -> Result<(), BoxError> {
    let fx = build_docker_workspace("docker-immutable")?;
    let (out, text) = halyard(fx.root(), &["plan"])?;
    assert!(out.status.success(), "the plan succeeds: {text}");
    let after = std::fs::read_to_string(fx.root().join("Dockerfile"))?;
    assert!(
        after.contains("org.opencontainers.image.version=\"1.0.0\""),
        "the Dockerfile keeps its bytes: {after}"
    );
    Ok(())
}

/// The dry forecast for a docker repository: zero publish targets, the
/// release recorded as a `publish_disabled` skip (the row's publish
/// decision carries the engine's disabled shape — `registry` names the
/// documented default registry host `docker.io`, `enabled` is false
/// until a `docker build`/`docker push` argv grammar lands), and a
/// successful offline run.
#[test]
fn publish_dry_records_disabled_for_a_docker_repository() -> Result<(), BoxError> {
    let fx = build_docker_workspace("docker-publish")?;
    let (out, text) = halyard(fx.root(), &["publish", "--dry", "--format", "json"])?;
    assert!(
        out.status.success(),
        "the docker forecast succeeded: {text}"
    );
    let value: serde_json::Value = serde_json::from_str(
        text.trim_start_matches(char::is_whitespace)
            .trim_start_matches(|c: char| c != '{'),
    )
    .map_err(|e| format!("the report is JSON: {e}\n{text}"))?;
    assert_eq!(
        value.get("targets").and_then(serde_json::Value::as_u64),
        Some(0),
        "zero publish targets: {text}"
    );
    let skips = value
        .get("not_targets")
        .and_then(serde_json::Value::as_array)
        .ok_or("not_targets is an array")?;
    let member = skips
        .iter()
        .find(|row| row.get("package").and_then(serde_json::Value::as_str) == Some("squid-image"))
        .ok_or("the docker release is recorded as a skip")?;
    assert_eq!(
        member.get("reason").and_then(serde_json::Value::as_str),
        Some("publish_disabled"),
        "the disabled-shape skip reason: {text}"
    );
    Ok(())
}

/// `halyard apply` rewrites the OCI version label in place: the label
/// value's inner bytes swap 1.0.0 → 1.1.0 and every other byte of the
/// Dockerfile is identical (I4) — the quotes, the title label, the
/// `FROM` rows, the `CMD`.
#[test]
fn apply_rewrites_the_version_label_of_a_dockerfile_repository() -> Result<(), BoxError> {
    let fx = build_docker_workspace("docker-apply")?;
    let (out, text) = halyard(fx.root(), &["apply"])?;
    assert!(out.status.success(), "apply failed: {text}");
    let dockerfile = std::fs::read_to_string(fx.root().join("Dockerfile"))?;
    assert!(
        dockerfile.contains("org.opencontainers.image.version=\"1.1.0\""),
        "the version moved: {dockerfile}"
    );
    assert!(
        dockerfile.contains("org.opencontainers.image.title=\"squid-image\"")
            && dockerfile.contains("FROM alpine:3.20")
            && dockerfile.contains("CMD [\"/app/squid\"]"),
        "the surrounding Dockerfile never moves: {dockerfile}"
    );
    Ok(())
}

/// Colliding fallback identities refuse through the orchestrator's
/// per-ecosystem duplicate check (`E-DISC-DUPLICATE`, §6.3): two
/// nameless Dockerfiles in same-named directories both resolve to the
/// containing directory's name, and the JSON envelope carries the stable
/// code.
#[test]
fn colliding_fallback_identities_refuse_duplicate() -> Result<(), BoxError> {
    let dir = fixture_dir("docker-duplicate")?;
    let mut fx = GitFixture::init_in(&dir)?;
    fx.commit_with(&CommitSpec::files(
        vec![
            (
                "services/a/Dockerfile".to_owned(),
                "FROM alpine\n".to_owned(),
            ),
            (
                "workers/a/Dockerfile".to_owned(),
                "FROM alpine\n".to_owned(),
            ),
        ],
        "chore: scaffold colliding contexts",
    ))?;
    let (out, text) = halyard(fx.root(), &["plan", "--format", "json"])?;
    assert!(
        !out.status.success(),
        "the colliding identities refuse: {text}"
    );
    let value: serde_json::Value = serde_json::from_str(
        text.trim_start_matches(char::is_whitespace)
            .trim_start_matches(|c: char| c != '{'),
    )
    .map_err(|e| format!("the error path is JSON: {e}\n{text}"))?;
    let code = value
        .get("error")
        .and_then(|error| error.get("code"))
        .and_then(serde_json::Value::as_str)
        .ok_or("the envelope carries error.code")?;
    assert_eq!(
        code, "E-DISC-DUPLICATE",
        "the refusal carries the stable code: {text}"
    );
    Ok(())
}

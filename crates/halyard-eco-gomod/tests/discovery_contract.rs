//! The M8 Go-modules cell's discovery contract (`docs/HALYARD_ARCHITECTURE.md`
//! §17 YEAR 2, M8 row, **Go modules** cell — *"Go modules (tag-only,
//! `go.work`, subdir tags)"*, gate *"each: discover/edit/publish/resume
//! tests + replay on 2 real repos"*; VERIFIED grammar in
//! `docs/research/go-module-grammars.md`).
//!
//! This is the plan's acceptance criterion pinned as code: a `go.work`
//! workspace's modules join the workspace as **tag-only** rows — the
//! `go.mod` grammar carries no version field for the module itself
//! (`go.dev/ref/mod`: *"A go.mod file must contain exactly one module
//! directive"*; versions are *"semantic version tags"*), the release
//! version comes from the release base tag, and a module rooted in a
//! subdirectory tags `subdir/vX.Y.Z`.

use halyard_core::EcosystemAdapter;
use halyard_core::discover::{DiscoverCtx, RawVersionSource};
use halyard_core::package::Ecosystem;
use halyard_eco_gomod::{GoModAdapter, tag};

/// `go.work` + member `go.mod`s discover as tag-only rows with subdir
/// tag templates (the §17 M8 cell's three facts: tag-only, `go.work`,
/// subdir tags).
/// The empty-row assertion (the pedantic `assert_is_empty` shape).
fn assert_empty<T>(rows: &[T]) {
    assert!(rows.is_empty(), "expected no rows");
}

#[test]
fn the_go_workspace_discovers_tag_only_rows_with_subdir_tags() {
    let mut ctx = DiscoverCtx::new();
    ctx.insert_file(
        "go.work",
        b"go 1.23.0\n\nuse (\n\t.\n\t./api\n\t./libs/metrics\n)\n".to_vec(),
    );
    ctx.insert_file(
        "go.mod",
        b"module example.com/halyard\n\ngo 1.23.0\n\nrequire example.com/halyard/api v0.1.2\n"
            .to_vec(),
    );
    ctx.insert_file(
        "api/go.mod",
        b"module example.com/halyard/api\n\ngo 1.23.0\n\nrequire example.com/external/sdk v1.4.0\n"
            .to_vec(),
    );
    ctx.insert_file(
        "libs/metrics/go.mod",
        b"module example.com/halyard/libs/metrics/v2\n\ngo 1.23.0\n".to_vec(),
    );

    let rows = GoModAdapter::new().discover(&ctx).expect("discovery");
    assert_eq!(rows.len(), 3, "root + api + libs/metrics");

    let root = rows.first().expect("the row");
    assert_eq!(root.name, "example.com/halyard");
    assert_eq!(root.ecosystem, Ecosystem::Go);
    assert_eq!(root.version, None, "tag-only: go.mod carries no version");
    assert_eq!(root.version_source, RawVersionSource::Tag);
    assert!(
        !root.publishable,
        "tag-only: no registry publish decision exists"
    );
    assert!(root.version_site.is_none(), "no editable version site");
    assert_eq!(
        root.tag_template
            .as_ref()
            .map(halyard_core::package::TagTemplate::as_str),
        Some("v{version}"),
        "root module tags v{{version}}"
    );

    let api = rows.get(1).expect("the row");
    assert_eq!(api.name, "example.com/halyard/api");
    assert_eq!(api.root.as_str(), "api");
    assert_eq!(
        api.tag_template
            .as_ref()
            .map(halyard_core::package::TagTemplate::as_str),
        Some("api/v{version}"),
        "subdir module tags subdir/v{{version}}"
    );
    // The external require stays registry-resolved (not internal).
    assert_eq!(api.deps.len(), 1);
    assert!(!api.deps.first().expect("the dep row").internal);

    let metrics = rows.get(2).expect("the row");
    assert_eq!(metrics.name, "example.com/halyard/libs/metrics/v2");
    assert_eq!(
        metrics
            .tag_template
            .as_ref()
            .map(halyard_core::package::TagTemplate::as_str),
        Some("libs/metrics/v{version}"),
        "the tag prefix is the directory, not the module path"
    );
    assert_eq!(
        tag::major_suffix(&metrics.name),
        Some(2),
        "the /v2 major version suffix is recognized"
    );
}

/// The internal edges: a member's require naming a workspace member, and
/// a local-path replace, both classify internal (the workspace proves
/// them); the planner's cascade rides them.
#[test]
fn workspace_member_requires_and_local_replaces_classify_internal() {
    let mut ctx = DiscoverCtx::new();
    ctx.insert_file(
        "go.work",
        b"go 1.23.0\n\nuse (\n\t./api\n\t./web\n)\n\nreplace example.com/external/sdk => ./vendor/sdk\n"
            .to_vec(),
    );
    ctx.insert_file(
        "api/go.mod",
        b"module example.com/halyard/api\n\ngo 1.23.0\n\nrequire (\n\texample.com/halyard/web v0.3.0\n\texample.com/external/sdk v1.4.0\n)\n"
            .to_vec(),
    );
    ctx.insert_file(
        "web/go.mod",
        b"module example.com/halyard/web\n\ngo 1.23.0\n".to_vec(),
    );
    ctx.insert_file(
        "vendor/sdk/go.mod",
        b"module example.com/external/sdk\n\ngo 1.23.0\n".to_vec(),
    );

    let rows = GoModAdapter::new().discover(&ctx).expect("discovery");
    assert_eq!(rows.len(), 2, "api + web (vendor/sdk is not a use member)");
    let api = rows.first().expect("the row");
    assert_eq!(api.deps.len(), 2);
    let member_dep = api
        .deps
        .iter()
        .find(|d| d.name == "example.com/halyard/web")
        .expect("member require row");
    assert!(
        member_dep.internal,
        "a require naming a use member is proven internal"
    );
    let replaced_dep = api
        .deps
        .iter()
        .find(|d| d.name == "example.com/external/sdk")
        .expect("replaced require row");
    assert!(
        replaced_dep.internal,
        "a require named by a local-path replace is proven internal"
    );
}

/// No `go.work`: the single root module is the workspace (the go command's
/// own posture — *"When no go.work file exists, the workspace consists of
/// the single module containing the current directory"*); nested go.mods
/// that nothing claims are skipped (the orphan-manifest posture).
#[test]
fn without_go_work_the_root_module_is_the_single_workspace() {
    let mut ctx = DiscoverCtx::new();
    ctx.insert_file(
        "go.mod",
        b"module example.com/standalone\n\ngo 1.22.0\n\nrequire example.com/dep/lib v0.9.1\n"
            .to_vec(),
    );
    ctx.insert_file(
        "tools/gen/go.mod",
        b"module example.com/standalone/tools-gen\n\ngo 1.22.0\n".to_vec(),
    );

    let rows = GoModAdapter::new().discover(&ctx).expect("discovery");
    assert_eq!(rows.len(), 1, "only the root module is claimed");
    assert_eq!(
        rows.first().expect("the row").name,
        "example.com/standalone"
    );
    assert_eq!(
        rows.first()
            .expect("the row")
            .tag_template
            .as_ref()
            .map(halyard_core::package::TagTemplate::as_str),
        Some("v{version}")
    );
}

/// A `use` member whose directory carries no go.mod fails discovery
/// (the explicit-member requirement — a named member is a requirement,
/// not a candidate root).
#[test]
fn a_use_member_without_go_mod_fails_discovery() {
    let mut ctx = DiscoverCtx::new();
    ctx.insert_file("go.work", b"go 1.23.0\n\nuse ./ghost\n".to_vec());
    let err = GoModAdapter::new()
        .discover(&ctx)
        .expect_err("the ghost member must fail");
    assert_eq!(err.code(), "E-DISC-GLOB");
}

/// No go manifest at all: an empty result (the orchestrator merges
/// adapters, so an empty result is correct — the pypi posture).
#[test]
fn a_repo_without_go_manifests_discovers_nothing() {
    let ctx = DiscoverCtx::new();
    let rows = GoModAdapter::new().discover(&ctx).expect("discovery");
    assert_empty(&rows);
}

/// The tag templates the adapter builds render the documented shapes.
#[test]
fn the_tag_templates_render_the_documented_shapes() {
    let root = tag::template_for("");
    assert_eq!(
        root.render("example.com/m", &"1.2.3".parse().unwrap(), "go"),
        "v1.2.3"
    );
    let sub = tag::template_for("libs/api");
    assert_eq!(
        sub.render("example.com/m/libs/api", &"2.0.0".parse().unwrap(), "go"),
        "libs/api/v2.0.0"
    );
    // The rendered tags round-trip through extract_version (the §8.2
    // base-tag parse rides this).
    assert_eq!(
        sub.extract_version("libs/api/v2.0.0", "example.com/m/libs/api", "go"),
        Some("2.0.0".parse().unwrap())
    );
}

/// The `/vN` suffix rules at the grammar (go.dev/ref/mod): not allowed at
/// v0/v1, must match the released major, `v1` and dotted numerics are not
/// suffixes.
#[test]
fn the_major_suffix_rules_follow_the_reference() {
    assert_eq!(tag::major_suffix("example.com/m"), None);
    assert_eq!(
        tag::major_suffix("example.com/m/v1"),
        None,
        "v1 is not a suffix"
    );
    assert_eq!(tag::major_suffix("example.com/m/v2"), Some(2));
    assert_eq!(
        tag::major_suffix("example.com/m/v2/nested"),
        None,
        "suffix is final"
    );
    assert_eq!(
        tag::major_suffix("example.com/m/v02"),
        None,
        "no leading zeros"
    );
    assert_eq!(tag::major_suffix("example.com/m/v2.1"), None, "no dots");
    assert_eq!(
        tag::major_suffix("example.com/m/v0"),
        None,
        "v0 is not a suffix"
    );
}

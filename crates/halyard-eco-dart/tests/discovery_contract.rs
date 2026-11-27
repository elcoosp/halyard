//! The M8 `Dart/pub` cell's discovery contract (`docs/HALYARD_ARCHITECTURE.md`
//! §17 YEAR 2, M8 row — *"More ecosystems"*, the `Dart/pub` cell — the
//! next listed cell after the shipped `NuGet` half (ADR-0059), gate
//! *"each: discover/edit/publish/resume tests + replay on 2 real
//! repos"*; VERIFIED grammar in `docs/research/dart-pub-grammars.md`).
//!
//! This is the plan's acceptance criterion pinned as code: `pubspec.yaml`
//! packages discover as dart rows — the identity is the `name` field
//! (*"Required for every package"*), the version rides the three-number
//! semver grammar (*"A version number is three numbers separated by
//! dots"*), the pub workspace walks the root's `workspace` entries with
//! `resolution: workspace` members (*"Pub discovers auth and api through
//! the workspace entry of the server package"*), a dependency *naming* a
//! workspace member is the proven-internal edge (*"they will
//! automatically resolve to the one in the workspace, regardless of the
//! source"*), a `path:` dependency naming a member manifest is the second
//! proof, and every pub-legal spelling the plan contract's semver cannot
//! render (a two-component `1.2`) is warned, unpublishable, never
//! rewritten.

use halyard_core::EcosystemAdapter;
use halyard_core::discover::{DiscoverCtx, RawVersionSource};
use halyard_core::package::Ecosystem;
use halyard_eco_dart::DartAdapter;

/// Builds a context from `(path, body)` fixture files.
fn ctx_with(files: &[(&str, &str)]) -> DiscoverCtx {
    let mut ctx = DiscoverCtx::new();
    for (path, body) in files {
        assert!(ctx.insert_file(path, body.as_bytes().to_vec()), "{path}");
    }
    ctx
}

/// The workspace-root fixture: the documented shape with exactly the
/// member entries the caller's tree carries (a listed member missing from
/// the committed tree is the `E-DISC-GLOB` refusal, so every fixture must
/// list only what it inserts).
fn root_pubspec(entries: &[&str]) -> String {
    let mut root = String::from(
        "# the pub workspace root (the documented shape)\nname: _\npublish_to: none\nenvironment:\n  sdk: ^3.6.0\nworkspace:\n",
    );
    for entry in entries {
        root.push_str("  - ");
        root.push_str(entry);
        root.push('\n');
    }
    root
}

/// The canonical root: `shared` + `client_package` (both inserted by the
/// tests that use it).
fn root_pubspec_canonical() -> String {
    root_pubspec(&["packages/shared", "packages/client_package"])
}

const SHARED_PUBSPEC: &str = r"name: shared
version: 2.3.0
resolution: workspace
";

const CLIENT_PUBSPEC: &str = r"name: client_package
version: 1.0.0
resolution: workspace
dependencies:
  shared: ^2.3.0
  http: ^1.2.0
dev_dependencies:
  test: ^1.25.0
";

/// The pub workspace discovers: the root pubspec is itself a row (the
/// documented `dart pub workspace list` prints `_ ./`), every member
/// carries `resolution: workspace`, and the dependency *naming* the
/// member is the proven-internal edge while unrelated hosted deps stay
/// registry-resolved.
/// The empty-row assertion (the pedantic `assert_is_empty` shape).
fn assert_empty<T>(rows: &[T]) {
    assert!(rows.is_empty(), "expected no rows");
}

#[test]
fn the_pub_workspace_discovers_rows_with_the_proven_edge() {
    let ctx = ctx_with(&[
        ("pubspec.yaml", &root_pubspec_canonical()),
        ("packages/shared/pubspec.yaml", SHARED_PUBSPEC),
        ("packages/client_package/pubspec.yaml", CLIENT_PUBSPEC),
    ]);
    let rows = DartAdapter::new().discover(&ctx).expect("discovery");
    assert_eq!(rows.len(), 3, "the root and both members are rows");
    let root = rows.first().expect("the root row");
    assert_eq!(root.name, "_", "the workspace-root row keeps its name");
    assert_eq!(root.ecosystem, Ecosystem::Dart);
    assert_eq!(root.manifest, "pubspec.yaml");
    assert!(
        !root.publishable,
        "no pub.dev publish argv/auth grammar exists yet (the disabled shape)"
    );
    let client = rows
        .iter()
        .find(|row| row.name == "client_package")
        .expect("the member row exists");
    assert_eq!(
        client
            .version
            .as_ref()
            .map(std::string::ToString::to_string)
            .as_deref(),
        Some("1.0.0")
    );
    let internal = client
        .deps
        .iter()
        .filter(|dep| dep.internal)
        .map(|dep| dep.name.clone())
        .collect::<Vec<_>>();
    assert_eq!(
        internal,
        vec!["shared"],
        "the name-matched member edge is internal"
    );
    let external = client
        .deps
        .iter()
        .filter(|dep| !dep.internal)
        .map(|dep| dep.name.clone())
        .collect::<Vec<_>>();
    assert_eq!(
        std::collections::BTreeSet::from_iter(external),
        std::collections::BTreeSet::from(["http".to_owned(), "test".to_owned()]),
        "hosted and dev deps stay external"
    );
}

/// The root's version is the three-number grammar with an editable site:
/// the patch the planner would emit lands on the value's inner bytes.
#[test]
fn a_canonical_version_carries_its_edit_site() {
    let ctx = ctx_with(&[
        ("pubspec.yaml", &root_pubspec_canonical()),
        ("packages/shared/pubspec.yaml", SHARED_PUBSPEC),
        ("packages/client_package/pubspec.yaml", CLIENT_PUBSPEC),
    ]);
    let rows = DartAdapter::new().discover(&ctx).expect("discovery");
    let client = rows
        .iter()
        .find(|row| row.name == "client_package")
        .expect("the member row exists");
    assert_eq!(client.version_source, RawVersionSource::Local);
    let site = client.version_site.as_ref().expect("the canonical site");
    assert_eq!(site.file.as_str(), "packages/client_package/pubspec.yaml");
    let bytes = ctx.file(site.file.as_str()).expect("the manifest bytes");
    let span = site.spans.first().expect("one span");
    assert_eq!(
        std::str::from_utf8(
            bytes
                .get(span.start as usize..span.end as usize)
                .expect("the span is in bounds"),
        )
        .expect("utf-8"),
        "1.0.0",
        "the span is the version value's inner bytes"
    );
}

/// A `path:` dependency naming a member manifest is the second
/// proven-internal edge (*"Relative paths are allowed and are considered
/// relative to the directory containing your pubspec"*).
#[test]
fn a_path_dependency_to_a_member_is_internal() {
    let tool = r"name: tool
version: 0.5.0
resolution: workspace
dependencies:
  shared:
    path: ../shared
";
    let ctx = ctx_with(&[
        (
            "pubspec.yaml",
            &root_pubspec(&["packages/shared", "packages/tool"]),
        ),
        ("packages/shared/pubspec.yaml", SHARED_PUBSPEC),
        ("packages/tool/pubspec.yaml", tool),
    ]);
    let rows = DartAdapter::new().discover(&ctx).expect("discovery");
    let tool_row = rows
        .iter()
        .find(|row| row.name == "tool")
        .expect("the member row exists");
    let internal = tool_row
        .deps
        .iter()
        .filter(|dep| dep.internal)
        .map(|dep| dep.name.clone())
        .collect::<Vec<_>>();
    assert_eq!(
        internal,
        vec!["shared"],
        "the path-named member is internal"
    );
}

/// A path dependency outside the workspace members is consumed as an
/// external edge with a warning — pub allows arbitrary filesystem paths,
/// so the adapter never refuses the package over one.
#[test]
fn a_path_dependency_to_a_non_member_warns() {
    let tool = r"name: tool
version: 0.5.0
resolution: workspace
dependencies:
  vendored:
    path: ../../vendor/vendored
";
    let ctx = ctx_with(&[
        ("pubspec.yaml", &root_pubspec(&["packages/tool"])),
        ("packages/tool/pubspec.yaml", tool),
    ]);
    let rows = DartAdapter::new().discover(&ctx).expect("discovery");
    let tool_row = rows.iter().find(|row| row.name == "tool").expect("row");
    assert!(
        tool_row.deps.iter().all(|dep| !dep.internal),
        "the non-member path dep stays external"
    );
    assert!(
        tool_row
            .warnings
            .iter()
            .any(|w| w.code == "external-path-dep" && w.subject == "tool"),
        "the non-member path dep is warned"
    );
}

/// The version strategy (the ADR-0056 discipline): two-component
/// spellings are pub-legal-adjacent but the plan contract cannot render
/// them (*"A version number is three numbers separated by dots"*) —
/// warned, unpublishable, no edit site, never rewritten.
#[test]
fn a_two_component_version_is_warned_and_never_rewritten() {
    let loose = r"name: loose
version: 1.2
resolution: workspace
";
    let ctx = ctx_with(&[
        ("pubspec.yaml", &root_pubspec(&["packages/loose"])),
        ("packages/loose/pubspec.yaml", loose),
    ]);
    let rows = DartAdapter::new().discover(&ctx).expect("discovery");
    let row = rows.iter().find(|row| row.name == "loose").expect("row");
    assert!(row.version.is_none(), "the spelling does not plan");
    assert!(row.version_site.is_none(), "no edit site exists");
    assert!(
        row.warnings
            .iter()
            .any(|w| w.code == "non-canonical-version"
                && w.message.contains("1.2")
                && w.message.contains("unpublishable")),
        "the non-canonical spelling is warned with the unpublishable posture"
    );
}

/// Prerelease and build suffixes are the documented grammar
/// (*"-dev.4", "+hotfix.oopsie"*) and plan verbatim with an editable site.
#[test]
fn prerelease_and_build_suffixes_plan() {
    let fancy = r"name: fancy
version: 2.0.0-dev.4+hotfix.oopsie
resolution: workspace
";
    let ctx = ctx_with(&[
        ("pubspec.yaml", &root_pubspec(&["packages/fancy"])),
        ("packages/fancy/pubspec.yaml", fancy),
    ]);
    let rows = DartAdapter::new().discover(&ctx).expect("discovery");
    let row = rows.iter().find(|row| row.name == "fancy").expect("row");
    assert_eq!(
        row.version
            .as_ref()
            .map(std::string::ToString::to_string)
            .as_deref(),
        Some("2.0.0-dev.4+hotfix.oopsie"),
        "the full spelling parses as semver"
    );
    assert!(row.version_site.is_some(), "the spelling is editable");
}

/// A member pubspec without `resolution: workspace` refuses (the
/// documented workspace contract: *"make sure their SDK constraint is at
/// least ^3.6.0 and add a `resolution` entry"*).
#[test]
fn a_member_without_workspace_resolution_refuses() {
    let rogue = r"name: rogue
version: 1.0.0
";
    let ctx = ctx_with(&[
        (
            "pubspec.yaml",
            &root_pubspec(&["packages/shared", "packages/rogue"]),
        ),
        ("packages/shared/pubspec.yaml", SHARED_PUBSPEC),
        ("packages/rogue/pubspec.yaml", rogue),
    ]);
    let err = DartAdapter::new().discover(&ctx).expect_err("refuses");
    assert_eq!(err.code(), "E-DISC-MANIFEST");
    assert!(
        err.to_string().contains("resolution: workspace"),
        "the refusal names the missing marker: {err}"
    );
}

/// The stray-file rule: with a workspace root present, an unclaimed
/// pubspec.yaml between the root and the members is pub's own
/// fail-closed error (*"pub get will report an error and fail to
/// resolve"*).
#[test]
fn a_stray_pubspec_between_root_and_members_refuses() {
    let stray = r"name: stray
version: 1.0.0
";
    let ctx = ctx_with(&[
        ("pubspec.yaml", &root_pubspec(&["packages/shared"])),
        ("packages/pubspec.yaml", stray),
        ("packages/shared/pubspec.yaml", SHARED_PUBSPEC),
    ]);
    let err = DartAdapter::new().discover(&ctx).expect_err("refuses");
    assert_eq!(err.code(), "E-DISC-MANIFEST");
    assert!(
        err.to_string().contains("stray"),
        "the refusal cites the stray-file rule: {err}"
    );
}

/// Without a workspace root, standalone pubspecs are each their own
/// package (the orphan-manifest posture — pub treats them as independent
/// resolutions).
#[test]
fn standalone_pubspecs_discover_without_a_workspace_root() {
    let a = r"name: alpha
version: 1.0.0
";
    let b = r"name: beta
version: 0.2.0
dependencies:
  alpha:
    path: ../alpha
";
    let ctx = ctx_with(&[
        ("packages/alpha/pubspec.yaml", a),
        ("packages/beta/pubspec.yaml", b),
    ]);
    let rows = DartAdapter::new().discover(&ctx).expect("discovery");
    assert_eq!(rows.len(), 2, "both standalone packages are rows");
    let beta = rows.iter().find(|row| row.name == "beta").expect("row");
    assert!(
        beta.deps
            .iter()
            .any(|dep| dep.internal && dep.name == "alpha"),
        "the path dep across standalone packages is internal"
    );
}

/// A workspace entry naming a path with no pubspec.yaml in the committed
/// tree refuses (`E-DISC-GLOB` — the Maven `<module>` posture).
#[test]
fn a_workspace_entry_with_no_member_manifest_refuses() {
    let root = r"name: _
publish_to: none
workspace:
  - packages/ghost
";
    let ctx = ctx_with(&[("pubspec.yaml", root)]);
    let err = DartAdapter::new().discover(&ctx).expect_err("refuses");
    assert_eq!(err.code(), "E-DISC-GLOB");
    assert!(
        err.to_string().contains("packages/ghost"),
        "the refusal names the missing member: {err}"
    );
}

/// The documented glob entry (`packages/*`, Dart 3.11+) expands over the
/// committed tree; an unsupported glob shape (`**`, a mid-segment `*`)
/// refuses instead of guessing.
#[test]
fn the_documented_glob_expands_and_other_shapes_refuse() {
    let root = r"name: _
publish_to: none
workspace:
  - packages/*
";
    let ctx = ctx_with(&[
        ("pubspec.yaml", root),
        ("packages/shared/pubspec.yaml", SHARED_PUBSPEC),
        ("packages/client_package/pubspec.yaml", CLIENT_PUBSPEC),
        ("docs/pubspec.yaml", "name: docs_pkg\nversion: 1.0.0\n"),
    ]);
    let rows = DartAdapter::new().discover(&ctx).expect("discovery");
    assert_eq!(rows.len(), 3, "the root plus the glob-expanded members");
    assert!(
        rows.iter().all(|row| row.name != "docs_pkg"),
        "the glob stays inside packages/"
    );

    let deep = r"name: _
publish_to: none
workspace:
  - packages/**
";
    let ctx = ctx_with(&[("pubspec.yaml", deep)]);
    let err = DartAdapter::new().discover(&ctx).expect_err("refuses");
    assert_eq!(err.code(), "E-DISC-MANIFEST");
    assert!(
        err.to_string().contains("glob"),
        "the refusal names the unsupported glob: {err}"
    );
}

/// Nested workspaces walk recursively (*"Pub discovers auth and api
/// through the workspace entry of the server package"*).
#[test]
fn nested_workspace_entries_walk_recursively() {
    let server = r"name: server
version: 1.0.0
resolution: workspace
workspace:
  - auth
";
    let auth = r"name: auth
version: 0.3.0
resolution: workspace
";
    let ctx = ctx_with(&[
        (
            "pubspec.yaml",
            &root_pubspec(&["packages/shared", "packages/server"]),
        ),
        ("packages/shared/pubspec.yaml", SHARED_PUBSPEC),
        ("packages/server/pubspec.yaml", server),
        ("packages/server/auth/pubspec.yaml", auth),
    ]);
    let rows = DartAdapter::new().discover(&ctx).expect("discovery");
    assert_eq!(rows.len(), 4, "root, shared, server and auth");
    assert!(
        rows.iter().any(|row| row.name == "auth"),
        "the nested member is discovered"
    );
}

/// `dependency_overrides` rows are consumed unmodeled (the section pins
/// local sources; the constraint there is not a published requirement).
#[test]
fn dependency_overrides_are_consumed_unmodeled() {
    let tool = r"name: tool
version: 0.5.0
resolution: workspace
dependency_overrides:
  shared:
    path: ../shared
  http: ^1.2.0
";
    let ctx = ctx_with(&[
        (
            "pubspec.yaml",
            &root_pubspec(&["packages/shared", "packages/tool"]),
        ),
        ("packages/shared/pubspec.yaml", SHARED_PUBSPEC),
        ("packages/tool/pubspec.yaml", tool),
    ]);
    let rows = DartAdapter::new().discover(&ctx).expect("discovery");
    let tool_row = rows.iter().find(|row| row.name == "tool").expect("row");
    assert!(
        tool_row
            .deps
            .iter()
            .all(|dep| dep.name != "http" || !dep.internal),
        "override rows carry no planning edge"
    );
    assert!(
        tool_row
            .warnings
            .iter()
            .any(|w| w.code == "unmodeled-override"),
        "the section is warned as consumed unmodeled"
    );
}

/// A pubspec without a `name` refuses (*"Required for every package"*).
#[test]
fn a_nameless_pubspec_refuses() {
    let nameless = "version: 1.0.0\nresolution: workspace\n";
    let ctx = ctx_with(&[
        (
            "pubspec.yaml",
            &root_pubspec(&["packages/shared", "packages/nameless"]),
        ),
        ("packages/shared/pubspec.yaml", SHARED_PUBSPEC),
        ("packages/nameless/pubspec.yaml", nameless),
    ]);
    let err = DartAdapter::new().discover(&ctx).expect_err("refuses");
    assert_eq!(err.code(), "E-DISC-MANIFEST");
}

/// A pubspec with no `version` is warned and plans as unpublished (the
/// *"implicitly versioned 0.0.0"* local-only posture).
#[test]
fn a_versionless_member_is_warned_unpublished() {
    let local = r"name: local_only
resolution: workspace
";
    let ctx = ctx_with(&[
        ("pubspec.yaml", &root_pubspec(&["packages/local_only"])),
        ("packages/local_only/pubspec.yaml", local),
    ]);
    let rows = DartAdapter::new().discover(&ctx).expect("discovery");
    let row = rows
        .iter()
        .find(|row| row.name == "local_only")
        .expect("row");
    assert!(row.version.is_none() && row.version_site.is_none());
    assert!(row.warnings.iter().any(|w| w.code == "no-version"));
}

/// Quoted scalars and trailing comments keep the span on the value's
/// inner bytes (the comment never taints the value — its bytes stay
/// verbatim through an edit).
#[test]
fn quoted_values_and_trailing_comments_keep_the_inner_span() {
    let quoted = "name: 'quoted_pkg'\nversion: \"1.4.2\" # the release\nresolution: workspace\n";
    let ctx = ctx_with(&[
        ("pubspec.yaml", &root_pubspec(&["packages/quoted"])),
        ("packages/quoted/pubspec.yaml", quoted),
    ]);
    let rows = DartAdapter::new().discover(&ctx).expect("discovery");
    let row = rows
        .iter()
        .find(|row| row.name == "quoted_pkg")
        .expect("row");
    assert_eq!(
        row.version
            .as_ref()
            .map(std::string::ToString::to_string)
            .as_deref(),
        Some("1.4.2")
    );
    let site = row.version_site.as_ref().expect("the site");
    let bytes = ctx.file(site.file.as_str()).expect("bytes");
    let span = site.spans.first().expect("span");
    assert_eq!(
        std::str::from_utf8(
            bytes
                .get(span.start as usize..span.end as usize)
                .expect("the span is in bounds"),
        )
        .expect("utf-8"),
        "1.4.2",
        "the span covers the inner bytes, not the quotes"
    );
}

/// `publish_to: none` is the documented *"prevent publication anywhere"*
/// posture — warned at discovery.
#[test]
fn publish_to_none_is_warned() {
    let local = r"name: internal_pkg
version: 1.0.0
publish_to: none
";
    let ctx = ctx_with(&[("pubspec.yaml", local)]);
    let rows = DartAdapter::new().discover(&ctx).expect("discovery");
    let row = rows.first().expect("the standalone row");
    assert!(row.warnings.iter().any(|w| w.code == "publish-disabled"));
}

/// The manifest-version probe the tag-target pinpoint walk dispatches to
/// (`halyard_git::tag_target::ManifestVersionProbe`): canonical spellings
/// pair, every other pubspec keeps the documented HEAD fallback.
#[test]
fn the_manifest_version_probe_pairs_canonical_spellings() {
    use halyard_eco_dart::manifest_version;
    assert_eq!(
        manifest_version(SHARED_PUBSPEC.as_bytes()).as_deref(),
        Some("2.3.0"),
        "the canonical version pairs"
    );
    let loose = "name: loose\nversion: 1.2\n";
    assert_eq!(
        manifest_version(loose.as_bytes()),
        None,
        "1.2 keeps the fallback"
    );
    let versionless = "name: tool\n";
    assert_eq!(manifest_version(versionless.as_bytes()), None);
}

/// Grammar diagnostics are total: tab indentation and duplicate keys
/// refuse with line numbers instead of panicking.
#[test]
fn grammar_diagnostics_refuse_structurally_invalid_pubspecs() {
    let tabbed = "name: a\n\tversion: 1.0.0\n";
    let ctx = ctx_with(&[("pubspec.yaml", tabbed)]);
    let err = DartAdapter::new().discover(&ctx).expect_err("refuses");
    assert_eq!(err.code(), "E-DISC-MANIFEST");

    let duplicated = "name: a\nname: b\nversion: 1.0.0\n";
    let ctx = ctx_with(&[("pubspec.yaml", duplicated)]);
    let err = DartAdapter::new().discover(&ctx).expect_err("refuses");
    assert_eq!(err.code(), "E-DISC-MANIFEST");

    let flowed = "name: a\nworkspace: [packages/a]\n";
    let ctx = ctx_with(&[("pubspec.yaml", flowed)]);
    let err = DartAdapter::new().discover(&ctx).expect_err("refuses");
    assert_eq!(err.code(), "E-DISC-MANIFEST");
}

/// A repository with no pubspec at all discovers zero dart rows (the
/// adapter stays silent — the other ecosystems' repositories are
/// unaffected).
#[test]
fn a_repository_without_pubspecs_yields_no_rows() {
    let ctx = ctx_with(&[("README.md", "# not a pubspec\n")]);
    let rows = DartAdapter::new().discover(&ctx).expect("discovery");
    assert_empty(&rows);
}

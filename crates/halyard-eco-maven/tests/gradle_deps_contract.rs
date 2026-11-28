//! The M8 Maven/Gradle cell's Gradle half **dependency-edge** contract
//! (`docs/HALYARD_ARCHITECTURE.md` §17 YEAR 2, the `Maven/Gradle` cell,
//! ADR-0058 continued — the recorded-open remainder the Gradle-half
//! note names). This is the plan line the first test pins (ADR-0058
//! continued, the Gradle half's recorded-open remainders):
//!
//! > *"the `project(":sub")` dependency edge (cross-package cascade
//! > rides the explicit `[[link]]` config surface in the meantime)"*
//!
//! VERIFIED shapes in `docs/research/gradle-grammars.md` §7 (the
//! `dependencies { }` block's `<configuration> project(':path')` row —
//! the documented project-path notation, *"the supplied paths must use
//! the ':' character as separator"*; the test configurations are the
//! dev edges the maven cell's `<scope>test</scope>` mapping already
//! established, §6.3). The requirement rides `ReqSpec::PathOnly` (a
//! project dependency carries no version constraint — nothing to
//! rewrite), the edge is proven-internal when the path resolves to an
//! include-proven member of the owning settings build, and the cascade
//! policy stays the planner's own (a `PathOnly` edge never
//! mismatches — `[[link]]` remains the forced-cascade surface).

use halyard_core::EcosystemAdapter;
use halyard_core::discover::{DiscoverCtx, RawVersionSource};
use halyard_core::package::{DepKind, ReqSpec};
use halyard_eco_maven::MavenAdapter;

fn ctx_with(files: &[(&str, &str)]) -> DiscoverCtx {
    let mut ctx = DiscoverCtx::new();
    for (path, body) in files {
        assert!(
            ctx.insert_file(path, (*body).as_bytes().to_vec()),
            "the fixture path {path} must be a normalized repo-relative path"
        );
    }
    ctx
}

const SETTINGS: &str = concat!(
    "rootProject.name = 'polyglot-parent'\n",
    "\n",
    "include 'library'\n",
    "include ':services:api'\n",
);

/// A governed root build file whose `dependencies` block names the
/// library member with the documented project-path notation — the edge
/// the recorded-open remainder pins.
const ROOT_BUILD_WITH_DEP: &str = concat!(
    "version = '1.0.0'\n",
    "\n",
    "dependencies {\n",
    "    implementation project(':library')\n",
    "}\n",
);

/// A proven `project(':…')` row inside a top-level `dependencies`
/// block resolves to the include-proven member: an internal,
/// path-only edge named after the member row (the orchestrator's
/// `resolve_dep` matches the workspace member by name).
#[test]
fn a_project_dependency_resolves_to_the_include_proven_member() {
    let ctx = ctx_with(&[
        ("settings.gradle", SETTINGS),
        ("build.gradle", ROOT_BUILD_WITH_DEP),
        ("library/build.gradle", "version = '2.0.0'\n"),
    ]);
    let rows = MavenAdapter::new().discover(&ctx).expect("discovers");
    let root = rows
        .iter()
        .find(|row| row.manifest == "build.gradle")
        .expect("the root row");
    assert_eq!(
        root.deps.len(),
        1,
        "the project row is an edge: {:?}",
        root.deps
    );
    let dep = root.deps.first().expect("the edge");
    assert_eq!(dep.name, "library", "the edge names the member row");
    assert!(dep.internal, "the include-proven path is internal");
    assert_eq!(
        dep.req,
        ReqSpec::PathOnly,
        "a project dependency carries no version constraint"
    );
    assert_eq!(dep.kind, DepKind::Normal, "implementation is a normal edge");
    assert_eq!(
        dep.raw.as_slice(),
        b":library".as_slice(),
        "the path rides verbatim"
    );
    assert!(
        dep.site.spans.is_empty(),
        "a path-only edge has no version slot to rewrite"
    );
}

/// A `test*` configuration is the dev edge (the maven cell's own
/// `<scope>test</scope>` mapping — dev edges don't force cascades by
/// default, §6.3).
#[test]
fn a_test_configuration_is_the_dev_edge() {
    let ctx = ctx_with(&[
        ("settings.gradle", SETTINGS),
        (
            "build.gradle",
            concat!(
                "version = '1.0.0'\n",
                "\n",
                "dependencies {\n",
                "    testImplementation project(':library')\n",
                "}\n",
            ),
        ),
        ("library/build.gradle", "version = '2.0.0'\n"),
    ]);
    let rows = MavenAdapter::new().discover(&ctx).expect("discovers");
    let root = rows
        .iter()
        .find(|row| row.manifest == "build.gradle")
        .expect("the root row");
    assert_eq!(root.deps.len(), 1, "the row is captured: {:?}", root.deps);
    assert_eq!(
        root.deps.first().expect("the edge").kind,
        DepKind::Dev,
        "test* is the dev edge"
    );
}

/// The Kotlin DSL's double-quoted call captures the same shape.
#[test]
fn the_kotlin_dsl_call_captures_the_same_shape() {
    let ctx = ctx_with(&[
        ("settings.gradle", SETTINGS),
        (
            "build.gradle.kts",
            concat!(
                "version = \"1.0.0\"\n",
                "\n",
                "dependencies {\n",
                "    implementation(project(\":library\"))\n",
                "}\n",
            ),
        ),
        ("library/build.gradle.kts", "version = \"2.0.0\"\n"),
    ]);
    let rows = MavenAdapter::new().discover(&ctx).expect("discovers");
    let root = rows
        .iter()
        .find(|row| row.manifest == "build.gradle.kts")
        .expect("the root row");
    assert_eq!(root.deps.len(), 1, "the row is captured: {:?}", root.deps);
    let dep = root.deps.first().expect("the edge");
    assert_eq!(dep.name, "library");
    assert!(dep.internal);
}

/// A project row outside a `dependencies` block (`evaluationDependsOn`,
/// a build-order declaration) is NOT a version-cascade dependency —
/// consumed unmodeled (the subset boundary).
#[test]
fn a_project_call_outside_a_dependencies_block_is_not_captured() {
    let ctx = ctx_with(&[
        ("settings.gradle", SETTINGS),
        (
            "build.gradle",
            concat!(
                "version = '1.0.0'\n",
                "evaluationDependsOn(':library')\n",
                "\n",
                "dependencies {\n",
                "    implementation project(':library')\n",
                "}\n",
            ),
        ),
        ("library/build.gradle", "version = '2.0.0'\n"),
    ]);
    let rows = MavenAdapter::new().discover(&ctx).expect("discovers");
    let root = rows
        .iter()
        .find(|row| row.manifest == "build.gradle")
        .expect("the root row");
    assert_eq!(
        root.deps.len(),
        1,
        "only the dependencies-block row is an edge: {:?}",
        root.deps
    );
}

/// A `dependencies` block nested inside `allprojects`/`subprojects`
/// configures OTHER projects' dependencies — consumed unmodeled (the
/// subset boundary; only the top-level block is the owning project's).
#[test]
fn a_nested_dependencies_block_is_not_captured() {
    let ctx = ctx_with(&[
        ("settings.gradle", SETTINGS),
        (
            "build.gradle",
            concat!(
                "version = '1.0.0'\n",
                "\n",
                "allprojects {\n",
                "    dependencies {\n",
                "        implementation project(':library')\n",
                "    }\n",
                "}\n",
            ),
        ),
        ("library/build.gradle", "version = '2.0.0'\n"),
    ]);
    let rows = MavenAdapter::new().discover(&ctx).expect("discovers");
    let root = rows
        .iter()
        .find(|row| row.manifest == "build.gradle")
        .expect("the root row");
    assert!(
        root.deps.is_empty(),
        "a nested block configures other projects — consumed unmodeled (the subset \
         boundary; no edge is invented for the owner): {:?}",
        root.deps
    );
}

/// A path the include set does not prove: the row is recognized but the
/// edge cannot resolve — warned, external (the fail-closed discipline).
#[test]
fn an_unresolvable_project_path_warns_and_stays_external() {
    let ctx = ctx_with(&[
        ("settings.gradle", SETTINGS),
        (
            "build.gradle",
            concat!(
                "version = '1.0.0'\n",
                "\n",
                "dependencies {\n",
                "    implementation project(':ghost')\n",
                "}\n",
            ),
        ),
        ("library/build.gradle", "version = '2.0.0'\n"),
    ]);
    let rows = MavenAdapter::new().discover(&ctx).expect("discovers");
    let root = rows
        .iter()
        .find(|row| row.manifest == "build.gradle")
        .expect("the root row");
    assert!(
        root.deps.is_empty(),
        "an unresolvable path is no internal edge: {:?}",
        root.deps
    );
    assert!(
        root.warnings
            .iter()
            .any(|warning| warning.code == "unresolved-project-dep"),
        "the unresolved path warns: {:?}",
        root.warnings
    );
}

/// Without a settings file the member set is unproven — every
/// `project(':…')` row stays external and warns (nothing is guessed).
#[test]
fn without_settings_governance_the_edge_cannot_resolve() {
    let ctx = ctx_with(&[
        (
            "build.gradle",
            concat!(
                "version = '1.0.0'\n",
                "\n",
                "dependencies {\n",
                "    implementation project(':library')\n",
                "}\n",
            ),
        ),
        ("library/build.gradle", "version = '2.0.0'\n"),
    ]);
    let rows = MavenAdapter::new().discover(&ctx).expect("discovers");
    let root = rows
        .iter()
        .find(|row| row.manifest == "build.gradle")
        .expect("the root row");
    assert!(
        root.deps.is_empty(),
        "no provenance, no internal edge: {:?}",
        root.deps
    );
    assert!(
        root.warnings
            .iter()
            .any(|warning| warning.code == "unresolved-project-dep"),
        "the unproven row warns: {:?}",
        root.warnings
    );
}

/// A dependency on the root project itself (`project(':')`) resolves to
/// the settings-governed root row — the root's name provenance applies.
#[test]
fn a_dependency_on_the_root_resolves_to_the_governed_root_name() {
    let ctx = ctx_with(&[
        ("settings.gradle", SETTINGS),
        (
            "library/build.gradle",
            concat!(
                "version = '2.0.0'\n",
                "\n",
                "dependencies {\n",
                "    implementation project(':')\n",
                "}\n",
            ),
        ),
        ("build.gradle", "version = '1.0.0'\n"),
    ]);
    let rows = MavenAdapter::new().discover(&ctx).expect("discovers");
    let library = rows
        .iter()
        .find(|row| row.manifest == "library/build.gradle")
        .expect("the member row");
    assert_eq!(library.deps.len(), 1, "the root edge: {:?}", library.deps);
    assert_eq!(
        library.deps.first().expect("the edge").name,
        "polyglot-parent",
        "the root edge names the governed root row"
    );
    assert!(library.deps.first().expect("the edge").internal);
}

/// A templated path (`":$name"`) is not a literal the grammar can
/// resolve — captured unmodeled, warned (the fail-closed discipline).
#[test]
fn a_templated_project_path_is_not_captured() {
    let ctx = ctx_with(&[
        ("settings.gradle", SETTINGS),
        (
            "build.gradle",
            concat!(
                "version = '1.0.0'\n",
                "\n",
                "dependencies {\n",
                "    implementation project(\":$name\")\n",
                "}\n",
            ),
        ),
        ("library/build.gradle", "version = '2.0.0'\n"),
    ]);
    let rows = MavenAdapter::new().discover(&ctx).expect("discovers");
    let root = rows
        .iter()
        .find(|row| row.manifest == "build.gradle")
        .expect("the root row");
    assert!(
        root.deps.is_empty(),
        "a templated path is not a proven edge: {:?}",
        root.deps
    );
    assert!(
        root.warnings
            .iter()
            .any(|warning| warning.code == "non-canonical-project-dep"),
        "the templated path warns: {:?}",
        root.warnings
    );
}

/// A relative project path (no leading `:` — resolved against the
/// owning project by Gradle) is out of the VERIFIED subset: recognized,
/// warned, never resolved (the fail-closed discipline — the subset
/// captures only the root-relative documented shape).
#[test]
fn a_relative_project_path_is_out_of_the_subset() {
    let ctx = ctx_with(&[
        ("settings.gradle", SETTINGS),
        (
            "build.gradle",
            concat!(
                "version = '1.0.0'\n",
                "\n",
                "dependencies {\n",
                "    implementation project('library')\n",
                "}\n",
            ),
        ),
        ("library/build.gradle", "version = '2.0.0'\n"),
    ]);
    let rows = MavenAdapter::new().discover(&ctx).expect("discovers");
    let root = rows
        .iter()
        .find(|row| row.manifest == "build.gradle")
        .expect("the root row");
    assert!(
        root.deps.is_empty(),
        "a relative path is not resolved: {:?}",
        root.deps
    );
    assert!(
        root.warnings
            .iter()
            .any(|warning| warning.code == "non-canonical-project-dep"),
        "the relative path warns: {:?}",
        root.warnings
    );
}

/// The version row is untouched by the dependency capture: the same
/// governed build file still carries its literal version and edit site
/// (the two grammar captures are independent).
#[test]
fn the_version_capture_is_independent_of_the_dependency_capture() {
    let ctx = ctx_with(&[
        ("settings.gradle", SETTINGS),
        ("build.gradle", ROOT_BUILD_WITH_DEP),
        ("library/build.gradle", "version = '2.0.0'\n"),
    ]);
    let rows = MavenAdapter::new().discover(&ctx).expect("discovers");
    let root = rows
        .iter()
        .find(|row| row.manifest == "build.gradle")
        .expect("the root row");
    assert_eq!(
        root.version.as_ref().map(std::string::ToString::to_string),
        Some("1.0.0".to_owned()),
        "the version capture is unchanged"
    );
    assert_eq!(
        root.version_source,
        RawVersionSource::Local,
        "the local source is unchanged"
    );
    assert!(root.version_site.is_some(), "the edit site is unchanged");
}

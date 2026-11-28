//! The M8 Maven/Gradle cell's **Gradle half** settings-file contract
//! (`docs/HALYARD_ARCHITECTURE.md` §17 YEAR 2, the `Maven/Gradle` cell,
//! ADR-0058 continued — the recorded-open remainder the Gradle-half
//! as-built note names). This is the plan line the first test pins
//! (ADR-0058 continued, the Gradle half's recorded-open remainders):
//!
//! > *"the `settings.gradle` `rootProject.name` / `include` grammar
//! > (the multi-project aggregator shape)"*
//!
//! VERIFIED grammar in `docs/research/gradle-grammars.md` §6 (the
//! Settings API: the root project's name *"defaults to the name of the
//! directory containing the settings file"*; `include` — *"Adds the
//! given projects to the build"* — *"The last element of the supplied
//! path is used as the project name"* and *"the supplied path is
//! converted to a project directory relative to the root project
//! directory"*; the `ProjectDescriptor` `setName` setter the
//! `rootProject.name` row delegates to).
//!
//! The shape: a settings file governs its own directory's tree — the
//! root build file is named by `rootProject.name` (the last literal
//! assignment wins, the `ProjectDescriptor` setter's ordinary overwrite),
//! each include-proven member is named by the path's last element, an
//! unincluded build file keeps the standalone row (warned — the
//! per-file shape never silently shrinks), and every row keeps the
//! engine's disabled publish shape. Nothing here guesses: a non-literal
//! name row falls back to the documented directory default, and an
//! ambiguous settings pair (`settings.gradle` + `settings.gradle.kts`
//! in one directory) declines all provenance.

use halyard_core::EcosystemAdapter;
use halyard_core::discover::DiscoverCtx;
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

const ROOT_BUILD: &str = concat!(
    "plugins {\n",
    "    id 'java'\n",
    "}\n",
    "\n",
    "version = '1.0.0'\n",
);

/// The governed fixture: a settings file naming the root and proving
/// two members (`:library` and the nested `:services:api`), every build
/// file carrying the documented literal assignment.
const SETTINGS_GOVERNED: &str = concat!(
    "rootProject.name = 'polyglot-parent'\n",
    "\n",
    "include 'library'\n",
    "include ':services:api'\n",
);

const SETTINGS_KTS_GOVERNED: &str = concat!(
    "rootProject.name = \"kotlin-parent\"\n",
    "\n",
    "include(\":library\")\n",
);

/// A root `settings.gradle` names the root package (`rootProject.name`)
/// and names each include-proven member by the path's last element —
/// the multi-project aggregator shape the recorded-open remainder
/// names, at the VERIFIED Settings grammar.
#[test]
fn a_settings_file_names_the_root_and_the_included_members() {
    let ctx = ctx_with(&[
        ("settings.gradle", SETTINGS_GOVERNED),
        ("build.gradle", ROOT_BUILD),
        ("library/build.gradle", "version = '2.0.0'\n"),
        ("services/api/build.gradle.kts", "version = \"0.3.0\"\n"),
    ]);
    let rows = MavenAdapter::new().discover(&ctx).expect("discovers");
    let root = rows
        .iter()
        .find(|row| row.manifest == "build.gradle")
        .expect("the root build file is a row");
    assert_eq!(
        root.name, "polyglot-parent",
        "the root package is named by rootProject.name"
    );
    let library = rows
        .iter()
        .find(|row| row.manifest == "library/build.gradle")
        .expect("the member build file is a row");
    assert_eq!(
        library.name, "library",
        "the member is named by the include path's last element"
    );
    let api = rows
        .iter()
        .find(|row| row.manifest == "services/api/build.gradle.kts")
        .expect("the nested member is a row");
    assert_eq!(
        api.name, "api",
        "a nested include path names its last element"
    );
    for row in &rows {
        assert!(
            !row.publishable,
            "the disabled publish shape is unchanged for {}",
            row.name
        );
    }
}

/// The Settings API's implicit-parent rule: *"It also adds the parent
/// `:a` project if it doesn't already exist"* — an include of
/// `:services:api` proves the `services` parent project too, so its
/// build file is a governed member named `services`.
#[test]
fn an_include_proves_its_implicit_parent_projects() {
    let ctx = ctx_with(&[
        (
            "settings.gradle",
            "rootProject.name = 'parent'\ninclude ':services:api'\n",
        ),
        ("services/build.gradle", "version = '1.1.0'\n"),
        ("services/api/build.gradle", "version = '1.2.0'\n"),
    ]);
    let rows = MavenAdapter::new().discover(&ctx).expect("discovers");
    let parent = rows
        .iter()
        .find(|row| row.manifest == "services/build.gradle")
        .expect("the implicit parent's build file is a governed member");
    assert_eq!(
        parent.name, "services",
        "the implicit parent is named by its own path's last element"
    );
}

/// The Kotlin DSL: a `settings.gradle.kts` carries the same grammar —
/// the double-quoted `rootProject.name = "…"` and the parenthesized
/// `include(":…")` call.
#[test]
fn a_kotlin_settings_file_carries_the_same_grammar() {
    let ctx = ctx_with(&[
        ("settings.gradle.kts", SETTINGS_KTS_GOVERNED),
        ("build.gradle.kts", "version = \"1.0.0\"\n"),
        ("library/build.gradle.kts", "version = \"2.0.0\"\n"),
    ]);
    let rows = MavenAdapter::new().discover(&ctx).expect("discovers");
    let root = rows
        .iter()
        .find(|row| row.manifest == "build.gradle.kts")
        .expect("the root build file is a row");
    assert_eq!(
        root.name, "kotlin-parent",
        "the Kotlin name row names the root"
    );
    let library = rows
        .iter()
        .find(|row| row.manifest == "library/build.gradle.kts")
        .expect("the member is a row");
    assert_eq!(
        library.name, "library",
        "the Kotlin include proves the member"
    );
}

/// A build file the settings file does not govern keeps the standalone
/// row (the shipped per-file shape never silently shrinks) — with an
/// honest warning that it rides outside the settings-governed build.
#[test]
fn an_unincluded_build_file_keeps_the_standalone_row_with_a_warning() {
    let ctx = ctx_with(&[
        ("settings.gradle", SETTINGS_GOVERNED),
        ("build.gradle", ROOT_BUILD),
        ("extra/build.gradle", "version = '3.0.0'\n"),
    ]);
    let rows = MavenAdapter::new().discover(&ctx).expect("discovers");
    let extra = rows
        .iter()
        .find(|row| row.manifest == "extra/build.gradle")
        .expect("the unincluded build file is still a row");
    assert_eq!(
        extra.name, "extra",
        "the standalone identity chain (the directory's own name) governs"
    );
    assert!(
        extra
            .warnings
            .iter()
            .any(|warning| warning.code == "unincluded-gradle-build"),
        "the unincluded row warns: {:?}",
        extra.warnings
    );
}

/// A proven member whose directory carries no build file: Gradle
/// permits build-less projects, so there is no row — the settings
/// tree's root row carries the honest warning instead.
#[test]
fn a_proven_member_without_a_build_file_warns_on_the_root_row() {
    let ctx = ctx_with(&[
        (
            "settings.gradle",
            "rootProject.name = 'parent'\ninclude ':tools'\n",
        ),
        ("build.gradle", ROOT_BUILD),
    ]);
    let rows = MavenAdapter::new().discover(&ctx).expect("discovers");
    let root = rows
        .iter()
        .find(|row| row.manifest == "build.gradle")
        .expect("the root build file is a row");
    assert!(
        root.warnings
            .iter()
            .any(|warning| warning.code == "include-without-manifest"),
        "the manifest-less member warns: {:?}",
        root.warnings
    );
    assert!(
        !rows.iter().any(|row| row.manifest == "tools/build.gradle"),
        "no row exists without a manifest"
    );
}

/// A non-literal `rootProject.name` right-hand side is recognized but
/// non-canonical: the name falls back to the **documented default**
/// (*"the name of the directory containing the settings file"* — the
/// repo-relative tree cannot know the checkout directory's own name,
/// so the root falls back to the manifest path the Dockerfile cell
/// established) and warns.
#[test]
fn a_non_literal_root_name_falls_back_to_the_documented_default() {
    let ctx = ctx_with(&[
        (
            "settings.gradle",
            "rootProject.name = findProperty('rootName') as String\ninclude ':library'\n",
        ),
        ("build.gradle", ROOT_BUILD),
        ("library/build.gradle", "version = '2.0.0'\n"),
    ]);
    let rows = MavenAdapter::new().discover(&ctx).expect("discovers");
    let root = rows
        .iter()
        .find(|row| row.manifest == "build.gradle")
        .expect("the root build file is a row");
    assert_eq!(
        root.name, "build.gradle",
        "the documented-default fallback at the tree root is the manifest path"
    );
    assert!(
        root.warnings
            .iter()
            .any(|warning| warning.code == "non-canonical-name"),
        "the non-literal name row warns: {:?}",
        root.warnings
    );
    let library = rows
        .iter()
        .find(|row| row.manifest == "library/build.gradle")
        .expect("the member is a row");
    assert_eq!(
        library.name, "library",
        "the include provenance is independent of the name row's shape"
    );
}

/// The last `rootProject.name` assignment in document order wins (the
/// `ProjectDescriptor` setter's ordinary overwrite — the version row's
/// own rule, the same property-setter semantics).
#[test]
fn the_last_root_name_assignment_wins() {
    let ctx = ctx_with(&[
        (
            "settings.gradle",
            "rootProject.name = 'first'\nrootProject.name = 'second'\n",
        ),
        ("build.gradle", ROOT_BUILD),
    ]);
    let rows = MavenAdapter::new().discover(&ctx).expect("discovers");
    let root = rows
        .iter()
        .find(|row| row.manifest == "build.gradle")
        .expect("the root build file is a row");
    assert_eq!(
        root.name, "second",
        "the last assignment is the name the setter leaves"
    );
}

/// Both settings spellings in one directory: the settings file is
/// ambiguous and the fail-closed discipline declines all provenance —
/// rows keep the standalone shape and the root row warns.
#[test]
fn an_ambiguous_settings_pair_declines_provenance() {
    let ctx = ctx_with(&[
        ("settings.gradle", SETTINGS_GOVERNED),
        ("settings.gradle.kts", SETTINGS_KTS_GOVERNED),
        ("build.gradle", ROOT_BUILD),
        ("library/build.gradle", "version = '2.0.0'\n"),
    ]);
    let rows = MavenAdapter::new().discover(&ctx).expect("discovers");
    let root = rows
        .iter()
        .find(|row| row.manifest == "build.gradle")
        .expect("the root build file is a row");
    assert_eq!(
        root.name, "build.gradle",
        "no provenance is guessed from an ambiguous settings pair"
    );
    let library = rows
        .iter()
        .find(|row| row.manifest == "library/build.gradle")
        .expect("the member is a row");
    assert_eq!(
        library.name, "library",
        "the standalone identity chain governs without provenance"
    );
    assert!(
        root.warnings
            .iter()
            .any(|warning| warning.code == "ambiguous-settings"),
        "the ambiguity warns: {:?}",
        root.warnings
    );
}

/// A non-literal include argument is recognized but non-canonical —
/// the member is not proven by the subset (warned on the root row) and
/// keeps the standalone shape.
#[test]
fn a_non_literal_include_argument_is_not_proven() {
    let ctx = ctx_with(&[
        (
            "settings.gradle",
            "rootProject.name = 'parent'\ninclude \":services:$name\"\n",
        ),
        ("build.gradle", ROOT_BUILD),
        ("services/api/build.gradle", "version = '2.0.0'\n"),
    ]);
    let rows = MavenAdapter::new().discover(&ctx).expect("discovers");
    let root = rows
        .iter()
        .find(|row| row.manifest == "build.gradle")
        .expect("the root build file is a row");
    assert!(
        root.warnings
            .iter()
            .any(|warning| warning.code == "non-canonical-include"),
        "the non-literal include warns: {:?}",
        root.warnings
    );
    let api = rows
        .iter()
        .find(|row| row.manifest == "services/api/build.gradle")
        .expect("the build file is still a row");
    assert_eq!(
        api.name, "api",
        "the unproven member keeps the standalone identity chain"
    );
}

/// Comments and sibling strings never capture a name or a member (the
/// shared lexer substrate — the version scanner's own discipline).
#[test]
fn comments_and_sibling_strings_are_skipped_in_place() {
    let ctx = ctx_with(&[
        (
            "settings.gradle",
            concat!(
                "// rootProject.name = 'commented'\n",
                "/* include ':commented' */\n",
                "description = \"include ':in-a-string'\"\n",
                "rootProject.name = 'real'\n",
                "include ':library'\n",
            ),
        ),
        ("build.gradle", ROOT_BUILD),
        ("library/build.gradle", "version = '2.0.0'\n"),
        ("commented/build.gradle", "version = '9.9.9'\n"),
    ]);
    let rows = MavenAdapter::new().discover(&ctx).expect("discovers");
    let root = rows
        .iter()
        .find(|row| row.manifest == "build.gradle")
        .expect("the root build file is a row");
    assert_eq!(root.name, "real", "only the live rows capture");
    // The commented include never proved the member: the build file
    // exists in the tree (its directory's own name is the standalone
    // chain), but it rides OUTSIDE the include set — the warning proves
    // the commented row was not captured.
    let commented = rows
        .iter()
        .find(|row| row.manifest == "commented/build.gradle")
        .expect("the build file is a row (the standalone shape)");
    assert!(
        commented
            .warnings
            .iter()
            .any(|warning| warning.code == "unincluded-gradle-build"),
        "the commented include is not provenance: {:?}",
        commented.warnings
    );
}

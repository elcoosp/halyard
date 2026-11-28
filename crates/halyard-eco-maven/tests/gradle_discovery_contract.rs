//! The M8 Maven/Gradle cell's **Gradle half** discovery contract
//! (`docs/HALYARD_ARCHITECTURE.md` §17 YEAR 2, M8 row — *"More
//! ecosystems"*, the `Maven/Gradle` cell, ADR-0058 continued; VERIFIED
//! grammar in `docs/research/gradle-grammars.md`).
//!
//! This is the plan's acceptance criterion pinned as code. ADR-0058's
//! Consequences name the gate this lifts:
//!
//! > *"the Gradle half stays recorded-open — a `build.gradle(.kts)`
//! > version is arbitrary Groovy/Kotlin build code … extracting one
//! > without an evaluated, VERIFIED grammar would guess, and halyard's
//! > discipline forbids guessing."*
//!
//! The M8 row's acceptance is *"each: discover/edit/publish/resume
//! tests + replay on 2 real repos"*; this file pins the **discover**
//! half for the Gradle build file — every `build.gradle` /
//! `build.gradle.kts` in the committed tree is a standalone Maven row
//! (the Maven/Gradle cell's single ecosystem — both publish to Maven
//! Central), the version is the documented literal `version = '…'` /
//! `version = "…"` assignment (Gradle's Project API: *"The version of
//! this project. Gradle always uses the `toString()` value of the
//! version. The version defaults to `unspecified`."*), only
//! semver-parseable literals are canonical, and the row carries the
//! engine's disabled publish shape (`publishable = false`, no `gradle`
//! argv grammar).

use halyard_core::EcosystemAdapter;
use halyard_core::discover::{DiscoverCtx, DiscoverError, ManifestFormat, RawVersionSource};
use halyard_core::package::Ecosystem;
use halyard_eco_maven::MavenAdapter;

/// A Groovy build file with the documented single-quoted literal
/// assignment — the plan line this first test pins: ADR-0058's Gradle
/// half at the VERIFIED `version = '1.2.3'` grammar (the Building Java
/// Projects guide's own Groovy form).
const GROOVY_BUILD: &str = concat!(
    "plugins {\n",
    "    id 'java'\n",
    "}\n",
    "\n",
    "version = '1.2.3'\n",
    "\n",
    "repositories {\n",
    "    mavenCentral()\n",
    "}\n",
);

/// A Kotlin build file with the documented double-quoted literal
/// assignment (the Building Java Projects guide's Kotlin form).
const KOTLIN_BUILD: &str = concat!(
    "plugins {\n",
    "    java\n",
    "}\n",
    "\n",
    "version = \"1.2.3\"\n",
);

/// The inner-byte span of `needle` inside `whole` (the bytes between
/// the quotes the fixture carries — the edit site the row must name).
/// Loud, panic-free failure for a constant fixture that cannot miss
/// (the sibling bench posture).
fn inner_span(whole: &str, needle: &str) -> (u32, u32) {
    let fail = |reason: &str| -> ! {
        eprintln!("the fixture must {reason}");
        std::process::exit(101);
    };
    let i = whole
        .find(needle)
        .unwrap_or_else(|| fail("carry the needle"));
    let start = u32::try_from(i).unwrap_or_else(|_| fail("fit the start offset"));
    let len = u32::try_from(needle.len()).unwrap_or_else(|_| fail("fit the needle length"));
    (start, start + len)
}

/// A root Groovy `build.gradle` discovers one Maven row whose version is
/// the literal `1.2.3`, whose edit site is the literal's inner bytes,
/// and whose publish shape is the engine's disabled posture — the plan
/// line the first test pins.
#[test]
fn a_groovy_build_gradle_discovers_a_maven_row_with_the_literal_version() {
    let mut ctx = DiscoverCtx::new();
    ctx.insert_file("build.gradle", GROOVY_BUILD.as_bytes().to_vec());
    let rows = MavenAdapter::new().discover(&ctx).expect("discovery");
    assert_eq!(rows.len(), 1, "one build file = one package row");
    let row = rows.first().expect("one row");
    assert_eq!(
        row.ecosystem,
        Ecosystem::Maven,
        "the Maven/Gradle cell's single ecosystem"
    );
    assert_eq!(
        row.version.as_ref().map(std::string::ToString::to_string),
        Some("1.2.3".to_owned()),
        "the literal assignment is the canonical version"
    );
    assert_eq!(row.version_source, RawVersionSource::Local);
    let site = row
        .version_site
        .as_ref()
        .expect("the literal's inner-byte site");
    assert_eq!(site.file.as_str(), "build.gradle");
    let (start, end) = inner_span(GROOVY_BUILD, "1.2.3");
    assert_eq!(site.spans.len(), 1);
    let span = site.spans.first().expect("one span");
    assert_eq!((span.start, span.end), (start, end));
    assert!(!row.publishable, "no gradle publish grammar exists yet");
    assert!(
        row.deps.is_empty(),
        "no project(\":sub\") dependency grammar yet"
    );
}

/// A Kotlin `build.gradle.kts` discovers the same row shape at the
/// documented double-quoted literal.
#[test]
fn a_kotlin_build_gradle_kts_discovers_the_double_quoted_literal() {
    let mut ctx = DiscoverCtx::new();
    ctx.insert_file("build.gradle.kts", KOTLIN_BUILD.as_bytes().to_vec());
    let rows = MavenAdapter::new().discover(&ctx).expect("discovery");
    assert_eq!(rows.len(), 1);
    let row = rows.first().expect("one row");
    assert_eq!(row.ecosystem, Ecosystem::Maven);
    assert_eq!(
        row.version.as_ref().map(std::string::ToString::to_string),
        Some("1.2.3".to_owned())
    );
    let site = row.version_site.as_ref().expect("the literal's site");
    assert_eq!(site.file.as_str(), "build.gradle.kts");
    let (start, end) = inner_span(KOTLIN_BUILD, "1.2.3");
    let span = site.spans.first().expect("one span");
    assert_eq!((span.start, span.end), (start, end));
}

/// A double-quoted value carrying `$` (a Groovy `GString` / Kotlin string
/// template) is interpolation, not a literal — the row warns
/// `non-canonical-version`, carries no edit site, and stays unpublishable
/// (the ADR-0056 fail-closed discipline).
#[test]
fn a_templated_double_quoted_version_is_non_canonical() {
    let build = "version = \"${major}.${minor}\"\n";
    let mut ctx = DiscoverCtx::new();
    ctx.insert_file("build.gradle", build.as_bytes().to_vec());
    let rows = MavenAdapter::new().discover(&ctx).expect("discovery");
    assert_eq!(rows.len(), 1);
    let row = rows.first().expect("one row");
    assert!(
        row.version.is_none(),
        "a template is not a canonical version"
    );
    assert!(row.version_site.is_none(), "no byte span to edit");
    assert!(
        row.warnings
            .iter()
            .any(|w| w.code == "non-canonical-version")
    );
}

/// A non-literal right-hand side (a property reference / lookup) is
/// recognized — a `version = …` row exists — but non-canonical: the row
/// warns `non-canonical-version` (not `no-version`).
#[test]
fn a_property_reference_version_is_non_canonical_not_no_version() {
    let build = "version = findProperty('appVersion')\n";
    let mut ctx = DiscoverCtx::new();
    ctx.insert_file("build.gradle", build.as_bytes().to_vec());
    let rows = MavenAdapter::new().discover(&ctx).expect("discovery");
    assert_eq!(rows.len(), 1);
    let row = rows.first().expect("one row");
    assert!(row.version.is_none());
    assert!(row.version_site.is_none());
    assert!(
        row.warnings
            .iter()
            .any(|w| w.code == "non-canonical-version")
    );
    assert!(
        !row.warnings.iter().any(|w| w.code == "no-version"),
        "a `version = …` row exists — the warning is non-canonical, not no-version"
    );
}

/// A build file with no `version = …` assignment warns `no-version` and
/// carries no site (Gradle's documented default is `"unspecified"`).
#[test]
fn a_build_file_without_a_version_assignment_warns_no_version() {
    let build = "plugins { id 'java' }\nrepositories { mavenCentral() }\n";
    let mut ctx = DiscoverCtx::new();
    ctx.insert_file("build.gradle", build.as_bytes().to_vec());
    let rows = MavenAdapter::new().discover(&ctx).expect("discovery");
    assert_eq!(rows.len(), 1);
    let row = rows.first().expect("one row");
    assert!(row.version.is_none());
    assert!(row.version_site.is_none());
    assert!(row.warnings.iter().any(|w| w.code == "no-version"));
}

/// The last `version = …` assignment in document order wins (Gradle's
/// Project-setter overwrite — the documented `toString()` value is the
/// last assignment's).
#[test]
fn the_last_version_assignment_in_document_order_wins() {
    let build = "version = '1.0.0'\nversion = '2.0.0'\n";
    let mut ctx = DiscoverCtx::new();
    ctx.insert_file("build.gradle", build.as_bytes().to_vec());
    let rows = MavenAdapter::new().discover(&ctx).expect("discovery");
    assert_eq!(rows.len(), 1);
    let row = rows.first().expect("one row");
    assert_eq!(
        row.version.as_ref().map(std::string::ToString::to_string),
        Some("2.0.0".to_owned()),
        "the last assignment is the row's source"
    );
    let site = row.version_site.as_ref().expect("the last literal's site");
    let (start, end) = inner_span(build, "2.0.0");
    let span = site.spans.first().expect("one span");
    assert_eq!((span.start, span.end), (start, end));
}

/// A `version = '1.0.0'` inside a comment is never captured (the
/// scanner skips `//` line and `/* */` block comments in place).
#[test]
fn a_version_inside_a_comment_is_not_captured() {
    let build = "// version = '9.9.9'\n/* version = '8.8.8' */\nversion = '1.0.0'\n";
    let mut ctx = DiscoverCtx::new();
    ctx.insert_file("build.gradle", build.as_bytes().to_vec());
    let rows = MavenAdapter::new().discover(&ctx).expect("discovery");
    assert_eq!(rows.len(), 1);
    let row = rows.first().expect("one row");
    assert_eq!(
        row.version.as_ref().map(std::string::ToString::to_string),
        Some("1.0.0".to_owned()),
        "only the uncommented assignment is the row's source"
    );
}

/// A nested `build.gradle` (a subproject's build file) discovers a row
/// whose identity is the containing directory's name — the documented
/// build context, the Dockerfile cell's identity chain.
#[test]
fn a_nested_build_gradle_row_names_the_containing_directory() {
    let build = "version = '1.0.0'\n";
    let mut ctx = DiscoverCtx::new();
    ctx.insert_file("services/api/build.gradle", build.as_bytes().to_vec());
    let rows = MavenAdapter::new().discover(&ctx).expect("discovery");
    assert_eq!(rows.len(), 1);
    let row = rows.first().expect("one row");
    assert_eq!(row.name, "api", "the containing directory's name");
    assert_eq!(row.root.as_str(), "services/api");
    assert_eq!(row.manifest, "services/api/build.gradle");
}

/// A malformed build file (an unclosed string literal) fails closed
/// through `DiscoverError::Manifest` naming the `GradleBuild` format.
#[test]
fn an_unclosed_literal_fails_closed_with_the_gradle_format() {
    let build = "version = '1.0.0\n";
    let mut ctx = DiscoverCtx::new();
    ctx.insert_file("build.gradle", build.as_bytes().to_vec());
    let err = MavenAdapter::new()
        .discover(&ctx)
        .expect_err("the unclosed literal refuses");
    let DiscoverError::Manifest { format, .. } = err else {
        panic!("a manifest error");
    };
    assert_eq!(format, ManifestFormat::GradleBuild);
}

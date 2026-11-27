//! The M8 Maven cell's discovery contract (`docs/HALYARD_ARCHITECTURE.md`
//! §17 YEAR 2, M8 row — *"More ecosystems"*, the `Maven/Gradle` cell —
//! the next listed cell after `PyPI` (ADR-0056) and `Go` (ADR-0057), gate
//! *"each: discover/edit/publish/resume tests + replay on 2 real
//! repos"*; VERIFIED grammar in `docs/research/maven-pom-grammars.md`).
//!
//! This is the plan's acceptance criterion pinned as code: a root
//! `pom.xml` aggregator's `<modules>` discover as maven rows — the
//! coordinates (`groupId:artifactId:version`) name the packages, a child
//! pom without its own `<version>` inherits the parent's (the documented
//! inheritance list: *"Most elements from the parent POM are inherited
//! by its children, including: groupId, version, ..."*), and a
//! `<dependency>` naming a member's (groupId, artifactId) pair is the
//! proven-internal edge.

use halyard_core::EcosystemAdapter;
use halyard_core::discover::{DiscoverCtx, DiscoverError, RawVersionSource};
use halyard_core::package::Ecosystem;
use halyard_eco_maven::MavenAdapter;

const ROOT_POM: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <!-- the aggregator: pom packaging is required for multi-module builds -->
  <groupId>com.example</groupId>
  <artifactId>halyard-demo</artifactId>
  <version>1.2.3</version>
  <packaging>pom</packaging>
  <modules>
    <module>api</module>
    <module>web</module>
  </modules>
</project>
"#;

const API_POM: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <parent>
    <groupId>com.example</groupId>
    <artifactId>halyard-demo</artifactId>
    <version>1.2.3</version>
  </parent>
  <artifactId>halyard-demo-api</artifactId>
  <!-- no <version>: inherited from the parent (the documented list) -->
  <dependencies>
    <dependency>
      <groupId>com.example</groupId>
      <artifactId>halyard-demo-web</artifactId>
      <version>1.2.3</version>
    </dependency>
    <dependency>
      <groupId>org.apache.commons</groupId>
      <artifactId>commons-lang3</artifactId>
      <version>3.14.0</version>
    </dependency>
  </dependencies>
</project>
"#;

const WEB_POM: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <parent>
    <groupId>com.example</groupId>
    <artifactId>halyard-demo</artifactId>
    <version>1.2.3</version>
  </parent>
  <artifactId>halyard-demo-web</artifactId>
  <version>1.2.3</version>
</project>
"#;

/// The aggregator workspace discovers: the root pom is a package (pom
/// packaging is a real artifact), members join with their `artifactId`
/// names, the child without its own `<version>` inherits the parent's
/// through the workspace-inheritance machinery (the cargo
/// `[workspace.package].version` posture — one edit site, on the parent).
/// The empty-row assertion (the pedantic `assert_is_empty` shape).
fn assert_empty<T>(rows: &[T]) {
    assert!(rows.is_empty(), "expected no rows");
}

#[test]
fn the_maven_multi_module_workspace_discovers_rows_with_parent_inheritance() {
    let mut ctx = DiscoverCtx::new();
    ctx.insert_file("pom.xml", ROOT_POM.as_bytes().to_vec());
    ctx.insert_file("api/pom.xml", API_POM.as_bytes().to_vec());
    ctx.insert_file("web/pom.xml", WEB_POM.as_bytes().to_vec());

    let rows = MavenAdapter::new().discover(&ctx).expect("discovery");
    assert_eq!(rows.len(), 3, "root + api + web");

    let root = rows.first().expect("the root row");
    assert_eq!(root.name, "halyard-demo");
    assert_eq!(root.ecosystem, Ecosystem::Maven);
    assert_eq!(
        root.version.as_ref().map(std::string::ToString::to_string),
        Some("1.2.3".to_owned()),
        "the root pom's own <version> is canonical semver"
    );
    assert_eq!(root.version_source, RawVersionSource::Local);
    let site = root.version_site.as_ref().expect("root own version site");
    assert_eq!(site.file.as_str(), "pom.xml");
    assert!(!root.publishable, "no maven publish grammar exists yet");

    let api = rows.get(1).expect("the api row");
    assert_eq!(api.name, "halyard-demo-api");
    assert_eq!(api.root.as_str(), "api");
    assert_eq!(api.manifest, "api/pom.xml");
    assert_eq!(
        api.version.as_ref().map(std::string::ToString::to_string),
        Some("1.2.3".to_owned()),
        "the inherited version resolves through the parent"
    );
    assert_eq!(
        api.version_source,
        RawVersionSource::WorkspaceInherited {
            file: "pom.xml".to_owned(),
            key_path: "project.version".to_owned(),
        },
        "the child inherits the parent's version"
    );
    let inherited_site = api.version_site.as_ref().expect("one shared edit site");
    assert_eq!(
        inherited_site.file.as_str(),
        "pom.xml",
        "the inheritor's edit site lives on the parent pom"
    );

    let web = rows.get(2).expect("the web row");
    assert_eq!(web.name, "halyard-demo-web");
    assert_eq!(web.version_source, RawVersionSource::Local);
}

/// The internal edges: a `<dependency>` whose (groupId, artifactId) pair
/// names a discovered member is proven internal; an external coordinate
/// stays registry-resolved (the cargo differential discipline).
#[test]
fn a_dependency_naming_a_member_coordinate_classifies_internal() {
    let mut ctx = DiscoverCtx::new();
    ctx.insert_file("pom.xml", ROOT_POM.as_bytes().to_vec());
    ctx.insert_file("api/pom.xml", API_POM.as_bytes().to_vec());
    ctx.insert_file("web/pom.xml", WEB_POM.as_bytes().to_vec());

    let rows = MavenAdapter::new().discover(&ctx).expect("discovery");
    let api = rows.get(1).expect("the api row");
    assert_eq!(api.deps.len(), 2, "both dependency rows are discovered");
    let member_dep = api
        .deps
        .iter()
        .find(|d| d.name == "halyard-demo-web")
        .expect("member dependency row");
    assert!(
        member_dep.internal,
        "a dependency naming a member's (groupId, artifactId) is proven internal"
    );
    let external_dep = api
        .deps
        .iter()
        .find(|d| d.name == "commons-lang3")
        .expect("external dependency row");
    assert!(
        !external_dep.internal,
        "an external coordinate stays registry-resolved (not internal)"
    );
}

/// No `<modules>`: the root pom is the single package (§6.1
/// single-package mode); nested poms nothing claims are skipped (the
/// orphan-manifest posture).
#[test]
fn without_modules_the_root_pom_is_the_single_package() {
    let mut ctx = DiscoverCtx::new();
    ctx.insert_file(
        "pom.xml",
        br#"<?xml version="1.0"?>
<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <groupId>org.standalone</groupId>
  <artifactId>tool</artifactId>
  <version>0.9.0</version>
</project>
"#
        .to_vec(),
    );
    ctx.insert_file("tools/gen/pom.xml", WEB_POM.as_bytes().to_vec());

    let rows = MavenAdapter::new().discover(&ctx).expect("discovery");
    assert_eq!(rows.len(), 1, "only the root pom is claimed");
    assert_eq!(rows.first().expect("the root row").name, "tool");
}

/// The Maven-specific version spellings fail closed (the ADR-0056 PEP 440
/// discipline): `ComparableVersion` admits arbitrary strings, the plan
/// contract is semver-typed — a spelling the contract cannot parse
/// (two-component `1.2`, a property reference) is
/// recognized-but-non-canonical: warned, unpublishable, no edit site,
/// never rewritten. Maven qualifier forms that ARE semver
/// (`1.2.3-SNAPSHOT` — semver prerelease identifiers) stay canonical: the
/// contract's type governs, not Maven's workflow names.
#[test]
fn non_semver_maven_spellings_fail_closed() {
    let mut ctx = DiscoverCtx::new();
    ctx.insert_file(
        "pom.xml",
        br#"<?xml version="1.0"?>
<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <groupId>org.snap</groupId>
  <artifactId>snap-demo</artifactId>
  <version>1.2</version>
</project>
"#
        .to_vec(),
    );

    let rows = MavenAdapter::new().discover(&ctx).expect("discovery");
    assert_eq!(rows.len(), 1);
    let row = rows.first().expect("the root row");
    assert_eq!(
        row.version, None,
        "the two-component spelling is not padded to 1.2.0"
    );
    assert!(
        row.version_site.is_none(),
        "no editable site exists for a spelling the plan contract cannot render"
    );
    assert!(
        row.warnings
            .iter()
            .any(|w| w.code == "non-canonical-version"),
        "the non-canonical spelling is warned, never guessed: {:?}",
        row.warnings
    );
}

/// Maven qualifier forms that parse as semver stay canonical (the plan
/// contract's type governs): `1.2.3-SNAPSHOT` pairs with an edit site —
/// the maintainer releases it, halyard never rewrites the spelling.
#[test]
fn semver_parseable_qualifier_forms_stay_canonical() {
    let mut ctx = DiscoverCtx::new();
    ctx.insert_file(
        "pom.xml",
        br#"<?xml version="1.0"?>
<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <groupId>org.qual</groupId>
  <artifactId>qual-demo</artifactId>
  <version>1.2.3-SNAPSHOT</version>
</project>
"#
        .to_vec(),
    );

    let rows = MavenAdapter::new().discover(&ctx).expect("discovery");
    assert_eq!(rows.len(), 1);
    let row = rows.first().expect("the root row");
    assert_eq!(
        row.version.as_ref().map(std::string::ToString::to_string),
        Some("1.2.3-SNAPSHOT".to_owned()),
        "the qualifier form is semver: it plans verbatim"
    );
    assert!(
        row.version_site.is_some(),
        "a canonical spelling carries its edit site"
    );
    assert!(row.warnings.is_empty(), "no warning for a canonical row");
}

/// A property reference in the version is never resolved (a pure
/// discovery pass does not evaluate build semantics — the fail-closed
/// posture above, recognized-but-non-canonical).
#[test]
fn a_property_reference_version_is_never_resolved() {
    let mut ctx = DiscoverCtx::new();
    ctx.insert_file(
        "pom.xml",
        br#"<?xml version="1.0"?>
<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <groupId>org.props</groupId>
  <artifactId>props-demo</artifactId>
  <version>${project.version}</version>
</project>
"#
        .to_vec(),
    );

    let rows = MavenAdapter::new().discover(&ctx).expect("discovery");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows.first().expect("the root row").version, None);
    assert!(rows.first().expect("the root row").version_site.is_none());
}

/// A listed module whose pom is missing from the committed tree fails
/// discovery (a `<module>` entry is a requirement, not a candidate root —
/// the npm explicit-member posture).
#[test]
fn a_module_member_without_pom_fails_discovery() {
    let mut ctx = DiscoverCtx::new();
    ctx.insert_file("pom.xml", ROOT_POM.as_bytes().to_vec());
    let err = MavenAdapter::new()
        .discover(&ctx)
        .expect_err("the ghost member must fail");
    assert_eq!(err.code(), "E-DISC-GLOB");
}

/// The documented parent rule: *"uses the POM it finds there only if its
/// coordinates match the <parent> element"* — a member whose `<parent>`
/// names a different aggregator refuses instead of guessing the
/// parentage.
#[test]
fn a_parent_coordinate_mismatch_fails_discovery() {
    let mut ctx = DiscoverCtx::new();
    ctx.insert_file("pom.xml", ROOT_POM.as_bytes().to_vec());
    ctx.insert_file("web/pom.xml", WEB_POM.as_bytes().to_vec());
    ctx.insert_file(
        "api/pom.xml",
        br#"<?xml version="1.0"?>
<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <parent>
    <groupId>com.example</groupId>
    <artifactId>some-other-aggregator</artifactId>
    <version>1.2.3</version>
  </parent>
  <artifactId>halyard-demo-api</artifactId>
</project>
"#
        .to_vec(),
    );

    let err = MavenAdapter::new().discover(&ctx).expect_err("must refuse");
    assert!(
        matches!(err, DiscoverError::Manifest { .. }),
        "the coordinate mismatch is a manifest failure, not a guess: {err}"
    );
}

/// No pom at all: an empty result (the orchestrator merges adapters, so
/// an empty result is correct — the go/pypi posture).
#[test]
fn a_repo_without_poms_discovers_nothing() {
    let ctx = DiscoverCtx::new();
    let rows = MavenAdapter::new().discover(&ctx).expect("discovery");
    assert_empty(&rows);
}

//! The M8 `NuGet` cell's discovery contract (`docs/HALYARD_ARCHITECTURE.md`
//! §17 YEAR 2, M8 row — *"More ecosystems"*, the `NuGet` cell — the next
//! listed cell after the shipped `Maven/Gradle` half (ADR-0058), gate
//! *"each: discover/edit/publish/resume tests + replay on 2 real
//! repos"*; VERIFIED grammar in `docs/research/nuget-grammars.md`).
//!
//! This is the plan's acceptance criterion pinned as code: SDK-style
//! `.csproj` projects discover as nuget rows — the package identity
//! follows the documented chain (`PackageId` → `$(AssemblyName)` → the
//! manifest file stem), the pack version resolves `PackageVersion` →
//! `Version` (*"Default is the value of `$(Version)`"*), a
//! `<ProjectReference Include="..\Lib\Lib.csproj"/>` is the
//! proven-internal edge (*"Project to project references are considered
//! by default as `NuGet` package references"*), and every `NuGet`-legal
//! spelling the plan contract's semver cannot render (`1.0.0.0`, a
//! `VersionPrefix`/`VersionSuffix` composite) is warned, unpublishable,
//! never rewritten.

use halyard_core::EcosystemAdapter;
use halyard_core::discover::{DiscoverCtx, DiscoverError, RawVersionSource};
use halyard_core::package::Ecosystem;
use halyard_eco_nuget::NuGetAdapter;

/// Builds a context from `(path, body)` fixture files.
fn ctx_with(files: &[(&str, &str)]) -> DiscoverCtx {
    let mut ctx = DiscoverCtx::new();
    for (path, body) in files {
        assert!(ctx.insert_file(path, body.as_bytes().to_vec()), "{path}");
    }
    ctx
}

const APP_CSPROJ: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<!-- the pack-consuming application project -->
<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <TargetFramework>net10.0</TargetFramework>
    <PackageId>Contoso.App</PackageId>
    <PackageVersion>1.2.3</PackageVersion>
  </PropertyGroup>
  <ItemGroup>
    <ProjectReference Include="..\lib\Lib.csproj" />
    <PackageReference Include="Newtonsoft.Json" Version="13.0.3" />
  </ItemGroup>
</Project>
"#;

const LIB_CSPROJ: &str = r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <TargetFramework>net10.0</TargetFramework>
    <Version>2.0.0</Version>
  </PropertyGroup>
</Project>
"#;

/// The SDK-style workspace discovers: every `.csproj` with the `Sdk`
/// attribute is a row, the identity rides `PackageId` (or the documented
/// `$(AssemblyName)` / file-stem defaults), the pack version resolves
/// `PackageVersion` → `Version`, and the `ProjectReference` naming a
/// member manifest is the proven-internal edge while the
/// `PackageReference` stays registry-resolved (external).
/// The empty-row assertion (the pedantic `assert_is_empty` shape).
fn assert_empty<T>(rows: &[T]) {
    assert!(rows.is_empty(), "expected no rows");
}

#[test]
fn the_sdk_style_workspace_discovers_rows_with_the_proven_edge() {
    let ctx = ctx_with(&[
        ("src/app/App.csproj", APP_CSPROJ),
        ("src/lib/Lib.csproj", LIB_CSPROJ),
    ]);
    let rows = NuGetAdapter::new().discover(&ctx).expect("discovery");
    assert_eq!(rows.len(), 2, "both SDK-style projects are rows");
    let app = rows.first().expect("the app row");
    let lib = rows.get(1).expect("the lib row");
    assert_eq!(app.name, "Contoso.App", "the PackageId is the identity");
    assert_eq!(app.ecosystem, Ecosystem::Nuget);
    assert_eq!(app.manifest, "src/app/App.csproj");
    assert_eq!(
        app.version
            .as_ref()
            .map(std::string::ToString::to_string)
            .as_deref(),
        Some("1.2.3"),
        "PackageVersion is the pack version"
    );
    assert_eq!(app.version_source, RawVersionSource::Local);
    assert!(
        app.publishable,
        "the dotnet argv arm landed (ADR-0059 continued) — the documented \
         packability gate governs, not the disabled posture"
    );
    assert_eq!(
        lib.name, "Lib",
        "the identity falls to the manifest file stem"
    );
    assert_eq!(
        lib.version
            .as_ref()
            .map(std::string::ToString::to_string)
            .as_deref(),
        Some("2.0.0"),
        "Version is the PackageVersion fallback"
    );
    // The proven-internal edge: the ProjectReference names the member's
    // manifest; the dep row resolves to the member's identity name.
    let proj_ref = app
        .deps
        .iter()
        .find(|dep| dep.internal)
        .expect("the project reference is internal");
    assert_eq!(proj_ref.name, "Lib", "the edge names the member identity");
    // The external package reference: registry-resolved, verbatim range.
    let pkg_ref = app
        .deps
        .iter()
        .find(|dep| !dep.internal)
        .expect("the package reference is external");
    assert_eq!(pkg_ref.name, "Newtonsoft.Json");
    assert!(
        !pkg_ref.raw.is_empty(),
        "the range text rides verbatim (ReqSpec::Other)"
    );
}

/// The edit site is the element pack consumes: `PackageVersion` when
/// present, else `Version` — the spans land on the value bytes.
#[test]
fn the_edit_site_is_the_packageversion_or_version_element() {
    let ctx = ctx_with(&[
        ("src/app/App.csproj", APP_CSPROJ),
        ("src/lib/Lib.csproj", LIB_CSPROJ),
    ]);
    let rows = NuGetAdapter::new().discover(&ctx).expect("discovery");
    let app = rows.first().expect("the app row");
    let site = app.version_site.as_ref().expect("the app row has a site");
    assert_eq!(site.file.as_str(), "src/app/App.csproj");
    let text = APP_CSPROJ.as_bytes();
    let expected = text
        .windows(5)
        .position(|w| w == b"1.2.3")
        .expect("fixture offset");
    let span = site.spans.first().expect("the span");
    assert_eq!(
        span.start,
        u32::try_from(expected).expect("fits"),
        "PackageVersion span"
    );
    assert_eq!(
        span.end,
        u32::try_from(expected + 5).expect("fits"),
        "PackageVersion span end"
    );
    let lib = rows.get(1).expect("the lib row");
    let lib_site = lib.version_site.as_ref().expect("the lib row has a site");
    let lib_text = LIB_CSPROJ.as_bytes();
    let lib_expected = lib_text
        .windows(5)
        .position(|w| w == b"2.0.0")
        .expect("fixture offset");
    let lib_span = lib_site.spans.first().expect("the span");
    assert_eq!(
        lib_span.start,
        u32::try_from(lib_expected).expect("fits"),
        "the Version span"
    );
}

/// A project whose root `Project` element carries neither the `Sdk`
/// attribute nor the `Sdk` child element is a classic (non-SDK) project:
/// its metadata may live outside the file — consumed unmodeled with a
/// warning, never guessed.
#[test]
fn a_classic_project_without_an_sdk_is_consumed_unmodeled() {
    let ctx = ctx_with(&[(
        "legacy/Legacy.csproj",
        r#"<Project ToolsVersion="15.0" xmlns="http://schemas.microsoft.com/developer/msbuild/2003">
  <PropertyGroup>
    <Version>1.0.0</Version>
  </PropertyGroup>
</Project>
"#,
    )]);
    let rows = NuGetAdapter::new().discover(&ctx).expect("discovery");
    assert!(rows.is_empty(), "the classic project plans no row");
}

/// The top-level `<Sdk Name="..."/>` child element declares the SDK too
/// (the documented second form) — the project is SDK-style.
#[test]
fn the_sdk_child_element_declares_the_sdk() {
    let ctx = ctx_with(&[(
        "sdkchild/SdkChild.csproj",
        r#"<Project>
  <Sdk Name="Microsoft.NET.Sdk" />
  <PropertyGroup>
    <Version>0.3.0</Version>
  </PropertyGroup>
</Project>
"#,
    )]);
    let rows = NuGetAdapter::new().discover(&ctx).expect("discovery");
    assert_eq!(rows.len(), 1, "the SDK-child form is SDK-style");
    assert_eq!(rows.first().expect("the row").name, "SdkChild");
    assert_eq!(
        rows.first()
            .expect("the row")
            .version
            .as_ref()
            .map(std::string::ToString::to_string)
            .as_deref(),
        Some("0.3.0")
    );
}

/// `NuGet`-legal spellings the plan contract's semver cannot render —
/// four-part `1.0.0.0`, two-part `1.0` — are warned, unpublishable, no
/// edit site, never rewritten (the ADR-0056 discipline; `package-versioning`
/// documents them as *"accepted and equal"* to their canonical forms).
#[test]
fn a_non_semver_nuget_spelling_is_warned_and_never_rewritten() {
    let ctx = ctx_with(&[(
        "four/Four.csproj",
        r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <Version>1.0.0.0</Version>
  </PropertyGroup>
</Project>
"#,
    )]);
    let rows = NuGetAdapter::new().discover(&ctx).expect("discovery");
    assert_eq!(rows.len(), 1);
    let row = rows.first().expect("the row");
    assert!(row.version.is_none(), "the four-part form plans no version");
    assert!(row.version_site.is_none(), "no site — never rewritten");
    assert!(
        row.warnings
            .iter()
            .any(|w| w.code == "non-canonical-version"),
        "the spelling is warned: {:?}",
        row.warnings
    );
}

/// The `VersionPrefix`/`VersionSuffix` composite is evaluation
/// semantics a pure discovery pass does not compute (*"Setting
/// `PackageVersion` overwrites `VersionPrefix`"*) — warned, no site.
#[test]
fn the_prefix_suffix_composite_is_warned_unplannable() {
    let ctx = ctx_with(&[(
        "pre/Pre.csproj",
        r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <VersionPrefix>1.2.0</VersionPrefix>
    <VersionSuffix>rc</VersionSuffix>
  </PropertyGroup>
</Project>
"#,
    )]);
    let rows = NuGetAdapter::new().discover(&ctx).expect("discovery");
    assert_eq!(rows.len(), 1);
    let row = rows.first().expect("the row");
    assert!(row.version.is_none(), "the composite plans no version");
    assert!(
        row.warnings
            .iter()
            .any(|w| w.code == "non-canonical-version"),
        "the composite is warned: {:?}",
        row.warnings
    );
}

/// A project with no version property at all is warned `no-version` and
/// plans no row version (the go/maven no-version posture — inherited
/// values from `Directory.Build.props` are evaluation semantics, §7 of
/// the research note).
#[test]
fn a_project_without_a_version_property_is_warned() {
    let ctx = ctx_with(&[(
        "plain/Plain.csproj",
        r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <TargetFramework>net10.0</TargetFramework>
  </PropertyGroup>
</Project>
"#,
    )]);
    let rows = NuGetAdapter::new().discover(&ctx).expect("discovery");
    assert_eq!(rows.len(), 1);
    let row = rows.first().expect("the row");
    assert!(row.version.is_none());
    assert!(row.version_site.is_none());
    assert!(
        row.warnings.iter().any(|w| w.code == "no-version"),
        "the missing version is warned: {:?}",
        row.warnings
    );
}

/// A conditioned `PropertyGroup` or version element is build evaluation
/// semantics (documented conditions on `ItemGroup`/properties) — the
/// value may not be the pack-time one: consumed unmodeled with a
/// warning, never planned.
#[test]
fn a_conditioned_version_is_consumed_unmodeled() {
    let ctx = ctx_with(&[(
        "cond/Cond.csproj",
        r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup Condition="'$(Configuration)' == 'Release'">
    <Version>9.9.9</Version>
  </PropertyGroup>
  <PropertyGroup>
    <TargetFramework>net10.0</TargetFramework>
  </PropertyGroup>
</Project>
"#,
    )]);
    let rows = NuGetAdapter::new().discover(&ctx).expect("discovery");
    assert_eq!(rows.len(), 1);
    let row = rows.first().expect("the row");
    assert!(
        row.version.is_none(),
        "a conditioned version never plans (the value may not be pack-time)"
    );
    assert!(
        row.warnings.iter().any(|w| w.code == "ignored-section"),
        "the conditioned block is warned: {:?}",
        row.warnings
    );
}

/// `Directory.Build.props` in the tree: the shared-import warning rides
/// every row (the imported defaults are evaluation semantics), while the
/// project file's own version still plans (documented override rule).
#[test]
fn directory_build_props_warns_but_own_versions_still_plan() {
    let ctx = ctx_with(&[
        ("Directory.Build.props", "<Project />\n"),
        ("src/lib/Lib.csproj", LIB_CSPROJ),
    ]);
    let rows = NuGetAdapter::new().discover(&ctx).expect("discovery");
    assert_eq!(rows.len(), 1);
    let row = rows.first().expect("the row");
    assert_eq!(
        row.version
            .as_ref()
            .map(std::string::ToString::to_string)
            .as_deref(),
        Some("2.0.0"),
        "the project file's own Version overrides the imported defaults"
    );
    assert!(
        row.warnings
            .iter()
            .any(|w| w.code == "ignored-section" && w.message.contains("Directory.Build.props")),
        "the shared import is warned: {:?}",
        row.warnings
    );
}

/// A `ProjectReference` naming a file the committed tree does not
/// contain is a fail-closed glob error (the Maven `<module>` posture —
/// a proven edge must prove itself against the tree).
#[test]
fn a_projectreference_outside_the_tree_refuses() {
    let ctx = ctx_with(&[(
        "dangling/Dangling.csproj",
        r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <Version>1.0.0</Version>
  </PropertyGroup>
  <ItemGroup>
    <ProjectReference Include="..\missing\Missing.csproj" />
  </ItemGroup>
</Project>
"#,
    )]);
    let err = NuGetAdapter::new()
        .discover(&ctx)
        .expect_err("the dangling reference refuses");
    assert!(
        matches!(err, DiscoverError::Glob { .. }),
        "fail closed: {err}"
    );
}

/// The identity chain's middle step: `AssemblyName` (the documented
/// `$(PackageId)` default source) is used when `PackageId` is absent.
#[test]
fn the_assembly_name_is_the_identity_fallback() {
    let ctx = ctx_with(&[(
        "asm/Any.csproj",
        r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <AssemblyName>Contoso.Assembly</AssemblyName>
    <Version>1.0.0</Version>
  </PropertyGroup>
</Project>
"#,
    )]);
    let rows = NuGetAdapter::new().discover(&ctx).expect("discovery");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows.first().expect("the row").name, "Contoso.Assembly");
}

/// `<IsPackable>false</IsPackable>`: the project cannot pack (the
/// documented default is `true`) — the row keeps its version (a release
/// tag still plans) and warns.
#[test]
fn an_unpackable_project_keeps_its_version_and_warns() {
    let ctx = ctx_with(&[(
        "nopack/NoPack.csproj",
        r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <IsPackable>false</IsPackable>
    <Version>1.0.0</Version>
  </PropertyGroup>
</Project>
"#,
    )]);
    let rows = NuGetAdapter::new().discover(&ctx).expect("discovery");
    assert_eq!(rows.len(), 1);
    let row = rows.first().expect("the row");
    assert_eq!(
        row.version
            .as_ref()
            .map(std::string::ToString::to_string)
            .as_deref(),
        Some("1.0.0"),
        "the version still plans"
    );
    assert!(
        row.warnings.iter().any(|w| w.code == "not-packable"),
        "the unpackable declaration is warned: {:?}",
        row.warnings
    );
}

/// A later re-declaration overwrites the earlier one (the documented
/// override rule): the row's version and edit site follow the winning
/// element.
#[test]
fn a_redeclared_version_follows_the_last_declaration() {
    let ctx = ctx_with(&[("twice/Twice.csproj", TWICE_CSPROJ)]);
    let rows = NuGetAdapter::new().discover(&ctx).expect("discovery");
    assert_eq!(rows.len(), 1);
    let row = rows.first().expect("the row");
    assert_eq!(
        row.version
            .as_ref()
            .map(std::string::ToString::to_string)
            .as_deref(),
        Some("1.4.0"),
        "the later declaration wins"
    );
    let site = row.version_site.as_ref().expect("the winner has a site");
    let expected = TWICE_CSPROJ
        .as_bytes()
        .windows(5)
        .position(|w| w == b"1.4.0")
        .expect("offset");
    assert_eq!(
        site.spans.first().expect("the span").start,
        u32::try_from(expected).expect("fits")
    );
}

/// No `.csproj` at all: an empty result (the orchestrator merges
/// adapters, so an empty result is correct — the go/pypi/maven posture).
#[test]
fn a_repo_without_csproj_files_discovers_nothing() {
    let ctx = DiscoverCtx::new();
    let rows = NuGetAdapter::new().discover(&ctx).expect("discovery");
    assert_empty(&rows);
}

/// The twice fixture (kept beside the test that pins its offset).
const TWICE_CSPROJ: &str = r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <Version>1.0.0</Version>
  </PropertyGroup>
  <PropertyGroup>
    <Version>1.4.0</Version>
  </PropertyGroup>
</Project>
"#;

/// The adapter consumes only `.csproj` files from the context: other
/// manifests (a stray `pom.xml`) never produce nuget rows.
#[test]
fn only_csproj_files_are_nuget_manifests() {
    let ctx = ctx_with(&[("pom.xml", "<project/>")]);
    let rows = NuGetAdapter::new().discover(&ctx).expect("discovery");
    assert_empty(&rows);
}

// The §17 M8 `NuGet` cell's publish half (ADR-0059): rows carry
// `publishable = true` the day the `dotnet` argv arm lands — the
// documented packability posture (`docs/research/nuget-grammars.md`
// §10) is the gate: *"IsPackable — A Boolean value that specifies
// whether the project can be packed. The default value is `true`"*;
// `<IsPackable>false</IsPackable>` keeps the row unpublishable.
#[test]
fn canonical_rows_publish_and_unpackable_rows_do_not() -> Result<(), Box<dyn std::error::Error>> {
    // A ref-free canonical row (a `ProjectReference` outside the ctx
    // refuses at the glob gate — the proven-edge contract).
    const CANONICAL: &str = r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <TargetFramework>net10.0</TargetFramework>
    <PackageId>Contoso.App</PackageId>
    <PackageVersion>1.2.3</PackageVersion>
  </PropertyGroup>
</Project>
"#;
    let rows = NuGetAdapter::new()
        .discover(&ctx_with(&[("App.csproj", CANONICAL)]))
        .expect("discovery");
    let row = rows.first().ok_or("the canonical row exists")?;
    assert!(
        row.publishable,
        "a canonical SDK-style row is publishable (the argv arm landed)"
    );

    let unpackable = r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <Version>1.0.0</Version>
    <IsPackable>false</IsPackable>
  </PropertyGroup>
</Project>
"#;
    let rows = NuGetAdapter::new()
        .discover(&ctx_with(&[("App.csproj", unpackable)]))
        .expect("discovery");
    let row = rows.first().ok_or("the unpackable row exists")?;
    assert!(
        !row.publishable,
        "IsPackable=false keeps the row unpublishable"
    );
    assert!(
        row.warnings.iter().any(|w| w.code == "not-packable"),
        "the packability refusal is a recorded warning: {:?}",
        row.warnings
    );
    Ok(())
}

//! Bench: `NuGet` csproj scan (§6.4's hot path at the .csproj grammar, M8
//! `NuGet` cell, ADR-0059). Measures the hand-rolled scanner + row
//! assembly over a representative SDK-style workspace (three project
//! files with property groups, package references, a project-reference
//! edge, comments, and one `PackageVersion` row).

use criterion::Criterion;
use halyard_core::EcosystemAdapter;
use halyard_core::discover::DiscoverCtx;
use halyard_eco_nuget::NuGetAdapter;

const APP_CSPROJ: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<!-- the pack-consuming application project -->
<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <TargetFramework>net10.0</TargetFramework>
    <PackageId>bench-demo.app</PackageId>
    <PackageVersion>1.4.0</PackageVersion>
  </PropertyGroup>
  <ItemGroup>
    <PackageReference Include="Newtonsoft.Json" Version="13.0.3" />
    <ProjectReference Include="..\lib\Lib.csproj" />
    <ProjectReference Include="..\src\tools\Tools.csproj" />
  </ItemGroup>
</Project>
"#;

const LIB_CSPROJ: &str = r#"<Project Sdk="Microsoft.NET.Sdk">
  <!-- the library; the identity falls to the file stem -->
  <PropertyGroup>
    <TargetFramework>net10.0</TargetFramework>
    <AssemblyName>bench-demo.lib</AssemblyName>
    <Version>1.4.0</Version>
  </PropertyGroup>
  <ItemGroup>
    <PackageReference Include="System.Text.Json" Version="9.0.4" />
  </ItemGroup>
</Project>
"#;

const TOOLS_CSPROJ: &str = r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <TargetFramework>net10.0</TargetFramework>
    <!-- 1.0.0.0 is NuGet-legal and non-canonical: warned, never rewritten -->
    <Version>1.0.0.0</Version>
  </PropertyGroup>
</Project>
"#;

fn ctx() -> DiscoverCtx {
    let mut ctx = DiscoverCtx::new();
    ctx.insert_file("src/app/App.csproj", APP_CSPROJ.as_bytes().to_vec());
    ctx.insert_file("src/lib/Lib.csproj", LIB_CSPROJ.as_bytes().to_vec());
    ctx.insert_file("src/tools/Tools.csproj", TOOLS_CSPROJ.as_bytes().to_vec());
    ctx
}

fn scan() {
    let ctx = ctx();
    // The fixture is a constant that must parse; a parse failure exits
    // the bench loudly (bench fns are not `#[test]`, so the test-lint
    // allowance does not reach them — the sibling pypi bench posture).
    let rows = match NuGetAdapter::new().discover(&ctx) {
        Ok(rows) => rows,
        Err(error) => {
            eprintln!("the bench fixture must parse: {error}");
            std::process::exit(101);
        }
    };
    assert_eq!(rows.len(), 3);
}

fn bench(c: &mut Criterion) {
    c.bench_function("nuget_csproj_scan_3_projects", |b| b.iter(scan));
}

/// The bench entry point (the criterion macros generate undocumented
/// items — the workspace's missing-docs gate forbids them; the sibling
/// benches call the group directly).
fn main() {
    bench(&mut Criterion::default().configure_from_args());
}

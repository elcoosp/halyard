//! Bench: `NuGet` csproj edit (the release half's hot path, M8 `NuGet`
//! cell, ADR-0059). Measures the semantic patch — parse, slot match,
//! in-place byte splice, read-back re-parse — over a representative
//! SDK-style project file.

use criterion::Criterion;
use halyard_core::EcosystemAdapter;
use halyard_core::package::RelPath;
use halyard_core::plan::PlanPatch;
use halyard_eco_nuget::NuGetAdapter;

const APP: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<!-- the pack-consuming application project -->
<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <TargetFramework>net10.0</TargetFramework>
    <PackageId>bench-demo.app</PackageId>
    <PackageVersion>1.4.0</PackageVersion>
  </PropertyGroup>
  <ItemGroup>
    <PackageReference Include="Newtonsoft.Json" Version="13.0.3" />
  </ItemGroup>
</Project>
"#;

fn edit() {
    let file = RelPath::new("src/app/App.csproj").expect("fixture path");
    let start = u32::try_from(APP.find("<PackageVersion>").expect("fixture")).expect("fits") + 16;
    let patches = [PlanPatch {
        start,
        end: start + 5,
        text: "1.5.0".to_owned(),
    }];
    let out = NuGetAdapter
        .edit_versions(&file, APP.as_bytes(), &patches)
        .expect("bench fixture edits");
    assert_eq!(out.applied, 1);
}

fn bench(c: &mut Criterion) {
    c.bench_function("nuget_csproj_edit", |b| b.iter(edit));
}

/// The bench entry point (the criterion macros generate undocumented
/// items — the workspace's missing-docs gate forbids them; the sibling
/// benches call the group directly).
fn main() {
    bench(&mut Criterion::default().configure_from_args());
}

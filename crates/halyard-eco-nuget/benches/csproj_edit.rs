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
    // The fixture is a constant that must parse; a miss exits loudly
    // (bench fns are not `#[test]`, so the test-lint allowance does not
    // reach them — the pom_edit bench posture).
    let fail = |what: &str| -> ! {
        eprintln!("the bench fixture must {what}");
        std::process::exit(101);
    };
    let Some(file) = RelPath::new("src/app/App.csproj") else {
        fail("carry a normalized fixture path");
    };
    let start = (APP
        .find("<PackageVersion>")
        .unwrap_or_else(|| fail("carry the pack version row"))
        + "<PackageVersion>".len())
    .try_into()
    .unwrap_or_else(|_| fail("carry the pack version row"));
    let patches = [PlanPatch {
        start,
        end: start + 5,
        text: "1.5.0".to_owned(),
    }];
    match NuGetAdapter.edit_versions(&file, APP.as_bytes(), &patches) {
        Ok(out) if out.applied == 1 => {}
        _ => {
            eprintln!("the bench fixture must edit");
            std::process::exit(101);
        }
    }
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

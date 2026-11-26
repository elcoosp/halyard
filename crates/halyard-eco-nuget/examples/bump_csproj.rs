//! Example: apply a plan-style version patch to a `NuGet` project file and
//! inspect the outcome (M8, the §17 **`NuGet`** cell — ADR-0059, the
//! release half).
//!
//! Run: `cargo run -p halyard-eco-nuget --example bump_csproj`
//!
//! The patch lands on the element pack consumes (`PackageVersion` when
//! present, else `Version`), the value's inner bytes swap right-to-left,
//! every other byte of the project file survives verbatim (I4), and the
//! read-back re-parse verifies the target before the patch is accepted.

use halyard_core::EcosystemAdapter;
use halyard_core::package::RelPath;
use halyard_core::plan::PlanPatch;
use halyard_eco_nuget::NuGetAdapter;

const APP: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<!-- the pack-consuming application project -->
<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <TargetFramework>net10.0</TargetFramework>
    <PackageId>demo.app</PackageId>
    <PackageVersion>1.2.3</PackageVersion>
  </PropertyGroup>
  <ItemGroup>
    <PackageReference Include="Newtonsoft.Json" Version="13.0.3" />
  </ItemGroup>
</Project>
"#;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let file = RelPath::new("src/app/App.csproj").ok_or("the fixture path is normalized")?;
    // The plan's own-version patch: the span discovery emitted (the
    // PackageVersion value) carries the plan's target spelling.
    let start = u32::try_from(
        APP.find("<PackageVersion>")
            .ok_or("the fixture carries a PackageVersion row")?
            + 16,
    )?;
    let patches = [PlanPatch {
        start,
        end: start + 5,
        text: "1.3.0".to_owned(),
    }];
    let out = NuGetAdapter.edit_versions(&file, APP.as_bytes(), &patches)?;
    println!("applied {} patch(es), skipped {}", out.applied, out.skipped);
    let bumped = String::from_utf8(out.content)
        .map_err(|error| format!("the patched project file is utf-8: {error}"))?;
    println!(
        "the markup around the value survives verbatim: {}",
        bumped.contains("    <PackageVersion>1.3.0</PackageVersion>")
    );
    Ok(())
}

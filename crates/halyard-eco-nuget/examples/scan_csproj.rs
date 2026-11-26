//! Example: discover a `NuGet` SDK-style workspace and inspect the rows
//! (M8, the §17 **`NuGet`** cell — ADR-0059).
//!
//! Run: `cargo run -p halyard-eco-nuget --example scan_csproj`
//!
//! Every SDK-style `.csproj` discovers as a nuget row: the identity
//! follows the documented chain (`PackageId` → `AssemblyName` → the
//! file stem), the pack version resolves `PackageVersion` → `Version`
//! (the edit site is the element pack consumes), a `<ProjectReference>`
//! naming a member manifest is the proven-internal edge, a
//! `<PackageReference>` stays registry-resolved (external), and
//! `NuGet`-specific version spellings fail closed (warned, unpublishable,
//! never rewritten).

use halyard_core::EcosystemAdapter;
use halyard_core::discover::DiscoverCtx;
use halyard_eco_nuget::NuGetAdapter;

const APP_CSPROJ: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<!-- the pack-consuming application project -->
<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <TargetFramework>net10.0</TargetFramework>
    <PackageId>demo.app</PackageId>
    <PackageVersion>1.2.3</PackageVersion>
  </PropertyGroup>
  <ItemGroup>
    <PackageReference Include="Newtonsoft.Json" Version="13.0.3" />
    <ProjectReference Include="..\lib\Lib.csproj" />
  </ItemGroup>
</Project>
"#;

const LIB_CSPROJ: &str = r#"<Project Sdk="Microsoft.NET.Sdk">
  <!-- the library; the identity falls to the AssemblyName property -->
  <PropertyGroup>
    <TargetFramework>net10.0</TargetFramework>
    <AssemblyName>demo.lib</AssemblyName>
    <Version>1.2.3</Version>
  </PropertyGroup>
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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut ctx = DiscoverCtx::new();
    ctx.insert_file("src/app/App.csproj", APP_CSPROJ.as_bytes().to_vec());
    ctx.insert_file("src/lib/Lib.csproj", LIB_CSPROJ.as_bytes().to_vec());
    // A NuGet-specific spelling rides along to show the fail-closed
    // posture (NuGetVersion accepts it; the plan contract cannot render
    // it).
    ctx.insert_file("src/tools/Tools.csproj", TOOLS_CSPROJ.as_bytes().to_vec());

    let rows = NuGetAdapter::new().discover(&ctx)?;
    println!("discovered {} nuget rows:\n", rows.len());
    for row in &rows {
        println!(
            "  {} @ {} (manifest {})",
            row.name,
            if row.root.is_empty() {
                "<root>"
            } else {
                &row.root
            },
            row.manifest
        );
        println!(
            "    version: {:?} (source {:?})",
            row.version.as_ref().map(std::string::ToString::to_string),
            row.version_source
        );
        if let Some(site) = &row.version_site {
            println!("    edit site: {} spans {:?}", site.file, site.spans);
        }
        println!(
            "    publishable: {} (no dotnet publish grammar yet — ADR-0059)",
            row.publishable
        );
        for dep in &row.deps {
            println!(
                "    dependency {:?} {} internal={}",
                dep.name,
                String::from_utf8_lossy(&dep.raw),
                dep.internal
            );
        }
        for warning in &row.warnings {
            println!("    warning [{}]: {}", warning.code, warning.message);
        }
        println!();
    }
    Ok(())
}

//! CLI e2e over the `dotnet` publish argv arm (the §17 M8 `NuGet`
//! cell's recorded-open argv arm, ADR-0059): a repository whose only
//! packages are SDK-style `.csproj` projects, publishing to nuget.org.
//! The spawn is exercised through a PATH shim that records the argv —
//! the packaged tool never runs; the argv is the assertion.
//!
//! The documented flow (`docs/research/nuget-grammars.md` §11): bare
//! `dotnet pack` (the quickstart's own command), then `dotnet nuget
//! push bin/Debug/<id>.<version>.nupkg --source
//! https://api.nuget.org/v3/index.json` — the key rides the documented
//! `NUGET_API_KEY` environment pair (I7: the argv never carries it).
//! The dry forecast runs the packaging half only: the push has **no
//! documented dry form** and no flag is invented (ADR-0033).

use std::process::Command;

use halyard_test_support::{CommitSpec, GitFixture};

type BoxError = Box<dyn std::error::Error>;

fn fixture_dir(label: &str) -> Result<std::path::PathBuf, BoxError> {
    let dir =
        std::env::temp_dir().join(format!("halyard-cli-nuget-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// A `dotnet` stand-in that records its argv and exits 0 (the packaged
/// tool never runs; the argv is the assertion).
fn write_dotnet_shim(label: &str) -> Result<std::path::PathBuf, BoxError> {
    let dir = fixture_dir(label)?;
    let script = dir.join("dotnet");
    std::fs::write(
        &script,
        "#!/bin/sh\nprintf '%s\\n' \"$@\" >> \"$HALYARD_SHIM_LOG\"\nexit 0\n",
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&script)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script, perms)?;
    }
    Ok(dir)
}

/// A nuget-only repository: two SDK-style `.csproj` members (the
/// identity chain: `PackageId` / the file stem), a feat commit driving
/// the release rows, and the publish config enabling the run.
fn build_workspace(label: &str) -> Result<GitFixture, BoxError> {
    let dir = fixture_dir(label)?;
    let mut fx = GitFixture::init_in(&dir)?;
    let scaffold = vec![
        (
            "halyard.toml".to_owned(),
            "[publish]\nenabled = true\n".to_owned(),
        ),
        (
            "App/App.csproj".to_owned(),
            r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <TargetFramework>net10.0</TargetFramework>
    <PackageId>Contoso.App</PackageId>
    <PackageVersion>0.1.0</PackageVersion>
  </PropertyGroup>
</Project>
"#
            .to_owned(),
        ),
        (
            "Lib/Lib.csproj".to_owned(),
            r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <TargetFramework>net10.0</TargetFramework>
    <Version>0.2.0</Version>
  </PropertyGroup>
</Project>
"#
            .to_owned(),
        ),
        ("App/Program.cs".to_owned(), "the app\n".to_owned()),
    ];
    fx.commit_with(&CommitSpec::files(
        scaffold,
        "chore: scaffold the nuget workspace",
    ))?;
    fx.commit_with(&CommitSpec::files(
        vec![("App/Program.cs".to_owned(), "the app changes\n".to_owned())],
        "feat: the app changes",
    ))?;
    Ok(fx)
}

fn halyard_env(
    root: &std::path::Path,
    args: &[&str],
    shim_dir: &std::path::Path,
    shim_log: &std::path::Path,
    vars: &[(&str, &str)],
) -> Result<std::process::Output, BoxError> {
    let mut command = Command::new(env!("CARGO_BIN_EXE_halyard"));
    command.arg("-C").arg(root).args(args).env("NO_COLOR", "1");
    let original = std::env::var("PATH").unwrap_or_default();
    let path = if original.is_empty() {
        shim_dir.display().to_string()
    } else {
        format!("{}:{original}", shim_dir.display())
    };
    command.env("PATH", path);
    command.env("HALYARD_SHIM_LOG", shim_log);
    for (key, value) in vars {
        command.env(key, value);
    }
    command.output().map_err(Into::into)
}

fn read_log(log: &std::path::Path) -> Result<String, BoxError> {
    Ok(std::fs::read_to_string(log).unwrap_or_default())
}

/// The dry forecast: the nuget targets exist (the argv arm flips the
/// engine's disabled shape), the shim captured the documented pack argv,
/// and — the push having no documented dry form — the push never ran.
#[test]
fn publish_dry_forecasts_nuget_rows_and_packs_only() -> Result<(), BoxError> {
    let fx = build_workspace("dry")?;
    let shim = write_dotnet_shim("shim-dry")?;
    let log = fixture_dir("shim-log-dry")?.join("argv.log");
    let out = halyard_env(
        fx.root(),
        &["publish", "--dry", "--format", "json"],
        &shim,
        &log,
        &[],
    )?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.status.success(), "publish --dry failed: {text}");
    let value: serde_json::Value = serde_json::from_str(
        text.trim_start_matches(char::is_whitespace)
            .trim_start_matches(|c: char| c != '{'),
    )
    .map_err(|e| format!("the report is JSON: {e}\n{text}"))?;
    let rows = value
        .get("rows")
        .and_then(serde_json::Value::as_array)
        .ok_or("rows is an array")?;
    let row = rows
        .iter()
        .find(|row| row.get("package").and_then(serde_json::Value::as_str) == Some("Contoso.App"))
        .ok_or("the nuget target forecasts")?;
    assert_eq!(
        row.pointer("/outcome/kind")
            .and_then(serde_json::Value::as_str),
        Some("forecast"),
        "the argv arm flipped the disabled shape: {row}"
    );
    // The disabled-shape skip row is gone.
    let not_targets = value
        .get("not_targets")
        .and_then(serde_json::Value::as_array)
        .ok_or("not_targets is an array")?;
    assert!(
        !not_targets.iter().any(
            |row| row.get("package").and_then(serde_json::Value::as_str) == Some("Contoso.App")
        ),
        "no publish_disabled skip for the nuget target: {not_targets:?}"
    );
    // The shim captured the documented packaging half; no push argv.
    let recorded = read_log(&log)?;
    assert!(recorded.contains("pack"), "the pack argv ran: {recorded}");
    assert!(
        !recorded.contains("push"),
        "the push has no documented dry form and never runs in a forecast: {recorded}"
    );
    Ok(())
}

/// The live run: pack then push, the documented official-server
/// `--source`, and the credential riding the `NUGET_API_KEY` environment
/// pair — the shim's argv carries no key fragment (I7).
#[test]
fn publish_live_spawns_the_documented_pack_then_push() -> Result<(), BoxError> {
    let fx = build_workspace("live")?;
    let shim = write_dotnet_shim("shim-live")?;
    let log = fixture_dir("shim-log-live")?.join("argv.log");
    let out = halyard_env(
        fx.root(),
        &["publish", "--format", "json"],
        &shim,
        &log,
        &[("NUGET_API_KEY", "shim-secret-token")],
    )?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.status.success(), "publish failed: {text}");
    let recorded = read_log(&log)?;
    // The pack step, then the push step with the documented source.
    assert!(recorded.contains("pack"), "pack ran: {recorded}");
    assert!(
        recorded.contains("push") && recorded.contains("--source"),
        "push ran with the documented source: {recorded}"
    );
    assert!(
        recorded.contains("bin/Debug/Contoso.App.0.1.1.nupkg"),
        "the push targets the pack output the quickstart documents: {recorded}"
    );
    assert!(
        recorded.contains("https://api.nuget.org/v3/index.json"),
        "the documented official-server source: {recorded}"
    );
    // The argv never carries the credential (I7) — the environment pair
    // is the documented surface (`NUGET_API_KEY`).
    assert!(
        !recorded.contains("shim-secret-token"),
        "the token never reaches argv: {recorded}"
    );
    Ok(())
}

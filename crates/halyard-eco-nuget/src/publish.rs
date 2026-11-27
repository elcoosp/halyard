//! The `dotnet` publish argv table (the §17 M8 `NuGet` cell's
//! recorded-open argv arm, ADR-0059): the shell owns the spawn, the
//! adapter owns the grammar. Verified against Microsoft Learn
//! (`docs/research/nuget-grammars.md` §11): the push reference states
//! the split itself — *"The command pushes an existing package. It
//! doesn't create a package. To create a package, use `dotnet pack`."*
//!
//! The two spawn steps:
//!
//! 1. bare `dotnet pack` — the quickstart's own command ("To build a
//!    `NuGet` package or `*.nupkg` file from the project, run the `dotnet`
//!    pack command"); its sample output documents where the artifact
//!    lands (`...\bin\Debug\Contoso....1.0.0.nupkg` — the default
//!    Debug configuration).
//! 2. `dotnet nuget push bin/Debug/<id>.<version>.nupkg
//!    --source https://api.nuget.org/v3/index.json` — the documented
//!    official-server example (`dotnet nuget push foo.nupkg -k … -s
//!    https://api.nuget.org/v3/index.json`); the credential rides the
//!    documented `NUGET_API_KEY` environment pair through the auth seam
//!    ("Starting with `NuGet` 7.6 (.NET SDK 10.0.300), you can set the
//!    `NUGET_API_KEY` environment variable instead of passing
//!    `--api-key` on the command line") — the argv never carries it
//!    (invariant I7). The `--source` rides the documented URL (the
//!    reference's own nuget.org example); a registry name the publish
//!    contract does not document a URL for declines instead of guessing
//!    (validate-then-act, §15.3).
//!
//! The dry posture: `dotnet pack` is local-only (no registry contact —
//! the packaging half is safe to exercise in a forecast), but the push
//! carries **no documented dry form** and no flag is invented
//! (ADR-0033): the dry forecast runs the pack and records the would-run
//! push argv, the live run spawns both.
//!
//! The rows are argv atoms (I7).

/// The `nuget.org` push endpoint the reference's own example names.
const NUGET_ORG_SOURCE: &str = "https://api.nuget.org/v3/index.json";

/// The `dotnet` pack step: bare, the quickstart's own command (the
/// artifact lands in `bin/Debug/` per the documented sample output; the
/// step is local-only — it exercises safely in a dry forecast).
#[must_use]
pub fn dotnet_pack_args() -> Vec<String> {
    vec!["pack".to_owned()]
}

/// The `dotnet` push step for one target: the pack output path
/// (`bin/Debug/<id>.<version>.nupkg` — the documented sample location,
/// the version the plan moves to) and the documented official-server
/// source. `registry` names the plan contract's registry; only
/// `nuget.org` has a documented URL mapping today — any other name
/// declines (the error names the one documented registry).
///
/// # Errors
///
/// A registry name without a documented push URL (the plan contract
/// names `nuget.org` only).
pub fn dotnet_push_args(
    registry: &str,
    package: &str,
    version: &str,
) -> Result<Vec<String>, String> {
    if registry != "nuget.org" {
        return Err(format!(
            "registry {registry:?} has no documented `dotnet nuget push` URL mapping — the \
             publish contract documents nuget.org only; extend the grammar through an ADR"
        ));
    }
    Ok(vec![
        "nuget".to_owned(),
        "push".to_owned(),
        format!("bin/Debug/{package}.{version}.nupkg"),
        "--source".to_owned(),
        NUGET_ORG_SOURCE.to_owned(),
    ])
}

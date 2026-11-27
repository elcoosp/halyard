//! The `dotnet` publish argv grammar contract (the §17 M8 `NuGet` cell's
//! recorded-open argv arm, ADR-0059 — *"the `dotnet pack`/`dotnet nuget
//! push` argv arm, the nuget.org probe arm"* — the plan line these
//! tests pin).
//!
//! The grammar is the documented flow, verified against Microsoft Learn
//! (`docs/research/nuget-grammars.md` §11): the push reference states
//! the split itself — *"The command pushes an existing package. It
//! doesn't create a package. To create a package, use `dotnet pack`."*
//! — so the arm is two spawn steps: bare `dotnet pack` (the
//! quickstart's own command; its sample output documents the `.nupkg`
//! landing in `bin/Debug/<id>.<version>.nupkg`) then `dotnet nuget
//! push <that path> --source https://api.nuget.org/v3/index.json` (the
//! documented official-server example). No flag is ever invented
//! (ADR-0033): the credential rides the documented `NUGET_API_KEY`
//! environment variable through the auth seam (*"Starting with NuGet
//! 7.6 (.NET SDK 10.0.300), you can set the `NUGET_API_KEY` environment
//! variable instead of passing `--api-key` on the command line"*), the
//! argv never carries it (invariant I7).
//!
//! The dry posture: `dotnet pack` is local-only (no registry contact),
//! but the push has **no documented dry form** — the dry forecast runs
//! the packaging half and records the would-run push argv, never an
//! invented `--dry-run`.

use halyard_eco_nuget::{dotnet_pack_args, dotnet_push_args};

#[test]
fn pack_argv_is_the_documented_bare_pack() {
    // The quickstart's own command: "To build a NuGet package or
    // *.nupkg* file from the project, run the dotnet pack command".
    assert_eq!(dotnet_pack_args(), vec!["pack".to_owned()]);
}

#[test]
fn push_argv_is_the_documented_official_server_form() {
    // The documented example: `dotnet nuget push foo.nupkg -k ...
    // -s https://api.nuget.org/v3/index.json` — the path is the pack
    // output the quickstart's sample output documents
    // (`bin/Debug/<id>.<version>.nupkg`); the key rides the
    // `NUGET_API_KEY` environment pair, never argv (I7).
    let args = dotnet_push_args("nuget.org", "Contoso.App", "1.2.3")
        .expect("nuget.org has a documented push form");
    assert_eq!(
        args,
        vec![
            "nuget".to_owned(),
            "push".to_owned(),
            "bin/Debug/Contoso.App.1.2.3.nupkg".to_owned(),
            "--source".to_owned(),
            "https://api.nuget.org/v3/index.json".to_owned(),
        ]
    );
}

#[test]
fn push_argv_declines_unknown_registries() {
    // The plan contract names `nuget.org` only; a custom registry name
    // has no documented URL mapping in the publish contract — the
    // grammar declines instead of guessing (validate-then-act §15.3).
    let error = dotnet_push_args("private-feed", "Contoso.App", "1.2.3")
        .expect_err("an unmapped registry declines");
    assert!(
        error.contains("nuget.org"),
        "the decline names the one documented registry: {error}"
    );
}

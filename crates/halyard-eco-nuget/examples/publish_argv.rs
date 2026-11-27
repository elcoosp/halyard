//! Example: the `dotnet` publish argv table (the §17 M8 `NuGet` cell's
//! argv arm, ADR-0059 continued).
//!
//! Run: `cargo run -p halyard-eco-nuget --example publish_argv`
//!
//! The documented two-step flow (the push reference: *"The command
//! pushes an existing package. It doesn't create a package. To create a
//! package, use `dotnet pack`."* — `docs/research/nuget-grammars.md`
//! §11): bare `dotnet pack`, then the documented official-server push
//! whose credential rides the `NUGET_API_KEY` environment pair (I7 —
//! the argv never carries it). The dry forecast runs the packaging half
//! only: the push has no documented dry form and no flag is invented.

use halyard_eco_nuget::{dotnet_pack_args, dotnet_push_args};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "pack argv: {:?} (local-only; exercises in a dry forecast)",
        dotnet_pack_args()
    );
    let push = dotnet_push_args("nuget.org", "Contoso.App", "1.2.4")?;
    println!("push argv: {push:?} (the live half; NUGET_API_KEY rides the environment pair, I7)");
    let declined = dotnet_push_args("private-feed", "Contoso.App", "1.2.4");
    println!("an unmapped registry declines: {declined:?}");
    Ok(())
}

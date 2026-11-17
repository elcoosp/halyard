//! The npm publish-argv half (M5.2 step 2 + ADR-0033): the `pnpm`
//! publish argv table over every option shape — the rows the spawn
//! executes and the dry forecast shows. Unset is semantic: a bare
//! `publish` argv is the honest default (the tool's own trusted-
//! publishing posture and the registry's access default govern).
//!
//! Run: `cargo run -p halyard-eco-npm --example publish_argv`

use halyard_eco_npm::{NpmPublishOptions, pnpm_publish_args};

fn main() {
    // The unset shape: no flag the user did not set.
    println!(
        "unset            → {}",
        pnpm_publish_args(false, &NpmPublishOptions::default()).join(" ")
    );
    // The dry forecast of the unset shape: publish-time semantics ride
    // the documented dry form.
    println!(
        "unset, dry       → {}",
        pnpm_publish_args(true, &NpmPublishOptions::default()).join(" ")
    );
    // The explicit ADR-0033 shapes: access before provenance, fixed
    // order, on both the live and the dry form.
    let public_provenance = NpmPublishOptions {
        access: Some("public".to_owned()),
        provenance: Some(true),
    };
    println!(
        "public+prov      → {}",
        pnpm_publish_args(false, &public_provenance).join(" ")
    );
    let restricted_no_provenance = NpmPublishOptions {
        access: Some("restricted".to_owned()),
        provenance: Some(false),
    };
    println!(
        "restricted−prov  → {}",
        pnpm_publish_args(true, &restricted_no_provenance).join(" ")
    );
}

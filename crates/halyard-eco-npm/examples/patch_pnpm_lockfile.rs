//! Example: patch a `pnpm-lock.yaml` for the plan's version moves (M3.4).
//!
//! Run: `cargo run -p halyard-eco-npm --example patch_pnpm_lockfile`
//!
//! Demonstrates the §9.3 lockfile seam: the planned version moves drive
//! the rewrite of importer `specifier:` rows (plain semver advances
//! through the canonical rewriter) and `<version>(link:…)` ref rows,
//! byte-in place (invariant I4). Anything the patcher cannot guarantee
//! pnpm-identical bytes for — resolution keys under `packages:`,
//! foreign-grammar specifiers, unknown headers — comes back as
//! `NeedsNative` (§9.3) instead of a guess.

use halyard_core::lockfile::{LockfileOutcome, LockfileVersionChange};
use halyard_eco_npm::lockfile::patch_pnpm_lockfile;

const LOCKFILE: &str = r"lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      ui:
        specifier: workspace:^
        version: link:packages/ui

  packages/app:
    dependencies:
      ui:
        specifier: ^0.4.1
        version: 0.4.1(link:../ui)

packages:

  /lodash@4.17.21:
    resolution: {integrity: sha512-aaaa}
";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let changes = [LockfileVersionChange {
        name: "ui".to_owned(),
        from: "0.4.1".to_owned(),
        to: "0.5.0".to_owned(),
    }];
    let outcome = patch_pnpm_lockfile(LOCKFILE, "pnpm-lock.yaml", &changes)?;
    match outcome {
        LockfileOutcome::Patched {
            content,
            applied,
            skipped,
        } => {
            let text = String::from_utf8(content)?;
            println!("patched: {applied} applied, {skipped} skipped\n");
            println!("{text}");
        }
        LockfileOutcome::NeedsNative { reason } => {
            println!("needs native refresh: {reason}");
        }
    }
    Ok(())
}

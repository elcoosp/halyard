//! Edit demo (M1.4): patch a manifest in memory, verify, and apply it to
//! a real file with the atomic protocol.
//!
//! Run: `cargo run -p halyard-edit --example apply_patch`

use std::error::Error;
use std::path::Path;

use halyard_edit::{EditError, EditSet, Patch, apply, apply_to_file_verified};

fn main() -> Result<(), Box<dyn Error>> {
    // 1. In-memory application (the planner's dry-run view).
    let manifest = b"[package]\nname = \"core\"\nversion = \"0.4.1\"\n";
    let value_start = manifest
        .windows(5)
        .position(|w| w == b"0.4.1")
        .ok_or("fixture lost its version")?;
    let patch = Patch::new(
        u32::try_from(value_start)?,
        u32::try_from(value_start + 5)?,
        "0.5.0".to_owned(),
    )?;
    let patched = apply(manifest, std::slice::from_ref(&patch))?;
    println!("before: {}", String::from_utf8_lossy(manifest).trim());
    println!("after:  {}", String::from_utf8_lossy(&patched).trim());

    // 2. EditSet: validated, sorted, deterministic per-file application.
    let mut set = EditSet::new();
    set.add(
        halyard_core::RelPath::new("Cargo.toml").ok_or("bad path")?,
        patch,
    )?;
    println!(
        "edit set: {} file(s), empty={}",
        set.files().count(),
        set.is_empty()
    );
    let file = halyard_core::RelPath::new("Cargo.toml").ok_or("bad path")?;
    let via_set = set.apply_content(&file, manifest)?;
    println!("via set:  {}", String::from_utf8_lossy(&via_set).trim());

    // 3. Atomic apply to a real file (temp → fsync → rename), with a
    //    re-parse-style verifier that must accept before anything lands.
    let demo_dir = std::env::temp_dir();
    let demo_path = demo_dir.join("halyard-edit-demo-Cargo.toml");
    std::fs::write(&demo_path, manifest).map_err(|source| EditError::Io {
        path: demo_path.display().to_string(),
        source,
    })?;
    let patch = Patch::new(
        u32::try_from(value_start)?,
        u32::try_from(value_start + 5)?,
        "0.5.0".to_owned(),
    )?;
    let verified = apply_to_file_verified(&demo_path, &[patch], |content| {
        if content.windows(5).any(|w| w == b"0.5.0") {
            Ok(())
        } else {
            Err(EditError::VerifyFailed {
                reason: "new version not found after re-read".to_owned(),
            })
        }
    })?;
    println!(
        "on disk:  {} ({})",
        String::from_utf8_lossy(&verified).trim(),
        Path::new(&demo_path).display()
    );
    std::fs::remove_file(&demo_path).map_err(|source| EditError::Io {
        path: demo_path.display().to_string(),
        source,
    })?;
    Ok(())
}

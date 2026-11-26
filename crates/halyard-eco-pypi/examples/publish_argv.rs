//! Example: the pypi publish argv contract end to end (M8 step 2,
//! ADR-0056).
//!
//! Run: `cargo run -p halyard-eco-pypi --example publish_argv`
//!
//! The shell spawns, the adapter owns the grammar: the live argv is
//! bare (uv's own posture governs credentials, the index and trusted
//! publishing) and the dry forecast rides uv's own `--dry-run`.

fn main() {
    println!(
        "live: uv {}",
        halyard_eco_pypi::uv_publish_args(false).join(" ")
    );
    println!(
        "dry:  uv {}",
        halyard_eco_pypi::uv_publish_args(true).join(" ")
    );
    println!();
    println!("the credential rides the auth seam's environment pair, not argv:");
    println!("  UV_PUBLISH_TOKEN (uv publish --help: `-t, --token`)");
}

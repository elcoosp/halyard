//! `size-gate`: the §14.1 binary-size budget (≤ 8 MB, musl, stripped).
//!
//! The release binary ships with the CLI skeleton (M1.10); until then the
//! gate passes with an explicit note instead of failing, and becomes
//! enforceable as soon as the path exists.

use std::path::PathBuf;

/// Default release binary path relative to the workspace root.
const DEFAULT_BINARY: &str = "target/release/halyard";
/// §14.1 budget: 8 MB (musl, stripped).
const BUDGET_BYTES: u64 = 8 * 1024 * 1024;

/// Run the size gate for the binary at `binary` (default `target/release/halyard`).
pub fn size_gate(binary: Option<String>) -> Result<(), String> {
    let root = crate::schema::workspace_root()?;
    let path = match binary {
        Some(b) => PathBuf::from(b),
        None => root.join(DEFAULT_BINARY),
    };
    let Some(meta) = std::fs::metadata(&path).ok() else {
        println!(
            "size-gate: no binary at {} yet (the CLI binary ships with M1.10); \
             gate is armed and will enforce the 8 MB budget then",
            path.display()
        );
        return Ok(());
    };
    let size = meta.len();
    if size <= BUDGET_BYTES {
        println!(
            "size-gate: {} is {} bytes (budget {} bytes) — ok",
            path.display(),
            size,
            BUDGET_BYTES
        );
        Ok(())
    } else {
        Err(format!(
            "size gate failed: {} is {size} bytes, budget is {BUDGET_BYTES} bytes",
            path.display()
        ))
    }
}

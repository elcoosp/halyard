//! `schema-check` (M0.7): regenerate the frozen JSON schemas from the Rust
//! types and verify (or, with `--write`, update) the checked-in files.
//!
//! The schemas are the contract:
//!
//! * `schemas/halyard-plan-v1.json` — generated from
//!   [`halyard_core::plan::Plan`] (§5.8).
//! * `schemas/halyard-config-v1.json` — generated from
//!   [`halyard_config::HalyardConfig`] (§16.4).
//!
//! The CI gate runs `xtask schema-check` (no `--write`), so any type drift
//! without a schema regeneration fails the build.

use std::path::{Path, PathBuf};

use serde_json::Value;

/// Plan schema document path, relative to the workspace root.
pub const PLAN_SCHEMA_PATH: &str = "schemas/halyard-plan-v1.json";
/// Config schema document path, relative to the workspace root.
pub const CONFIG_SCHEMA_PATH: &str = "schemas/halyard-config-v1.json";

/// Verify (or rewrite) the checked-in schema documents. `write` updates the
/// files instead of failing on drift.
pub fn schema_check(write: bool) -> Result<(), String> {
    let root = workspace_root()?;
    let pairs = [
        (PLAN_SCHEMA_PATH, plan_schema()?),
        (CONFIG_SCHEMA_PATH, config_schema()?),
    ];
    let mut dirty = Vec::new();
    for (rel, value) in &pairs {
        let path = root.join(rel);
        let want = pretty(value)?;
        match std::fs::read_to_string(&path) {
            Ok(have) if have == want => println!("ok    {rel}"),
            Ok(_) => {
                if write {
                    std::fs::write(&path, &want)
                        .map_err(|e| format!("write {}: {e}", path.display()))?;
                    println!("wrote {rel}");
                } else {
                    dirty.push(*rel);
                }
            }
            Err(_) => {
                if write {
                    if let Some(parent) = path.parent() {
                        std::fs::create_dir_all(parent)
                            .map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
                    }
                    std::fs::write(&path, &want)
                        .map_err(|e| format!("write {}: {e}", path.display()))?;
                    println!("wrote {rel}");
                } else {
                    dirty.push(*rel);
                }
            }
        }
    }
    if dirty.is_empty() {
        println!("schemas match the frozen types (halyard-plan/v1, halyard.toml v1)");
        Ok(())
    } else {
        Err(format!(
            "schema drift in {dirty:?}; run `cargo run -p xtask -- schema-check --write` and \
             review the diff"
        ))
    }
}

/// The `halyard-plan/v1` schema as a JSON value.
pub fn plan_schema() -> Result<Value, String> {
    halyard_core::plan::schema_document()
}

/// The `halyard.toml` v1 schema as a JSON value.
pub fn config_schema() -> Result<Value, String> {
    halyard_config::model::schema_document()
}

fn pretty(value: &Value) -> Result<String, String> {
    let mut out =
        serde_json::to_string_pretty(value).map_err(|e| format!("pretty-print failed: {e}"))?;
    out.push('\n');
    Ok(out)
}

/// Workspace root: two levels up from this binary's manifest dir, falling
/// back to the CWD when running outside the checkout.
pub fn workspace_root() -> Result<PathBuf, String> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = manifest
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| "cannot locate workspace root".to_owned())?;
    if root.join("Cargo.toml").exists() {
        Ok(root.to_path_buf())
    } else {
        Ok(PathBuf::from("."))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_schema_is_draft2020_with_id() {
        let v = plan_schema().expect("plan schema");
        assert_eq!(
            v.get("$schema").and_then(Value::as_str),
            Some("https://json-schema.org/draft/2020-12/schema")
        );
        assert_eq!(
            v.get("$id").and_then(Value::as_str),
            Some("https://halyard.dev/schemas/halyard-plan-v1.json")
        );
        let props = v
            .get("properties")
            .and_then(Value::as_object)
            .expect("properties");
        for key in [
            "schema",
            "base_branch",
            "head",
            "plan_hash",
            "releases",
            "edits",
            "order",
            "skipped",
        ] {
            assert!(props.contains_key(key), "plan schema misses {key}");
        }
    }

    #[test]
    fn config_schema_describes_documented_sections() {
        let v = config_schema().expect("config schema");
        let props = v
            .get("properties")
            .and_then(Value::as_object)
            .expect("properties");
        for key in [
            "repo",
            "versioning",
            "commits",
            "changelog",
            "publish",
            "forge",
            "package",
            "link",
        ] {
            assert!(props.contains_key(key), "config schema misses {key}");
        }
    }

    #[test]
    fn checked_in_schemas_match_types() {
        schema_check(false).expect("schemas are in sync (drift test)");
    }
}

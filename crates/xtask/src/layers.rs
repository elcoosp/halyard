//! `check-layers`: enforce the crate dependency rules of §7.2.
//!
//! Rules (workspace-internal deps, non-dev):
//!
//! * `halyard-core` — no workspace deps; external deps limited to `semver`,
//!   `smallvec`, `bstr`, `blake3`, `serde`, `serde_json` (runtime, ADR-0005)
//!   and the feature-gated `schemars` (ADR-0001, absent from the runtime
//!   graph).
//! * `halyard-config` — may depend on `halyard-core` (+ `serde`, `toml`,
//!   `serde_json`).
//! * `halyard-conventional` — standalone parser (`winnow`, `bstr`).
//! * `halyard-edit` — depends on `halyard-core` only.
//! * `halyard-forge` — depends on `halyard-core` only (§11.4 seam; the
//!   REST adapter in M4.1b inherits the rule).
//! * `halyard-test-support` — may depend on `halyard-core` and
//!   `halyard-forge` (the mock-forge re-export, §7.1).
//! * `halyard-version` — depends on `halyard-core` + `semver`.
//! * `xtask` / `halyard-cli` — workspace tooling and user shell: may depend
//!   on any workspace crate.
//! * No crate may depend on `halyard-cli`.
//!
//! Dev-dependencies are exempt (test-only by construction). An unknown
//! workspace member is an error: the table must be updated consciously when
//! a crate lands.

use std::process::Command;

use serde_json::Value;

/// Per-crate layer rule.
struct LayerRule {
    /// Crate name.
    name: &'static str,
    /// Allowed workspace-internal dependencies (non-dev).
    workspace: &'static [&'static str],
    /// Allowed external crates; `None` = unrestricted (tooling/shell crates).
    external: Option<&'static [&'static str]>,
}

const RULES: &[LayerRule] = &[
    LayerRule {
        name: "halyard-core",
        workspace: &[],
        external: Some(&[
            "semver",
            "smallvec",
            "bstr",
            "blake3",
            "serde",
            // ADR-0005: `serde_json` is part of the core runtime graph
            // (canonical JSON + plan hash, §5.8/I3).
            "serde_json",
            // ADR-0001: `schemars` stays feature-gated (`schema-gen`) and
            // absent from the default/runtime graph.
            "schemars",
        ]),
    },
    LayerRule {
        name: "halyard-config",
        workspace: &["halyard-core"],
        external: Some(&["serde", "schemars", "serde_json", "toml"]),
    },
    LayerRule {
        name: "halyard-conventional",
        workspace: &[],
        external: Some(&["winnow", "bstr"]),
    },
    LayerRule {
        name: "halyard-edit",
        workspace: &["halyard-core"],
        external: Some(&[]),
    },
    // M4.1a forge seam (§11.4): pure vocabulary — the trait, its data
    // types and the failure taxonomy. No HTTP here; the REST adapter
    // joins in M4.1b and inherits the same core-only rule.
    LayerRule {
        name: "halyard-forge",
        workspace: &["halyard-core"],
        external: Some(&[]),
    },
    LayerRule {
        name: "halyard-test-support",
        workspace: &["halyard-core", "halyard-forge"],
        // `serde_json` parses `cargo metadata` ground truth for the M1.5
        // differential comparator; test-support only.
        external: Some(&["tempfile", "criterion", "serde_json"]),
    },
    LayerRule {
        name: "halyard-version",
        workspace: &["halyard-core"],
        external: Some(&["semver"]),
    },
    // M1.5 (§6.3): adapters carry no workspace deps beyond core; the
    // parser toolchain crates (toml_edit, semver) are the sanctioned
    // external set, mirroring core's own allowances.
    LayerRule {
        name: "halyard-eco-cargo",
        workspace: &["halyard-core"],
        external: Some(&["semver", "toml_edit"]),
    },
    // M1.6 (§6.4): the JSON/YAML scanners are dependency-free — only the
    // shared semver grammar rides along.
    LayerRule {
        name: "halyard-eco-npm",
        workspace: &["halyard-core"],
        external: Some(&["semver"]),
    },
    // M1.5/M1.6 orchestrator: walks the repo (`ignore`) and dispatches to
    // the adapters (§6.1/§6.2). `rayon` is deliberately absent — §6.2's
    // parallel parse uses scoped std threads until budgets demand more.
    LayerRule {
        name: "halyard-discover",
        workspace: &["halyard-core", "halyard-eco-cargo", "halyard-eco-npm"],
        external: Some(&["ignore"]),
    },
    // M2 git engine (ADR-0009): depends on core **and** the conventional
    // parser leaf; slice D adds `halyard-cache` (§8.7 — the cache is
    // consulted before the tree diff) and `tracing` for the §14.3 span
    // counters. `gix`/`gix-commitgraph` are the sanctioned object-access
    // crates; `semver` parses tag versions; `bstr` carries commit
    // messages/paths as bytes (§5.7 — `BString` fields of the digest).
    LayerRule {
        name: "halyard-git",
        workspace: &["halyard-core", "halyard-conventional", "halyard-cache"],
        external: Some(&["gix", "gix-commitgraph", "semver", "tracing", "bstr"]),
    },
    // M3.5 changelog renderer (§9.4): consumes the Plan; `minijinja` is
    // the template engine named by the spec.
    LayerRule {
        name: "halyard-changelog",
        workspace: &["halyard-core"],
        external: Some(&["minijinja"]),
    },
    // M2.5 persistent cache (§8.7): the only crate permitted `unsafe`
    // (invariant I10). External set: `blake3` (content-addressed keys),
    // `libc` (mmap + advisory flock), `tracing` (hit/miss counters).
    LayerRule {
        name: "halyard-cache",
        workspace: &["halyard-core"],
        external: Some(&["blake3", "libc", "tracing"]),
    },
    LayerRule {
        name: "xtask",
        workspace: &["*"],
        external: None,
    },
    LayerRule {
        name: "halyard-cli",
        workspace: &["*"],
        external: None,
    },
];

/// The user-facing shell crate nothing may depend on.
const TOP_CRATE: &str = "halyard-cli";

/// Run the layer check over `cargo metadata` output.
pub fn check_layers() -> Result<(), String> {
    let meta = cargo_metadata()?;
    let packages = meta
        .get("packages")
        .and_then(Value::as_array)
        .ok_or_else(|| "cargo metadata: no packages".to_owned())?;
    let members = meta
        .get("workspace_members")
        .and_then(Value::as_array)
        .ok_or_else(|| "cargo metadata: no workspace_members".to_owned())?;
    let member_ids: Vec<&str> = members.iter().filter_map(Value::as_str).collect();

    let mut violations: Vec<String> = Vec::new();
    for pkg in packages {
        let id = pkg.get("id").and_then(Value::as_str).unwrap_or_default();
        if !member_ids.contains(&id) {
            continue;
        }
        let name = pkg
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| "cargo metadata: package without name".to_owned())?;
        let rule = RULES.iter().find(|r| r.name == name).ok_or_else(|| {
            format!(
                "crate {name:?} has no layer rule; add one to xtask/src/layers.rs when the \
                 crate lands"
            )
        })?;
        let deps: &[Value] = pkg
            .get("dependencies")
            .and_then(Value::as_array)
            .map_or(&[], Vec::as_slice);
        for dep in deps {
            let dep_name = dep.get("name").and_then(Value::as_str).unwrap_or_default();
            let kind = dep.get("kind").and_then(Value::as_str);
            if kind == Some("dev") {
                continue; // test-only by construction
            }
            let optional = dep
                .get("optional")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let is_workspace_dep = RULES.iter().any(|r| r.name == dep_name);
            if dep_name == TOP_CRATE && name != TOP_CRATE {
                violations.push(format!("{name}: depends on {TOP_CRATE} (forbidden)"));
                continue;
            }
            if is_workspace_dep {
                let allowed = rule.workspace.iter().any(|w| *w == dep_name || *w == "*");
                if !allowed {
                    violations.push(format!("{name}: workspace dep {dep_name:?} not allowed"));
                }
            } else if let Some(allowed) = rule.external {
                // ADR-0001 (as amended by ADR-0005): `schemars` is only
                // tolerated when optional (feature-gated `schema-gen`);
                // `serde_json` is a sanctioned runtime dep of core.
                let schemars_optional_ok = optional && dep_name == "schemars";
                if !allowed.contains(&dep_name) && !schemars_optional_ok {
                    violations.push(format!("{name}: external dep {dep_name:?} not allowed"));
                }
            }
        }
    }
    if violations.is_empty() {
        println!(
            "layers ok: {} workspace crates satisfy §7.2 (core deps, adapter deps, no {TOP_CRATE} back-edges)",
            member_ids.len()
        );
        Ok(())
    } else {
        Err(format!(
            "layer violations ({}):\n  {}",
            violations.len(),
            violations.join("\n  ")
        ))
    }
}

fn cargo_metadata() -> Result<Value, String> {
    let out = Command::new("cargo")
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .output()
        .map_err(|e| format!("spawn cargo metadata: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "cargo metadata failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    serde_json::from_slice::<Value>(&out.stdout).map_err(|e| format!("parse cargo metadata: {e}"))
}

/// Lookup used by tests: rule table as a map for deterministic reporting.
#[cfg(test)]
pub(crate) fn rule_names() -> std::collections::BTreeMap<&'static str, usize> {
    RULES.iter().map(|r| (r.name, r.workspace.len())).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_rule_locks_external_set() {
        let core = RULES
            .iter()
            .find(|r| r.name == "halyard-core")
            .expect("core rule");
        let external = core.external.expect("core has an explicit allow-list");
        for unexpected in ["gix", "toml_edit", "clap", "tokio", "anyhow", "miette"] {
            assert!(
                !external.contains(&unexpected),
                "core must not gain {unexpected} without an ADR"
            );
        }
        assert_eq!(core.workspace, &[] as &[&str], "core has no workspace deps");
    }

    #[test]
    fn every_current_member_has_a_rule() {
        let names = rule_names();
        for member in [
            "halyard-cache",
            "halyard-changelog",
            "halyard-cli",
            "halyard-core",
            "halyard-conventional",
            "halyard-discover",
            "halyard-eco-cargo",
            "halyard-eco-npm",
            "halyard-edit",
            "halyard-forge",
            "halyard-git",
            "halyard-test-support",
            "halyard-version",
            "xtask",
        ] {
            assert!(names.contains_key(member), "{member} lacks a layer rule");
        }
    }

    /// Regression test (slice D close-out): the two engine-critical
    /// manifests may not gain a dependency the rule table does not know.
    /// Reads the checked-in manifests directly, so the drift is caught even
    /// when the gate is run from a non-workspace cwd.
    #[test]
    fn git_and_cli_manifests_stay_within_their_rules() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(std::path::Path::parent)
            .expect("workspace root");
        for crate_name in ["halyard-git", "halyard-cli"] {
            let manifest =
                std::fs::read_to_string(root.join("crates").join(crate_name).join("Cargo.toml"))
                    .unwrap_or_else(|e| panic!("read {crate_name} manifest: {e}"));
            let value: Value =
                toml::from_str(&manifest).unwrap_or_else(|e| panic!("parse {crate_name}: {e}"));
            let deps = value
                .get("dependencies")
                .and_then(Value::as_object)
                .expect("dependencies table");
            let rule = RULES
                .iter()
                .find(|r| r.name == crate_name)
                .expect("rule exists (every_current_member_has_a_rule)");
            for dep in deps.keys() {
                let is_workspace_crate = RULES.iter().any(|r| r.name == dep);
                let allowed = if is_workspace_crate {
                    rule.workspace.iter().any(|w| *w == dep || *w == "*")
                } else {
                    // Mirror the gate: an unrestricted rule (None) allows
                    // every external dep, an allow-list must contain it.
                    match rule.external {
                        None => true,
                        Some(list) => list.contains(&dep.as_str()),
                    }
                };
                assert!(
                    allowed,
                    "{crate_name}: dependency {dep:?} violates its layer rule; update \
                     xtask/src/layers.rs consciously (§7.2)"
                );
            }
        }
    }
}

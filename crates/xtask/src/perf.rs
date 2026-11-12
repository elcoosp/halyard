//! `perf-gate`: compare measured scenario timings (§14.1) against the
//! registered budgets.
//!
//! Inputs (both deterministic TOML, checked in):
//!
//! * `bench/budgets.toml` — `[[budget]]` entries with `scenario`, `p50_ms`,
//!   `p99_ms`. Budgets are registered as the corresponding milestones land
//!   (first at M2.6); an empty registry passes vacuously.
//! * `bench/measurements.toml` — `[[measurement]]` entries produced by the
//!   benchmark harness (`xtask bench-public`, M6.5) for the fixed corpus.
//!
//! Gate rule (§14.1/§15.6): a scenario fails when its measured p50 exceeds
//! the budget by more than 5 %; p99 overruns are reported as warnings.

use std::path::PathBuf;

use serde::Deserialize;

/// One registered budget from `bench/budgets.toml`.
#[derive(Debug, Deserialize)]
struct Budget {
    /// Scenario identifier matching the harness (`no-op-warm`, …).
    scenario: String,
    /// p50 budget in milliseconds.
    p50_ms: f64,
    /// p99 budget in milliseconds.
    p99_ms: f64,
}

/// One measured scenario from `bench/measurements.toml`.
#[derive(Debug, Deserialize)]
struct Measurement {
    /// Scenario identifier matching a budget.
    scenario: String,
    /// Measured p50 in milliseconds.
    p50_ms: f64,
    /// Measured p99 in milliseconds.
    p99_ms: f64,
}

#[derive(Debug, Deserialize)]
struct Budgets {
    /// Registered budgets.
    #[serde(default)]
    budget: Vec<Budget>,
}

#[derive(Debug, Deserialize)]
struct Measurements {
    /// Measured scenarios.
    #[serde(default)]
    measurement: Vec<Measurement>,
}

/// Run the perf gate.
pub fn perf_gate() -> Result<(), String> {
    let root = crate::schema::workspace_root()?;
    let budgets: Budgets = parse_toml(&root.join("bench/budgets.toml"))?;
    if budgets.budget.is_empty() {
        println!(
            "perf-gate: no budgets registered yet (first budgets land with M2.6); \
             bench/budgets.toml is intentionally empty"
        );
        return Ok(());
    }
    let measurements: Measurements = parse_toml(&root.join("bench/measurements.toml"))?;
    let by_scenario: std::collections::BTreeMap<&str, &Measurement> = measurements
        .measurement
        .iter()
        .map(|m| (m.scenario.as_str(), m))
        .collect();
    let mut failures = Vec::new();
    let mut warnings = Vec::new();
    for b in &budgets.budget {
        match by_scenario.get(b.scenario.as_str()) {
            None => failures.push(format!(
                "{}: no measurement found; run the bench harness first",
                b.scenario
            )),
            Some(m) => {
                if m.p50_ms > b.p50_ms * 1.05 {
                    failures.push(format!(
                        "{}: p50 {:.2} ms exceeds budget {:.2} ms by more than 5 %",
                        b.scenario, m.p50_ms, b.p50_ms
                    ));
                }
                if m.p99_ms > b.p99_ms {
                    warnings.push(format!(
                        "{}: p99 {:.2} ms over budget {:.2} ms (reported, not gating)",
                        b.scenario, m.p99_ms, b.p99_ms
                    ));
                }
            }
        }
    }
    for w in &warnings {
        println!("warn  {w}");
    }
    if failures.is_empty() {
        println!("perf-gate: all {} budgets met", budgets.budget.len());
        Ok(())
    } else {
        Err(format!("perf gate failed:\n  {}", failures.join("\n  ")))
    }
}

fn parse_toml<T: for<'de> Deserialize<'de>>(path: &PathBuf) -> Result<T, String> {
    let text =
        std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    toml::from_str(&text).map_err(|e| format!("parse {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_budget_registry_parses() {
        let b: Budgets = toml::from_str("# empty\n").expect("empty budgets");
        assert!(b.budget.is_empty());
    }

    #[test]
    fn budget_row_parses() {
        let b: Budgets =
            toml::from_str("[[budget]]\nscenario = \"no-op-warm\"\np50_ms = 8\np99_ms = 20\n")
                .expect("budget row");
        assert_eq!(b.budget.len(), 1);
        assert_eq!(b.budget.first().expect("row").scenario, "no-op-warm");
    }
}

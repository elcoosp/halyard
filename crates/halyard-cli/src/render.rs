//! Output rendering (§16.2/§16.3): `human` (tables, aligned, colour-aware),
//! `json` (stable machine documents) and `gha` (GitHub Actions job-summary
//! markdown plus `::warning` annotations). All output is deterministic —
//! identical inputs render byte-identical text (I3).

use std::collections::BTreeMap;

use halyard_core::HistoryProvider;
use halyard_core::{Plan, PlanRelease};

use crate::doctor::DoctorReport;
use crate::Context;

/// Renders the plan as an aligned human table plus per-release reasons.
pub fn plan_human(plan: &Plan) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "plan {plan_hash} (head {head}, base {base})\n",
        plan_hash = plan.plan_hash,
        head = short(&plan.head),
        base = plan.base_branch,
    ));
    if plan.releases.is_empty() {
        out.push_str("nothing to release.\n");
    } else {
        out.push_str(&format!(
            "{:20} {:10} {:10} {:6} tag\n",
            "package", "from", "to", "bump"
        ));
        for release in &plan.releases {
            out.push_str(&format!(
                "{:20} {:10} {:10} {:6} {}\n",
                release.package, release.from, release.to, format_args!("{:?}", release.bump).to_string().to_lowercase(), release.tag,
            ));
        }
        out.push_str(&format!(
            "\npublish order: {}\n",
            plan.order.join(" -> ")
        ));
    }
    for skip in &plan.skipped {
        out.push_str(&format!("skipped {}: {}\n", skip.package, skip.reason));
    }
    for edit in &plan.edits {
        out.push_str(&format!(
            "edit {} ({} patch{})\n",
            edit.file,
            edit.patches.len(),
            if edit.patches.len() == 1 { "" } else { "es" }
        ));
    }
    out
}

/// One release's reason chain in human form (§16.2 `explain`).
pub fn explain_human(release: &PlanRelease) -> String {
    let mut out = format!(
        "{} {} -> {} ({:?}), tag {}\n",
        release.package, release.from, release.to, release.bump, release.tag
    );
    for reason in &release.reasons {
        match reason {
            halyard_core::PlanReason::Commit {
                oid,
                kind,
                breaking,
            } => out.push_str(&format!(
                "  commit {} {}{}{}\n",
                short(oid),
                kind,
                if *breaking { "!" } else { "" },
                " (breaking)" .trim_end()
            )),
            halyard_core::PlanReason::ReleaseAs { oid, version } => {
                out.push_str(&format!(
                    "  release-as {} → {}\n",
                    short(oid),
                    version
                ));
            }
            halyard_core::PlanReason::Changeset { file } => {
                out.push_str(&format!("  changeset {}\n", file));
            }
            halyard_core::PlanReason::Cascade { via, edge, why } => {
                out.push_str(&format!(
                    "  cascade via {via} ({edge}, {why})\n"
                ));
            }
            halyard_core::PlanReason::InitialRelease => {
                out.push_str("  initial release (no tag yet)\n");
            }
        }
    }
    if let Some(changelog) = &release.changelog {
        out.push_str(&format!("\nchangelog preview ({}):\n{}\n", changelog.file, changelog.entry_md));
    }
    out
}

/// Status rows as data (shared by `human` and `json`).
pub fn status_rows(context: &Context) -> Vec<BTreeMap<&'static str, String>> {
    let ws = &context.discovery.workspace;
    ws.packages()
        .iter()
        .map(|pkg| {
            let name = ws.interner().get(pkg.name).unwrap_or_default().to_owned();
            let base = context
                .history
                .digest()
                .base_of(pkg.id)
                .map(|b| b.tag.to_string())
                .unwrap_or_else(|| "(initial)".to_owned());
            let commits = context.history.digest().commits_for(pkg.id).len();
            let mut row = BTreeMap::new();
            row.insert("package", name);
            row.insert("version", pkg.version.as_ref().map(|v| v.to_string()).unwrap_or_else(|| "-".to_owned()));
            row.insert("base", base);
            row.insert("unreleased", commits.to_string());
            row.insert("pending", commits.to_string());
            row
        })
        .collect()
}

/// Status table (human).
pub fn status_human(context: &Context) -> String {
    let mut out = format!("{:20} {:10} {:24} {}\n", "package", "version", "base", "unreleased");
    for row in status_rows(context) {
        out.push_str(&format!(
            "{:20} {:10} {:24} {}\n",
            row.get("package").map(String::as_str).unwrap_or_default(),
            row.get("version").map(String::as_str).unwrap_or_default(),
            row.get("base").map(String::as_str).unwrap_or_default(),
            row.get("unreleased").map(String::as_str).unwrap_or_default(),
        ));
    }
    out
}

/// Dependency edges as data (shared by graph renderers).
pub fn graph_edges(context: &Context) -> Vec<BTreeMap<&'static str, String>> {
    let ws = &context.discovery.workspace;
    let mut edges = Vec::new();
    for pkg in ws.packages() {
        for dep in &pkg.deps {
            let mut row = BTreeMap::new();
            row.insert(
                "from",
                ws.interner().get(pkg.name).unwrap_or_default().to_owned(),
            );
            row.insert(
                "to",
                ws.package(dep.to)
                    .and_then(|t| ws.interner().get(t.name).map(str::to_owned))
                    .unwrap_or_default(),
            );
            row.insert("kind", format!("{:?}", dep.kind).to_lowercase());
            row.insert("req", String::from_utf8_lossy(&dep.raw).to_string());
            edges.push(row);
        }
    }
    edges
}

/// Graph as an indented listing.
pub fn graph_text(context: &Context) -> String {
    let mut out = String::new();
    for row in graph_edges(context) {
        out.push_str(&format!(
            "{} -> {} [{}] {}\n",
            row.get("from").map(String::as_str).unwrap_or_default(),
            row.get("to").map(String::as_str).unwrap_or_default(),
            row.get("kind").map(String::as_str).unwrap_or_default(),
            row.get("req").map(String::as_str).unwrap_or_default(),
        ));
    }
    if out.is_empty() {
        out.push_str("(no workspace edges)\n");
    }
    out
}

/// Graph as a Mermaid snippet (§16.1).
pub fn graph_mermaid(context: &Context) -> String {
    let mut out = String::from("graph TD\n");
    for row in graph_edges(context) {
        out.push_str(&format!(
            "    {} --> {}\n",
            mermaid_id(row.get("from").map(String::as_str).unwrap_or_default()),
            mermaid_id(row.get("to").map(String::as_str).unwrap_or_default())
        ));
    }
    out
}

/// Mermaid-safe node id (scoped npm names contain `@` and `/`).
fn mermaid_id(name: &str) -> String {
    let mut id = String::from("n_");
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            id.push(ch);
        } else {
            id.push('_');
        }
    }
    id
}

/// Graph as Graphviz DOT.
pub fn graph_dot(context: &Context) -> String {
    let mut out = String::from("digraph workspace {\n");
    for row in graph_edges(context) {
        out.push_str(&format!(
            "    {:?} -> {:?};\n",
            row.get("from").map(String::as_str).unwrap_or_default(),
            row.get("to").map(String::as_str).unwrap_or_default()
        ));
    }
    out.push_str("}\n");
    out
}

/// Doctor report in human form.
pub fn doctor_human(report: &DoctorReport) -> String {
    let mut out = String::from("halyard doctor\n");
    for check in &report.checks {
        out.push_str(&format!(
            "  [{}] {}: {}\n",
            if check.ok { "ok" } else { "!!" },
            check.name,
            check.detail
        ));
    }
    for warning in &report.warnings {
        out.push_str(&format!("  warning: {warning}\n"));
    }
    if report.healthy {
        out.push_str("all checks passed\n");
    } else {
        out.push_str("some checks failed; see docs/errors/ for codes\n");
    }
    out
}

/// Doctor report as a GHA summary.
pub fn doctor_gha(report: &DoctorReport) -> String {
    let mut out = String::from("## halyard doctor\n\n| check | status | detail |\n|---|---|---|\n");
    for check in &report.checks {
        out.push_str(&format!(
            "| {name} | {status} | {detail} |\n",
            name = check.name,
            status = if check.ok { "ok" } else { "fail" },
            detail = check.detail
        ));
    }
    for warning in &report.warnings {
        out.push_str(&format!("::warning::halyard doctor: {warning}\n"));
    }
    out
}

/// Plan as a GHA job summary.
pub fn plan_gha(plan: &Plan) -> String {
    let mut out = String::from("## halyard plan\n\n");
    if plan.releases.is_empty() {
        out.push_str("Nothing to release.\n");
    } else {
        out.push_str("| Package | From | To | Bump | Tag |\n|---|---|---|---|---|\n");
        for release in &plan.releases {
            out.push_str(&format!(
                "| {pkg} | {from} | **{to}** | {bump:?} | {tag} |\n",
                pkg = release.package,
                from = release.from,
                to = release.to,
                bump = release.bump,
                tag = release.tag,
            ));
        }
    }
    for skip in &plan.skipped {
        out.push_str(&format!(
            "::warning::halyard plan: skipped {} ({})\n",
            skip.package, skip.reason
        ));
    }
    out.push_str(&format!("\nplan hash: `{}`\n", plan.plan_hash));
    out
}

/// Explain as a GHA summary.
pub fn explain_gha(release: &PlanRelease) -> String {
    let mut out = format!(
        "## halyard explain: {pkg} {from} → **{to}**\n\n",
        pkg = release.package,
        from = release.from,
        to = release.to
    );
    for reason in &release.reasons {
        out.push_str(&format!("- {reason:?}\n"));
    }
    out
}

/// Status as a GHA summary.
pub fn status_gha(context: &Context) -> String {
    let mut out = String::from("## halyard status\n\n| package | version | base | unreleased |\n|---|---|---|---|\n");
    for row in status_rows(context) {
        out.push_str(&format!(
            "| {p} | {v} | {b} | {u} |\n",
            p = row.get("package").map(String::as_str).unwrap_or_default(),
            v = row.get("version").map(String::as_str).unwrap_or_default(),
            b = row.get("base").map(String::as_str).unwrap_or_default(),
            u = row.get("unreleased").map(String::as_str).unwrap_or_default(),
        ));
    }
    out
}

/// Truncated oid for display.
fn short(oid: &str) -> String {
    oid.chars().take(8).collect()
}

//! Output rendering (§16.2/§16.3): `human` (tables, aligned, colour-aware),
//! `json` (stable machine documents) and `gha` (GitHub Actions job-summary
//! markdown plus `::warning` annotations). All output is deterministic —
//! identical inputs render byte-identical text (I3).

use std::collections::BTreeMap;
use std::fmt::Write as _;

use halyard_core::HistoryProvider;
use halyard_core::{Plan, PlanRelease};
use halyard_edit::{ApplyReport, DryPatch};

use crate::Context;
use crate::doctor::DoctorReport;

/// Renders the `apply --dry` preview (`plan_apply::dry_run_preview`): the
/// byte patches the plan would write, per file, sorted by path (I3).
pub fn apply_dry(preview: &BTreeMap<String, Vec<DryPatch>>) -> String {
    let mut out = String::new();
    let patches: usize = preview.values().map(Vec::len).sum();
    let _ = writeln!(
        out,
        "dry run: {} file{}, {} patch{}",
        preview.len(),
        if preview.len() == 1 { "" } else { "s" },
        patches,
        if patches == 1 { "" } else { "es" }
    );
    for (file, edits) in preview {
        let _ = writeln!(out, "{file}:");
        for patch in edits {
            let _ = writeln!(
                out,
                "  [{}..{}) -> {:?}",
                patch.start, patch.end, patch.text
            );
        }
    }
    out
}

/// Renders the deterministic per-file apply summary (M3.6): sorted paths,
/// applied/skipped counts.
pub fn apply_report(report: &ApplyReport) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "applied {} edit{} across {} file{} ({} skipped)",
        report.total_applied(),
        if report.total_applied() == 1 { "" } else { "s" },
        report.files.len(),
        if report.files.len() == 1 { "" } else { "s" },
        report.total_skipped(),
    );
    for file in &report.files {
        let _ = writeln!(
            out,
            "{}: {} applied, {} skipped",
            file.file, file.applied, file.skipped
        );
    }
    out
}

/// Renders the plan as an aligned human table plus per-release reasons.
pub fn plan_human(plan: &Plan) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "plan {plan_hash} (head {head}, base {base})",
        plan_hash = plan.plan_hash,
        head = short(&plan.head),
        base = plan.base_branch,
    );
    if plan.releases.is_empty() {
        out.push_str("nothing to release.\n");
    } else {
        let _ = writeln!(
            out,
            "{:20} {:10} {:10} {:6} tag",
            "package", "from", "to", "bump"
        );
        for release in &plan.releases {
            let _ = writeln!(
                out,
                "{:20} {:10} {:10} {:6} {}",
                release.package,
                release.from,
                release.to,
                format_args!("{:?}", release.bump)
                    .to_string()
                    .to_lowercase(),
                release.tag,
            );
        }
        let _ = writeln!(out, "\npublish order: {}", plan.order.join(" -> "));
    }
    for skip in &plan.skipped {
        let _ = writeln!(out, "skipped {}: {}", skip.package, skip.reason);
    }
    for edit in &plan.edits {
        let _ = writeln!(
            out,
            "edit {} ({} patch{})",
            edit.file,
            edit.patches.len(),
            if edit.patches.len() == 1 { "" } else { "es" }
        );
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
            } => {
                let _ = writeln!(
                    out,
                    "  commit {} {}{}{}",
                    short(oid),
                    kind,
                    if *breaking { "!" } else { "" },
                    " (breaking)".trim_end()
                );
            }
            halyard_core::PlanReason::ReleaseAs { oid, version } => {
                let _ = writeln!(out, "  release-as {} → {}", short(oid), version);
            }
            halyard_core::PlanReason::Changeset { file } => {
                let _ = writeln!(out, "  changeset {file}");
            }
            halyard_core::PlanReason::Cascade { via, edge, why } => {
                let _ = writeln!(out, "  cascade via {via} ({edge}, {why})");
            }
            halyard_core::PlanReason::InitialRelease => {
                out.push_str("  initial release (no tag yet)\n");
            }
        }
    }
    if let Some(changelog) = &release.changelog {
        let _ = writeln!(
            out,
            "\nchangelog preview ({}):\n{}",
            changelog.file, changelog.entry_md
        );
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
                .map_or_else(|| "(initial)".to_owned(), |b| b.tag.to_string());
            let commits = context.history.digest().commits_for(pkg.id).len();
            let mut row = BTreeMap::new();
            row.insert("package", name);
            row.insert(
                "version",
                pkg.version
                    .as_ref()
                    .map_or_else(|| "-".to_owned(), ToString::to_string),
            );
            row.insert("base", base);
            row.insert("unreleased", commits.to_string());
            row.insert("pending", commits.to_string());
            row
        })
        .collect()
}

/// Status table (human).
pub fn status_human(context: &Context) -> String {
    let mut out = format!(
        "{:20} {:10} {:24} {}\n",
        "package", "version", "base", "unreleased"
    );
    for row in status_rows(context) {
        let _ = writeln!(
            out,
            "{:20} {:10} {:24} {}",
            row.get("package").map(String::as_str).unwrap_or_default(),
            row.get("version").map(String::as_str).unwrap_or_default(),
            row.get("base").map(String::as_str).unwrap_or_default(),
            row.get("unreleased")
                .map(String::as_str)
                .unwrap_or_default(),
        );
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
        let _ = writeln!(
            out,
            "{} -> {} [{}] {}",
            row.get("from").map(String::as_str).unwrap_or_default(),
            row.get("to").map(String::as_str).unwrap_or_default(),
            row.get("kind").map(String::as_str).unwrap_or_default(),
            row.get("req").map(String::as_str).unwrap_or_default(),
        );
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
        let _ = writeln!(
            out,
            "    {} --> {}",
            mermaid_id(row.get("from").map(String::as_str).unwrap_or_default()),
            mermaid_id(row.get("to").map(String::as_str).unwrap_or_default())
        );
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
        let _ = writeln!(
            out,
            "    {:?} -> {:?};",
            row.get("from").map(String::as_str).unwrap_or_default(),
            row.get("to").map(String::as_str).unwrap_or_default()
        );
    }
    out.push_str("}\n");
    out
}

/// Doctor report in human form.
pub fn doctor_human(report: &DoctorReport) -> String {
    let mut out = String::from("halyard doctor\n");
    for check in &report.checks {
        let _ = writeln!(
            out,
            "  [{}] {}: {}",
            if check.ok { "ok" } else { "!!" },
            check.name,
            check.detail
        );
    }
    for warning in &report.warnings {
        let _ = writeln!(out, "  warning: {warning}");
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
        let _ = writeln!(
            out,
            "| {name} | {status} | {detail} |",
            name = check.name,
            status = if check.ok { "ok" } else { "fail" },
            detail = check.detail
        );
    }
    for warning in &report.warnings {
        let _ = writeln!(out, "::warning::halyard doctor: {warning}");
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
            let _ = writeln!(
                out,
                "| {pkg} | {from} | **{to}** | {bump:?} | {tag} |",
                pkg = release.package,
                from = release.from,
                to = release.to,
                bump = release.bump,
                tag = release.tag,
            );
        }
    }
    for skip in &plan.skipped {
        let _ = writeln!(
            out,
            "::warning::halyard plan: skipped {} ({})",
            skip.package, skip.reason
        );
    }
    let _ = writeln!(out, "\nplan hash: `{}`", plan.plan_hash);
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
        let _ = writeln!(out, "- {reason:?}");
    }
    out
}

/// Status as a GHA summary.
pub fn status_gha(context: &Context) -> String {
    let mut out = String::from(
        "## halyard status\n\n| package | version | base | unreleased |\n|---|---|---|---|\n",
    );
    for row in status_rows(context) {
        let _ = writeln!(
            out,
            "| {p} | {v} | {b} | {u} |",
            p = row.get("package").map(String::as_str).unwrap_or_default(),
            v = row.get("version").map(String::as_str).unwrap_or_default(),
            b = row.get("base").map(String::as_str).unwrap_or_default(),
            u = row
                .get("unreleased")
                .map(String::as_str)
                .unwrap_or_default(),
        );
    }
    out
}

/// Truncated oid for display.
fn short(oid: &str) -> String {
    oid.chars().take(8).collect()
}

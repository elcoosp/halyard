//! Output rendering (§16.2/§16.3): `human` (tables, aligned, colour-aware),
//! `json` (stable machine documents) and `gha` (GitHub Actions job-summary
//! markdown plus `::warning` annotations). All output is deterministic —
//! identical inputs render byte-identical text (I3).

use std::collections::BTreeMap;
use std::fmt::Write as _;

use halyard_core::HistoryProvider;
use halyard_core::{
    AffectedReason, AffectedSelection, PackageTagState, PkgId, Plan, PlanRelease, TagState,
};
use halyard_edit::{ApplyReport, DryPatch};
use halyard_forge::{GovernorEvent, MachineEffect, MachineReport};
use halyard_git::push::{TagPushReport, TagPushRow};
use halyard_git::tag::{TagOutcome, TagPhaseReport, TagPhaseRow};
use halyard_git::tag_target::TagTargetSource;

use crate::Context;
use crate::check;
use crate::doctor::DoctorReport;
use halyard_conventional::TitleLint;

/// The walk provenance the shell surfaces (the M2.6 stats wiring): the
/// git engine's own counters — [`halyard_git::GitHistory`]'s tree-diff
/// statistics (§8.4, the M2.3 acceptance metric: entries read vs a
/// naive diff), merge-policy statistics (§8.5) and the degraded /
/// truncated markers (§8.3) — rendered identically in every report.
/// Plain deterministic data (I3): no clock, no randomness.
pub struct HistoryStats {
    /// True when the walk ran without a commit-graph (§8.3 degraded
    /// marker; correctness unaffected).
    pub degraded: bool,
    /// True when the walk stopped at `initial_release.max_commits` (§8.3).
    pub truncated: bool,
    /// Tree objects opened by the guided walk (§8.4).
    pub trees_read: u64,
    /// Tree entries iterated by the guided walk.
    pub entries_read: u64,
    /// Subtree comparisons answered from the memo LRU.
    pub memo_hits: u64,
    /// Commits whose whole tree comparison was skipped by a Bloom
    /// negative.
    pub bloom_skips: u64,
    /// True when no changed-path filter was available (the documented
    /// ceiling, never a wrong answer).
    pub bloom_unavailable: bool,
    /// Merge commits dropped under the first-parent policy (§8.5).
    pub merges_dropped: usize,
    /// Merge commits kept as change entries.
    pub merges_kept: usize,
    /// (commit, revert) pairs canceled inside the window.
    pub reverts_canceled: usize,
    /// Reverts kept as `fix` because their target is outside the window.
    pub reverts_as_fix: usize,
}

impl HistoryStats {
    /// Reads the counters off a built [`halyard_git::GitHistory`] (the
    /// engine's own public fields — no re-measurement, no second walk).
    #[must_use]
    pub fn of(history: &halyard_git::GitHistory) -> Self {
        let diff = &history.tree_diff_stats;
        let merge = &history.merge_stats;
        Self {
            degraded: history.degraded,
            truncated: history.truncated,
            trees_read: diff.trees_read,
            entries_read: diff.entries_read,
            memo_hits: diff.memo_hits,
            bloom_skips: diff.bloom_skips,
            bloom_unavailable: diff.bloom_unavailable,
            merges_dropped: merge.merges_dropped,
            merges_kept: merge.merges_kept,
            reverts_canceled: merge.reverts_canceled,
            reverts_as_fix: merge.reverts_as_fix,
        }
    }

    /// The stable JSON shape the report documents carry under `history`
    /// (field order via `json!` — `BTreeMap` ordering, deterministic I3).
    #[must_use]
    pub fn json(&self) -> serde_json::Value {
        serde_json::json!({
            "degraded": self.degraded,
            "truncated": self.truncated,
            "trees_read": self.trees_read,
            "entries_read": self.entries_read,
            "memo_hits": self.memo_hits,
            "bloom_skips": self.bloom_skips,
            "bloom_unavailable": self.bloom_unavailable,
            "merges_dropped": self.merges_dropped,
            "merges_kept": self.merges_kept,
            "reverts_canceled": self.reverts_canceled,
            "reverts_as_fix": self.reverts_as_fix,
        })
    }

    /// One skimmable human line (§16.3) naming the walk shape and every
    /// counter.
    #[must_use]
    pub fn human_line(&self) -> String {
        let mut flags = Vec::new();
        if self.degraded {
            flags.push("degraded (no commit-graph)");
        }
        if self.truncated {
            flags.push("truncated at initial_release.max_commits");
        }
        let flags = if flags.is_empty() {
            String::new()
        } else {
            format!(" {};", flags.join(", "))
        };
        format!(
            "walk:{flags} {trees} trees / {entries} entries read, {memo} memo hits, \
             {bloom} bloom skips (filter {availability}); merges {dropped} dropped / \
             {kept} kept, reverts {canceled} canceled / {as_fix} as-fix",
            trees = self.trees_read,
            entries = self.entries_read,
            memo = self.memo_hits,
            bloom = self.bloom_skips,
            availability = if self.bloom_unavailable {
                "unavailable"
            } else {
                "available"
            },
            dropped = self.merges_dropped,
            kept = self.merges_kept,
            canceled = self.reverts_canceled,
            as_fix = self.reverts_as_fix,
        )
    }
}

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
/// applied/skipped counts, plus the M3.2/M3.4 lockfile stage rows
/// (patches, or the native-refresh outcome when a patcher declined,
/// §9.2 rule 4 / §9.3).
/// One changelog-stage row (M2, §9.4): the file the stage rendered and
/// what the write did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangelogRow {
    /// The repo-relative changelog file.
    pub file: String,
    /// The style that drove the section bodies — a built-in name, or
    /// `template:<path>` for the user-template path.
    pub style: String,
    /// `created` (fresh document), `updated` (anchored insertion) or
    /// `unchanged` (the re-render was byte-identical — no write, I1).
    pub outcome: &'static str,
}

/// One changelog-stage row as a stable JSON row.
#[must_use]
pub fn changelog_row_json(row: &ChangelogRow) -> serde_json::Value {
    serde_json::json!({
        "file": row.file,
        "style": row.style,
        "outcome": row.outcome,
    })
}

/// Renders the apply report for humans: the edit rows, the lockfile
/// rows, the native refreshes and the M2 changelog stage.
pub fn apply_report(
    report: &ApplyReport,
    native_refreshes: &[(String, Vec<String>)],
    changelog_rows: &[ChangelogRow],
) -> String {
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
    for lock in &report.lockfiles {
        match &lock.native_reason {
            None => {
                let _ = writeln!(
                    out,
                    "{}: {} applied, {} skipped",
                    lock.file, lock.applied, lock.skipped
                );
            }
            Some(reason) => {
                let _ = writeln!(
                    out,
                    "{}: left untouched, native refresh required: {reason}",
                    lock.file
                );
            }
        }
    }
    for (file, moved) in native_refreshes {
        let _ = writeln!(
            out,
            "{file}: natively refreshed, members moved: {}",
            moved.join(", ")
        );
    }
    for row in changelog_rows {
        let _ = writeln!(
            out,
            "{}: changelog {} ({})",
            row.file, row.outcome, row.style
        );
    }
    out
}

/// Renders the plan as an aligned human table plus per-release reasons.
/// The publish report, human/gha shape (M5.2 step 2, §13.7/§13.8): the
/// rows in execution order (the engine guarantees the order), the
/// tallies derived from the rows, the not-targets the plan carried,
/// and the auth provenance tail (M5.3 step 2 — which documented
/// variable authenticates each registry, names only).
pub fn publish_report(
    report: &halyard_publish::PublishReport,
    mode: &str,
    auth: &std::collections::BTreeMap<String, String>,
) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "publish ({mode}): {} published, {} already published, {} forecast, {} skipped, \
         {} failed",
        report.published, report.already_published, report.forecast, report.skipped, report.failed,
    );
    for row in &report.rows {
        let what = match &row.outcome {
            halyard_publish::PublishOutcome::Published {
                visibility_confirmed,
            } => {
                if *visibility_confirmed {
                    "published (visibility confirmed)".to_owned()
                } else {
                    "published (index never confirmed — dependents skip)".to_owned()
                }
            }
            halyard_publish::PublishOutcome::AlreadyPublished => {
                "already published (no-op)".to_owned()
            }
            halyard_publish::PublishOutcome::Forecast => "forecast (dry run)".to_owned(),
            halyard_publish::PublishOutcome::Skipped { reason } => format!("skipped ({reason})"),
            halyard_publish::PublishOutcome::Failed {
                code,
                message,
                retryable,
            } => format!("failed [{code}] (retryable: {retryable}) — {message}"),
        };
        let _ = writeln!(
            out,
            "  layer {} {}@{} ({}): {}{}",
            row.layer,
            row.package,
            row.version,
            row.registry,
            what,
            npm_options_suffix(row)
        );
    }
    let _ = writeln!(out, "{}", publish_auth_line(auth));
    out
}

/// The npm publish options as a bracketed human suffix (ADR-0033): only
/// when the contract carries at least one option — unset stays absent,
/// so cargo-only and unset-npm lines keep the byte-stable shape (I3).
fn npm_options_suffix(row: &halyard_publish::PublishRow) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(access) = &row.access {
        parts.push(format!("access {access}"));
    }
    match row.provenance {
        Some(true) => parts.push("provenance".to_owned()),
        Some(false) => parts.push("no-provenance".to_owned()),
        None => {}
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!(" [npm: {}]", parts.join(", "))
    }
}

/// The publish-auth provenance as a one-line human/gha tail (M5.3 step
/// 2, ADR-0019): `registry = env:VARIABLE` pairs, name-sorted; `none`
/// means nothing will be injected (the tool's own credential store
/// governs). Provenance only — a token value never reaches this line.
pub fn publish_auth_line(auth: &std::collections::BTreeMap<String, String>) -> String {
    if auth.is_empty() {
        return "publish-auth (M5.3): no publish targets".to_owned();
    }
    let pairs: Vec<String> = auth
        .iter()
        .map(|(registry, provenance)| format!("{registry} = {provenance}"))
        .collect();
    format!("publish-auth (M5.3): {}", pairs.join(", "))
}

/// The publish report's machine shape (deterministic field order rides
/// `serde_json`, §16.3 "stable JSON"). The `auth` block (M5.3 step 2)
/// carries the per-registry provenance — variable names only.
pub fn publish_report_json(
    report: &halyard_publish::PublishReport,
    not_targets: &[halyard_publish::SkippedRelease],
    mode: &str,
    auth: &std::collections::BTreeMap<String, String>,
) -> serde_json::Value {
    serde_json::json!({
        "mode": mode,
        "targets": report.rows.len(),
        "rows": report.rows.iter().map(publish_row_json).collect::<Vec<_>>(),
        "not_targets": not_targets.iter().map(publish_not_target_json).collect::<Vec<_>>(),
        "auth": auth,
        "tallies": {
            "published": report.published,
            "already_published": report.already_published,
            "forecast": report.forecast,
            "skipped": report.skipped,
            "failed": report.failed,
        },
    })
}

/// One row's machine shape: the outcome is tagged by `kind` (the error
/// taxonomy convention — a stable discriminator). The npm publish
/// options (ADR-0033) ride the row only when the contract carries them
/// — unset stays absent, so cargo-only and unset-npm reports keep the
/// byte-stable shape (I3).
pub fn publish_row_json(row: &halyard_publish::PublishRow) -> serde_json::Value {
    let outcome = match &row.outcome {
        halyard_publish::PublishOutcome::Published {
            visibility_confirmed,
        } => serde_json::json!({
            "kind": "published",
            "visibility_confirmed": visibility_confirmed,
        }),
        halyard_publish::PublishOutcome::AlreadyPublished => {
            serde_json::json!({ "kind": "already_published" })
        }
        halyard_publish::PublishOutcome::Forecast => {
            serde_json::json!({ "kind": "forecast" })
        }
        halyard_publish::PublishOutcome::Skipped { reason } => {
            serde_json::json!({ "kind": "skipped", "reason": reason })
        }
        halyard_publish::PublishOutcome::Failed {
            code,
            message,
            retryable,
        } => serde_json::json!({
            "kind": "failed",
            "code": code,
            "message": message,
            "retryable": retryable,
        }),
    };
    let mut row_json = serde_json::json!({
        "package": row.package,
        "version": row.version,
        "ecosystem": row.ecosystem,
        "registry": row.registry,
        "manifest": row.manifest,
        "manifest_dir": row.manifest_dir,
        "layer": row.layer,
        "outcome": outcome,
    });
    if let Some(object) = row_json.as_object_mut() {
        if let Some(access) = &row.access {
            object.insert(
                "access".to_owned(),
                serde_json::Value::String(access.clone()),
            );
        }
        if let Some(provenance) = row.provenance {
            object.insert("provenance".to_owned(), serde_json::Value::Bool(provenance));
        }
    }
    row_json
}

/// One not-target's machine shape (§16.3: what was ignored and why).
pub fn publish_not_target_json(skip: &halyard_publish::SkippedRelease) -> serde_json::Value {
    serde_json::json!({
        "package": skip.package,
        "version": skip.version,
        "reason": skip.reason,
    })
}

pub fn plan_human(plan: &Plan, stats: &HistoryStats) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "plan {plan_hash} (head {head}, base {base})",
        plan_hash = plan.plan_hash,
        head = short(&plan.head),
        base = plan.base_branch,
    );
    out.push_str(&stats.human_line());
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

/// The §8.8 smart-minimal-fetch report (human): the selection
/// provenance (why each tag was requested), the per-tag outcomes and
/// the executed argv — the same observable the JSON document carries.
pub fn fetch_human(report: &halyard_git::fetch::FetchReport) -> String {
    let mut out = String::from("halyard fetch (§8.8)\n");
    match report.remote.as_deref() {
        Some(remote) => {
            let _ = writeln!(
                out,
                "  remote {remote} · base branch {} · {} probed tags",
                report.base_branch.as_deref().unwrap_or("(none)"),
                report.probe_tags
            );
            if !report.selection.is_empty() {
                out.push_str("selection (package → requested tag):\n");
                for (package, tag) in &report.selection {
                    let _ = writeln!(out, "  {package} → {tag}");
                }
            }
            for row in &report.rows {
                match row {
                    halyard_git::fetch::FetchRow::Fetched { tag } => {
                        let _ = writeln!(out, "  fetched: {tag}");
                    }
                    halyard_git::fetch::FetchRow::AlreadyLocal { tag } => {
                        let _ = writeln!(out, "  already local: {tag}");
                    }
                    halyard_git::fetch::FetchRow::Failed { tag, error } => {
                        let _ = writeln!(out, "  failed: {tag} — {error}");
                    }
                }
            }
            if report.fallback_used {
                out.push_str("  degraded: the minimal fetch failed; the full-fetch fallback ran (§8.8 step 3)\n");
            }
            if !report.argv.is_empty() {
                let _ = writeln!(out, "  git {}", report.argv.join(" "));
            }
            let _ = writeln!(
                out,
                "counters: fetched {} · already local {} · failed {}",
                report.fetched, report.already_local, report.failed
            );
        }
        None => out.push_str("  no remote configured — nothing to fetch\n"),
    }
    out
}

/// The §8.8 fetch report as a GHA job summary.
pub fn fetch_gha(report: &halyard_git::fetch::FetchReport) -> String {
    let mut out = String::from("## halyard fetch (§8.8)\n\n");
    match report.remote.as_deref() {
        Some(remote) => {
            let _ = writeln!(
                out,
                "remote `{remote}` · base branch `{}` · {} probed tags\n",
                report.base_branch.as_deref().unwrap_or("(none)"),
                report.probe_tags
            );
            if !report.selection.is_empty() {
                out.push_str("| package | requested tag | outcome |\n|---|---|---|\n");
                for row in &report.rows {
                    match row {
                        halyard_git::fetch::FetchRow::Fetched { tag } => {
                            let _ =
                                writeln!(out, "| {} | {tag} | fetched |", label_of(report, tag));
                        }
                        halyard_git::fetch::FetchRow::AlreadyLocal { tag } => {
                            let _ = writeln!(
                                out,
                                "| {} | {tag} | already local (I1) |",
                                label_of(report, tag)
                            );
                        }
                        halyard_git::fetch::FetchRow::Failed { tag, error } => {
                            let _ = writeln!(
                                out,
                                "| {} | {tag} | failed ({}) |",
                                label_of(report, tag),
                                error.code()
                            );
                        }
                    }
                }
            }
            if report.fallback_used {
                let _ = writeln!(
                    out,
                    "::warning::halyard fetch: the minimal fetch failed; the full-fetch \
                     fallback ran (§8.8 step 3)"
                );
            }
            let _ = writeln!(
                out,
                "fetched {} · already local {} · failed {}",
                report.fetched, report.already_local, report.failed
            );
        }
        None => out.push_str("No remote configured — nothing to fetch.\n"),
    }
    out
}

/// The package that selected `tag` (the §8.8 step-1 provenance), for
/// the GHA table's first column; `(multiple)` when several packages
/// selected the same tag (shared tags are legal).
fn label_of<'a>(report: &'a halyard_git::fetch::FetchReport, tag: &str) -> &'a str {
    report
        .selection
        .iter()
        .find(|(_, selected)| selected.as_str() == tag)
        .map_or("(multiple)", |(package, _)| package.as_str())
}

/// Plan as a GHA job summary.
pub fn plan_gha(plan: &Plan, stats: &HistoryStats) -> String {
    let mut out = format!("## halyard plan\n\n{}\n", stats.human_line());
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

/// The name of one workspace package (empty for a foreign id — the
/// renderers stay total over stale selections).
pub(crate) fn package_name(context: &Context, pkg: PkgId) -> String {
    context
        .discovery
        .workspace
        .package(pkg)
        .and_then(|p| {
            context
                .discovery
                .workspace
                .interner()
                .get(p.name)
                .map(str::to_owned)
        })
        .unwrap_or_default()
}

/// The repo-relative root of one workspace package (empty for a
/// foreign id — the renderers stay total over stale selections). The
/// CI-selector adapters' Bazel patterns read it (M10 step 2, ADR-0034).
pub(crate) fn package_root(context: &Context, pkg: PkgId) -> &str {
    context
        .discovery
        .workspace
        .package(pkg)
        .map(|p| p.root.as_str())
        .unwrap_or_default()
}

/// The `affected` report as data (§16.3/M10): the base the selection was
/// made against (the `--base` ref, else the release bases), the
/// name-sorted changed roots and the name-sorted closed rows with their
/// reasons (`via` = the nearest affected ancestor's name). Byte-stable
/// across runs (I3).
pub fn affected_report_json(
    context: &Context,
    selection: &AffectedSelection,
    base: Option<&str>,
) -> serde_json::Value {
    let mut changed: Vec<String> = selection
        .rows()
        .iter()
        .filter(|row| row.reason == AffectedReason::Changed)
        .map(|row| package_name(context, row.package))
        .collect();
    changed.sort();
    let mut rows: Vec<(String, serde_json::Value)> = selection
        .rows()
        .iter()
        .map(|row| {
            let name = package_name(context, row.package);
            let value = match row.reason {
                AffectedReason::Changed => serde_json::json!({
                    "package": name.clone(),
                    "reason": "changed",
                }),
                AffectedReason::Dependent { via } => serde_json::json!({
                    "package": name.clone(),
                    "reason": "dependent",
                    "via": package_name(context, via),
                }),
            };
            (name, value)
        })
        .collect();
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    let packages: Vec<serde_json::Value> = rows.into_iter().map(|(_, value)| value).collect();
    serde_json::json!({
        "base": base,
        "changed": changed,
        "packages": packages,
        "total": selection.len(),
    })
}

/// The `affected` table (human): an explicit base line when the
/// selection was made against a ref, then package / reason / via.
pub fn affected_human(
    context: &Context,
    selection: &AffectedSelection,
    base: Option<&str>,
) -> String {
    let mut out = String::new();
    if let Some(base) = base {
        let _ = writeln!(out, "affected against base: {base}");
    }
    // The header is a literal: `{:20} {:10} {}` over constant strings is
    // computable at compile time (the clippy useless-format rule).
    let _ = writeln!(out, "package              reason     via");
    for row in selection.rows() {
        let _ = writeln!(
            out,
            "{:20} {:10} {}",
            package_name(context, row.package),
            match row.reason {
                AffectedReason::Changed => "changed",
                AffectedReason::Dependent { .. } => "dependent",
            },
            match row.reason {
                AffectedReason::Changed => "-".to_owned(),
                AffectedReason::Dependent { via } => package_name(context, via),
            },
        );
    }
    out
}

/// The `affected` selection as a GHA summary: the markdown table plus a
/// machine-greppable tally line (`affected=N changed=M`).
pub fn affected_gha(
    context: &Context,
    selection: &AffectedSelection,
    base: Option<&str>,
) -> String {
    let heading = match base {
        Some(base) => format!("## halyard affected (base: {base})\n"),
        None => String::from("## halyard affected\n"),
    };
    let mut out = heading;
    out.push_str("\n| package | reason | via |\n|---|---|---|\n");
    let mut changed = 0usize;
    for row in selection.rows() {
        if row.reason == AffectedReason::Changed {
            changed += 1;
        }
        let _ = writeln!(
            out,
            "| {p} | {r} | {v} |",
            p = package_name(context, row.package),
            r = match row.reason {
                AffectedReason::Changed => "changed",
                AffectedReason::Dependent { .. } => "dependent",
            },
            v = match row.reason {
                AffectedReason::Changed => "-".to_owned(),
                AffectedReason::Dependent { via } => package_name(context, via),
            },
        );
    }
    let _ = writeln!(out, "\naffected={} changed={}", selection.len(), changed);
    out
}

/// Renders the `check --pr-title` verdict (§11.6): the accepted lint,
/// the conveyed bump and the per-package prediction ("if merged, this
/// will release"). Deterministic — the same title and plan render
/// byte-identical text (I3).
pub fn check_human(title: &str, lint: &TitleLint, predictions: &[check::Prediction]) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "title: {title:?}");
    let scope = lint.scope.as_deref().unwrap_or("-");
    let _ = writeln!(
        out,
        "verdict: accepted — {}({scope}), conveys {}",
        lint.kind, lint.level
    );
    if predictions.is_empty() {
        let _ = writeln!(
            out,
            "no pending releases: merging this title releases nothing"
        );
        return out;
    }
    let _ = writeln!(out, "if merged, this will release:");
    let conveyed = check::bump_str(check::bump_of(lint.level));
    for prediction in predictions {
        if prediction.remapped {
            let _ = writeln!(
                out,
                "  {} {} → {} (0.x policy: {} → {})",
                prediction.package,
                prediction.from,
                check::bump_str(prediction.bump),
                conveyed,
                check::bump_str(prediction.bump)
            );
        } else {
            let _ = writeln!(
                out,
                "  {} {} → {}",
                prediction.package,
                prediction.from,
                check::bump_str(prediction.bump)
            );
        }
    }
    out
}

/// Renders the pending-release gate of `check` without a title (§16.2:
/// "`check` | lint PR title/commits; `--exit-code` if release pending").
pub fn check_pending_human(plan: &Plan) -> String {
    let mut out = String::new();
    if plan.releases.is_empty() {
        let _ = writeln!(out, "no pending releases");
        return out;
    }
    let _ = writeln!(out, "pending: {} release(s)", plan.releases.len());
    for release in &plan.releases {
        let _ = writeln!(
            out,
            "  {} {} → {} ({})",
            release.package,
            release.from,
            release.to,
            check::bump_str(release.bump)
        );
    }
    out
}

/// The lint finding's human wording (one line, the verdict plus its
/// did-you-mean or its ignore reason).
fn check_commit_finding_text(finding: &check::CommitFinding) -> String {
    match finding {
        check::CommitFinding::Drives { bump } => {
            format!("drives {}", check::bump_str(*bump))
        }
        check::CommitFinding::MappedNone => "ignored (mapped to no bump)".to_owned(),
        check::CommitFinding::Typo { kind, closest } => {
            format!("TYPO: {kind:?} is not a known type — did you mean {closest:?}? (E-CLI-COMMIT)")
        }
        check::CommitFinding::NotConventional { reason } => {
            format!("ignored (not conventional: {reason})")
        }
    }
}

/// The `check --commits` findings as a human table (§16.2's commits
/// half): one row per (package, pending commit), discovery order with
/// walk order inside a package.
pub fn check_commits_human(rows: &[check::CommitRow], pending: bool) -> String {
    let mut out = String::new();
    if rows.is_empty() {
        let _ = writeln!(
            out,
            "no pending commits (every package sits at its release base)"
        );
        return out;
    }
    let _ = writeln!(out, "pending commits: {} row(s)", rows.len());
    for row in rows {
        let short = row.oid.get(..7).unwrap_or(row.oid.as_str());
        let _ = writeln!(
            out,
            "  {p} {short} {s:?}: {f}",
            p = row.package,
            s = row.subject,
            f = check_commit_finding_text(&row.finding)
        );
    }
    if pending {
        let _ = writeln!(out, "pending: yes (the driving rows above plan a release)");
    } else {
        let _ = writeln!(out, "pending: no");
    }
    out
}

/// The `check --commits` findings as a GHA job summary.
pub fn check_commits_gha(rows: &[check::CommitRow], pending: bool) -> String {
    let mut out = String::from("## halyard check (commits)\n\n");
    if rows.is_empty() {
        let _ = writeln!(out, "- no pending commits");
        return out;
    }
    for row in rows {
        let short = row.oid.get(..7).unwrap_or(row.oid.as_str());
        let _ = writeln!(
            out,
            "- {p} `{short}` `{s}`: {f}",
            p = row.package,
            s = row.subject,
            f = check_commit_finding_text(&row.finding)
        );
    }
    let _ = writeln!(out, "- pending: {}", if pending { "yes" } else { "no" });
    out
}

/// The `check --pr-title` verdict as a GHA job summary.
pub fn check_gha(title: &str, lint: &TitleLint, predictions: &[check::Prediction]) -> String {
    let mut out = String::from("## halyard check\n\n");
    let _ = writeln!(out, "- title: `{title}`");
    let _ = writeln!(out, "- verdict: accepted, conveys **{}**", lint.level);
    if predictions.is_empty() {
        let _ = writeln!(
            out,
            "- no pending releases: merging this title releases nothing"
        );
        return out;
    }
    let _ = writeln!(out, "- if merged, this will release:");
    for prediction in predictions {
        let _ = writeln!(
            out,
            "  - {p}: {f} → {b}",
            p = prediction.package,
            f = prediction.from,
            b = check::bump_str(prediction.bump)
        );
    }
    out
}

/// The pending-release gate as a GHA summary.
pub fn check_pending_gha(plan: &Plan) -> String {
    let mut out = String::from("## halyard check\n\n");
    if plan.releases.is_empty() {
        let _ = writeln!(out, "- no pending releases");
        return out;
    }
    let _ = writeln!(out, "- pending: {} release(s)", plan.releases.len());
    for release in &plan.releases {
        let _ = writeln!(
            out,
            "  - {p}: {f} → {t} ({b})",
            p = release.package,
            f = release.from,
            t = release.to,
            b = check::bump_str(release.bump)
        );
    }
    out
}

/// One machine effect as a stable JSON row (the `pr --dry` document).
pub fn pr_effect_json(effect: &MachineEffect) -> serde_json::Value {
    match effect {
        MachineEffect::ReleaseCreated { tag, url } => serde_json::json!({
            "type": "release_created", "tag": tag, "url": url,
        }),
        MachineEffect::ReleaseExisted { tag } => {
            serde_json::json!({ "type": "release_existed", "tag": tag })
        }
        MachineEffect::PrOpened { number } => {
            serde_json::json!({ "type": "pr_opened", "number": number })
        }
        MachineEffect::PrUpdated { number } => {
            serde_json::json!({ "type": "pr_updated", "number": number })
        }
        MachineEffect::PrUnchanged { number } => {
            serde_json::json!({ "type": "pr_unchanged", "number": number })
        }
        MachineEffect::PrClosed { number } => {
            serde_json::json!({ "type": "pr_closed", "number": number })
        }
    }
}

/// One machine effect with its payload, for the GHA summary.
fn pr_effect_gha(effect: &MachineEffect) -> String {
    match effect {
        MachineEffect::ReleaseCreated { tag, url } => {
            format!("would create release `{tag}` ({url})")
        }
        MachineEffect::ReleaseExisted { tag } => format!("release `{tag}` exists"),
        MachineEffect::PrOpened { number } => format!("would open PR #{number} (preview)"),
        MachineEffect::PrUpdated { number } => format!("would update PR #{number}"),
        MachineEffect::PrUnchanged { number } => {
            format!("PR #{number} already current (no mutating calls)")
        }
        MachineEffect::PrClosed { number } => format!("would close PR #{number}"),
    }
}

/// Renders the `pr --dry` forecast (M4.2b step 1): the §11.1 step-2
/// reconciliation rows, the machine's effects over the preview forge,
/// the §11.5 call-budget forecast and the rendered §11.3 body.
/// Deterministic (I3): the same repo state renders byte-identical text.
pub fn pr_dry_human(
    title: &str,
    branch: &str,
    states: &[PackageTagState],
    report: &MachineReport,
    body: Option<&str>,
) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "dry run: Release-PR forecast over the preview forge (nothing written)"
    );
    let _ = writeln!(out, "title: {title:?}");
    let _ = writeln!(out, "branch: {branch}");
    // The §11.1 step-2 reconciliation, visible in the report (§16.3:
    // the forecast shows what the live run would tag — and what it
    // deliberately would not: a Behind row is reported, never tagged).
    let pending: Vec<&PackageTagState> = states
        .iter()
        .filter(|s| s.state == TagState::Pending)
        .collect();
    let behind: Vec<&PackageTagState> = states
        .iter()
        .filter(|s| s.state == TagState::Behind)
        .collect();
    let _ = writeln!(
        out,
        "pending tags (manifest-vs-tags, §11.1 step 2): {}",
        pending.len()
    );
    for state in &pending {
        let _ = writeln!(
            out,
            "  - {} ({} {})",
            state.tag, state.package, state.manifest_version
        );
    }
    if pending.is_empty() {
        let _ = writeln!(out, "  (every manifest version is tagged)");
    }
    for state in &behind {
        let _ = writeln!(
            out,
            "  behind, not tagged: {} carries {} but the manifest says {} (a reverted \
             merge is never re-tagged downward)",
            state.latest_tag.as_deref().unwrap_or_default(),
            state
                .latest_version
                .as_ref()
                .map_or_else(String::new, ToString::to_string),
            state.manifest_version
        );
    }
    if report.effects.is_empty() {
        let _ = writeln!(
            out,
            "nothing to do: no pending plan, no stale Release PR ({} probe call)",
            report.calls
        );
    }
    for effect in &report.effects {
        let _ = writeln!(out, "{}", pr_effect_gha(effect));
    }
    if report.propagated {
        let _ = writeln!(
            out,
            "prepare phase: deferred (PROPAGATE, §11.1 step 2) — the Release PR is \
             forecast once the pending tags are current"
        );
    }
    let _ = writeln!(out, "forge calls forecast: {}", report.calls);
    if let Some(body) = body {
        let _ = writeln!(out, "body:");
        let _ = writeln!(out, "{body}");
    }
    out
}

/// The `pr --dry` forecast as a GHA job summary (body folded into a
/// details block).
pub fn pr_dry_gha(
    title: &str,
    branch: &str,
    states: &[PackageTagState],
    report: &MachineReport,
    body: Option<&str>,
) -> String {
    let mut out = String::from("## halyard pr (dry)\n\n");
    let _ = writeln!(out, "- title: `{title}`");
    let _ = writeln!(out, "- branch: `{branch}`");
    let pending: Vec<&PackageTagState> = states
        .iter()
        .filter(|s| s.state == TagState::Pending)
        .collect();
    let _ = writeln!(out, "- pending tags (§11.1 step 2): {}", pending.len());
    for state in &pending {
        let _ = writeln!(
            out,
            "  - `{}` ({} {})",
            state.tag, state.package, state.manifest_version
        );
    }
    if report.effects.is_empty() {
        let _ = writeln!(
            out,
            "- nothing to do: no pending plan, no stale Release PR ({} probe call)",
            report.calls
        );
    }
    for effect in &report.effects {
        let _ = writeln!(out, "- {}", pr_effect_gha(effect));
    }
    if report.propagated {
        let _ = writeln!(out, "- prepare phase deferred (PROPAGATE, §11.1 step 2)");
    }
    let _ = writeln!(out, "- forge calls forecast: {}", report.calls);
    if let Some(body) = body {
        let _ = writeln!(out, "\n<details><summary>PR body preview</summary>\n");
        let _ = writeln!(out, "{body}");
        let _ = writeln!(out, "</details>");
    }
    out
}

/// Renders the live `pr` run (M4.1b) for humans: the same prepare-phase
/// shapes as the forecast, over the real adapter — effects are effects,
/// not forecasts.
#[must_use]
pub fn pr_live_human(
    title: &str,
    branch: &str,
    states: &[PackageTagState],
    report: &MachineReport,
    body: Option<&str>,
) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "live run: prepare phase over the GitHub REST adapter (M4.1b)"
    );
    let _ = writeln!(out, "title: {title:?}");
    let _ = writeln!(out, "branch: {branch}");
    // The §11.1 step-2 reconciliation, visible in the live report too:
    // what the tag phase owed when this pass ran.
    let pending: Vec<&PackageTagState> = states
        .iter()
        .filter(|s| s.state == TagState::Pending)
        .collect();
    let _ = writeln!(
        out,
        "pending tags (manifest-vs-tags, §11.1 step 2): {}",
        pending.len()
    );
    for state in &pending {
        let _ = writeln!(
            out,
            "  - {} ({} {})",
            state.tag, state.package, state.manifest_version
        );
    }
    let _ = writeln!(out, "forge calls: {}", report.calls);
    if report.effects.is_empty() {
        let _ = writeln!(
            out,
            "nothing to do: no pending plan, no stale Release PR ({} probe call)",
            report.calls
        );
    }
    for effect in &report.effects {
        let _ = writeln!(out, "{}", pr_effect_gha(effect));
    }
    if report.propagated {
        let _ = writeln!(
            out,
            "prepare phase: deferred (PROPAGATE, §11.1 step 2) — the Release PR moves once \
             the pending tags are current"
        );
    }
    if let Some(body) = body {
        let _ = writeln!(out, "body:");
        let _ = writeln!(out, "{body}");
    }
    out
}

/// Renders the live `pr` run (M4.1b) as a GHA job summary.
#[must_use]
pub fn pr_live_gha(
    title: &str,
    branch: &str,
    states: &[PackageTagState],
    report: &MachineReport,
    body: Option<&str>,
) -> String {
    let mut out = String::from("## halyard pr (live)\n\n");
    let _ = writeln!(out, "- title: `{title}`");
    let _ = writeln!(out, "- branch: `{branch}`");
    let pending: Vec<&PackageTagState> = states
        .iter()
        .filter(|s| s.state == TagState::Pending)
        .collect();
    let _ = writeln!(out, "- pending tags (§11.1 step 2): {}", pending.len());
    for state in &pending {
        let _ = writeln!(
            out,
            "  - `{}` ({} {})",
            state.tag, state.package, state.manifest_version
        );
    }
    if report.effects.is_empty() {
        let _ = writeln!(
            out,
            "- nothing to do: no pending plan, no stale Release PR ({} probe call)",
            report.calls
        );
    }
    for effect in &report.effects {
        let _ = writeln!(out, "- {}", pr_effect_gha(effect));
    }
    if report.propagated {
        let _ = writeln!(out, "- prepare phase deferred (PROPAGATE, §11.1 step 2)");
    }
    let _ = writeln!(out, "- forge calls: {}", report.calls);
    if let Some(body) = body {
        let _ = writeln!(out, "\n<details><summary>PR body</summary>\n");
        let _ = writeln!(out, "{body}");
        let _ = writeln!(out, "</details>");
    }
    out
}

/// Truncated oid for display.
fn short(oid: &str) -> String {
    oid.chars().take(8).collect()
}

/// The provenance of one directive's resolved target (§11.1 precision
/// rule): the commit the tag would land on and how the walk arrived at
/// it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TargetProvenance {
    /// The target commit (full hex; renderers truncate).
    pub commit: String,
    /// Whether the walk pinpointed the introducing commit or fell back.
    pub source: TagTargetSource,
}

/// The provenance of every directive, keyed by tag name (the executor
/// re-sorts rows by tag, so the renderers look provenance up by name).
pub type TargetProvenanceMap = BTreeMap<String, TargetProvenance>;

/// The row's tag name — the renderers key the target provenance by it.
fn row_tag(row: &TagPhaseRow) -> &str {
    match row {
        TagPhaseRow::Applied { tag, .. } | TagPhaseRow::Failed { tag, .. } => tag,
    }
}

/// The human label of a target source (the forecast reads "manifest
/// pinpoint" — the commit that introduced the version — and "HEAD
/// fallback" — the documented degradation, e.g. a blob-less clone).
fn source_text(source: TagTargetSource) -> &'static str {
    match source {
        TagTargetSource::ManifestPinpoint => "manifest pinpoint",
        TagTargetSource::HeadFallback => "HEAD fallback",
    }
}

/// The stable JSON key of a target source.
fn source_key(source: TagTargetSource) -> &'static str {
    match source {
        TagTargetSource::ManifestPinpoint => "manifest_pinpoint",
        TagTargetSource::HeadFallback => "head_fallback",
    }
}

/// The outcome text of one tag-phase row (shared by the renderers): the
/// forecast reads "would create" (nothing was written), the live run
/// reads "created", the no-op reads "already tagged" (I1), failures
/// carry their `E-GIT-*` code. Resolved rows carry their target
/// provenance (the §11.1 precision rule's answer and its degradation
/// status).
fn release_row_text(
    row: &TagPhaseRow,
    provenance: Option<&TargetProvenance>,
    created: &str,
) -> String {
    let target =
        provenance.map(|p| format!(" — target {} ({})", short(&p.commit), source_text(p.source)));
    match row {
        TagPhaseRow::Applied { tag, outcome } => {
            let outcome = match outcome {
                TagOutcome::Created { .. } => created,
                TagOutcome::AlreadyTagged { .. } => "already tagged",
            };
            format!("{tag} — {outcome}{}", target.as_deref().unwrap_or_default())
        }
        TagPhaseRow::Failed { tag, error } => format!(
            "{tag} — failed [{}]: {error}{}",
            error.code(),
            target.as_deref().unwrap_or_default()
        ),
    }
}

/// The tag-phase rows + the Behind rows (the shared body of the dry and
/// live human renders — the two modes differ only in header, outcome
/// wording and the push section).
fn release_rows_human(
    out: &mut String,
    states: &[PackageTagState],
    report: &TagPhaseReport,
    provenance: &TargetProvenanceMap,
    created: &str,
) {
    if report.rows.is_empty() {
        let _ = writeln!(out, "  (nothing owed: every manifest version is tagged)");
    }
    for row in &report.rows {
        let _ = writeln!(
            out,
            "  - {}",
            release_row_text(row, provenance.get(row_tag(row)), created)
        );
    }
    let behind: Vec<&PackageTagState> = states
        .iter()
        .filter(|s| s.state == TagState::Behind)
        .collect();
    for state in &behind {
        let _ = writeln!(
            out,
            "  behind, not tagged: {} carries {} but the manifest says {} (a reverted \
             merge is never re-tagged downward)",
            state.latest_tag.as_deref().unwrap_or_default(),
            state
                .latest_version
                .as_ref()
                .map_or_else(String::new, ToString::to_string),
            state.manifest_version
        );
    }
}

/// The push section of the release render (§11.2): skipped honestly
/// under `--no-push` or without a configured remote, a forecast in the
/// dry run (`would push` rows), the real outcomes in the live run.
pub fn release_push_human(push: Option<&TagPushReport>, forecast: bool) -> String {
    let mut out = String::new();
    match push {
        None => {
            let _ = writeln!(out, "push: skipped (--no-push)");
        }
        Some(report) if report.remote.is_none() => {
            let _ = writeln!(
                out,
                "push: skipped (no remote 'origin' configured; the tags stay local)"
            );
        }
        Some(report) => {
            let label = if forecast {
                "push forecast (origin, offline)"
            } else {
                "push (origin)"
            };
            let _ = writeln!(
                out,
                "{label}: {} pushed, {} already on remote, {} failed",
                report.pushed, report.already_on_remote, report.failed
            );
            for row in &report.rows {
                let line = match row {
                    TagPushRow::Pushed { tag } => format!("{tag} — pushed"),
                    TagPushRow::AlreadyOnRemote { tag } => format!("{tag} — already on remote"),
                    TagPushRow::WouldPush { tag } => format!("{tag} — would push"),
                    TagPushRow::Failed { tag, error } => {
                        format!("{tag} — failed [{}]: {error}", error.code())
                    }
                };
                let _ = writeln!(out, "  - {line}");
            }
        }
    }
    out
}

/// The human render of a finished tag phase — shared by the dry forecast
/// (preview writer, "would create") and the live run (real writer,
/// "created").
fn release_phase_human(
    head: &str,
    states: &[PackageTagState],
    report: &TagPhaseReport,
    provenance: &TargetProvenanceMap,
    created: &str,
    counters: (usize, usize, usize, usize),
) -> String {
    let (owed, created_n, already_n, failed_n) = counters;
    let mut out = String::new();
    let _ = writeln!(out, "head: {}", short(head));
    let _ = writeln!(
        out,
        "tag directives (§11.1 step 2): {owed} owed, {created_n} {created}, {already_n} already \
         tagged, {failed_n} failed"
    );
    release_rows_human(&mut out, states, report, provenance, created);
    out
}

/// Renders the `release --dry` forecast (§11.1 step 2, git side): the
/// directives the tag phase owes, classified against the real ref state,
/// with the counters, each directive's resolved target provenance and
/// the push forecast (§11.2). Nothing was written.
pub fn release_dry_human(
    head: &str,
    states: &[PackageTagState],
    report: &TagPhaseReport,
    provenance: &TargetProvenanceMap,
    push: Option<&TagPushReport>,
) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "dry run: tag-phase forecast over the preview writer (nothing written)"
    );
    out.push_str(&release_phase_human(
        head,
        states,
        report,
        provenance,
        "would create",
        (
            report.rows.len(),
            report.created,
            report.already_tagged,
            report.failed,
        ),
    ));
    out.push_str(&release_push_human(push, true));
    out
}

/// Renders the live release run (§11.1 step 2, git side): the annotated
/// tags the real writer created, the §11.2 push outcomes, and the forge
/// half's real outcome (M4.1b — ran over the adapter or an honest skip).
pub fn release_live_human(
    head: &str,
    states: &[PackageTagState],
    report: &TagPhaseReport,
    provenance: &TargetProvenanceMap,
    push: Option<&TagPushReport>,
    forge_releases: &ForgeReleases,
) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "live run: tag phase over the real writer (annotated tags)"
    );
    out.push_str(&release_phase_human(
        head,
        states,
        report,
        provenance,
        "created",
        (
            report.rows.len(),
            report.created,
            report.already_tagged,
            report.failed,
        ),
    ));
    out.push_str(&release_push_human(push, false));
    out.push_str(&forge_releases_human(forge_releases));
    out
}

/// One `release` directive as a stable JSON row, carrying its resolved
/// target provenance (commit + source). `created_key` distinguishes the
/// forecast (`would_create`) from the live run (`created`).
pub fn release_row_json(
    row: &TagPhaseRow,
    provenance: &TargetProvenanceMap,
    created_key: &'static str,
) -> serde_json::Value {
    let target = provenance.get(row_tag(row));
    let mut value = match row {
        TagPhaseRow::Applied { tag, outcome } => serde_json::json!({
            "tag": tag,
            "outcome": match outcome {
                TagOutcome::Created { .. } => created_key,
                TagOutcome::AlreadyTagged { .. } => "already_tagged",
            },
        }),
        TagPhaseRow::Failed { tag, error } => serde_json::json!({
            "tag": tag,
            "outcome": "failed",
            "error_code": error.code(),
        }),
    };
    if let Some(provenance) = target
        && let Some(object) = value.as_object_mut()
    {
        object.insert(
            "target".to_owned(),
            serde_json::json!({
                "commit": provenance.commit,
                "source": source_key(provenance.source),
            }),
        );
    }
    value
}

/// One push row as a stable JSON row.
fn push_row_json(row: &TagPushRow) -> serde_json::Value {
    match row {
        TagPushRow::Pushed { tag } => serde_json::json!({ "tag": tag, "outcome": "pushed" }),
        TagPushRow::AlreadyOnRemote { tag } => {
            serde_json::json!({ "tag": tag, "outcome": "already_on_remote" })
        }
        TagPushRow::WouldPush { tag } => {
            serde_json::json!({ "tag": tag, "outcome": "would_push" })
        }
        TagPushRow::Failed { tag, error } => serde_json::json!({
            "tag": tag,
            "outcome": "failed",
            "error_code": error.code(),
        }),
    }
}

/// The push section as a stable JSON document (`remote: null` when the
/// phase was skipped — `--no-push` or no configured remote).
pub fn release_push_json(push: Option<&TagPushReport>, forecast: bool) -> serde_json::Value {
    match push {
        None => serde_json::json!({
            "remote": serde_json::Value::Null,
            "forecast": forecast,
            "rows": [],
            "pushed": 0,
            "already_on_remote": 0,
            "failed": 0,
        }),
        Some(report) => serde_json::json!({
            "remote": report.remote,
            "forecast": forecast,
            "rows": report.rows.iter().map(push_row_json).collect::<Vec<_>>(),
            "pushed": report.pushed,
            "already_on_remote": report.already_on_remote,
            "failed": report.failed,
        }),
    }
}

/// Renders the `release --dry` forecast for GitHub Actions summaries
/// (each directive carries its resolved target provenance, plus the push
/// forecast).
pub fn release_dry_gha(
    head: &str,
    states: &[PackageTagState],
    report: &TagPhaseReport,
    provenance: &TargetProvenanceMap,
    push: Option<&TagPushReport>,
) -> String {
    let mut out = String::from("## halyard release (dry)\n\n");
    let _ = writeln!(out, "- head: `{}`", short(head));
    let _ = writeln!(
        out,
        "- tag directives (§11.1 step 2): {} owed ({} would create, {} already tagged, {} failed)",
        report.rows.len(),
        report.created,
        report.already_tagged,
        report.failed
    );
    release_rows_human(&mut out, states, report, provenance, "would create");
    out.push_str(&release_push_human(push, true));
    if report.failed > 0 {
        let _ = writeln!(
            out,
            "\n::warning title=tag-phase conflicts::{} directive(s) failed; resolve the \
             conflicts by hand and re-run",
            report.failed
        );
    }
    out
}

/// The §11.5 call governor's run summary (M5.5, ADR-0015): the policy
/// the shell resolved, the forwarded-attempt ledger and the event log —
/// the ingredients of the end-of-run API usage summary (§11.5). Plain
/// deterministic data (I3), rendered identically in every live report.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GovernorSummary {
    /// Minimum start-to-start spacing between content-creating calls,
    /// in milliseconds (`0` = pacing off).
    pub min_spacing_ms: u64,
    /// The armed per-run call budget (`None` = unbounded).
    pub max_calls: Option<u32>,
    /// Retry attempts per operation after the initial one.
    pub max_retries: u32,
    /// Forwarded attempts this run — injected failures and retries
    /// included (§11.5: a dropped call is an observable one).
    pub spent: u32,
    /// The governor's events, oldest first (paces, honored retries,
    /// budget refusals).
    pub events: Vec<GovernorEvent>,
}

impl GovernorSummary {
    /// Reads the summary off a run governor: the resolved policy, the
    /// forwarded-attempt ledger and the event log — the §11.5 end-of-run
    /// usage summary's ingredients, read once per live pass.
    #[must_use]
    pub fn of(governor: &halyard_forge::Governor<'_>) -> Self {
        let policy = governor.policy();
        Self {
            min_spacing_ms: policy.min_spacing_ms,
            max_calls: policy.max_calls,
            max_retries: policy.max_retries,
            spent: governor.spent(),
            events: governor.events(),
        }
    }

    /// The one-line human/gha tail: policy, spend and the event tally.
    /// Deterministic (I3).
    #[must_use]
    pub fn line(&self) -> String {
        let budget = self
            .max_calls
            .map_or_else(|| "unbounded".to_owned(), |budget| budget.to_string());
        let mut paced = 0_usize;
        let mut retried = 0_usize;
        let mut blocked = 0_usize;
        for event in &self.events {
            match event {
                GovernorEvent::Paced { .. } => paced += 1,
                GovernorEvent::RateLimitRetry { .. } | GovernorEvent::TransportRetry { .. } => {
                    retried += 1;
                }
                GovernorEvent::BudgetBlocked { .. } => blocked += 1,
            }
        }
        format!(
            "governor (§11.5): pacing {} ms · budget {budget} · retries {}; spent {} \
             call(s), {paced} paced, {retried} retried, {blocked} blocked",
            self.min_spacing_ms, self.max_retries, self.spent,
        )
    }

    /// The stable JSON row (the live documents' `governor` field).
    #[must_use]
    pub fn json(&self) -> serde_json::Value {
        serde_json::json!({
            "min_spacing_ms": self.min_spacing_ms,
            "max_calls": self.max_calls,
            "max_retries": self.max_retries,
            "spent": self.spent,
            "events": self.events.iter().map(governor_event_json).collect::<Vec<_>>(),
        })
    }
}

/// One governor event as a JSON row (`type` first, the row's identity).
fn governor_event_json(event: &GovernorEvent) -> serde_json::Value {
    match event {
        GovernorEvent::Paced { op, waited_ms } => serde_json::json!({
            "type": "paced",
            "op": op,
            "waited_ms": waited_ms,
        }),
        GovernorEvent::RateLimitRetry {
            op,
            attempt,
            waited_ms,
        } => serde_json::json!({
            "type": "rate_limit_retry",
            "op": op,
            "attempt": attempt,
            "waited_ms": waited_ms,
        }),
        GovernorEvent::TransportRetry {
            op,
            attempt,
            waited_ms,
        } => serde_json::json!({
            "type": "transport_retry",
            "op": op,
            "attempt": attempt,
            "waited_ms": waited_ms,
        }),
        GovernorEvent::BudgetBlocked { op, spent } => serde_json::json!({
            "type": "budget_blocked",
            "op": op,
            "spent": spent,
        }),
    }
}

/// The live release's forge half (M4.1b, §11.1 step 2): what the §11.1
/// machine's tag phase did over the GitHub REST adapter — or the honest
/// skip (no credentials, foreign origin, dry forecast).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ForgeReleases {
    /// The pass ran: the effects, the repo they ran against, the §11.5
    /// call count (from the adapter's own log) and the governor's
    /// summary (M5.5).
    Ran {
        /// The `owner/repo` the effects ran against.
        repo: String,
        /// The machine's effects, oldest first (`ReleaseCreated` /
        /// `ReleaseExisted`).
        effects: Vec<MachineEffect>,
        /// Forge calls this pass consumed (the adapter's call log).
        calls: usize,
        /// The §11.5 call governor's run summary (policy + spend +
        /// events).
        governor: GovernorSummary,
    },
    /// The half did not run, and why — reported, never faked.
    Skipped {
        /// The actionable reason.
        reason: String,
    },
}

/// The forge-half JSON row (`release`'s `forge_releases` field).
#[must_use]
pub fn forge_releases_json(forge: &ForgeReleases) -> serde_json::Value {
    match forge {
        ForgeReleases::Ran {
            repo,
            effects,
            calls,
            governor,
        } => serde_json::json!({
            "status": "ran",
            "repo": repo,
            "effects": effects.iter().map(pr_effect_json).collect::<Vec<_>>(),
            "calls": calls,
            "governor": governor.json(),
        }),
        ForgeReleases::Skipped { reason } => {
            serde_json::json!({ "status": "skipped", "reason": reason })
        }
    }
}

/// The forge-half human block.
#[must_use]
pub fn forge_releases_human(forge: &ForgeReleases) -> String {
    let mut out = String::new();
    match forge {
        ForgeReleases::Ran {
            repo,
            effects,
            calls,
            governor,
        } => {
            let _ = writeln!(out, "forge releases: ran over {repo} ({calls} call(s))");
            let _ = writeln!(out, "  {}", governor.line());
            if effects.is_empty() {
                let _ = writeln!(out, "  (nothing pending — the tags already carry releases)");
            }
            for effect in effects {
                let _ = writeln!(out, "  {}", pr_effect_gha(effect));
            }
        }
        ForgeReleases::Skipped { reason } => {
            let _ = writeln!(out, "forge releases: skipped — {reason}");
        }
    }
    out
}

/// Renders the live release run for GitHub Actions summaries.
pub fn release_live_gha(
    head: &str,
    states: &[PackageTagState],
    report: &TagPhaseReport,
    provenance: &TargetProvenanceMap,
    push: Option<&TagPushReport>,
    forge_releases: &ForgeReleases,
) -> String {
    let mut out = String::from("## halyard release (live)\n\n");
    let _ = writeln!(out, "- head: `{}`", short(head));
    let _ = writeln!(
        out,
        "- tag directives (§11.1 step 2): {} owed ({} created, {} already tagged, {} failed)",
        report.rows.len(),
        report.created,
        report.already_tagged,
        report.failed
    );
    release_rows_human(&mut out, states, report, provenance, "created");
    out.push_str(&release_push_human(push, false));
    match forge_releases {
        ForgeReleases::Ran {
            repo,
            effects,
            calls,
            governor,
        } => {
            let _ = writeln!(out, "- forge releases: ran over `{repo}` ({calls} call(s))");
            let _ = writeln!(out, "  - {}", governor.line());
            for effect in effects {
                let _ = writeln!(out, "  - {}", pr_effect_gha(effect));
            }
        }
        ForgeReleases::Skipped { reason } => {
            let _ = writeln!(out, "- forge releases: skipped ({reason})");
        }
    }
    if report.failed > 0 || push.is_some_and(|p| p.failed > 0) {
        let _ = writeln!(
            out,
            "\n::warning title=tag-phase failures::{} directive(s) and {} push row(s) failed; \
             the phase is resumable — resolve the conflicts by hand and re-run",
            report.failed,
            push.map_or(0, |p| p.failed)
        );
    }
    out
}

// ---------------------------------------------------------------------------
// migrate (M4.7, §16.2)
// ---------------------------------------------------------------------------

/// The migration report's machine shape: the engine's own
/// [`halyard_config::MigrationReport`] serialization (the engine type is
/// the single source of the field names; `BTreeMap`/sorted vectors keep
/// it deterministic, I3). The `write` block exists only when `--write`
/// ran — without the flag the document is the frozen report-only shape.
pub fn migrate_report_json(
    report: &halyard_config::MigrationReport,
    write: Option<&halyard_config::DraftWriteReport>,
) -> serde_json::Result<serde_json::Value> {
    let mut value = serde_json::to_value(report)?;
    if let (Some(write), Some(map)) = (write, value.as_object_mut()) {
        map.insert("write".to_owned(), serde_json::to_value(write)?);
    }
    Ok(value)
}

/// The migration report, human shape (M4.7): per tool the found
/// files, the mapped rows with their notes, the semantic differences
/// and the draft `halyard.toml`. The header names what the command did
/// with the draft: without `--write` (or when nothing was configured)
/// it wrote nothing; with `--write` it names the file, the byte count
/// and the tool the draft came from.
pub fn migrate_report(
    report: &halyard_config::MigrationReport,
    write: Option<&halyard_config::DraftWriteReport>,
) -> String {
    let configured = report.tools.iter().filter(|t| t.configured).count();
    let mut out = String::new();
    let landed = write.filter(|w| w.outcome == halyard_config::DraftWriteOutcome::Written);
    match landed {
        Some(write) => {
            let _ = writeln!(
                out,
                "migrate (M4.7): {} tool(s) surveyed, {} configured — wrote `{}` ({} bytes) \
                 from the {} draft; the pre-flight refusals still fail closed",
                report.tools.len(),
                configured,
                write.path,
                write.bytes,
                write.tool
            );
        }
        None => {
            let _ = writeln!(
                out,
                "migrate (M4.7): {} tool(s) surveyed, {} configured with foreign configs — \
                 the report is data; halyard never wrote anything",
                report.tools.len(),
                configured
            );
        }
    }
    for survey in &report.tools {
        if !survey.configured {
            let _ = writeln!(out, "{}: not detected", survey.tool);
            continue;
        }
        let _ = writeln!(
            out,
            "{}: configured ({})",
            survey.tool,
            survey.files.join(", ")
        );
        for row in &survey.rows {
            let _ = writeln!(
                out,
                "  mapped: {} → {} = {}",
                row.foreign_key, row.halyard_key, row.value
            );
            if let Some(note) = &row.note {
                let _ = writeln!(out, "    note: {note}");
            }
        }
        for row in &survey.unmapped {
            let _ = writeln!(out, "  unmapped: {} — {}", row.foreign_key, row.reason);
        }
        for package in &survey.packages {
            let _ = writeln!(
                out,
                "  package: {} (component: {}, tag: {})",
                package.path,
                package.component.as_deref().unwrap_or("(path)"),
                package.tag
            );
            for note in &package.notes {
                let _ = writeln!(out, "    {note}");
            }
        }
        if let Some(draft) = &survey.draft {
            let label = match landed {
                Some(write) if survey.tool == write.tool => format!(
                    "draft halyard.toml (written to `{}`, {} bytes):",
                    write.path, write.bytes
                ),
                _ => "draft halyard.toml (report copy, never written):".to_owned(),
            };
            let _ = writeln!(out, "  {label}");
            for line in draft.lines() {
                let _ = writeln!(out, "    {line}");
            }
        }
    }
    if write.is_some_and(|w| w.outcome == halyard_config::DraftWriteOutcome::NothingConfigured) {
        let _ = writeln!(
            out,
            "nothing to write: no foreign configuration was detected — absence is a value, \
             not an error"
        );
    }
    out
}

/// The migration report, GHA shape (M4.7): a job-summary table
/// per configured tool plus `::warning` annotations for the semantic
/// differences (the rows a migration review must read); a `::notice`
/// names the write when `--write` ran.
pub fn migrate_report_gha(
    report: &halyard_config::MigrationReport,
    write: Option<&halyard_config::DraftWriteReport>,
) -> String {
    let mut out = String::new();
    if let Some(write) = write.filter(|w| w.outcome == halyard_config::DraftWriteOutcome::Written) {
        let _ = writeln!(
            out,
            "::notice::halyard migrate: wrote `{}` ({} bytes) from the {} draft",
            write.path, write.bytes, write.tool
        );
    }
    for survey in &report.tools {
        if !survey.configured {
            let _ = writeln!(
                out,
                "::notice::halyard migrate: {} not detected",
                survey.tool
            );
            continue;
        }
        let _ = writeln!(out, "## halyard migrate: {}\n", survey.tool);
        let _ = writeln!(out, "| foreign key | halyard key | value |\n|---|---|---|");
        for row in &survey.rows {
            let _ = writeln!(
                out,
                "| {} | {} | {} |",
                row.foreign_key, row.halyard_key, row.value
            );
        }
        let _ = writeln!(out);
        for row in &survey.unmapped {
            let _ = writeln!(
                out,
                "::warning::halyard migrate ({}): {} — {}",
                survey.tool, row.foreign_key, row.reason
            );
        }
        if let Some(draft) = &survey.draft {
            let label = match write {
                Some(write)
                    if survey.tool == write.tool
                        && write.outcome == halyard_config::DraftWriteOutcome::Written =>
                {
                    format!("Draft `halyard.toml` (written, {} bytes):\n", write.bytes)
                }
                _ => "Draft `halyard.toml` (the report carries it; nothing was written):\n"
                    .to_owned(),
            };
            let _ = writeln!(out, "\n{label}");
            let _ = writeln!(out, "```toml\n{draft}```");
        }
    }
    out
}

// ---------------------------------------------------------------------------
// init (§16.1/§16.2, ADR-0029)
// ---------------------------------------------------------------------------

/// The init report's machine shape: the engine's own
/// [`halyard_config::InitReport`] serialization (the engine type is the
/// single source of the field names; the shape line and the landed-file
/// rows are plain data, I3).
pub fn init_report_json(
    report: &halyard_config::InitReport,
) -> serde_json::Result<serde_json::Value> {
    serde_json::to_value(report)
}

/// The init report, human shape (ADR-0029): the detected shape, one row
/// per landed file with its byte count, and the next step — the
/// zero-config plan the scaffolded repo runs right away.
pub fn init_report(report: &halyard_config::InitReport) -> String {
    let mut out = String::new();
    let noun = if report.files.len() == 1 {
        "file"
    } else {
        "files"
    };
    let _ = writeln!(
        out,
        "init (§16.1): scaffolded {} {noun} — {}",
        report.files.len(),
        report.shape
    );
    for file in &report.files {
        let note = if file.path == halyard_config::INIT_WORKFLOW_TARGET {
            "the §16.1 release workflow (the `halyard/action` glue lands with its milestone)"
        } else {
            "every key optional; the defaults ride as comments"
        };
        let _ = writeln!(
            out,
            "  wrote `{}` ({} bytes) — {note}",
            file.path, file.bytes
        );
    }
    let _ = writeln!(
        out,
        "  next: `halyard plan` shows the zero-config release plan; commit the \
         scaffold when it looks right"
    );
    out
}

/// The init report, GHA shape (ADR-0029): a `::notice` per landed file
/// plus a small job-summary table (the rows a review reads).
pub fn init_report_gha(report: &halyard_config::InitReport) -> String {
    let mut out = String::new();
    let noun = if report.files.len() == 1 {
        "file"
    } else {
        "files"
    };
    let _ = writeln!(
        out,
        "::notice::halyard init: scaffolded {} {noun} — {}",
        report.files.len(),
        report.shape
    );
    let _ = writeln!(out, "\n## halyard init\n");
    let _ = writeln!(out, "| file | bytes |\n|---|---|");
    for file in &report.files {
        let _ = writeln!(out, "| `{}` | {} |", file.path, file.bytes);
    }
    out
}

/// The `bench` report as the human table (§16.2, ADR-0042): the pass
/// posture and the repo shape the timings belong to, then one row per
/// pipeline stage (the `--profile` trace's own span vocabulary) with
/// the nearest-rank percentile columns, then the budgets posture —
/// a measurement, never a gate (the §14.1 budgets are shape-bound to
/// the synthetic fixtures; this repo's shape is its own).
pub fn bench_human(report: &crate::bench::BenchReport) -> String {
    let cache = if report.cache_warm {
        "warm".to_owned()
    } else {
        "cold (--no-cache)".to_owned()
    };
    let mut out = format!(
        "halyard bench — the planning pipeline over this repo\npasses: {} timed (+1 warmup) · cache: {cache}\nshape: {} packages · {} pending commits\n\n",
        report.iterations, report.packages, report.pending_commits
    );
    let _ = writeln!(
        out,
        "stage      samples      p50       p99       min      mean"
    );
    for stage in &report.stages {
        let _ = writeln!(
            out,
            "{stage:<10} {samples:>7} {p50:>9} {p99:>9} {min:>9} {mean:>9}",
            stage = stage.stage,
            samples = stage.samples,
            p50 = crate::bench::format_duration(stage.p50_us),
            p99 = crate::bench::format_duration(stage.p99_us),
            min = crate::bench::format_duration(stage.min_us),
            mean = crate::bench::format_duration(stage.mean_us),
        );
    }
    out.push_str(
        "budgets: none — the §14.1 budgets are shape-bound to bench/budgets.toml's \
         synthetic fixtures; this repo's shape is its own (measurement, not a gate)\n",
    );
    out
}

/// The `bench` report as the stable JSON document (§16.2): the posture,
/// the shape and the name-sorted stage distributions. The timings are
/// wall-clock — the schema, the stage order and the shape are the
/// run-stable contract.
pub fn bench_json(report: &crate::bench::BenchReport) -> serde_json::Value {
    serde_json::json!({
        "iterations": report.iterations,
        "cache": if report.cache_warm { "warm" } else { "cold" },
        "shape": {
            "packages": report.packages,
            "pending_commits": report.pending_commits,
        },
        "stages": report.stages.iter().map(|stage| serde_json::json!({
            "stage": stage.stage,
            "samples": stage.samples,
            "p50_us": stage.p50_us,
            "p99_us": stage.p99_us,
            "min_us": stage.min_us,
            "mean_us": stage.mean_us,
        })).collect::<Vec<serde_json::Value>>(),
    })
}

/// The `bench` report as a GHA summary (§16.3): the same table the
/// human render carries, markdown-shaped for the job summary.
pub fn bench_gha(report: &crate::bench::BenchReport) -> String {
    let cache = if report.cache_warm { "warm" } else { "cold" };
    let mut out = format!(
        "## halyard bench\n\n{} timed passes (+1 warmup) · cache: {cache} · shape: {} packages, {} pending commits\n\n| stage | samples | p50 | p99 | min | mean |\n|---|---|---|---|---|---|\n",
        report.iterations, report.packages, report.pending_commits
    );
    for stage in &report.stages {
        let _ = writeln!(
            out,
            "| {stage} | {samples} | {p50} | {p99} | {min} | {mean} |",
            stage = stage.stage,
            samples = stage.samples,
            p50 = crate::bench::format_duration(stage.p50_us),
            p99 = crate::bench::format_duration(stage.p99_us),
            min = crate::bench::format_duration(stage.min_us),
            mean = crate::bench::format_duration(stage.mean_us),
        );
    }
    out.push_str(
        "\nBudgets: none — the §14.1 budgets are shape-bound to the synthetic \
         fixtures; measurement, not a gate.\n",
    );
    out
}

//! The Cargo manifest patcher (M3.1, §7.3 edit subset): re-applies the
//! plan's byte patches (§5.8 `edits`, §9.1) to a `Cargo.toml`
//! **semantically and in place**, preserving every byte outside the
//! edited literals (invariant I4).
//!
//! The planner emits patches whose spans come from discovery's
//! `toml_edit` spans: the span of a string value covers the quoted
//! literal, and the patch text is the new requirement/version **without**
//! the string delimiters. This module:
//!
//! 1. **Validates** every patch against a span-carrying `toml_edit` parse
//!    of the manifest. A patch span must match exactly one *editable
//!    slot*: the requirement value of a dependency row in a dep-kind
//!    section (`dependencies`, `dev-dependencies`,
//!    `build-dependencies`, `workspace.dependencies`,
//!    `target.<cfg>.…`), the own `[package].version` value, or the
//!    inherited `[workspace.package].version` value.
//! 2. **Classifies** the row exactly like the planner's rewriter
//!    ([`halyard_core::rewrite::rewrite_requirement`]): plain semver
//!    requirements rewrite; protocols (`workspace:`/`catalog:`/`link:`/
//!    `file:`) and `*` are skip-and-count (the rewriter returns
//!    `Unchanged` for them); an unparseable row is
//!    `E-EDIT-UNREWRITABLE` because plan patches are required-to-edit by
//!    construction. Version-inheritance rows (`version.workspace = true`,
//!    `dep = { workspace = true }`) are never rewritten — rule (c): their
//!    authoritative site is the workspace-root manifest, which carries
//!    the matching `[workspace.dependencies]` slot instead (§6.3).
//! 3. **Rewrites** only the matched literal's bytes — the new text is
//!    re-quoted with the literal's original quote character, so every
//!    byte outside the span (decor, comments, ordering, CRLF, BOM)
//!    survives untouched. The rewrite is deliberately a byte replacement
//!    over the source text, NOT a `toml_edit::DocumentMut`
//!    re-serialization: the mutable document normalizes CRLF to LF on
//!    output (verified against `toml_edit` 0.25), which would violate
//!    invariant I4 for CRLF manifests.
//! 4. **Verifies** by re-parse that every rewritten value reads back as
//!    the patch text (M3.1 acceptance: semantic read-back equality) and
//!    that untouched slot values are unchanged.
//!
//! All patches validate before any byte is rewritten — a failure leaves
//! the text untouched (§9.1 apply protocol: all or none).

use halyard_core::edit::EditFailure;
use halyard_core::package::Span;
use halyard_core::plan::PlanPatch;
use halyard_core::rewrite::{UpperBoundPolicy, rewrite_requirement};

/// Requirement prefixes the planner's rewriter classifies as `Unchanged`
/// (protocols resolved at publish time, ADR-0006 / appendix A.2).
const PROTOCOL_PREFIXES: [&str; 4] = ["workspace:", "catalog:", "link:", "file:"];

/// How a matched slot is addressed for the read-back check: the key path
/// from the document root to the row table, the row key and (for
/// inline-table/long-form rows) the field carrying the requirement.
#[derive(Clone, Debug, PartialEq, Eq)]
struct KeyPath {
    /// Table path from the document root (e.g. `["dependencies"]`,
    /// `["workspace", "dependencies"]`, `["target", "'cfg(unix)'",
    /// "dev-dependencies"]`, `["package"]`).
    path: Vec<String>,
    /// Row key inside the path's table.
    key: String,
    /// `None` = the row item IS the string value; `Some(field)` = the
    /// named field of the row's inline table / long-form table.
    field: Option<String>,
}

/// Classification of a matched row (mirrors the planner's rewriter
/// outcomes, [`halyard_core::rewrite::RewriteOutcome`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SlotClass {
    /// A version value (`[package].version`); validates as a full semver
    /// version.
    Version,
    /// A requirement value; validates as a plain semver requirement.
    Requirement,
    /// A version-inheritance row (`workspace = true`); never rewritten —
    /// rule (c), the workspace-root row is the edit site (§6.3).
    Inherited,
}

/// One editable slot discovered during the validation walk.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Slot {
    /// Byte span of the value literal (string values include the quotes —
    /// discovery records the same spans, §9.1).
    span: Span,
    /// The decoded text currently in the slot (no delimiters; empty for
    /// inheritance rows, which carry no readable requirement).
    current: String,
    /// Key path for the read-back check.
    path: KeyPath,
    /// Row classification.
    class: SlotClass,
}

/// Applied/skipped row counts of one edit pass (M3.6 apply summary).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EditCounts {
    /// Rows whose text was rewritten.
    pub applied: usize,
    /// Rows matched but intentionally not rewritten (already applied,
    /// protocols, `*`, inheritance rows).
    pub skipped: usize,
}

/// Result of one patcher pass: the patched manifest text plus the row
/// counts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditResult {
    /// The patched manifest text (byte-identical to the input when no
    /// patch applied).
    pub text: String,
    /// Row counts.
    pub counts: EditCounts,
}

/// Applies the plan's requirement/version patches to `raw` in place
/// (M3.1). `raw` must be the manifest text the patches were computed
/// against (the current working-tree file in the normal apply flow).
///
/// # Errors
///
/// [`EditFailure::SpanMismatch`] when a patch span matches no editable
/// slot (the manifest drifted from the plan — re-plan instead of forcing
/// the patch), [`EditFailure::Unrewritable`] when a required-to-edit row
/// cannot be rewritten (`E-EDIT-UNREWRITABLE`), [`EditFailure::Parse`]
/// when the manifest does not parse or the read-back check fails.
pub fn edit_versions(
    raw: &str,
    file: &str,
    patches: &[PlanPatch],
) -> Result<EditResult, EditFailure> {
    if patches.is_empty() {
        return Ok(EditResult {
            text: raw.to_owned(),
            counts: EditCounts::default(),
        });
    }
    let slots = collect_slots(&parse_reference(raw, file)?);
    let plan = validate(patches, &slots, file)?;
    let counts = EditCounts {
        applied: plan.iter().filter(|edit| edit.applied).count(),
        skipped: plan.iter().filter(|edit| !edit.applied).count(),
    };
    let text = rewrite_bytes(raw, file, &plan)?;
    verify(&text, file, &plan)?;
    Ok(EditResult { text, counts })
}

/// Parses the manifest for the span-carrying validation walk.
fn parse_reference<'a>(
    raw: &'a str,
    file: &str,
) -> Result<toml_edit::Document<&'a str>, EditFailure> {
    toml_edit::Document::parse(raw).map_err(|e| EditFailure::Parse {
        file: file.to_owned(),
        message: e.to_string(),
    })
}

/// One validated patch: its input, the matched slot and the decision.
struct PlannedEdit<'a> {
    /// The patch being applied.
    patch: &'a PlanPatch,
    /// The matched slot.
    slot: Slot,
    /// `true` = rewrite the value; `false` = skip (already applied /
    /// protocol / inherited).
    applied: bool,
}

/// Validates every patch against the slot inventory. Fail-closed: the
/// first unmatched or unrewritable patch aborts before any byte changes.
fn validate<'a>(
    patches: &'a [PlanPatch],
    slots: &[Slot],
    file: &str,
) -> Result<Vec<PlannedEdit<'a>>, EditFailure> {
    let mut planned = Vec::with_capacity(patches.len());
    for patch in patches {
        let wanted = Span {
            start: patch.start,
            end: patch.end,
        };
        let Some(slot) = slots.iter().find(|slot| slot.span == wanted) else {
            return Err(EditFailure::SpanMismatch {
                file: file.to_owned(),
                start: patch.start,
                end: patch.end,
            });
        };
        let applied = decide(patch, slot, file)?;
        planned.push(PlannedEdit {
            patch,
            slot: slot.clone(),
            applied,
        });
    }
    Ok(planned)
}

/// Classifies one matched patch (rules (b)/(c)): already-applied rows,
/// protocols, `*` and inheritance rows are skipped; rows that parse as
/// plain semver rewrite; everything else is `E-EDIT-UNREWRITABLE`
/// (required-to-edit, consistent with the planner's rewriter outcome).
fn decide(patch: &PlanPatch, slot: &Slot, file: &str) -> Result<bool, EditFailure> {
    if slot.current == patch.text {
        // Idempotent re-apply: the manifest already carries the target
        // text (the rewriter returns `Unchanged` here too).
        return Ok(false);
    }
    if slot.class == SlotClass::Inherited {
        // Rule (c): inheritance rows are never rewritten; the workspace
        // root row is the authoritative edit site.
        return Ok(false);
    }
    if slot.current == "*"
        || PROTOCOL_PREFIXES
            .iter()
            .any(|p| slot.current.starts_with(p))
    {
        // `Unchanged { because: Protocol | Any }` — no text change needed.
        return Ok(false);
    }
    let parses = match slot.class {
        SlotClass::Version => semver::Version::parse(&slot.current).is_ok(),
        SlotClass::Requirement => semver::VersionReq::parse(&slot.current).is_ok(),
        SlotClass::Inherited => false,
    };
    if parses {
        return Ok(true);
    }
    Err(EditFailure::Unrewritable {
        file: file.to_owned(),
        // The rewriter is the classification authority (appendix A.2): an
        // unparseable requirement text is its `E-EDIT-UNREWRITABLE`
        // failure, and the synthetic target below never changes that
        // outcome (the text fails to parse before any version logic).
        detail: unrewritable_detail(slot, patch),
    })
}

/// Builds the rewriter-consistent failure detail for an unrewritable row.
fn unrewritable_detail(slot: &Slot, patch: &PlanPatch) -> halyard_core::rewrite::VersionError {
    let target =
        semver::Version::parse(&patch.text).unwrap_or_else(|_| semver::Version::new(0, 0, 0));
    rewrite_requirement(&slot.current, &target, UpperBoundPolicy::Widen)
        .err()
        .unwrap_or_else(|| {
            // Unreachable by construction: `decide` only routes rows here
            // that failed to parse, and the rewriter fails identically on
            // them.
            halyard_core::rewrite::VersionError::unparseable(&slot.current, &target)
        })
}

/// Replaces each applied patch's literal bytes with the re-quoted patch
/// text, applied right-to-left so earlier spans stay valid. Everything
/// outside the matched spans is copied verbatim (invariant I4).
fn rewrite_bytes(raw: &str, file: &str, plan: &[PlannedEdit<'_>]) -> Result<String, EditFailure> {
    let mut replacements: Vec<(u32, u32, String)> = Vec::new();
    for edit in plan.iter().filter(|edit| edit.applied) {
        let (start, end) = (
            usize::try_from(edit.slot.span.start).unwrap_or(usize::MAX),
            usize::try_from(edit.slot.span.end).unwrap_or(usize::MAX),
        );
        let Some(literal) = raw.get(start..end) else {
            return Err(EditFailure::SpanMismatch {
                file: file.to_owned(),
                start: edit.slot.span.start,
                end: edit.slot.span.end,
            });
        };
        // The span covers the quoted literal; re-quote the new text with
        // the literal's own quote character.
        let quote = if literal.starts_with('\'') { '\'' } else { '"' };
        replacements.push((
            edit.slot.span.start,
            edit.slot.span.end,
            format!("{quote}{}{quote}", edit.patch.text),
        ));
    }
    replacements.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
    let mut out = raw.as_bytes().to_vec();
    for (start, end, text) in replacements {
        let (s, e) = (
            usize::try_from(start).unwrap_or(usize::MAX),
            usize::try_from(end).unwrap_or(usize::MAX),
        );
        if s > e || e > out.len() {
            return Err(EditFailure::SpanMismatch {
                file: file.to_owned(),
                start,
                end,
            });
        }
        if let Some(tail) = out.get(e..) {
            let mut next = out.get(..s).unwrap_or(&[]).to_vec();
            next.extend_from_slice(text.as_bytes());
            next.extend_from_slice(tail);
            out = next;
        }
    }
    String::from_utf8(out).map_err(|_| EditFailure::Parse {
        file: file.to_owned(),
        message: "rewritten manifest is not valid UTF-8".to_owned(),
    })
}

/// Verifies by re-parse that every rewritten value reads back as the
/// patch text and every skipped row kept its text (M3.1 acceptance:
/// semantic read-back equality).
fn verify(text: &str, file: &str, plan: &[PlannedEdit<'_>]) -> Result<(), EditFailure> {
    let output = parse_reference(text, file)?;
    for edit in plan {
        let path = &edit.slot.path;
        let read_back =
            row_ref(&output, &path.path, &path.key).and_then(|item| match &path.field {
                None => item.as_value().and_then(toml_edit::Value::as_str),
                Some(field) => item
                    .as_table_like()
                    .and_then(|table| table.get(field.as_str()))
                    .and_then(toml_edit::Item::as_value)
                    .and_then(toml_edit::Value::as_str),
            });
        let expected = if edit.applied {
            Some(edit.patch.text.as_str())
        } else if edit.slot.class == SlotClass::Inherited {
            None
        } else {
            Some(edit.slot.current.as_str())
        };
        match (expected, read_back) {
            (None, _) => {}
            (Some(expected), Some(actual)) if actual == expected => {}
            (Some(expected), found) => {
                return Err(EditFailure::Parse {
                    file: file.to_owned(),
                    message: format!(
                        "read-back check failed for {} (expected {expected:?}, got {found:?})",
                        render_path(path)
                    ),
                });
            }
        }
    }
    Ok(())
}

/// Flattens a [`KeyPath`] for error messages.
fn render_path(path: &KeyPath) -> String {
    let mut out = path.path.join(".");
    out.push('.');
    out.push_str(&path.key);
    if let Some(field) = &path.field {
        out.push('.');
        out.push_str(field);
    }
    out
}

/// Navigates the document immutably by table path and row key (read-back
/// verification).
fn row_ref<'a>(
    doc: &'a toml_edit::Document<&'a str>,
    path: &[String],
    key: &str,
) -> Option<&'a toml_edit::Item> {
    let mut table: &dyn toml_edit::TableLike = doc.as_table();
    for segment in path {
        table = table.get(segment.as_str())?.as_table_like()?;
    }
    table.get(key)
}

/// Walks the span-carrying document and records every editable slot: the
/// own/inherited version values, every dependency requirement value in
/// the dep-kind sections (rule (a)) and the inheritance rows themselves
/// (rule (c)).
fn collect_slots(doc: &toml_edit::Document<&str>) -> Vec<Slot> {
    let mut slots = Vec::new();
    // Own version + inherited workspace version.
    if let Some(version) = doc
        .get("package")
        .and_then(toml_edit::Item::as_table)
        .and_then(|table| table.get("version"))
    {
        push_version_slot(version, &["package".to_owned()], &mut slots);
    }
    if let Some(version) = doc
        .get("workspace")
        .and_then(toml_edit::Item::as_table)
        .and_then(|table| table.get("package"))
        .and_then(toml_edit::Item::as_table)
        .and_then(|table| table.get("version"))
    {
        push_version_slot(
            version,
            &["workspace".to_owned(), "package".to_owned()],
            &mut slots,
        );
    }
    // Root-level dependency sections (rule (a)).
    for section in ["dependencies", "dev-dependencies", "build-dependencies"] {
        if let Some(table) = doc.get(section).and_then(toml_edit::Item::as_table) {
            collect_section(table, &[section], &mut slots);
        }
    }
    // [workspace.dependencies]: the authoritative site for rows inherited
    // by members (`dep = { workspace = true }`, §6.3).
    if let Some(table) = doc
        .get("workspace")
        .and_then(toml_edit::Item::as_table)
        .and_then(|table| table.get("dependencies"))
        .and_then(toml_edit::Item::as_table)
    {
        collect_section(table, &["workspace", "dependencies"], &mut slots);
    }
    // [target.<cfg>.<section>] rows (rule (a)).
    if let Some(targets) = doc.get("target").and_then(toml_edit::Item::as_table) {
        for (cfg, target_item) in targets {
            let Some(target_table) = target_item.as_table() else {
                continue;
            };
            for section in ["dependencies", "dev-dependencies", "build-dependencies"] {
                if let Some(table) = target_table
                    .get(section)
                    .and_then(toml_edit::Item::as_table)
                {
                    collect_section(table, &["target", cfg, section], &mut slots);
                }
            }
        }
    }
    slots
}

/// Records the slots of one dependency section table. Renames are
/// irrelevant here: patches match by span, not by dependency name.
fn collect_section(table: &toml_edit::Table, path: &[&str], slots: &mut Vec<Slot>) {
    let path: Vec<String> = path.iter().map(|segment| (*segment).to_owned()).collect();
    for (name, item) in table {
        match item {
            toml_edit::Item::Value(value) => {
                if let Some(text) = value.as_str() {
                    // Plain string requirement row.
                    push_slot(
                        value,
                        text.to_owned(),
                        row_path(&path, name, None),
                        SlotClass::Requirement,
                        slots,
                    );
                    continue;
                }
                if let Some(inline) = value.as_inline_table() {
                    if is_inherit_inline(inline) {
                        // Rule (c): `dep = { workspace = true }` — record
                        // the row as inherited so a stale plan patch on
                        // its span is skipped, not forced.
                        push_slot(
                            value,
                            String::new(),
                            row_path(&path, name, None),
                            SlotClass::Inherited,
                            slots,
                        );
                        continue;
                    }
                    if let Some(version) = inline.get("version")
                        && let Some(text) = version.as_str()
                    {
                        // `{ version = "…", features = … }`: only the
                        // version field is the edit site (rule (b)).
                        push_slot(
                            version,
                            text.to_owned(),
                            row_path(&path, name, Some("version")),
                            SlotClass::Requirement,
                            slots,
                        );
                    }
                }
            }
            toml_edit::Item::Table(sub) => {
                if is_inherit_table(sub) {
                    // Rule (c): dotted `dep.workspace = true`.
                    push_slot_item(item, row_path(&path, name, None), slots);
                    continue;
                }
                if let Some(version) = sub.get("version").and_then(toml_edit::Item::as_value)
                    && let Some(text) = version.as_str()
                {
                    // Long-form `[dependencies.<name>] version = "…"`.
                    push_slot(
                        version,
                        text.to_owned(),
                        row_path(&path, name, Some("version")),
                        SlotClass::Requirement,
                        slots,
                    );
                }
            }
            _ => {}
        }
    }
}

/// Builds the key path for one dependency row.
fn row_path(path: &[String], key: &str, field: Option<&str>) -> KeyPath {
    KeyPath {
        path: path.to_vec(),
        key: key.to_owned(),
        field: field.map(str::to_owned),
    }
}

/// True when an inline-table row is the `workspace = true` inherit form.
fn is_inherit_inline(inline: &toml_edit::InlineTable) -> bool {
    inline.get("workspace").and_then(toml_edit::Value::as_bool) == Some(true)
}

/// True when a long-form row is the dotted `workspace = true` inherit form.
fn is_inherit_table(table: &toml_edit::Table) -> bool {
    table.get("workspace").and_then(toml_edit::Item::as_bool) == Some(true)
}

/// Records a version slot from a version item (`path` labels the row for
/// the read-back key path).
fn push_version_slot(item: &toml_edit::Item, path: &[String], slots: &mut Vec<Slot>) {
    if let Some(value) = item.as_value()
        && let Some(text) = value.as_str()
    {
        push_slot(
            value,
            text.to_owned(),
            KeyPath {
                path: path.to_vec(),
                key: "version".to_owned(),
                field: None,
            },
            SlotClass::Version,
            slots,
        );
    }
}

/// Records a slot from a value (span includes the quotes).
fn push_slot(
    value: &toml_edit::Value,
    current: String,
    path: KeyPath,
    class: SlotClass,
    slots: &mut Vec<Slot>,
) {
    let Some(range) = value.span() else {
        return;
    };
    slots.push(Slot {
        span: crate::span_of(range),
        current,
        path,
        class,
    });
}

/// Records an inheritance-row slot from an item (span of the row header;
/// no readable requirement).
fn push_slot_item(item: &toml_edit::Item, path: KeyPath, slots: &mut Vec<Slot>) {
    let Some(range) = item.span() else {
        return;
    };
    slots.push(Slot {
        span: crate::span_of(range),
        current: String::new(),
        path,
        class: SlotClass::Inherited,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use halyard_core::plan::PlanPatch;

    /// Byte patch over `needle`'s literal span (string values: quotes
    /// included — discovery's span convention).
    fn patch_at(raw: &str, needle: &str, text: &str) -> PlanPatch {
        let start = u32::try_from(raw.find(needle).unwrap_or(0)).unwrap_or(u32::MAX);
        let end = start.saturating_add(u32::try_from(needle.len()).unwrap_or(u32::MAX));
        PlanPatch {
            start,
            end,
            text: text.to_owned(),
        }
    }

    fn edit(raw: &str, patches: &[PlanPatch]) -> (String, EditCounts) {
        let out = edit_versions(raw, "Cargo.toml", patches).unwrap();
        (out.text, out.counts)
    }

    #[test]
    fn plain_requirement_and_own_version_rewrite() {
        let raw = "[package]\nname = \"app\"\nversion = \"0.4.1\"\nedition = \"2021\"\n\n[dependencies]\ncore =  \"0.4\"  # keep\nserde = \"1.0\"\n";
        let patches = [
            patch_at(raw, "\"0.4.1\"", "0.5.0"),
            patch_at(raw, "\"0.4\"", "0.5"),
        ];
        let (out, counts) = edit(raw, &patches);
        assert_eq!(
            out,
            "[package]\nname = \"app\"\nversion = \"0.5.0\"\nedition = \"2021\"\n\n[dependencies]\ncore =  \"0.5\"  # keep\nserde = \"1.0\"\n"
        );
        assert_eq!(
            counts,
            EditCounts {
                applied: 2,
                skipped: 0
            }
        );
    }

    #[test]
    fn workspace_dependencies_row_rewrites_only_the_version_field() {
        let raw = "[workspace.dependencies]\ncommon = { version = \"0.2\", features = [\"std\"] }\nplain = \"1.2\"\n";
        let patches = [patch_at(raw, "\"0.2\"", "0.3")];
        let (out, counts) = edit(raw, &patches);
        assert_eq!(
            out,
            "[workspace.dependencies]\ncommon = { version = \"0.3\", features = [\"std\"] }\nplain = \"1.2\"\n"
        );
        assert_eq!(counts.applied, 1);
    }

    #[test]
    fn target_specific_section_rewrites() {
        let raw =
            "[target.'cfg(unix)'.dependencies]\nfoo = \"1.0\"\n\n[dependencies]\nbar = \"2.0\"\n";
        let patches = [patch_at(raw, "\"1.0\"", "1.1")];
        let (out, counts) = edit(raw, &patches);
        assert_eq!(
            out,
            "[target.'cfg(unix)'.dependencies]\nfoo = \"1.1\"\n\n[dependencies]\nbar = \"2.0\"\n"
        );
        assert_eq!(counts.applied, 1);
    }

    #[test]
    fn long_form_table_row_rewrites() {
        let raw = "[dependencies.serde]\nversion = \"1.0\"\nfeatures = [\"derive\"]\n";
        let patches = [patch_at(raw, "\"1.0\"", "1.1")];
        let (out, counts) = edit(raw, &patches);
        assert_eq!(
            out,
            "[dependencies.serde]\nversion = \"1.1\"\nfeatures = [\"derive\"]\n"
        );
        assert_eq!(counts.applied, 1);
    }

    #[test]
    fn skip_and_count_star_protocol_and_inherited_rows() {
        let raw = "[dependencies]\nany = \"*\"\nweird = \"workspace:1.2\"\ninherited = { workspace = true }\nkept = \"3.0\"\n";
        let patches = [
            patch_at(raw, "\"*\"", "4.0"),
            patch_at(raw, "\"workspace:1.2\"", "4.0"),
            patch_at(raw, "{ workspace = true }", "4.0"),
        ];
        let (out, counts) = edit(raw, &patches);
        assert_eq!(out, raw, "skipped rows leave the manifest byte-identical");
        assert_eq!(counts.applied, 0);
        assert_eq!(counts.skipped, 3);
    }

    #[test]
    fn already_applied_rows_are_skipped_idempotent_reapply() {
        let raw = "[package]\nversion = \"0.4.1\"\n\n[dependencies]\ncore = \"0.4\"\n";
        let patches = [
            patch_at(raw, "\"0.4.1\"", "0.5.0"),
            patch_at(raw, "\"0.4\"", "0.5"),
        ];
        let (once, first) = edit(raw, &patches);
        assert_eq!(first.applied, 2);
        // Second pass over the edited manifest: spans still align (same
        // text length), every row already carries the target text.
        let (twice, second) = edit(&once, &patches);
        assert_eq!(second.applied, 0);
        assert_eq!(second.skipped, 2);
        assert_eq!(twice, once, "re-apply is byte-identical (idempotence)");
    }

    #[test]
    fn unparseable_requirement_row_is_e_edit_unrewritable() {
        let raw = "[dependencies]\nodd = \">=8 <10\"\n";
        let patches = [patch_at(raw, "\">=8 <10\"", "9.0")];
        let err = edit_versions(raw, "Cargo.toml", &patches).unwrap_err();
        assert_eq!(err.code(), "E-EDIT-UNREWRITABLE");
        assert!(matches!(err, EditFailure::Unrewritable { .. }));
    }

    #[test]
    fn stale_span_is_e_edit_patches() {
        let raw = "[dependencies]\ncore = \"0.4\"\n";
        let patches = [PlanPatch {
            start: 0,
            end: 2,
            text: "9.9".to_owned(),
        }];
        let err = edit_versions(raw, "Cargo.toml", &patches).unwrap_err();
        assert_eq!(err.code(), "E-EDIT-PATCHES");
        assert!(matches!(err, EditFailure::SpanMismatch { .. }));
    }

    #[test]
    fn inherited_version_site_is_the_workspace_root_row() {
        // Member manifest: `version.workspace = true` carries no version
        // value; the patch targets the root manifest's
        // [workspace.package].version (rule (c), §6.3).
        let root = "[workspace.package]\nversion = \"0.2.0\"\n\n[dependencies]\nlib = \"0.1\"\n";
        let patches = [patch_at(root, "\"0.2.0\"", "0.3.0")];
        let (out, counts) = edit(root, &patches);
        assert_eq!(
            out,
            "[workspace.package]\nversion = \"0.3.0\"\n\n[dependencies]\nlib = \"0.1\"\n"
        );
        assert_eq!(counts.applied, 1);
    }

    #[test]
    fn formatting_comments_and_crlf_survive_byte_for_byte_outside_spans() {
        let raw = "# lead comment\n[package]\nname = \"app\"  # trailing\nversion = \"1.0.0\"\r\n\r\n[dev-dependencies]\ntemp = \"2.2\"   # aligned\n";
        let patches = [
            patch_at(raw, "\"1.0.0\"", "1.0.1"),
            patch_at(raw, "\"2.2\"", "2.3"),
        ];
        let (out, counts) = edit(raw, &patches);
        // Byte compare outside the two edited spans: mask the literals on
        // both sides and require everything else to match verbatim.
        let unchanged = raw.replace("\"1.0.0\"", "@A@").replace("\"2.2\"", "@B@");
        let edited = out.replace("\"1.0.1\"", "@A@").replace("\"2.3\"", "@B@");
        assert_eq!(unchanged, edited);
        assert!(out.contains('\r'), "CRLF preserved");
        assert_eq!(counts.applied, 2);
    }
}

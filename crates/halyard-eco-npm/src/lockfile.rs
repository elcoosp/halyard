//! The `pnpm-lock.yaml` patcher (M3.4, §9.3): rewrites the importer rows
//! pnpm itself would rewrite for the plan's version moves, byte-in place
//! (invariant I4).
//!
//! §9.3 direct-patch scope — edits are **restricted to the `importers:`
//! section**; everything else declines to [`LockfileOutcome::NeedsNative`]
//! so the orchestrator can run the native refresh effect step
//! (`pnpm install --lockfile-only --offline`):
//!
//! 1. A changed member's importer rows follow the move when the member is
//!    resolved as a workspace **link** (the only resolution a byte-safe
//!    patch can express):
//!    - `specifier:` mirrors `package.json`. Workspace/`catalog:`/
//!      `file:`/`link:` protocol specifiers need no change (the plan does
//!      not rewrite protocol rows, M3.3's skip-and-count); a plain semver
//!      specifier is advanced by [`halyard_core::rewrite::rewrite_requirement`]
//!      — the canonical rewriter (ADR-0008) the manifest patcher uses, so
//!      lockfile and manifest converge on the same text.
//!    - `version:` rows are `link:<path>` (no version embedded — nothing
//!      to change) or `<version>(link:<path>)` (the resolved member
//!      version rides the link ref; its version token follows the move).
//! 2. Anything unrecognized declines: a member appearing as a resolution
//!    key (`/name@version`) under `packages:`/`snapshots:` (a registry
//!    resolution, not a link), a bare version `version:` row (same), a
//!    foreign-grammar or unrewritable specifier, a lockfile header other
//!    than `lockfileVersion: ≥ 6` (importer `specifier`/`version` rows
//!    exist since 6.0), or a multi-document lockfile. Non-UTF-8 input is
//!    a hard `E-LOCK-PARSE` failure.
//!
//! Unlike `Cargo.lock`, a pnpm lockfile has **no row that must exist** for
//! a member: a workspace member nobody depends on contributes no importer
//! rows, so "no rows found" is a legitimate no-op (documented difference
//! from §9.2 rule 1). Rows that reference a changed member but already sit
//! at the target version are skipped (idempotent re-apply, I1).
//!
//! Rewrites are byte replacements over the source text (spans from a
//! line-oriented scan of the restricted grammar), never a re-serialization:
//! every byte outside the replaced scalars — comments, ordering,
//! whitespace, EOLs, quoting — survives verbatim. A read-back re-analysis
//! verifies the patched file (§9.1 apply protocol): the pass is
//! all-or-nothing and idempotent.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

use halyard_core::lockfile::{LockfileFailure, LockfileOutcome, LockfileVersionChange};
use halyard_core::rewrite::{RewriteOutcome, UpperBoundPolicy, rewrite_requirement};

/// Repo-relative name of the pnpm lockfile (§9.3).
pub const PNPM_LOCKFILE: &str = "pnpm-lock.yaml";

/// Which npm-family lockfile a workspace carries (the native refresh
/// dispatch key, §9.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LockfileKind {
    /// `pnpm-lock.yaml` — patched in memory (§9.3) and refreshed with
    /// `pnpm install --lockfile-only --offline`.
    Pnpm,
    /// `package-lock.json` — native-only in v1.
    Npm,
    /// `yarn.lock` — native-only in v1.
    Yarn,
    /// `bun.lock` / `bun.lockb` — native-only in v1.
    Bun,
    /// None of the npm-family lockfiles exists under the root.
    None,
}

/// Detects the workspace's lockfile kind by the files present under
/// `ws_root` (a filesystem stat — the effect-step input, not planning
/// I/O, §9.3).
#[must_use]
pub fn detect_lockfile_kind(ws_root: &std::path::Path) -> LockfileKind {
    let exists = |name: &str| ws_root.join(name).is_file();
    if exists(PNPM_LOCKFILE) {
        LockfileKind::Pnpm
    } else if exists("package-lock.json") {
        LockfileKind::Npm
    } else if exists("yarn.lock") {
        LockfileKind::Yarn
    } else if exists("bun.lock") || exists("bun.lockb") {
        LockfileKind::Bun
    } else {
        LockfileKind::None
    }
}

/// The native refresh command for one lockfile kind: the ecosystem tool's
/// own lockfile-only rewrite (§9.3 — `pnpm install --lockfile-only
/// --offline` for pnpm; `npm install --package-lock-only`,
/// `yarn install --mode update-lockfile`, `bun install --lockfile-only`
/// for the native-only lockfiles). Returns `(program, argv, display)` —
/// argv arrays only, no shell (invariant I7). `None` when no lockfile is
/// present.
#[must_use]
pub fn refresh_command(
    kind: LockfileKind,
) -> Option<(&'static str, &'static [&'static str], &'static str)> {
    match kind {
        LockfileKind::Pnpm => Some((
            "pnpm",
            &["install", "--lockfile-only", "--offline"],
            "pnpm install --lockfile-only --offline",
        )),
        LockfileKind::Npm => Some((
            "npm",
            &["install", "--package-lock-only"],
            "npm install --package-lock-only",
        )),
        LockfileKind::Yarn => Some((
            "yarn",
            &["install", "--mode", "update-lockfile"],
            "yarn install --mode update-lockfile",
        )),
        LockfileKind::Bun => Some((
            "bun",
            &["install", "--lockfile-only"],
            "bun install --lockfile-only",
        )),
        LockfileKind::None => None,
    }
}

/// One planned byte replacement over the source text (I4: only these
/// ranges change; everything outside survives verbatim).
struct Replacement {
    /// Inclusive start byte offset (over the whole lockfile text).
    start: usize,
    /// Exclusive end byte offset.
    end: usize,
    /// Replacement text (no surrounding syntax; callers pre-quote).
    text: String,
}

/// The planned replacements + counts of one analysis pass (§9.3 rules).
struct Analysis {
    replacements: Vec<Replacement>,
    applied: usize,
    skipped: usize,
}

/// One scalar value as scanned from the source text.
struct Scalar {
    /// The value with quotes stripped (`specifier: "^0.4.1"` →
    /// `^0.4.1`).
    text: String,
    /// Byte range of the raw scalar (quotes included) over the whole
    /// lockfile text.
    raw: Range<usize>,
    /// Byte range of the scalar's inner text (quotes excluded) — the
    /// range partial rewrites (link-ref version tokens) replace.
    inner: Range<usize>,
    /// The quote character wrapping the literal, when quoted.
    quote: Option<char>,
}

/// One importer dependency row (`specifier:`/`version:` pair) as scanned
/// from the `importers:` section.
struct DepRow {
    /// Importer path (the `importers:` key, e.g. `.` or `packages/app`).
    importer: String,
    /// Dependency name (the row's enclosing key).
    dep: String,
    /// The `specifier:` scalar.
    specifier: Option<Scalar>,
    /// The `version:` scalar.
    version: Option<Scalar>,
}

/// The line-oriented scan of the restricted pnpm-lock grammar.
struct Scan {
    /// The `lockfileVersion:` scalar, when present.
    header: Option<Scalar>,
    /// Importer dependency rows in document order.
    importers: Vec<DepRow>,
    /// Container keys under every non-importers section, as
    /// `(section, key)` pairs in document order (the resolution-key check
    /// walks these).
    foreign_keys: Vec<(String, String)>,
}

/// Validates the planned changes before any byte is examined: exact
/// semver versions, a real move, non-empty names. A malformed change is a
/// plan bug, not a lockfile problem — hard `E-LOCK-PARSE`, never a
/// rewrite.
fn validate_changes(changes: &[LockfileVersionChange], file: &str) -> Result<(), LockfileFailure> {
    for change in changes {
        if change.name.is_empty() {
            return Err(LockfileFailure::Parse {
                file: file.to_owned(),
                message: "planned change has an empty package name — re-plan".to_owned(),
            });
        }
        let from = semver::Version::parse(&change.from).is_ok();
        let to = semver::Version::parse(&change.to).is_ok();
        if !from || !to {
            return Err(LockfileFailure::Parse {
                file: file.to_owned(),
                message: format!(
                    "planned change for {:?} carries a non-semver version (from {:?}, to {:?}) — \
                     re-plan",
                    change.name, change.from, change.to
                ),
            });
        }
        if change.from == change.to {
            return Err(LockfileFailure::Parse {
                file: file.to_owned(),
                message: format!(
                    "planned change for {:?} is not a version move (from == to == {:?}); \
                     no-move releases must not touch the lockfile",
                    change.name, change.from
                ),
            });
        }
    }
    Ok(())
}

/// Applies the plan's version moves to one `pnpm-lock.yaml` (M3.4). `raw`
/// must be the lockfile text as it exists in the working tree.
///
/// # Errors
///
/// [`LockfileFailure::Parse`] when the lockfile is not valid UTF-8 or a
/// planned change is not a usable exact version.
pub fn patch_pnpm_lockfile(
    raw: &str,
    file: &str,
    changes: &[LockfileVersionChange],
) -> Result<LockfileOutcome, LockfileFailure> {
    validate_changes(changes, file)?;
    if changes.is_empty() {
        // Nothing to move: the byte-identical no-op (I1/I4).
        return Ok(LockfileOutcome::Patched {
            content: raw.as_bytes().to_vec(),
            applied: 0,
            skipped: 0,
        });
    }
    let analysis = match analyze(raw, changes) {
        Ok(analysis) => analysis,
        Err(reason) => return Ok(LockfileOutcome::NeedsNative { reason }),
    };
    let applied = analysis.applied;
    let skipped = analysis.skipped;
    if analysis.replacements.is_empty() {
        // Nothing to rewrite: the byte-identical no-op (I1/I4).
        return Ok(LockfileOutcome::Patched {
            content: raw.as_bytes().to_vec(),
            applied,
            skipped,
        });
    }
    let text = apply_replacements(raw, file, &analysis.replacements)?;
    // Read-back verification (§9.1): the patched lockfile must re-analyze
    // as a pure skip pass — every row at its target, nothing left to
    // rewrite, no decline.
    let re_analysis = analyze(&text, changes).map_err(|reason| LockfileFailure::Parse {
        file: file.to_owned(),
        message: format!("read-back check failed: {reason}"),
    })?;
    if re_analysis.applied != 0 || re_analysis.skipped != applied + skipped {
        return Err(LockfileFailure::Parse {
            file: file.to_owned(),
            message: format!(
                "read-back check failed: the patched lockfile does not re-analyze as a skip \
                 pass (applied {}, skipped {} instead of 0/{})",
                re_analysis.applied,
                re_analysis.skipped,
                applied + skipped
            ),
        });
    }
    Ok(LockfileOutcome::Patched {
        content: text.into_bytes(),
        applied,
        skipped,
    })
}

/// The §9.3 analysis of one pass over `raw`: either the planned
/// replacements + counts, or the reason the pass declines to the native
/// refresh (`Err(reason)` = [`LockfileOutcome::NeedsNative`]).
fn analyze(raw: &str, changes: &[LockfileVersionChange]) -> Result<Analysis, String> {
    let decline = |reason: String| Err(reason);
    // Multi-document lockfiles (pnpm's revision feature) cannot be
    // patched byte-safely by a single-document scan.
    if raw
        .split('\n')
        .any(|line| line.trim_end_matches('\r') == "---")
    {
        return decline(
            "multi-document lockfile: patch only the last document with the ecosystem's own \
             tool; refresh natively"
                .to_owned(),
        );
    }
    let scan = scan(raw)?;
    // Header: importer `specifier`/`version` rows exist since 6.0; any
    // other (or missing) header is an unhandled shape.
    let header = scan.header.as_ref().ok_or_else(|| {
        "lockfile has no lockfileVersion header; refresh natively instead of guessing".to_owned()
    })?;
    let major = header.text.split('.').next().unwrap_or_default();
    let major = major.parse::<u64>().ok();
    match major {
        Some(value) if value >= 6 => {}
        _ => {
            return decline(format!(
                "lockfileVersion {:?} is not handled (importer specifier rows require pnpm \
                 lockfile ≥ 6); refresh natively instead of guessing",
                header.text
            ));
        }
    }
    // Resolution keys for the changed members (registry-resolved
    // members): the key itself must move, which is the native tool's job.
    for (section, key) in &scan.foreign_keys {
        for change in changes {
            if is_member_key(key, &change.name) {
                return decline(format!(
                    "member {:?} appears as resolution key {:?} under {section}: — the \
                     registry-side rows must move via the native tool",
                    change.name, key
                ));
            }
        }
    }
    let wanted: BTreeMap<&str, &LockfileVersionChange> = changes
        .iter()
        .map(|change| (change.name.as_str(), change))
        .collect();
    let mut state = Analysis {
        replacements: Vec::new(),
        applied: 0,
        skipped: 0,
    };
    for row in &scan.importers {
        let Some(change) = wanted.get(row.dep.as_str()) else {
            continue;
        };
        analyze_row(row, change, &mut state)?;
    }
    // A changed member without importer rows is a legitimate no-op (pnpm
    // records no mandatory member rows — unlike Cargo.lock, §9.2 rule 1).
    Ok(state)
}

/// §9.3 rules 1 for one importer row: the `specifier` mirrors
/// `package.json` (protocols skip, plain semver advances through the
/// canonical rewriter) and the `version` link ref follows the move.
/// `Err(reason)` = decline (`NeedsNative`).
fn analyze_row(
    row: &DepRow,
    change: &LockfileVersionChange,
    state: &mut Analysis,
) -> Result<(), String> {
    let decline = |reason: String| Err(reason);
    // The specifier row.
    let Some(spec) = &row.specifier else {
        return decline(format!(
            "importer {:?} has a dependency row for {:?} without a specifier row; refresh \
             natively instead of guessing",
            row.importer, row.dep
        ));
    };
    if !spec.text.starts_with("workspace:")
        && !spec.text.starts_with("catalog:")
        && !spec.text.starts_with("file:")
        && !spec.text.starts_with("link:")
    {
        let Some(to) = semver::Version::parse(&change.to).ok() else {
            return decline(format!(
                "planned target version {:?} for {:?} is not semver — re-plan",
                change.to, change.name
            ));
        };
        match rewrite_requirement(&spec.text, &to, UpperBoundPolicy::Widen) {
            Ok(RewriteOutcome::Rewritten(text)) => {
                state.replacements.push(Replacement {
                    start: spec.raw.start,
                    end: spec.raw.end,
                    text: requoted(text, spec.quote),
                });
                state.applied += 1;
            }
            Ok(RewriteOutcome::Unchanged { .. }) => {
                state.skipped += 1;
            }
            Err(error) => {
                return decline(format!(
                    "specifier {:?} on {:?} (importer {:?}) cannot be advanced to {:?}: \
                     {error}; refresh natively",
                    spec.text, row.dep, row.importer, change.to
                ));
            }
        }
    } else {
        // Protocol specifiers mirror package.json, which the plan does
        // not rewrite for protocols (M3.3's skip-and-count).
        state.skipped += 1;
    }
    // The version row.
    let Some(vers) = &row.version else {
        return decline(format!(
            "importer {:?} has a dependency row for {:?} without a version row; refresh \
             natively instead of guessing",
            row.importer, row.dep
        ));
    };
    if vers.text.starts_with("link:") {
        // A workspace link without an embedded version: nothing to move.
        state.skipped += 1;
    } else if vers.text == change.from {
        return decline(format!(
            "importer {:?} resolves member {:?} from the registry at {:?} (bare version row); \
             the packages:/snapshots: resolution must move via the native tool",
            row.importer, row.dep, change.from
        ));
    } else if let Some(tail) = vers.text.strip_prefix(&change.from) {
        // `<from>(link:<path>)`: the resolved member version rides the
        // link ref — rewrite the version token only.
        if !tail.starts_with("(link:") {
            return decline(format!(
                "version row {:?} for {:?} (importer {:?}) is not a link ref; refresh natively",
                vers.text, row.dep, row.importer
            ));
        }
        state.replacements.push(Replacement {
            start: vers.inner.start,
            end: vers.inner.start + change.from.len(),
            text: change.to.clone(),
        });
        state.applied += 1;
    } else if vers.text == change.to || vers.text.strip_prefix(&change.to).is_some() {
        // Already at the target (idempotent re-apply).
        state.skipped += 1;
    } else {
        return decline(format!(
            "version row {:?} for member {:?} (importer {:?}) is neither at the planned from \
             {:?} nor the target {:?} — drift; refresh natively",
            vers.text, row.dep, row.importer, change.from, change.to
        ));
    }
    Ok(())
}

/// True when a resolution key under `packages:`/`snapshots:` names the
/// member: `/name@version`, `host/name@version`, `name@version(peer…)`,
/// with `@scope/…` names matched with their scope prefix.
fn is_member_key(key: &str, name: &str) -> bool {
    let key = key.strip_prefix('/').unwrap_or(key);
    if name.starts_with('@') {
        key.starts_with(&format!("{name}@")) || key.contains(&format!("/{name}@"))
    } else {
        let last = key.rsplit('/').next().unwrap_or(key);
        last.starts_with(&format!("{name}@"))
    }
}

/// Re-quotes `text` with the literal's own quote character when the
/// scanned scalar was quoted (the replacement must carry the same
/// delimiters as the raw scalar it replaces).
fn requoted(text: String, quote: Option<char>) -> String {
    match quote {
        Some(quote) => format!("{quote}{text}{quote}"),
        None => text,
    }
}

/// Applies the replacements right-to-left so earlier spans stay valid.
/// Everything outside the replaced ranges is copied verbatim (I4).
fn apply_replacements(
    raw: &str,
    file: &str,
    replacements: &[Replacement],
) -> Result<String, LockfileFailure> {
    let mut ordered: Vec<&Replacement> = replacements.iter().collect();
    ordered.sort_by(|a, b| b.start.cmp(&a.start).then(b.end.cmp(&a.end)));
    let mut out = raw.as_bytes().to_vec();
    for replacement in ordered {
        let (start, end) = (replacement.start, replacement.end);
        if start > end || end > out.len() {
            return Err(LockfileFailure::Parse {
                file: file.to_owned(),
                message: format!("replacement span {start}..{end} does not fit the lockfile"),
            });
        }
        let head = out.get(..start).unwrap_or(&[]).to_vec();
        let tail = out.get(end..).unwrap_or(&[]).to_vec();
        out = head;
        out.extend_from_slice(replacement.text.as_bytes());
        out.extend_from_slice(&tail);
    }
    String::from_utf8(out).map_err(|_| LockfileFailure::Parse {
        file: file.to_owned(),
        message: "rewritten lockfile is not valid UTF-8".to_owned(),
    })
}

/// The raw text of a scalar value with its span metadata (relative to the
/// line's content start).
struct ScalarText {
    /// Quotes-stripped value.
    text: String,
    /// Byte offset of the raw scalar (quotes included) relative to the
    /// line's content start.
    offset: usize,
    /// Raw scalar length (quotes included).
    len: usize,
    /// The quote character, when quoted.
    quote: Option<char>,
}

impl ScalarText {
    /// Builds the absolute [`Scalar`] given the line's content-start byte
    /// offset.
    fn absolute(&self, content_start: usize) -> Scalar {
        let raw_start = content_start + self.offset;
        let inner_start = raw_start + usize::from(self.quote.is_some());
        let inner_len = self.len - usize::from(self.quote.is_some()) * 2;
        Scalar {
            text: self.text.clone(),
            raw: raw_start..raw_start + self.len,
            inner: inner_start..inner_start + inner_len,
            quote: self.quote,
        }
    }
}

/// Splits `key: value` content into the key text and the scalar (when
/// present). `None` when the line has no key colon at all.
fn split_key_value(content: &str) -> Option<(&str, Option<ScalarText>)> {
    let idx = content.find(':')?;
    let key = &content[..idx];
    if key.is_empty() || key.contains(' ') || key.contains('\t') {
        return None;
    }
    let rest = &content[idx + 1..];
    // Strip a trailing `#` comment (only outside quotes, preceded by
    // whitespace) and measure the scalar.
    let bytes = rest.as_bytes();
    let mut in_single = false;
    let mut in_double = false;
    let mut end = rest.len();
    for (i, b) in bytes.iter().enumerate() {
        match *b {
            b'\'' if !in_double => in_single = !in_single,
            b'"' if !in_single => in_double = !in_double,
            b'#' if !in_single && !in_double && i > 0 && bytes.get(i - 1) == Some(&b' ') => {
                end = i;
                break;
            }
            _ => {}
        }
    }
    let value = rest.get(..end).unwrap_or(rest).trim_end();
    if value.is_empty() {
        return Some((key, None));
    }
    let leading = value.len() - value.trim_start().len();
    let value = value.trim_start();
    let offset = idx + 1 + leading;
    let quote = match value.as_bytes().first() {
        Some(b'\'' | b'"')
            if value.len() >= 2 && value.ends_with(value.get(..1).unwrap_or_default()) =>
        {
            value.chars().next()
        }
        _ => None,
    };
    let text = match quote {
        Some(_) => value.get(1..value.len() - 1)?.to_owned(),
        None => value.to_owned(),
    };
    Some((
        key,
        Some(ScalarText {
            text,
            offset,
            len: value.len(),
            quote,
        }),
    ))
}

/// Unquotes a scalar (`'single'` / `"double"`), pass-through otherwise.
fn unquote(scalar: &str) -> String {
    let trimmed = scalar.trim();
    if trimmed.len() >= 2
        && ((trimmed.starts_with('\'') && trimmed.ends_with('\''))
            || (trimmed.starts_with('"') && trimmed.ends_with('"')))
    {
        return trimmed[1..trimmed.len() - 1].to_owned();
    }
    trimmed.to_owned()
}

/// Mutable state of the `importers:` scan.
struct ImportersScan {
    /// The importer headers' indent (fixed for the block).
    base: Option<usize>,
    /// The current importer path.
    importer: Option<String>,
    /// The container stack (indent, key) beneath the base indent.
    containers: Vec<(usize, String)>,
    /// Row index by (importer, dependency) for the document-order merge.
    rows: BTreeMap<(String, String), usize>,
    /// The merged rows in document order.
    importers: Vec<DepRow>,
}

impl ImportersScan {
    /// Scans one line inside the `importers:` block: importer headers at
    /// the block's base indent, dependency containers nested beneath, and
    /// the `specifier:`/`version:` leaves recorded onto their dependency
    /// row.
    fn line(
        &mut self,
        content_start: usize,
        indent: usize,
        key_text: &str,
        value: Option<&ScalarText>,
    ) -> Result<(), String> {
        let base = *self.base.get_or_insert(indent);
        while self
            .containers
            .last()
            .is_some_and(|(c_indent, _)| *c_indent >= indent)
        {
            self.containers.pop();
        }
        let Some(value) = value else {
            // A container header: an importer at the base indent, or a
            // dependency table/name nested beneath.
            if indent == base {
                self.importer = Some(unquote(key_text));
            }
            self.containers.push((indent, unquote(key_text)));
            return Ok(());
        };
        let key = unquote(key_text);
        if key != "specifier" && key != "version" {
            // Other leaf keys carry no version semantics for the patch.
            return Ok(());
        }
        if indent == base {
            return Err(format!(
                "unexpected {key:?} row at importer level (expected under a dependency)"
            ));
        }
        let Some(&(dep_indent, ref dep)) = self.containers.last() else {
            return Err(format!(
                "unexpected {key:?} row with no enclosing dependency container"
            ));
        };
        // The leaf must sit directly beneath a dependency-name container
        // (which itself sits beneath a dependency table), never beneath
        // the table key itself.
        if dep_indent <= base || DEP_TABLES.contains(&dep.as_str()) {
            return Err(format!(
                "unexpected {key:?} row shape under importer {:?}",
                self.importer.as_deref().unwrap_or_default()
            ));
        }
        let importer_name = self
            .importer
            .clone()
            .ok_or_else(|| "a dependency row appeared before any importer header".to_owned())?;
        let row_key = (importer_name.clone(), dep.clone());
        let index = if let Some(index) = self.rows.get(&row_key) {
            *index
        } else {
            self.importers.push(DepRow {
                importer: importer_name,
                dep: dep.clone(),
                specifier: None,
                version: None,
            });
            let index = self.importers.len() - 1;
            self.rows.insert(row_key, index);
            index
        };
        let Some(row) = self.importers.get_mut(index) else {
            return Err("row index out of range".to_owned());
        };
        let scalar = value.absolute(content_start);
        let slot = if key == "specifier" {
            &mut row.specifier
        } else {
            &mut row.version
        };
        if slot.is_some() {
            return Err(format!("duplicate {key:?} row for {:?}", row.dep));
        }
        *slot = Some(scalar);
        Ok(())
    }
}

/// The dependency-table keys a leaf must not be attributed to (the leaf's
/// direct container must be the dependency name, not the table).
const DEP_TABLES: [&str; 4] = [
    "dependencies",
    "devDependencies",
    "optionalDependencies",
    "peerDependencies",
];

/// Scans the restricted pnpm-lock grammar. `Err(reason)` is a structural
/// surprise the pass declines on (never a panic, never a guess).
fn scan(raw: &str) -> Result<Scan, String> {
    let mut header: Option<Scalar> = None;
    let mut foreign_keys: Vec<(String, String)> = Vec::new();
    let mut importers_scan = ImportersScan {
        base: None,
        importer: None,
        containers: Vec::new(),
        rows: BTreeMap::new(),
        importers: Vec::new(),
    };
    // Section state.
    let mut section: Option<String> = None;
    let mut section_child_indent: Option<usize> = None;

    let mut offset = 0usize;
    for line_text in raw.split('\n') {
        let line_start = offset;
        offset += line_text.len() + 1;
        let line = line_text.strip_suffix('\r').unwrap_or(line_text);
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        if line.trim_start().starts_with('\t') {
            return Err("tab used for indentation (YAML forbids tabs)".to_owned());
        }
        if indent == 0 {
            // Top level: reset section state.
            section_child_indent = None;
            importers_scan.base = None;
            importers_scan.importer = None;
            importers_scan.containers.clear();
            let Some((key_text, value)) = split_key_value(line) else {
                return Err(format!("unexpected top-level line {line:?}"));
            };
            match value {
                None => {
                    // A section opener.
                    section = Some(unquote(key_text));
                    section_child_indent = None;
                }
                Some(scalar) => {
                    if key_text == "lockfileVersion" {
                        header = Some(scalar.absolute(line_start));
                    }
                    // A top-level scalar ends any section.
                    section = None;
                }
            }
            continue;
        }
        let content = &line[indent..];
        let Some((key_text, value)) = split_key_value(content) else {
            return Err(format!("unexpected line {content:?} (not `key: value`)"));
        };
        match section.as_deref() {
            Some("importers") => {
                importers_scan.line(line_start + indent, indent, key_text, value.as_ref())?;
            }
            Some(section_name) => {
                // Resolution-key sections: collect container keys at the
                // section's first child indent only.
                let child_indent = *section_child_indent.get_or_insert(indent);
                if indent == child_indent && value.is_none() {
                    foreign_keys.push((section_name.to_owned(), unquote(key_text)));
                }
            }
            None => {
                return Err(format!(
                    "unexpectedly indented line outside any section: {content:?}"
                ));
            }
        }
    }
    Ok(Scan {
        header,
        importers: importers_scan.importers,
        foreign_keys,
    })
}

/// Post-refresh verification of the §9.3 native fallback (M3.4): the
/// native tool may only change **importer rows of workspace members**
/// (their `specifier`/`version` rows). Anything else — a resolution key
/// appearing, moving or disappearing under `packages:`/`snapshots:`, a
/// changed header, a new or dropped importer — is a resolution change and
/// fails closed (`E-LOCK-NATIVE`): re-plan instead of trusting a shifted
/// tree.
///
/// Returns the sorted names of the members whose importer rows moved.
///
/// # Errors
///
/// [`halyard_core::lockfile::NativeRefreshError::Verification`] when the
/// refresh moved rows beyond the members' importer rows or produced an
/// unrecognizable lockfile.
pub fn verify_native_refresh(
    before: &[u8],
    after: &[u8],
    members: &[String],
) -> Result<Vec<String>, halyard_core::lockfile::NativeRefreshError> {
    use halyard_core::lockfile::NativeRefreshError;
    let verification = |reason: String| NativeRefreshError::Verification { reason };
    let before = parse_snapshot(before).map_err(verification)?;
    let after = parse_snapshot(after).map_err(verification)?;
    let member = |dep: &str| members.iter().any(|m| m == dep);
    let mut moved: BTreeSet<String> = BTreeSet::new();

    // The header is cargo's business... pnpm's: any change is a format
    // change the plan cannot express.
    if before.header != after.header {
        return Err(verification(
            "the lockfileVersion header changed".to_owned(),
        ));
    }
    // Resolution keys under packages:/snapshots:/… must be identical: a
    // key appearing, disappearing or changing count is a resolution
    // change (workspace links never appear as keys).
    if before.keys != after.keys {
        return Err(verification(
            "resolution keys under packages:/snapshots: changed (a dependency resolution \
             change)"
                .to_owned(),
        ));
    }
    // Importer rows: member rows may move, everything else must be
    // byte-identical in content.
    for (row_key, before_row) in &before.rows {
        let (importer, dep) = row_key;
        let Some(after_row) = after.rows.get(row_key) else {
            if member(dep) {
                moved.insert(dep.clone());
                continue;
            }
            return Err(verification(format!(
                "dependency row for {dep:?} in importer {importer:?} disappeared (a \
                 resolution change)"
            )));
        };
        if before_row != after_row {
            if member(dep) {
                moved.insert(dep.clone());
            } else {
                return Err(verification(format!(
                    "dependency row for {dep:?} in importer {importer:?} changed but \
                     {dep:?} is not a workspace member"
                )));
            }
        }
    }
    for row_key in after.rows.keys() {
        if !before.rows.contains_key(row_key) {
            let (importer, dep) = row_key;
            if member(dep) {
                moved.insert(dep.clone());
                continue;
            }
            return Err(verification(format!(
                "dependency row for {dep:?} in importer {importer:?} appeared (a resolution \
                 change)"
            )));
        }
    }
    Ok(moved.into_iter().collect())
}

/// The before/after snapshot of one pnpm-lock state (owned: the strings
/// outlive the scan).
struct LockSnapshot {
    /// The `lockfileVersion` scalar as written (empty when missing).
    header: String,
    /// `(importer, dep) → (specifier, version)` rows.
    rows: BTreeMap<(String, String), (String, String)>,
    /// Resolution keys per section (`packages`, `snapshots`, …).
    keys: BTreeMap<(String, String), usize>,
}

/// Parses one pnpm-lock state for the before/after comparison.
fn parse_snapshot(bytes: &[u8]) -> Result<LockSnapshot, String> {
    let text =
        std::str::from_utf8(bytes).map_err(|_| "the lockfile is not valid UTF-8".to_owned())?;
    let scan = scan(text)?;
    let mut rows = BTreeMap::new();
    for row in &scan.importers {
        rows.insert(
            (row.importer.clone(), row.dep.clone()),
            (
                row.specifier
                    .as_ref()
                    .map(|s| s.text.clone())
                    .unwrap_or_default(),
                row.version
                    .as_ref()
                    .map(|s| s.text.clone())
                    .unwrap_or_default(),
            ),
        );
    }
    let mut keys: BTreeMap<(String, String), usize> = BTreeMap::new();
    for (section, key) in &scan.foreign_keys {
        *keys.entry((section.clone(), key.clone())).or_default() += 1;
    }
    Ok(LockSnapshot {
        header: scan
            .header
            .as_ref()
            .map(|s| s.text.clone())
            .unwrap_or_default(),
        rows,
        keys,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A representative v9 lock: the root importer links `ui` through the
    /// `workspace:*` protocol (no version embedded); `app` depends on
    /// `ui` through a plain semver specifier resolved as a member link
    /// with the resolved version riding the ref; one foreign registry
    /// package under `packages:`/`snapshots:`.
    const LOCK_V9: &str = r"lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      '@scope/ui':
        specifier: workspace:*
        version: link:packages/ui

  packages/app:
    dependencies:
      '@scope/ui':
        specifier: ^0.4.1
        version: 0.4.1(link:../ui)
      lodash:
        specifier: ^4.17.0
        version: 4.17.21

  packages/ui:
    devDependencies:

packages:

  /lodash@4.17.21:
    resolution: {integrity: sha512-aaaa}

snapshots:

  /lodash@4.17.21: {}
";

    fn change(name: &str, from: &str, to: &str) -> LockfileVersionChange {
        LockfileVersionChange {
            name: name.to_owned(),
            from: from.to_owned(),
            to: to.to_owned(),
        }
    }

    fn patch(raw: &str, changes: &[LockfileVersionChange]) -> LockfileOutcome {
        patch_pnpm_lockfile(raw, PNPM_LOCKFILE, changes)
            .unwrap_or_else(|f| unreachable!("fixture patch failed: {f}"))
    }

    fn patched_text(out: &LockfileOutcome) -> String {
        match out {
            LockfileOutcome::Patched { content, .. } => {
                String::from_utf8(content.clone()).unwrap_or_else(|e| unreachable!("utf8: {e}"))
            }
            other @ LockfileOutcome::NeedsNative { .. } => {
                unreachable!("expected a patched outcome, got {other:?}")
            }
        }
    }

    fn decline_reason(out: &LockfileOutcome) -> String {
        match out {
            LockfileOutcome::NeedsNative { reason } => reason.clone(),
            other @ LockfileOutcome::Patched { .. } => {
                unreachable!("expected a decline, got {other:?}")
            }
        }
    }

    #[test]
    fn plain_semver_specifier_and_link_version_follow_the_member() {
        let out = patch(LOCK_V9, &[change("@scope/ui", "0.4.1", "0.5.0")]);
        let text = patched_text(&out);
        // The specifier advanced with the canonical rewriter's shape.
        assert!(
            text.contains("specifier: ^0.5.0"),
            "specifier rewritten: {text}"
        );
        // The resolved version riding the link ref follows the move.
        assert!(
            text.contains("version: 0.5.0(link:../ui)"),
            "link ref version rewritten: {text}"
        );
        // The workspace:* importer rows keep their bytes (link, no
        // version) — skipped, not rewritten.
        assert!(text.contains("specifier: workspace:*"));
        assert!(text.contains("version: link:packages/ui"));
        // The foreign registry row keeps its bytes.
        assert!(text.contains("/lodash@4.17.21:"));
        let LockfileOutcome::Patched {
            applied, skipped, ..
        } = out
        else {
            unreachable!("patched")
        };
        assert_eq!(applied, 2, "specifier + link-ref version");
        assert_eq!(skipped, 2, "root importer's two protocol rows");
    }

    #[test]
    fn unedited_bytes_survive_verbatim_outside_replaced_scalars() {
        let out = patch(LOCK_V9, &[change("@scope/ui", "0.4.1", "0.5.0")]);
        let text = patched_text(&out);
        // Mask the two rewritten scalars on both sides: everything else
        // must compare byte-for-byte (I4).
        let before = LOCK_V9
            .replace("specifier: ^0.4.1", "@SPEC@")
            .replace("version: 0.4.1(link:../ui)", "@REF@");
        let after = text
            .replace("specifier: ^0.5.0", "@SPEC@")
            .replace("version: 0.5.0(link:../ui)", "@REF@");
        assert_eq!(before, after);
    }

    #[test]
    fn workspace_protocol_rows_are_a_byte_identical_no_op() {
        // Nobody depends on a member through a rewritable specifier: the
        // pass reports skips and returns the input bytes.
        let only_links = "lockfileVersion: '9.0'\n\nimporters:\n\n  .:\n    dependencies:\n      ui:\n        specifier: workspace:^\n        version: link:packages/ui\n";
        let out = patch(only_links, &[change("ui", "0.4.1", "0.5.0")]);
        let LockfileOutcome::Patched {
            content,
            applied,
            skipped,
        } = out
        else {
            unreachable!("protocol-only pass must patch, got {out:?}")
        };
        assert_eq!(applied, 0);
        assert_eq!(skipped, 2);
        assert_eq!(content, only_links.as_bytes(), "byte-identical no-op");
    }

    #[test]
    fn already_applied_changes_are_skipped_idempotent_reapply() {
        let first = patch(LOCK_V9, &[change("@scope/ui", "0.4.1", "0.5.0")]);
        let once = patched_text(&first);
        let second = patch(&once, &[change("@scope/ui", "0.4.1", "0.5.0")]);
        let LockfileOutcome::Patched {
            content,
            applied,
            skipped,
        } = second
        else {
            unreachable!("second pass must patch, got {second:?}")
        };
        assert_eq!(applied, 0, "rows already at target are skipped");
        assert_eq!(skipped, 4, "all rows now sit at the target");
        assert_eq!(
            String::from_utf8(content).unwrap_or_else(|e| unreachable!("utf8: {e}")),
            once,
            "re-apply is byte-identical (idempotence)"
        );
    }

    #[test]
    fn member_resolution_key_in_packages_declines() {
        // A registry-resolved member (packages:/snapshots: keys) cannot
        // be patched byte-safely: the key itself must move.
        let with_key = LOCK_V9.replace(
            "packages:\n\n  /lodash@4.17.21:",
            "packages:\n\n  /@scope/ui@0.4.1:\n    resolution: {directory: packages/ui, type: directory}\n\n  /lodash@4.17.21:",
        );
        let out = patch(&with_key, &[change("@scope/ui", "0.4.1", "0.5.0")]);
        let reason = decline_reason(&out);
        assert!(reason.contains("@scope/ui"), "names the member: {reason}");
        assert!(reason.contains("resolution"), "explains: {reason}");
    }

    #[test]
    fn bare_registry_version_row_declines() {
        let registry_resolved = LOCK_V9.replace("version: 0.4.1(link:../ui)", "version: 0.4.1");
        let out = patch(&registry_resolved, &[change("@scope/ui", "0.4.1", "0.5.0")]);
        let reason = decline_reason(&out);
        assert!(
            reason.contains("registry"),
            "explains the registry resolution: {reason}"
        );
    }

    #[test]
    fn unrewritable_specifier_declines() {
        // `latest` is not a semver requirement: pnpm's resolution for it
        // is not a byte-safe rewrite.
        let latest = LOCK_V9.replace("specifier: ^0.4.1", "specifier: latest");
        let out = patch(&latest, &[change("@scope/ui", "0.4.1", "0.5.0")]);
        let reason = decline_reason(&out);
        assert!(reason.contains("latest"), "names the specifier: {reason}");
    }

    #[test]
    fn closed_upper_bound_specifier_declines() {
        let pinned = LOCK_V9.replace("specifier: ^0.4.1", "specifier: <=0.4.1");
        let out = patch(&pinned, &[change("@scope/ui", "0.4.1", "0.5.0")]);
        assert!(
            matches!(out, LockfileOutcome::NeedsNative { .. }),
            "closed upper bounds are E-EDIT-UNREWRITABLE: {out:?}"
        );
    }

    #[test]
    fn drifted_version_row_declines() {
        // Link-ref version at neither `from` nor `to`.
        let drifted = LOCK_V9.replace("version: 0.4.1(link:../ui)", "version: 0.3.0(link:../ui)");
        let out = patch(&drifted, &[change("@scope/ui", "0.4.1", "0.5.0")]);
        assert!(
            matches!(out, LockfileOutcome::NeedsNative { .. }),
            "drifted link ref must decline, got {out:?}"
        );
    }

    #[test]
    fn unknown_or_missing_header_declines() {
        // An unsupported but parseable header names the problem.
        let lock = LOCK_V9.replace("lockfileVersion: '9.0'", "lockfileVersion: '5.4'");
        let out = patch(&lock, &[change("@scope/ui", "0.4.1", "0.5.0")]);
        let reason = decline_reason(&out);
        assert!(
            reason.contains("lockfileVersion"),
            "reason explains the header: {reason}"
        );
        // A missing header declines; a top-level line that is not
        // `key: value` is a scan-level structural surprise (also a
        // decline — the reason names the line).
        let no_header = LOCK_V9.replacen("lockfileVersion: '9.0'\n", "", 1);
        assert!(matches!(
            patch(&no_header, &[change("@scope/ui", "0.4.1", "0.5.0")]),
            LockfileOutcome::NeedsNative { .. }
        ));
        let garbage = LOCK_V9.replace("lockfileVersion: '9.0'", "banana");
        assert!(matches!(
            patch(&garbage, &[change("@scope/ui", "0.4.1", "0.5.0")]),
            LockfileOutcome::NeedsNative { .. }
        ));
    }

    #[test]
    fn multi_document_lockfile_declines() {
        let multi = format!("{LOCK_V9}\n---\nlockfileVersion: '9.0'\n");
        let out = patch(&multi, &[change("@scope/ui", "0.4.1", "0.5.0")]);
        let reason = decline_reason(&out);
        assert!(reason.contains("document"), "reason explains: {reason}");
    }

    #[test]
    fn no_rows_for_a_changed_member_is_a_no_op() {
        // A member nobody depends on contributes no importer rows —
        // unlike Cargo.lock (§9.2 rule 1) there is no mandatory row.
        let lonely = patch(LOCK_V9, &[change("ghost", "1.0.0", "1.1.0")]);
        let LockfileOutcome::Patched {
            content,
            applied,
            skipped,
        } = lonely
        else {
            unreachable!("member without rows must be a no-op, got {lonely:?}")
        };
        assert_eq!(applied, 0);
        assert_eq!(skipped, 0);
        assert_eq!(content, LOCK_V9.as_bytes(), "byte-identical no-op");
    }

    #[test]
    fn empty_changes_patch_nothing() {
        let out = patch(LOCK_V9, &[]);
        let LockfileOutcome::Patched { content, .. } = out else {
            unreachable!("empty changes must be a no-op patch, got {out:?}")
        };
        assert_eq!(content, LOCK_V9.as_bytes());
    }

    #[test]
    fn crlf_and_quoting_are_preserved() {
        let crlf = LOCK_V9
            .replace('\n', "\r\n")
            .replace("specifier: ^0.4.1", "specifier: \"^0.4.1\"");
        let out = patch(&crlf, &[change("@scope/ui", "0.4.1", "0.5.0")]);
        let text = patched_text(&out);
        assert!(
            text.contains("specifier: \"^0.5.0\""),
            "re-quoted with the literal's own quote char: {text}"
        );
        assert_eq!(
            text.matches("\r\n").count(),
            LOCK_V9.matches('\n').count(),
            "CRLF count preserved"
        );
    }

    #[test]
    fn scoped_and_plain_names_resolve_identically() {
        // The same move applied through an unquoted dep name.
        let plain = LOCK_V9.replace("'@scope/ui':", "ui:");
        let out = patch(&plain, &[change("ui", "0.4.1", "0.5.0")]);
        let text = patched_text(&out);
        assert!(text.contains("specifier: ^0.5.0"), "{text}");
    }

    /// House-style failure extraction (no `unwrap_err` chains; impossible
    /// paths are `unreachable!`).
    fn expect_failure(result: Result<LockfileOutcome, LockfileFailure>) -> LockfileFailure {
        match result {
            Err(failure) => failure,
            Ok(outcome) => unreachable!("expected a hard failure, got {outcome:?}"),
        }
    }

    #[test]
    fn non_utf8_planned_change_is_e_lock_parse() {
        let err = expect_failure(patch_pnpm_lockfile(
            LOCK_V9,
            PNPM_LOCKFILE,
            &[change("ui", "0.4.1", "not-semver")],
        ));
        assert_eq!(err.code(), "E-LOCK-PARSE");
        let err = expect_failure(patch_pnpm_lockfile(
            LOCK_V9,
            PNPM_LOCKFILE,
            &[change("ui", "0.4.1", "0.4.1")],
        ));
        assert_eq!(err.code(), "E-LOCK-PARSE", "from == to is not a move");
        let err = expect_failure(patch_pnpm_lockfile(
            LOCK_V9,
            PNPM_LOCKFILE,
            &[change("", "0.4.1", "0.5.0")],
        ));
        assert_eq!(err.code(), "E-LOCK-PARSE", "empty name is a plan bug");
    }

    #[test]
    fn native_refresh_verification_accepts_member_only_moves() {
        let member_only = LOCK_V9
            .replace("specifier: ^0.4.1", "specifier: ^0.5.0")
            .replace("version: 0.4.1(link:../ui)", "version: 0.5.0(link:../ui)");
        let moved = verify_native_refresh(
            LOCK_V9.as_bytes(),
            member_only.as_bytes(),
            &["@scope/ui".to_owned()],
        )
        .unwrap_or_else(|e| unreachable!("member-only moves verify: {e}"));
        assert_eq!(moved, vec!["@scope/ui".to_owned()]);
    }

    #[test]
    fn native_refresh_verification_rejects_foreign_changes() {
        // A registry row moving is a resolution change.
        let foreign = LOCK_V9.replace("/lodash@4.17.21:", "/lodash@4.17.22:");
        let err = verify_native_refresh(
            LOCK_V9.as_bytes(),
            foreign.as_bytes(),
            &["@scope/ui".to_owned()],
        )
        .unwrap_err();
        assert_eq!(err.code(), "E-LOCK-NATIVE");
        // The header moving is a resolution change.
        let header = LOCK_V9.replace("lockfileVersion: '9.0'", "lockfileVersion: '10.0'");
        assert!(
            verify_native_refresh(
                LOCK_V9.as_bytes(),
                header.as_bytes(),
                &["@scope/ui".to_owned()],
            )
            .is_err()
        );
        // A non-member importer row changing is a resolution change.
        let foreign_row = LOCK_V9.replace("specifier: ^4.17.0", "specifier: ^4.18.0");
        assert!(
            verify_native_refresh(
                LOCK_V9.as_bytes(),
                foreign_row.as_bytes(),
                &["@scope/ui".to_owned()],
            )
            .is_err()
        );
        // A member's importer row disappearing is attributed to the
        // member (the cargo verifier's convention), not an error.
        let dropped = LOCK_V9.replace(
            "      '@scope/ui':\n        specifier: ^0.4.1\n        version: 0.4.1(link:../ui)\n",
            "",
        );
        let moved = verify_native_refresh(
            LOCK_V9.as_bytes(),
            dropped.as_bytes(),
            &["@scope/ui".to_owned()],
        )
        .unwrap_or_else(|e| unreachable!("member row drop verifies: {e}"));
        assert_eq!(moved, vec!["@scope/ui".to_owned()]);
    }
}

//! Patch application in memory (§9.1): validated, sorted, non-overlapping
//! byte-range replacements applied over ascending ranges.

use std::collections::BTreeMap;

use halyard_core::RelPath;
use halyard_core::plan::PlanEdit;

use crate::error::EditError;

/// A single byte-range replacement (§9.1). `start..end` is over the file
/// as it exists at plan time; `start == end` inserts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Patch {
    start: u32,
    end: u32,
    text: String,
}

impl Patch {
    /// Constructs and validates a patch (`start <= end`).
    pub fn new(start: u32, end: u32, text: String) -> Result<Self, EditError> {
        if start > end {
            return Err(EditError::InvalidPatches {
                reason: format!("inverted range {start}..{end}"),
            });
        }
        Ok(Self { start, end, text })
    }

    /// Inclusive start byte offset.
    pub fn start(&self) -> u32 {
        self.start
    }

    /// Exclusive end byte offset.
    pub fn end(&self) -> u32 {
        self.end
    }

    /// Replacement text (UTF-8).
    pub fn text(&self) -> &str {
        &self.text
    }
}

/// Line-ending shape of a file, for patch authors that must match the
/// host style when synthesizing new lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineEndings {
    /// Only `\n`.
    Lf,
    /// Only `\r\n`.
    Crlf,
    /// Both (nothing here is inferred — a mixed file forces explicitness).
    Mixed,
}

/// Detects the line-ending shape (§9.1: CRLF detected per file). An empty
/// file reports [`LineEndings::Lf`] (no EOLs to preserve; synthesized
/// lines use the default style).
pub fn detect_line_endings(content: &[u8]) -> LineEndings {
    let mut crlf = 0usize;
    let mut lf_only = 0usize;
    let mut i = 0usize;
    while i < content.len() {
        match content.get(i) {
            Some(b'\r') if content.get(i + 1) == Some(&b'\n') => {
                crlf += 1;
                i += 2;
            }
            Some(b'\n') => {
                lf_only += 1;
                i += 1;
            }
            _ => i += 1,
        }
    }
    match (crlf > 0, lf_only > 0 || content.is_empty()) {
        (true, false) => LineEndings::Crlf,
        (false, true) => LineEndings::Lf,
        _ => LineEndings::Mixed,
    }
}

/// Applies `patches` (ascending, non-overlapping, in-bounds) to `content`.
///
/// Untouched bytes are copied verbatim — comments, key order, whitespace,
/// CRLF and BOM survive automatically (invariant I4).
pub fn apply(content: &[u8], patches: &[Patch]) -> Result<Vec<u8>, EditError> {
    // Validate ordering and bounds before touching anything.
    let mut last_end: Option<u32> = None;
    for patch in patches {
        if patch.start > patch.end {
            return Err(EditError::InvalidPatches {
                reason: format!("inverted range {}..{}", patch.start, patch.end),
            });
        }
        if last_end.is_some_and(|prev| patch.start < prev) {
            return Err(EditError::InvalidPatches {
                reason: format!(
                    "patch at {} overlaps or precedes the previous patch (end {})",
                    patch.start,
                    last_end.unwrap_or(0)
                ),
            });
        }
        if patch.end > u32::try_from(content.len()).unwrap_or(u32::MAX) {
            return Err(EditError::InvalidPatches {
                reason: format!(
                    "patch {}..{} is outside the file ({} bytes)",
                    patch.start,
                    patch.end,
                    content.len()
                ),
            });
        }
        last_end = Some(patch.end);
    }

    let mut out = Vec::with_capacity(content.len());
    let mut pos = 0usize;
    for patch in patches {
        let (start, end) = (patch.start as usize, patch.end as usize);
        out.extend_from_slice(content.get(pos..start).unwrap_or(&[]));
        out.extend_from_slice(patch.text.as_bytes());
        pos = end;
    }
    out.extend_from_slice(content.get(pos..).unwrap_or(&[]));
    Ok(out)
}

/// Byte patches for one file — patches are kept ascending and
/// non-overlapping (validated at [`EditSet::add`]).
///
/// The `BTreeMap` keeps file iteration sorted and deterministic
/// (invariant I3).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EditSet {
    files: BTreeMap<RelPath, Vec<Patch>>,
}

impl EditSet {
    /// Empty edit set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds one patch for `file`, validating sort order and non-overlap
    /// within the file's patch list.
    pub fn add(&mut self, file: RelPath, patch: Patch) -> Result<(), EditError> {
        let list = self.files.entry(file).or_default();
        if let Some(prev) = list.last() {
            if patch.start < prev.end {
                return Err(EditError::InvalidPatches {
                    reason: format!(
                        "patch at {} overlaps the previous patch (ends at {})",
                        patch.start, prev.end
                    ),
                });
            }
            if patch.start == prev.start && patch.end == prev.end {
                return Err(EditError::InvalidPatches {
                    reason: format!("duplicate patch at {}..{}", patch.start, patch.end),
                });
            }
        }
        list.push(patch);
        Ok(())
    }

    /// True when no file has patches.
    pub fn is_empty(&self) -> bool {
        self.files.values().all(Vec::is_empty)
    }

    /// Files carrying patches, sorted.
    pub fn files(&self) -> impl Iterator<Item = &RelPath> {
        self.files.keys()
    }

    /// Patches for one file (ascending).
    pub fn patches(&self, file: &RelPath) -> Option<&[Patch]> {
        self.files.get(file).map(Vec::as_slice)
    }

    /// Applies the patches for one file to in-memory content.
    pub fn apply_content(&self, file: &RelPath, content: &[u8]) -> Result<Vec<u8>, EditError> {
        match self.files.get(file) {
            Some(patches) => apply(content, patches),
            None => Ok(content.to_vec()),
        }
    }

    /// Applies every file through a content lookup (the caller owns the
    /// I/O so this stays testable without a filesystem). Returns the
    /// patched contents keyed by file, in sorted file order.
    pub fn apply_all(
        &self,
        mut read: impl FnMut(&RelPath) -> std::io::Result<Vec<u8>>,
    ) -> Result<BTreeMap<RelPath, Vec<u8>>, EditError> {
        let mut out = BTreeMap::new();
        for (file, patches) in &self.files {
            let content = read(file).map_err(|source| EditError::Io {
                path: file.as_str().to_owned(),
                source,
            })?;
            out.insert(file.clone(), apply(&content, patches)?);
        }
        Ok(out)
    }

    /// Builds an edit set from the plan wire format (`halyard-plan/v1`
    /// `edits`, §5.8): file paths are validated as repo-relative paths
    /// and patches keep their byte ranges.
    pub fn from_plan_edits(edits: &[PlanEdit]) -> Result<Self, EditError> {
        let mut set = Self::new();
        for edit in edits {
            let file = RelPath::new(&edit.file).ok_or(EditError::InvalidPatches {
                reason: format!(
                    "plan edit path {:?} is not a valid repo-relative path",
                    edit.file
                ),
            })?;
            for plan_patch in &edit.patches {
                set.add(
                    file.clone(),
                    Patch::new(plan_patch.start, plan_patch.end, plan_patch.text.clone())?,
                )?;
            }
        }
        Ok(set)
    }
}

/// Runs `verifier` over every patched file's new content; a failure
/// aborts the whole set (§9.1: re-parse + semantic check before writing).
pub fn verify_outputs(
    outputs: &BTreeMap<RelPath, Vec<u8>>,
    verifier: impl Fn(&RelPath, &[u8]) -> Result<(), EditError>,
) -> Result<(), EditError> {
    for (file, content) in outputs {
        verifier(file, content)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_edits_roundtrip_into_edit_set() {
        let plan_edit = PlanEdit {
            file: "crates/core/Cargo.toml".to_owned(),
            patches: vec![halyard_core::plan::PlanPatch {
                start: 21,
                end: 26,
                text: "0.5.0".to_owned(),
            }],
        };
        let set = EditSet::from_plan_edits(&[plan_edit]).unwrap_or_else(|e| panic!("build: {e}"));
        let file = RelPath::new("crates/core/Cargo.toml").unwrap_or_else(|| panic!("path"));
        let content = b"[package]\nversion = \"0.4.1\"\n";
        let out = set
            .apply_content(&file, content)
            .unwrap_or_else(|e| panic!("apply: {e}"));
        assert_eq!(
            String::from_utf8(out).unwrap_or_default(),
            "[package]\nversion = \"0.5.0\"\n"
        );
    }

    #[test]
    fn detection_handles_empty_and_mixed() {
        assert_eq!(detect_line_endings(b"\r\n\r\n"), LineEndings::Crlf);
        assert_eq!(detect_line_endings(b"\n\n"), LineEndings::Lf);
        assert_eq!(detect_line_endings(b""), LineEndings::Lf);
        assert_eq!(detect_line_endings(b"a\r\nb\n"), LineEndings::Mixed);
    }
}

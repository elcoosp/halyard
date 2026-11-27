//! The `RubyGems` adapter's edit half — the semantic in-place version
//! patch (M2 of ADR-0061, the release half).
//!
//! The plan's own-version byte patches re-apply **semantically and in
//! place** (invariant I4): each patch span must land on the gemspec's
//! `version` literal's inner-byte span (the site the discovery row
//! emitted — *"Versions shall be represented by three non-negative
//! integers, separated by periods (e.g. 3.1.4)."*), the literal's inner
//! bytes swap for the plan's target, and the surrounding Ruby — the
//! quote characters, the `.freeze` idiom, comments, every other row —
//! never moves. A re-parse read-back verifies the gemspec now carries
//! the target before the patch is accepted (the §9.1 semantic check);
//! any other outcome fails closed (`E-EDIT-PATCHES` / `E-EDIT-VERIFY`)
//! with the input bytes untouched.
//!
//! A gemspec with no literal `version` row (absent, computed, or
//! conditional — the deterministic rule) has no slot: a patch aimed at
//! its manifest refuses. A patch whose text already equals the gemspec's
//! current value is the idempotent re-run (I1): matched, counted as
//! skipped, bytes verbatim. A patch aimed at a dependency requirement's
//! span refuses — requirement rewrites never touch rubygems rows
//! (foreign-grammar requirements ride verbatim, ADR-0006); the version
//! literal is the only editable site.

use halyard_core::edit::{EditFailure, EditOutcome};
use halyard_core::package::RelPath;
use halyard_core::plan::PlanPatch;

use crate::grammar;

/// The semantic patch: validate every patch span against the gemspec's
/// `version` literal's inner span, swap the inner bytes right-to-left,
/// and verify by re-parse before returning.
pub fn edit_versions(
    file: &RelPath,
    content: &[u8],
    patches: &[PlanPatch],
) -> Result<EditOutcome, EditFailure> {
    // The sibling adapters' posture for a legitimate empty pass (nothing
    // to apply, nothing changed).
    if patches.is_empty() {
        return Ok(EditOutcome {
            content: content.to_vec(),
            applied: 0,
            skipped: 0,
        });
    }
    let file_string = file.to_string();
    let text = std::str::from_utf8(content).map_err(|_| EditFailure::Parse {
        file: file_string.clone(),
        message: ".gemspec is not valid UTF-8".to_owned(),
    })?;
    let gemspec = grammar::parse_gemspec(text).map_err(|error| EditFailure::Parse {
        file: file_string.clone(),
        message: error.to_string(),
    })?;
    // The editable slot: the `version` literal's inner span (one slot —
    // a computed or conditional version row carries none).
    let slot = gemspec.version.as_ref().map(|field| field.span);
    for patch in patches {
        let matches = slot.is_some_and(|span| span.start == patch.start && span.end == patch.end);
        if !matches {
            return Err(EditFailure::SpanMismatch {
                file: file_string.clone(),
                start: patch.start,
                end: patch.end,
            });
        }
    }
    // Validate the spans are in bounds before any byte moves (the
    // manifest drifted check — a patch beyond the file cannot match a
    // slot, but stay defensive about the splice bounds).
    let mut out = content.to_vec();
    let mut applied = 0usize;
    let mut skipped = 0usize;
    let mut ordered: Vec<&PlanPatch> = patches.iter().collect();
    ordered.sort_by_key(|patch| patch.start);
    for patch in ordered.into_iter().rev() {
        let start = patch.start as usize;
        let end = patch.end as usize;
        if end > out.len() || start > end {
            return Err(EditFailure::SpanMismatch {
                file: file_string.clone(),
                start: patch.start,
                end: patch.end,
            });
        }
        let carried = out
            .get(start..end)
            .is_some_and(|current| current == patch.text.as_bytes());
        if carried {
            // The idempotent re-run (I1): the value already carries the
            // target — matched, nothing to rewrite.
            skipped += 1;
            continue;
        }
        out.splice(start..end, patch.text.as_bytes().iter().copied());
        applied += 1;
    }
    // The read-back verify (§9.1): the re-parsed gemspec carries the
    // target the patch aimed at — never a guessed half-application.
    let bumped = std::str::from_utf8(&out).map_err(|_| EditFailure::Parse {
        file: file_string.clone(),
        message: "the patched gemspec is not valid UTF-8".to_owned(),
    })?;
    let reparsed = grammar::parse_gemspec(bumped).map_err(|error| EditFailure::Parse {
        file: file_string.clone(),
        message: format!("read-back re-parse failed: {error}"),
    })?;
    for patch in patches {
        let carries = reparsed
            .version
            .as_ref()
            .is_some_and(|field| field.text == patch.text);
        if !carries {
            return Err(EditFailure::Parse {
                file: file_string.clone(),
                message: format!(
                    "read-back verify: gemspec carries {:?} after the patch, expected {:?} — \
                     nothing was mutated",
                    reparsed.version.as_ref().map(|field| field.text.as_str()),
                    patch.text
                ),
            });
        }
    }
    Ok(EditOutcome {
        content: out,
        applied,
        skipped,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const GEMSPEC: &str = "# frozen\nGem::Specification.new do |spec|\n  spec.name = 'app'\n  spec.version = '1.2.3' # the release row\nend\n";

    #[test]
    fn swaps_the_value_and_preserves_the_markup() {
        let file = RelPath::new("app.gemspec").expect("fixture path");
        // The version literal's inner span: "1.2.3" on the fourth line.
        let start = u32::try_from(GEMSPEC.find("1.2.3").expect("fixture")).expect("fits");
        let patches = [PlanPatch {
            start,
            end: start + 5,
            text: "1.3.0".to_owned(),
        }];
        let out = edit_versions(&file, GEMSPEC.as_bytes(), &patches).expect("applies");
        assert_eq!((out.applied, out.skipped), (1, 0));
        assert_eq!(
            String::from_utf8(out.content).expect("utf-8"),
            GEMSPEC.replace("spec.version = '1.2.3'", "spec.version = '1.3.0'")
        );
    }

    #[test]
    fn a_span_off_the_version_row_refuses() {
        let file = RelPath::new("app.gemspec").expect("fixture path");
        let start = u32::try_from(GEMSPEC.find("'app'").expect("fixture")).expect("fits");
        let patches = [PlanPatch {
            start,
            end: start + 5,
            text: "1.3.0".to_owned(),
        }];
        let err = edit_versions(&file, GEMSPEC.as_bytes(), &patches).expect_err("refuses");
        assert_eq!(err.code(), "E-EDIT-PATCHES");
    }

    #[test]
    fn a_slotless_gemspec_has_no_slot() {
        let plain = "# frozen\nGem::Specification.new do |spec|\n  spec.name = 'plain'\nend\n";
        let file = RelPath::new("plain.gemspec").expect("fixture path");
        let patches = [PlanPatch {
            start: 4,
            end: 9,
            text: "1.3.0".to_owned(),
        }];
        let err = edit_versions(&file, plain.as_bytes(), &patches).expect_err("refuses");
        assert_eq!(err.code(), "E-EDIT-PATCHES");
    }

    #[test]
    fn the_idempotent_rerun_skips_the_matched_value() {
        let file = RelPath::new("app.gemspec").expect("fixture path");
        let start = u32::try_from(GEMSPEC.find("1.2.3").expect("fixture")).expect("fits");
        let patches = [PlanPatch {
            start,
            end: start + 5,
            text: "1.2.3".to_owned(),
        }];
        let out = edit_versions(&file, GEMSPEC.as_bytes(), &patches).expect("applies");
        assert_eq!((out.applied, out.skipped), (0, 1), "already carried");
        assert_eq!(out.content, GEMSPEC.as_bytes(), "the bytes are verbatim");
    }

    #[test]
    fn empty_passes_return_the_verbatim_bytes() {
        let file = RelPath::new("app.gemspec").expect("fixture path");
        let out = edit_versions(&file, GEMSPEC.as_bytes(), &[]).expect("an empty pass");
        assert_eq!(out.content, GEMSPEC.as_bytes());
        assert_eq!((out.applied, out.skipped), (0, 0));
    }

    #[test]
    fn a_dependency_requirement_is_never_the_slot() {
        let with_dep = "# frozen\nGem::Specification.new do |spec|\n  spec.name = 'app'\n  spec.version = '1.2.3'\n  spec.add_dependency 'json', '>= 2.0'\nend\n";
        let file = RelPath::new("app.gemspec").expect("fixture path");
        let start = u32::try_from(with_dep.find(">= 2.0").expect("fixture")).expect("fits");
        let patches = [PlanPatch {
            start,
            end: start + 6,
            text: ">= 3.0".to_owned(),
        }];
        let err = edit_versions(&file, with_dep.as_bytes(), &patches).expect_err("refuses");
        assert_eq!(err.code(), "E-EDIT-PATCHES");
    }
}

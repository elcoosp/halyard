//! The nuget adapter's edit half — the semantic in-place version patch
//! (M2 of ADR-0059, the release half).
//!
//! The plan's own-version byte patches re-apply **semantically and in
//! place** (invariant I4): each patch span must land on the element
//! pack consumes — the `<PackageVersion>` property's value text when
//! present, else the `<Version>` property's (the documented
//! *"Default is the value of `$(Version)`"* resolution) — the value's
//! inner bytes swap for the plan's target, and the surrounding markup —
//! comments, attribute order, indentation, whitespace — never moves. A
//! re-parse read-back verifies the project now carries the target
//! before the patch is accepted (the §9.1 semantic check); any other
//! outcome fails closed (`E-EDIT-PATCHES` / `E-EDIT-VERIFY`) with the
//! input bytes untouched.
//!
//! A project carrying only the `VersionPrefix`/`VersionSuffix`
//! composite has no slot at all (the composite is evaluation semantics
//! — discovery plans no site), so a patch aimed at it refuses. A patch
//! whose text already equals the project's current value is the
//! idempotent re-run (I1): matched, counted as skipped, bytes verbatim.

use halyard_core::edit::{EditFailure, EditOutcome};
use halyard_core::package::RelPath;
use halyard_core::plan::PlanPatch;

use crate::grammar;

/// The semantic patch: validate every patch span against the pack
/// slot's value span, swap the inner bytes right-to-left, and verify by
/// re-parse before returning.
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
        message: "the project file is not valid UTF-8".to_owned(),
    })?;
    let csproj = grammar::parse_csproj(text, &file_string).map_err(|error| EditFailure::Parse {
        file: file_string.clone(),
        message: error.to_string(),
    })?;
    // The editable slot: the element pack consumes — `PackageVersion`
    // when present, else `Version` (the documented resolution).
    let slot = csproj
        .package_version
        .as_ref()
        .or(csproj.version.as_ref())
        .map(|field| field.span);
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
    // The read-back verify (§9.1): the re-parsed project carries the
    // target the patch aimed at — never a guessed half-application.
    let bumped = std::str::from_utf8(&out).map_err(|_| EditFailure::Parse {
        file: file_string.clone(),
        message: "the patched project file is not valid UTF-8".to_owned(),
    })?;
    let reparsed =
        grammar::parse_csproj(bumped, &file_string).map_err(|error| EditFailure::Parse {
            file: file_string.clone(),
            message: format!("read-back re-parse failed: {error}"),
        })?;
    for patch in patches {
        let carries = reparsed
            .package_version
            .as_ref()
            .or(reparsed.version.as_ref())
            .is_some_and(|field| field.text == patch.text);
        if !carries {
            return Err(EditFailure::Parse {
                file: file_string.clone(),
                message: format!(
                    "read-back verify: the project carries {:?} after the patch, expected \
                     {:?} — nothing was mutated",
                    reparsed
                        .package_version
                        .as_ref()
                        .or(reparsed.version.as_ref())
                        .map(|field| field.text.as_str()),
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

    const APP: &str = "<Project Sdk=\"Microsoft.NET.Sdk\"><PropertyGroup><PackageId>App</PackageId>\
         <PackageVersion>1.2.3</PackageVersion></PropertyGroup></Project>";

    #[test]
    fn swaps_the_value_and_preserves_the_markup() {
        let file = RelPath::new("src/app/App.csproj").expect("fixture path");
        // The PackageVersion value's span: the "1.2.3" between the tags.
        let start =
            u32::try_from(APP.find("<PackageVersion>").expect("fixture")).expect("fits") + 16;
        let patches = [PlanPatch {
            start,
            end: start + 5,
            text: "1.3.0".to_owned(),
        }];
        let out = edit_versions(&file, APP.as_bytes(), &patches).expect("applies");
        assert_eq!((out.applied, out.skipped), (1, 0));
        assert_eq!(
            String::from_utf8(out.content).expect("utf-8"),
            APP.replace("<PackageVersion>1.2.3", "<PackageVersion>1.3.0")
        );
    }

    #[test]
    fn a_span_off_the_slot_refuses() {
        let file = RelPath::new("src/app/App.csproj").expect("fixture path");
        let start = u32::try_from(APP.find("App</PackageId>").expect("fixture")).expect("fits");
        let patches = [PlanPatch {
            start,
            end: start + 1,
            text: "1.3.0".to_owned(),
        }];
        let err = edit_versions(&file, APP.as_bytes(), &patches).expect_err("refuses");
        assert_eq!(err.code(), "E-EDIT-PATCHES");
    }

    #[test]
    fn the_idempotent_rerun_skips_the_matched_value() {
        let file = RelPath::new("src/app/App.csproj").expect("fixture path");
        let start =
            u32::try_from(APP.find("<PackageVersion>").expect("fixture")).expect("fits") + 16;
        let patches = [PlanPatch {
            start,
            end: start + 5,
            text: "1.2.3".to_owned(),
        }];
        let out = edit_versions(&file, APP.as_bytes(), &patches).expect("applies");
        assert_eq!((out.applied, out.skipped), (0, 1), "already carried");
        assert_eq!(out.content, APP.as_bytes(), "the bytes are verbatim");
    }

    #[test]
    fn empty_passes_return_the_verbatim_bytes() {
        let file = RelPath::new("src/app/App.csproj").expect("fixture path");
        let out = edit_versions(&file, APP.as_bytes(), &[]).expect("an empty pass");
        assert_eq!(out.content, APP.as_bytes());
        assert_eq!((out.applied, out.skipped), (0, 0));
    }
}

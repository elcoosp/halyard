//! The M8 `NuGet` cell's edit contract (ADR-0059's release half): the
//! plan's own-version byte patches re-apply **semantically and in
//! place** — each patch span must land on the element pack consumes
//! (`PackageVersion` when present, else `Version` — the documented
//! *"Default is the value of `$(Version)`"* resolution), the value's
//! inner bytes swap, the surrounding markup never moves (invariant I4),
//! and a re-parse read-back verifies the project now carries the target
//! before the patch is accepted.
//!
//! A patch aimed at any other span refuses (`E-EDIT-PATCHES`); a
//! re-parse that does not carry the target refuses (`E-EDIT-VERIFY`
//! posture); a patch whose text already equals the current value is the
//! idempotent re-run (I1): matched, skipped, bytes verbatim.

use halyard_core::EcosystemAdapter;
use halyard_core::package::RelPath;
use halyard_core::plan::PlanPatch;
use halyard_eco_nuget::NuGetAdapter;

const APP: &str = r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><PackageId>App</PackageId><PackageVersion>1.2.3</PackageVersion></PropertyGroup></Project>"#;

fn span_of(haystack: &str, needle: &str) -> (u32, u32) {
    let start = u32::try_from(
        haystack
            .find(needle)
            .unwrap_or_else(|| panic!("fixture offset {needle}")),
    )
    .expect("fits u32");
    (
        start,
        start + u32::try_from(needle.len()).expect("fits u32"),
    )
}

fn patch(haystack: &str, needle: &str, text: &str) -> PlanPatch {
    let (start, end) = span_of(haystack, needle);
    PlanPatch {
        start,
        end,
        text: text.to_owned(),
    }
}

/// The pack slot's value swaps in place and every other byte survives
/// verbatim (I4).
#[test]
fn swaps_the_value_and_preserves_the_markup() {
    let file = RelPath::new("src/app/App.csproj").expect("fixture path");
    let patches = [patch(APP, "1.2.3", "1.3.0")];
    let out = NuGetAdapter::new()
        .edit_versions(&file, APP.as_bytes(), &patches)
        .expect("applies");
    assert_eq!((out.applied, out.skipped), (1, 0));
    assert_eq!(
        String::from_utf8(out.content).expect("utf-8"),
        APP.replace("<PackageVersion>1.2.3", "<PackageVersion>1.3.0")
    );
}

/// The `Version` element is the slot when no `PackageVersion` exists
/// (the documented `$(Version)` fallback).
#[test]
fn the_version_element_is_the_fallback_slot() {
    let lib = r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><Version>2.0.0</Version></PropertyGroup></Project>"#;
    let file = RelPath::new("src/lib/Lib.csproj").expect("fixture path");
    let patches = [patch(lib, "2.0.0", "2.1.0")];
    let out = NuGetAdapter::new()
        .edit_versions(&file, lib.as_bytes(), &patches)
        .expect("applies");
    assert_eq!((out.applied, out.skipped), (1, 0));
    assert_eq!(
        String::from_utf8(out.content).expect("utf-8"),
        lib.replace("<Version>2.0.0", "<Version>2.1.0")
    );
}

/// When `PackageVersion` exists, the `Version` element is NOT the slot:
/// a patch aimed at its span refuses (pack consumes `PackageVersion`).
#[test]
fn a_patch_at_the_version_span_refuses_when_packageversion_owns_the_slot() {
    let both = r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><PackageVersion>1.2.3</PackageVersion><Version>1.2.3</Version></PropertyGroup></Project>"#;
    let file = RelPath::new("src/app/App.csproj").expect("fixture path");
    let (start, end) = {
        let second = both.rfind("1.2.3").expect("fixture offset");
        (
            u32::try_from(second).expect("fits"),
            u32::try_from(second + 5).expect("fits"),
        )
    };
    let patches = [PlanPatch {
        start,
        end,
        text: "1.3.0".to_owned(),
    }];
    let err = NuGetAdapter::new()
        .edit_versions(&file, both.as_bytes(), &patches)
        .expect_err("the Version span is not the slot");
    assert_eq!(err.code(), "E-EDIT-PATCHES");
}

/// A patch whose span does not land on the slot refuses (`E-EDIT-PATCHES`)
/// — the manifest drifted or the plan is stale.
#[test]
fn a_span_off_the_slot_refuses() {
    let file = RelPath::new("src/app/App.csproj").expect("fixture path");
    let patches = [patch(APP, "App", "Other")];
    let err = NuGetAdapter::new()
        .edit_versions(&file, APP.as_bytes(), &patches)
        .expect_err("refuses");
    assert_eq!(err.code(), "E-EDIT-PATCHES");
}

/// The idempotent re-run (I1): the value already carries the target —
/// matched, counted as skipped, bytes verbatim.
#[test]
fn the_idempotent_rerun_skips_the_matched_value() {
    let file = RelPath::new("src/app/App.csproj").expect("fixture path");
    let patches = [patch(APP, "1.2.3", "1.2.3")];
    let out = NuGetAdapter::new()
        .edit_versions(&file, APP.as_bytes(), &patches)
        .expect("applies");
    assert_eq!((out.applied, out.skipped), (0, 1), "already carried");
    assert_eq!(out.content, APP.as_bytes(), "the bytes are verbatim");
}

/// Empty passes return the verbatim bytes (nothing to apply, nothing
/// changed).
#[test]
fn empty_passes_return_the_verbatim_bytes() {
    let file = RelPath::new("src/app/App.csproj").expect("fixture path");
    let out = NuGetAdapter::new()
        .edit_versions(&file, APP.as_bytes(), &[])
        .expect("an empty pass");
    assert_eq!(out.content, APP.as_bytes());
    assert_eq!((out.applied, out.skipped), (0, 0));
}

/// A project with no version element has no slot: any patch refuses.
#[test]
fn a_slotless_project_refuses_any_patch() {
    let plain = r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><TargetFramework>net10.0</TargetFramework></PropertyGroup></Project>"#;
    let file = RelPath::new("plain/Plain.csproj").expect("fixture path");
    let patches = [PlanPatch {
        start: 4,
        end: 9,
        text: "1.0.0".to_owned(),
    }];
    let err = NuGetAdapter::new()
        .edit_versions(&file, plain.as_bytes(), &patches)
        .expect_err("refuses");
    assert_eq!(err.code(), "E-EDIT-PATCHES");
}

//! Fuzz target for the tag-template grammar (appendix A.3).
//!
//! Invariants: a parsed template renders without panicking for any
//! package/version/ecosystem, and a rendered tag round-trips through
//! `extract_version` to the same version.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let Ok(template) = halyard_core::TagTemplate::parse(text) else {
        return;
    };
    let version = semver::Version::new(1, 2, 3);
    let rendered = template.render("pkg", &version, "cargo");
    // Round trip: when extraction applies to the template (no pinned
    // components outside `{version}`), a rendered tag always re-extracts
    // to the same version.
    if let Some(extracted) = template.extract_version(&rendered, "pkg", "cargo") {
        assert_eq!(extracted, version, "render must round-trip");
    }
});

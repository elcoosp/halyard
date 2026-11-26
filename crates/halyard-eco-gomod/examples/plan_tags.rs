//! Example: the release half of the Go-modules cell (M8, ADR-0057) —
//! the tag shapes and the no-publish contract.
//!
//! Run: `cargo run -p halyard-eco-gomod --example plan_tags`
//!
//! A tag-only release ships as the repository tag itself: the per-package
//! template derives from the module's directory (root `v{version}`,
//! subdir `{dir}/v{version}` — a /vN module's version carries the major),
//! and the release row carries NO publish decision — there is no go
//! registry, no spawn, no probe to resume. The manifest-version probe
//! contract is the same shape: a go.mod never carries a version, so the
//! tag-target pinpoint walk keeps its documented HEAD fallback.

use halyard_core::EcosystemAdapter;
use halyard_core::discover::DiscoverCtx;
use halyard_core::package::TagTemplate;
use halyard_eco_gomod::{GoModAdapter, tag};

const GO_WORK: &str = "go 1.23.0\n\nuse (\n\t.\n\t./api\n\t./libs/metrics\n)\n";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // The fixture workspace (the same shape the contract tests pin).
    let mut ctx = DiscoverCtx::new();
    ctx.insert_file("go.work", GO_WORK.as_bytes().to_vec());
    ctx.insert_file(
        "go.mod",
        b"module example.com/halyard\n\ngo 1.23.0\n".to_vec(),
    );
    ctx.insert_file(
        "api/go.mod",
        b"module example.com/halyard/api\n\ngo 1.23.0\n".to_vec(),
    );
    ctx.insert_file(
        "libs/metrics/go.mod",
        b"module example.com/halyard/libs/metrics/v2\n\ngo 1.23.0\n".to_vec(),
    );
    let rows = GoModAdapter::new().discover(&ctx)?;

    // The tag shapes each package's release rides (the §8.2 base-tag
    // parse consumes the same templates).
    let samples: [(&str, &str, &str, &str); 3] = [
        ("example.com/halyard", "", "1.2.3", "v1.2.3"),
        ("example.com/halyard/api", "api", "0.2.0", "api/v0.2.0"),
        (
            "example.com/halyard/libs/metrics/v2",
            "libs/metrics",
            "2.1.0",
            "libs/metrics/v2.1.0",
        ),
    ];
    for (name, root, version, expected) in samples {
        let template = tag::template_for(root);
        let parsed: semver::Version = version.parse()?;
        let rendered = template.render(name, &parsed, "go");
        assert_eq!(rendered, expected, "the subdir tag shape");
        println!(
            "{name}: template {:?} renders {rendered} (round-trips to {:?})",
            template.as_str(),
            template
                .extract_version(&rendered, name, "go")
                .map(|v| v.to_string())
        );
    }

    // The discovery rows agree with the derived templates and carry the
    // tag-only facts (no manifest version, unpublishable, no version
    // site).
    for row in &rows {
        let template = row
            .tag_template
            .clone()
            .unwrap_or_else(TagTemplate::independent_default);
        println!(
            "row {:?} root {:?} template {:?} version {:?} publishable {}",
            row.name,
            row.root,
            template.as_str(),
            row.version,
            row.publishable
        );
    }

    // The probe contract: a go.mod never carries a version — the
    // tag-target pinpoint walk keeps the documented HEAD fallback for
    // the go ecosystem.
    let probed = halyard_eco_gomod::manifest_version(b"module example.com/halyard\n\ngo 1.23.0\n");
    println!("manifest_version(go.mod) = {probed:?} (None by the grammar — tag-only)");
    Ok(())
}

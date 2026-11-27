//! Example: discover a `go.work` workspace and plan the tag-only rows
//! (M8, the §17 M8 **Go modules** cell — *"Go modules (tag-only,
//! `go.work`, subdir tags)"*, ADR-0057).
//!
//! Run: `cargo run -p halyard-eco-gomod --example scan_gomod`
//!
//! Every Go row is tag-only: the manifest carries no version (the
//! release tag does), there is no editable version site, and the tag
//! template renders the documented shapes — the repo root tags
//! `v{version}`, a module in a subdirectory tags `{subdir}/v{version}`.
//! Internal edges are the workspace-proven ones (a require naming a
//! `use` member, a require named by a local-path replace).

use halyard_core::EcosystemAdapter;
use halyard_core::discover::DiscoverCtx;
use halyard_eco_gomod::GoModAdapter;

const GO_WORK: &str = "go 1.23.0\n\nuse (\n\t.\n\t./api\n\t./libs/metrics\n)\n\nreplace example.com/external/sdk => ./vendor/sdk\n";

const ROOT_GOMOD: &str = r"module example.com/halyard

go 1.23.0

require (
	example.com/halyard/api v0.1.2
	example.com/external/sdk v1.4.0
)
";

const API_GOMOD: &str = r"module example.com/halyard/api

go 1.23.0

require example.com/halyard/libs/metrics/v2 v2.1.0
";

const METRICS_GOMOD: &str = r"module example.com/halyard/libs/metrics/v2

go 1.23.0
";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // The fixture workspace: the repo-root module plus two subdir
    // members, one of them a /v2 module.
    let mut ctx = DiscoverCtx::new();
    ctx.insert_file("go.work", GO_WORK.as_bytes().to_vec());
    ctx.insert_file("go.mod", ROOT_GOMOD.as_bytes().to_vec());
    ctx.insert_file("api/go.mod", API_GOMOD.as_bytes().to_vec());
    ctx.insert_file("libs/metrics/go.mod", METRICS_GOMOD.as_bytes().to_vec());

    let rows = GoModAdapter::new().discover(&ctx)?;
    for row in &rows {
        println!(
            "module {:?} root {:?} tag template {:?} publishable {}",
            row.name,
            row.root,
            row.tag_template
                .as_ref()
                .map(halyard_core::package::TagTemplate::as_str),
            row.publishable,
        );
        println!(
            "  manifest version: {:?} (tag-only — the release tag carries it)",
            row.version
        );
        for dep in &row.deps {
            println!(
                "  require {:?} {} internal={}",
                dep.name,
                String::from_utf8_lossy(&dep.raw),
                dep.internal
            );
        }
    }

    // The tag-only edit seam: go.mod has no editable version row, so
    // any patch fails closed without touching the bytes.
    let file = halyard_core::package::RelPath::new("api/go.mod")
        .ok_or("the fixture path is normalized")?;
    let patches = [halyard_core::plan::PlanPatch {
        start: 4,
        end: 9,
        text: "0.2.0".to_owned(),
    }];
    match GoModAdapter::new().edit_versions(&file, API_GOMOD.as_bytes(), &patches) {
        Err(failure) => println!(
            "\nthe patch fails closed as expected: {failure} (E-EDIT-PATCHES) — a go.mod \
             has no version row",
        ),
        Ok(_) => println!("\nunreachable: a tag-only manifest has no editable row"),
    }
    Ok(())
}

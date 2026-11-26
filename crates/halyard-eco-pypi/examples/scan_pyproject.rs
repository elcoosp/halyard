//! Example: discover a uv workspace and edit one `pyproject.toml`
//! version in place, PEP 440 spelling preserved (M8 step 1, ADR-0056).
//!
//! Run: `cargo run -p halyard-eco-pypi --example scan_pyproject`
//!
//! The edit replaces only the version value's inner bytes; every
//! untouched byte (quoting, key order, trailing newline) survives —
//! invariant I4 — and the plan's semver target renders back to the
//! canonical PEP 440 spelling.

use halyard_core::EcosystemAdapter;
use halyard_core::discover::DiscoverCtx;
use halyard_core::plan::PlanPatch;
use halyard_eco_pypi::{PyPiAdapter, pep440};

const ROOT: &str = "[project]\nname = \"albatross\"\nversion = \"1.4.2\"\ndependencies = [\"bird-feeder>=0.2\", \"tqdm>=4,<5\"]\n\n[tool.uv.sources]\nbird-feeder = { workspace = true }\n\n[tool.uv.workspace]\nmembers = [\"packages/*\"]\n";

const MEMBER: &str = "[project]\nname = \"bird-feeder\"\nversion = \"1.2.3rc1\"\n";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // The PEP 440 strategy: canonical spellings pair with the plan
    // contract's semver type, both directions.
    let paired = pep440::parse_pep440("1.2.3rc1").ok_or("canonical spelling")?;
    println!(
        "PEP 440 1.2.3rc1 pairs with semver {} and renders back as {}",
        paired,
        pep440::render_pep440(&paired).as_deref().unwrap_or("?")
    );

    // Discovery: the root plus the globed uv members.
    let mut ctx = DiscoverCtx::new();
    assert!(ctx.insert_file("pyproject.toml", ROOT.as_bytes().to_vec()));
    assert!(ctx.insert_file(
        "packages/bird-feeder/pyproject.toml",
        MEMBER.as_bytes().to_vec()
    ));
    let rows = PyPiAdapter.discover(&ctx)?;
    for row in &rows {
        println!(
            "package {:?} root {:?} version {:?} publishable {} internal deps {}",
            row.name,
            row.root,
            row.version.as_ref().map(std::string::ToString::to_string),
            row.publishable,
            row.deps.iter().filter(|d| d.internal).count(),
        );
    }

    // The plan's edit: the planner pins the version value's inner span;
    // the target renders through the PEP 440 strategy on write-back.
    let root = rows
        .iter()
        .find(|r| r.name == "albatross")
        .ok_or("the root row is missing")?;
    let site = root
        .version_site
        .as_ref()
        .ok_or("the version site is missing")?;
    let span = site
        .spans
        .first()
        .ok_or("the version site span is missing")?;
    let patch = PlanPatch {
        start: span.start,
        end: span.end,
        text: "1.5.0".to_owned(),
    };
    let outcome = PyPiAdapter.edit_versions(
        &halyard_core::package::RelPath::new("pyproject.toml").ok_or("normalized")?,
        ROOT.as_bytes(),
        &[patch],
    )?;
    println!("--- patched root ---");
    print!("{}", String::from_utf8(outcome.content)?);
    Ok(())
}

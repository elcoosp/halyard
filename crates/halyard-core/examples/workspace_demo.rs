//! Workspace model demo (M1.1): intern names, register packages with
//! dependency edges, group them, and hash a plan — the pure-core pipeline
//! without any discovery or git.
//!
//! Run: `cargo run -p halyard-core --example workspace_demo`

use std::error::Error;

use halyard_core::plan::{Bump, Plan, PlanReason, PlanRelease};
use halyard_core::{
    DepEdge, DepKind, Ecosystem, GroupId, GroupPolicy, ManifestSite, PLAN_SCHEMA_V1, Package,
    PathFilter, PkgId, RelPath, ReqSpec, Span, TagTemplate, VersionSource, Workspace,
};
use semver::{Version, VersionReq};

/// Validates a repo-relative path with a readable error (no panics in
/// examples — the same restriction lints apply).
fn rel(raw: &str) -> Result<RelPath, Box<dyn Error>> {
    RelPath::new(raw).ok_or_else(|| format!("invalid repo-relative path: {raw:?}").into())
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut workspace = Workspace::new();

    // Intern once, reference everywhere (§5.1).
    let core_name = workspace.intern("core");
    let cli_name = workspace.intern("cli");

    let core = Package {
        id: PkgId::FIRST,
        name: core_name,
        ecosystem: Ecosystem::Cargo,
        root: rel("crates/core")?,
        manifest: rel("crates/core/Cargo.toml")?,
        version: Some(Version::parse("0.4.1")?),
        version_source: VersionSource::Local,
        publishable: true,
        deps: Vec::new(),
        tag_template: TagTemplate::parse("{name}-v{version}")?,
        changelog: None,
        version_site: None,
        include: PathFilter::new(Vec::new(), Vec::new()),
        group: GroupId::new(0),
        facets: halyard_core::Facets::default(),
    };
    let core_id = workspace.push_package(core);

    let manifest = rel("crates/cli/Cargo.toml")?;
    let cli = Package {
        id: PkgId::FIRST,
        name: cli_name,
        ecosystem: Ecosystem::Cargo,
        root: rel("crates/cli")?,
        manifest: manifest.clone(),
        version: Some(Version::parse("0.4.1")?),
        version_source: VersionSource::Local,
        publishable: true,
        deps: vec![DepEdge {
            to: core_id,
            kind: DepKind::Normal,
            req: ReqSpec::VersionReq(VersionReq::parse("0.4")?),
            site: ManifestSite {
                file: manifest,
                spans: vec![Span { start: 61, end: 66 }],
            },
            raw: halyard_core::BString::from("0.4"),
        }],
        tag_template: TagTemplate::parse("{name}-v{version}")?,
        changelog: None,
        version_site: None,
        include: PathFilter::new(Vec::new(), Vec::new()),
        group: GroupId::new(0),
        facets: halyard_core::Facets::default(),
    };
    let cli_id = workspace.push_package(cli);

    let group_name = workspace.intern("release-group");
    let group = workspace.add_group(
        group_name,
        GroupPolicy::Independent,
        vec![core_id, cli_id],
        TagTemplate::parse("{name}-v{version}")?,
    );

    println!("packages: {}", workspace.packages().len());
    for pkg in workspace.packages() {
        println!(
            "  {} {} ({})",
            pkg.id.to_u32(),
            workspace.interner().get(pkg.name).unwrap_or("?"),
            pkg.ecosystem.resolve(workspace.interner())
        );
    }
    println!(
        "groups: {} policy {:?}",
        workspace.groups().len(),
        workspace.group(group).map(|g| g.policy)
    );

    // Tag template round trip (appendix A.3).
    let template = TagTemplate::parse("{name}-v{version}")?;
    let next = Version::parse("0.5.0")?;
    println!("core tag: {}", template.render("core", &next, "cargo"));
    println!(
        "extract from core-v0.5.0: {:?}",
        template
            .extract_version("core-v0.5.0", "core", "cargo")
            .map(|v| v.to_string())
    );

    // Plan hash over a minimal wire plan (§5.8, I3).
    let plan = Plan {
        schema: PLAN_SCHEMA_V1.to_owned(),
        base_branch: "main".to_owned(),
        head: "9f2ca67".to_owned(),
        plan_hash: String::new(),
        releases: vec![PlanRelease {
            package: "core".to_owned(),
            ecosystem: "cargo".to_owned(),
            from: "0.4.1".to_owned(),
            to: "0.5.0".to_owned(),
            bump: Bump::Minor,
            tag: "core-v0.5.0".to_owned(),
            reasons: vec![PlanReason::Commit {
                oid: "ab12cd34".to_owned(),
                kind: "feat".to_owned(),
                breaking: false,
            }],
            changelog: None,
            publish: None,
        }],
        edits: Vec::new(),
        order: vec!["core".to_owned()],
        skipped: Vec::new(),
    };
    println!("plan hash: {}", halyard_core::plan_hash(&plan));

    Ok(())
}

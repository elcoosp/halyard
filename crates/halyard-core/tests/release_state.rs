//! The §11.1 step-2 stateless reconciliation (manifest-vs-tags): the tag
//! phase's "what is still owed" decision as a pure function over the
//! workspace, the enumerated tag names and the planner config. The
//! machine (halyard-forge) consumes the pending list; the shell supplies
//! the tags (M1 reconciliation slice).

use halyard_core::package::{
    Ecosystem, GroupId, Package, PathFilter, RelPath, TagTemplate, VersionSource, Workspace,
};
use halyard_core::planner::PlanConfig;
use halyard_core::release_state::{TagState, pending_releases, reconcile_tags};
use halyard_core::{BString, PkgId};
use semver::Version;

/// Registers one package with the given name, version and template.
fn push_pkg(ws: &mut Workspace, name: &str, version: Option<&str>, template: &str, eco: Ecosystem) {
    let pkg = Package {
        id: PkgId::FIRST,
        name: ws.intern(name),
        ecosystem: eco,
        root: RelPath::new("crates/x").unwrap_or_else(|| unreachable!("fixture root is valid")),
        manifest: RelPath::new("crates/x/Cargo.toml")
            .unwrap_or_else(|| unreachable!("fixture manifest is valid")),
        version: version
            .map(|v| Version::parse(v).unwrap_or_else(|e| unreachable!("fixture version: {e}"))),
        version_source: VersionSource::Local,
        publishable: true,
        deps: Vec::new(),
        tag_template: TagTemplate::parse(template)
            .unwrap_or_else(|e| unreachable!("fixture template is valid: {e}")),
        changelog: None,
        version_site: None,
        include: PathFilter::new(Vec::new(), Vec::new()),
        group: GroupId::new(0),
        facets: halyard_core::discover::Facets::default(),
    };
    ws.push_package(pkg);
}

fn tags(names: &[&str]) -> Vec<BString> {
    names.iter().map(|n| BString::from(*n)).collect()
}

#[test]
fn initial_repo_with_no_tags_is_pending() {
    let mut ws = Workspace::new();
    push_pkg(
        &mut ws,
        "solo",
        Some("0.1.0"),
        "v{version}",
        Ecosystem::Cargo,
    );
    let cfg = PlanConfig::default_values();
    let states = reconcile_tags(&ws, &tags(&[]), &cfg);
    assert_eq!(states.len(), 1);
    let first = states.first().expect("one row");
    assert_eq!(first.package, "solo");
    assert_eq!(first.tag, "v0.1.0");
    assert_eq!(first.state, TagState::Pending);
    let pending = pending_releases(&ws, &tags(&[]), &cfg);
    assert_eq!(pending.len(), 1);
    let release = pending.first().expect("one pending");
    assert_eq!(release.tag, "v0.1.0");
    assert_eq!(release.version, Version::parse("0.1.0").expect("valid"));
}

#[test]
fn manifest_version_equal_to_latest_tag_is_tagged() {
    let mut ws = Workspace::new();
    push_pkg(
        &mut ws,
        "solo",
        Some("0.1.0"),
        "v{version}",
        Ecosystem::Cargo,
    );
    let cfg = PlanConfig::default_values();
    let states = reconcile_tags(&ws, &tags(&["v0.1.0"]), &cfg);
    let first = states.first().expect("one row");
    assert_eq!(first.state, TagState::Tagged);
    assert_eq!(
        first.latest_version.as_ref().map(ToString::to_string),
        Some("0.1.0".to_owned())
    );
    assert_eq!(pending_releases(&ws, &tags(&["v0.1.0"]), &cfg), Vec::new());
}

#[test]
fn manifest_ahead_of_latest_tag_is_pending() {
    // The post-merge Release-PR state: the manifest says 0.1.1, only
    // v0.1.0 is tagged — the tag phase owes v0.1.1 (§11.1 step 2).
    let mut ws = Workspace::new();
    push_pkg(
        &mut ws,
        "solo",
        Some("0.1.1"),
        "v{version}",
        Ecosystem::Cargo,
    );
    let cfg = PlanConfig::default_values();
    let states = reconcile_tags(&ws, &tags(&["v0.1.0", "v0.0.9"]), &cfg);
    let first = states.first().expect("one row");
    assert_eq!(first.state, TagState::Pending);
    assert_eq!(first.tag, "v0.1.1");
    let pending = pending_releases(&ws, &tags(&["v0.1.0", "v0.0.9"]), &cfg);
    assert_eq!(
        pending.iter().map(|p| p.tag.as_str()).collect::<Vec<_>>(),
        vec!["v0.1.1"]
    );
}

#[test]
fn manifest_behind_latest_tag_is_behind_and_never_pending() {
    // A reverted merge: the manifest went back below the released tag.
    // The reconciliation reports the anomaly but never tags downward.
    let mut ws = Workspace::new();
    push_pkg(
        &mut ws,
        "solo",
        Some("0.1.0"),
        "v{version}",
        Ecosystem::Cargo,
    );
    let cfg = PlanConfig::default_values();
    let states = reconcile_tags(&ws, &tags(&["v0.2.0", "v0.1.0"]), &cfg);
    let first = states.first().expect("one row");
    assert_eq!(first.state, TagState::Behind);
    assert_eq!(
        first.latest_version.as_ref().map(ToString::to_string),
        Some("0.2.0".to_owned())
    );
    assert_eq!(
        pending_releases(&ws, &tags(&["v0.2.0", "v0.1.0"]), &cfg),
        Vec::new()
    );
}

#[test]
fn unrelated_tags_do_not_interfere() {
    // Tags that do not match the package's template carry no version and
    // cannot gate the tag phase. Multi-package workspace ⇒ the
    // discovery-resolved template applies (no single-package override).
    let mut ws = Workspace::new();
    push_pkg(
        &mut ws,
        "core",
        Some("0.4.2"),
        "core-v{version}",
        Ecosystem::Cargo,
    );
    push_pkg(
        &mut ws,
        "docs",
        Some("0.0.1"),
        "docs-v{version}",
        Ecosystem::Cargo,
    );
    let cfg = PlanConfig::default_values();
    let states = reconcile_tags(&ws, &tags(&["v1.0.0", "other-v9.9.9", "core-v0.4.1"]), &cfg);
    let core = states
        .iter()
        .find(|s| s.package == "core")
        .expect("core row");
    assert_eq!(
        core.state,
        TagState::Pending,
        "manifest 0.4.2 > latest core tag 0.4.1"
    );
    assert_eq!(
        core.latest_version.as_ref().map(ToString::to_string),
        Some("0.4.1".to_owned())
    );
}

#[test]
fn config_tag_style_override_flows_through() {
    // The planner's tag-style override (appendix A.3, config wins) is
    // the reconciliation's own resolution: a scoped npm single package
    // renders its scoped tag when the config says so — the tag the plan
    // would carry is the tag the tag phase owes.
    let mut ws = Workspace::new();
    push_pkg(
        &mut ws,
        "@scope/app",
        Some("1.2.3"),
        "@scope/app@{version}",
        Ecosystem::Npm,
    );
    let mut cfg = PlanConfig::default_values();
    cfg.tag_template = Some(TagTemplate::parse("@scope/app@{version}").expect("valid"));
    let states = reconcile_tags(&ws, &tags(&["@scope/app@1.2.2"]), &cfg);
    let first = states.first().expect("one row");
    assert_eq!(first.state, TagState::Pending);
    assert_eq!(first.tag, "@scope/app@1.2.3");
    let pending = pending_releases(&ws, &tags(&["@scope/app@1.2.2"]), &cfg);
    assert_eq!(
        pending.first().expect("one pending").tag,
        "@scope/app@1.2.3"
    );
}

#[test]
fn packages_without_a_version_are_skipped() {
    // A manifest without a version (unpublishable, §6.3) has nothing to
    // reconcile — no state row, no pending release.
    let mut ws = Workspace::new();
    push_pkg(&mut ws, "anonymous", None, "v{version}", Ecosystem::Cargo);
    push_pkg(
        &mut ws,
        "versioned",
        Some("0.1.0"),
        "v{version}",
        Ecosystem::Cargo,
    );
    let cfg = PlanConfig::default_values();
    let states = reconcile_tags(&ws, &tags(&[]), &cfg);
    assert_eq!(states.len(), 1, "the version-less package is skipped");
    assert_eq!(states.first().expect("one row").package, "versioned");
}

#[test]
fn release_disabled_packages_are_skipped() {
    // `[[package]] release = false` (the planner's skip set) excludes a
    // package from the tag phase exactly as it excludes it from the plan.
    let mut ws = Workspace::new();
    push_pkg(
        &mut ws,
        "internal-tool",
        Some("3.1.4"),
        "{name}-v{version}",
        Ecosystem::Cargo,
    );
    push_pkg(
        &mut ws,
        "lib",
        Some("0.2.0"),
        "{name}-v{version}",
        Ecosystem::Cargo,
    );
    let mut cfg = PlanConfig::default_values();
    cfg.release_disabled.insert("internal-tool".to_owned());
    let states = reconcile_tags(&ws, &tags(&[]), &cfg);
    let names: Vec<&str> = states.iter().map(|s| s.package.as_str()).collect();
    assert_eq!(
        names,
        vec!["lib"],
        "the release-disabled package is skipped"
    );
    let pending = pending_releases(&ws, &tags(&[]), &cfg);
    assert_eq!(
        pending.iter().map(|p| p.tag.as_str()).collect::<Vec<_>>(),
        vec!["lib-v0.2.0"],
        "only the release-enabled package is owed a tag"
    );
}

#[test]
fn output_is_sorted_by_package_name() {
    // Determinism (I3): the workspace registration order must not leak
    // into the reconciliation order.
    let mut ws = Workspace::new();
    push_pkg(
        &mut ws,
        "zeta",
        Some("1.0.0"),
        "{name}-v{version}",
        Ecosystem::Cargo,
    );
    push_pkg(
        &mut ws,
        "alpha",
        Some("2.0.0"),
        "{name}-v{version}",
        Ecosystem::Cargo,
    );
    let cfg = PlanConfig::default_values();
    let states = reconcile_tags(&ws, &tags(&[]), &cfg);
    let names: Vec<&str> = states.iter().map(|s| s.package.as_str()).collect();
    assert_eq!(names, vec!["alpha", "zeta"]);
}

#[test]
fn identical_rendered_tags_are_deduped() {
    // Two packages sharing one rendered tag produce ONE pending release:
    // a forge release is keyed by its tag (§11.4 create_release is
    // GET-by-tag-first), and a duplicate tag would be a duplicate effect.
    let mut ws = Workspace::new();
    push_pkg(&mut ws, "a", Some("1.0.0"), "v{version}", Ecosystem::Cargo);
    push_pkg(&mut ws, "b", Some("1.0.0"), "v{version}", Ecosystem::Cargo);
    let cfg = PlanConfig::default_values();
    let pending = pending_releases(&ws, &tags(&[]), &cfg);
    assert_eq!(pending.len(), 1, "one tag, one release");
    let release = pending.first().expect("one pending");
    assert_eq!(release.tag, "v1.0.0");
    // The first package in name order owns the shared tag row.
    assert_eq!(release.package, "a");
}

#[test]
fn tagged_and_pending_packages_coexist() {
    let mut ws = Workspace::new();
    push_pkg(
        &mut ws,
        "core",
        Some("0.4.1"),
        "core-v{version}",
        Ecosystem::Cargo,
    );
    push_pkg(
        &mut ws,
        "cli",
        Some("1.0.0"),
        "cli-v{version}",
        Ecosystem::Cargo,
    );
    let cfg = PlanConfig::default_values();
    let all_tags = tags(&["core-v0.4.1", "core-v0.4.0"]);
    let states = reconcile_tags(&ws, &all_tags, &cfg);
    let names: Vec<&str> = states.iter().map(|s| s.package.as_str()).collect();
    assert_eq!(names, vec!["cli", "core"]);
    let by_name: std::collections::BTreeMap<&str, &TagState> = states
        .iter()
        .map(|s| (s.package.as_str(), &s.state))
        .collect();
    assert!(matches!(by_name.get("cli"), Some(TagState::Pending)));
    assert!(matches!(by_name.get("core"), Some(TagState::Tagged)));
    let pending = pending_releases(&ws, &all_tags, &cfg);
    assert_eq!(
        pending.iter().map(|p| p.tag.as_str()).collect::<Vec<_>>(),
        vec!["cli-v1.0.0"]
    );
}

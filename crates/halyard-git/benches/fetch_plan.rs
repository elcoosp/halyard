//! Smart-minimal-fetch benchmark (§8.8): the pure planning shapes every
//! fetch run exercises — the per-package selection over the probed tag
//! listing ([`latest_tags_for`], the §8.8 step-1 matcher) and the argv
//! composition ([`fetch_attempts`], the §8.8 step-2/3 sequence). The
//! live effect itself is a subprocess round trip (network-bound, not
//! CPU-bound), so the CPU cost of `halyard fetch` is exactly these two
//! functions; both must stay far below the §14.1 budget of any command
//! that carries them.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use criterion::Criterion;
use halyard_core::PkgId;
use halyard_core::package::{
    DepEdge, Ecosystem, GroupId, Package, PathFilter, RelPath, TagTemplate, VersionSource,
    Workspace,
};
use halyard_core::planner::PlanConfig;
use halyard_git::fetch::{deepen_boundaries_for, fetch_attempts, latest_tags_for};

/// Packages in the synthetic workspace (the §14.1 40-crate shape).
const PACKAGES: usize = 40;
/// Remote tags the probe listing carries (name-sorted, one per package
/// plus a historical tail of five releases each).
const REMOTE_TAGS: usize = PACKAGES * 5;

/// Builds (once) a synthetic 40-package workspace with per-package tag
/// templates (`pkg<N>-v{version}`).
fn workspace() -> &'static Workspace {
    static WS: OnceLock<Workspace> = OnceLock::new();
    WS.get_or_init(|| {
        let mut ws = Workspace::new();
        for index in 0..PACKAGES {
            let name = format!("pkg{index}");
            let name_sym = ws.intern(&name);
            ws.push_package(Package {
                id: PkgId::FIRST,
                name: name_sym,
                ecosystem: Ecosystem::Cargo,
                root: RelPath::new(&format!("pkg{index}"))
                    .unwrap_or_else(|| unreachable!("package roots are valid RelPaths")),
                manifest: RelPath::new(&format!("pkg{index}/Cargo.toml"))
                    .unwrap_or_else(|| unreachable!("manifest paths are valid RelPaths")),
                version: Some(semver::Version::new(0, 1, 0)),
                version_source: VersionSource::Local,
                publishable: true,
                deps: Vec::<DepEdge>::new(),
                tag_template: TagTemplate::parse(&format!("pkg{index}-v{{version}}"))
                    .unwrap_or_else(|e| unreachable!("fixture templates parse: {e}")),
                changelog: None,
                version_site: None,
                include: PathFilter::new(Vec::new(), Vec::new()),
                group: GroupId::new(0),
                facets: halyard_core::Facets::default(),
            });
        }
        ws
    })
}

/// Builds (once) the probed listing: five releases per package plus
/// foreign and invalid tags the matcher must reject.
fn tag_targets() -> &'static BTreeMap<String, String> {
    static TARGETS: OnceLock<BTreeMap<String, String>> = OnceLock::new();
    TARGETS.get_or_init(|| {
        let mut targets = BTreeMap::new();
        for index in 0..PACKAGES {
            for minor in 1..=5u64 {
                let tag = format!("pkg{index}-v0.{minor}.0");
                targets.insert(tag, "0123456789abcdef0123456789abcdef01234567".to_owned());
            }
        }
        // Noise the §8.8 step-1 matcher must ignore, not choke on.
        targets.insert("not-a-version".to_owned(), String::new());
        targets.insert("pkg0-vzero".to_owned(), String::new());
        targets.insert("foreign-v0.1.0".to_owned(), String::new());
        assert_eq!(targets.len(), REMOTE_TAGS + 3);
        targets
    })
}

fn bench_fetch_plan(c: &mut Criterion) {
    let ws = workspace();
    let targets = tag_targets();
    let selection = latest_tags_for(ws, &PlanConfig::default_values(), targets);
    assert_eq!(selection.len(), PACKAGES);
    let boundaries = deepen_boundaries_for(ws, &PlanConfig::default_values(), targets);
    assert_eq!(boundaries.len(), PACKAGES);
    let tags: Vec<String> = selection.values().cloned().collect();

    c.bench_function("fetch_select_latest_40x202", |b| {
        b.iter(|| latest_tags_for(ws, &PlanConfig::default_values(), targets));
    });
    c.bench_function("fetch_deepen_boundaries_40x202", |b| {
        b.iter(|| deepen_boundaries_for(ws, &PlanConfig::default_values(), targets));
    });
    c.bench_function("fetch_attempts_40", |b| {
        b.iter(|| fetch_attempts("origin", Some("main"), &tags, &boundaries, true).len());
    });
}

fn main() {
    let mut criterion = Criterion::default();
    bench_fetch_plan(&mut criterion);
}

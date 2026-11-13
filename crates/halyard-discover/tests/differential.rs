//! M1.5 differential tests: 25 fixture workspaces + determinism (§17).
//!
//! Every Cargo fixture is discovered twice — once by our adapter
//! pipeline, once by `cargo metadata --no-deps` — and the two outputs
//! must agree exactly on members, versions and internal edges (kinds
//! normalized). Subprocesses here are test-only ground truth (invariant
//! I6 governs planning, not test harnesses).

use std::collections::BTreeSet;

use halyard_discover::{DiscoverInput, discover_from_ctx};
use halyard_test_support::manifest_fixtures::{
    CargoWorkspaceSpec, build_cargo_fixture, cargo_metadata_expectation, workspace_expectation,
};

/// The M1.5 corpus: 25 fixture workspaces covering the §6.3 surface.
/// Fixture 16 (`nested_workspace`) asserts the guidance error instead of
/// the differential (cargo errors there too).
static CARGO_FIXTURES: &[CargoWorkspaceSpec] = &[
    // 1. Single package (no workspace table).
    CargoWorkspaceSpec {
        name: "single-pkg",
        root: "[package]\nname = \"solo\"\nversion = \"1.0.0\"\n",
        members: &[],
    },
    // 2. Simple glob workspace.
    CargoWorkspaceSpec {
        name: "simple-glob",
        root: "[workspace]\nmembers = [\"crates/*\"]\n",
        members: &[
            (
                "crates/alpha",
                "[package]\nname = \"alpha\"\nversion = \"0.1.0\"\n",
            ),
            (
                "crates/beta",
                "[package]\nname = \"beta\"\nversion = \"0.2.0\"\n",
            ),
            (
                "crates/gamma",
                "[package]\nname = \"gamma\"\nversion = \"0.3.0\"\n",
            ),
        ],
    },
    // 3. Explicit members + exclude.
    CargoWorkspaceSpec {
        name: "explicit-and-exclude",
        root: "[workspace]\nmembers = [\"crates/keep\", \"crates/drop\"]\nexclude = [\"crates/drop\"]\n",
        members: &[
            (
                "crates/keep",
                "[package]\nname = \"keep\"\nversion = \"0.1.0\"\n",
            ),
            (
                "crates/drop",
                "[package]\nname = \"drop\"\nversion = \"0.1.0\"\n",
            ),
        ],
    },
    // 4. Virtual workspace (no root [package]).
    CargoWorkspaceSpec {
        name: "virtual-workspace",
        root: "[workspace]\nmembers = [\"crates/*\"]\n\n[workspace.package]\nversion = \"1.0.0\"\n",
        members: &[
            (
                "crates/vone",
                "[package]\nname = \"vone\"\nversion.workspace = true\n",
            ),
            (
                "crates/vtwo",
                "[package]\nname = \"vtwo\"\nversion.workspace = true\n",
            ),
        ],
    },
    // 5. Version inheritance through [workspace.package].
    CargoWorkspaceSpec {
        name: "version-inheritance",
        root: "[workspace]\nmembers = [\"crates/*\"]\n\n[workspace.package]\nversion = \"2.3.4\"\n",
        members: &[
            (
                "crates/inh-a",
                "[package]\nname = \"inh-a\"\nversion.workspace = true\n",
            ),
            (
                "crates/inh-b",
                "[package]\nname = \"inh-b\"\nversion.workspace = true\n",
            ),
        ],
    },
    // 6. [workspace.dependencies] + member `workspace = true`.
    CargoWorkspaceSpec {
        name: "workspace-deps",
        root: "[workspace]\nmembers = [\"crates/*\"]\n\n[workspace.dependencies]\nwscore = { path = \"crates/wscore\", version = \"0.4.0\" }\nserde = \"1.0\"\n",
        members: &[
            (
                "crates/wscore",
                "[package]\nname = \"wscore\"\nversion = \"0.4.0\"\n",
            ),
            (
                "crates/wuser",
                "[package]\nname = \"wuser\"\nversion = \"0.1.0\"\n\n[dependencies]\nwscore = { workspace = true }\nserde = { workspace = true }\n",
            ),
        ],
    },
    // 7. Dependency renames (`package = "…"`, inline and table forms).
    CargoWorkspaceSpec {
        name: "renames",
        root: "[workspace]\nmembers = [\"crates/*\"]\n",
        members: &[
            (
                "crates/real",
                "[package]\nname = \"real\"\nversion = \"0.1.0\"\n",
            ),
            (
                "crates/user",
                "[package]\nname = \"user\"\nversion = \"0.1.0\"\n\n[dependencies]\naliased = { package = \"real\", version = \"0.1.0\", path = \"../real\" }\n",
            ),
        ],
    },
    // 8. [target.'cfg(..)'.dependencies] on an internal crate.
    CargoWorkspaceSpec {
        name: "target-deps",
        root: "[workspace]\nmembers = [\"crates/*\"]\n",
        members: &[
            (
                "crates/tcore",
                "[package]\nname = \"tcore\"\nversion = \"0.1.0\"\n",
            ),
            (
                "crates/tuser",
                "[package]\nname = \"tuser\"\nversion = \"0.1.0\"\n\n[target.'cfg(unix)'.dependencies]\ntcore = { path = \"../tcore\" }\n",
            ),
        ],
    },
    // 9. dev-dependencies on an internal crate.
    CargoWorkspaceSpec {
        name: "dev-deps",
        root: "[workspace]\nmembers = [\"crates/*\"]\n",
        members: &[
            (
                "crates/dcore",
                "[package]\nname = \"dcore\"\nversion = \"0.1.0\"\n",
            ),
            (
                "crates/duser",
                "[package]\nname = \"duser\"\nversion = \"0.1.0\"\n\n[dev-dependencies]\ndcore = { path = \"../dcore\" }\n",
            ),
        ],
    },
    // 10. build-dependencies on an internal crate.
    CargoWorkspaceSpec {
        name: "build-deps",
        root: "[workspace]\nmembers = [\"crates/*\"]\n",
        members: &[
            (
                "crates/bcore",
                "[package]\nname = \"bcore\"\nversion = \"0.1.0\"\n",
            ),
            (
                "crates/buser",
                "[package]\nname = \"buser\"\nversion = \"0.1.0\"\n\n[build-dependencies]\nbcore = { path = \"../bcore\" }\n",
            ),
        ],
    },
    // 11. Optional dependency (internal, optional = true).
    CargoWorkspaceSpec {
        name: "optional-dep",
        root: "[workspace]\nmembers = [\"crates/*\"]\n",
        members: &[
            (
                "crates/ocore",
                "[package]\nname = \"ocore\"\nversion = \"0.1.0\"\n",
            ),
            (
                "crates/ouser",
                "[package]\nname = \"ouser\"\nversion = \"0.1.0\"\n\n[dependencies]\nocore = { path = \"../ocore\", optional = true }\n",
            ),
        ],
    },
    // 12. Features with `dep:` references over an optional dep.
    CargoWorkspaceSpec {
        name: "dep-features",
        root: "[workspace]\nmembers = [\"crates/*\"]\n",
        members: &[
            (
                "crates/fcore",
                "[package]\nname = \"fcore\"\nversion = \"0.1.0\"\n",
            ),
            (
                "crates/fuser",
                "[package]\nname = \"fuser\"\nversion = \"0.1.0\"\n\n[dependencies]\nfcore = { path = \"../fcore\", optional = true }\n\n[features]\nfull = [\"dep:fcore\"]\n",
            ),
        ],
    },
    // 13. publish = false member.
    CargoWorkspaceSpec {
        name: "publish-false",
        root: "[workspace]\nmembers = [\"crates/*\"]\n",
        members: &[
            (
                "crates/priv",
                "[package]\nname = \"priv\"\nversion = \"0.1.0\"\npublish = false\n",
            ),
            (
                "crates/pub",
                "[package]\nname = \"pub\"\nversion = \"0.1.0\"\n",
            ),
        ],
    },
    // 14. publish = ["custom-registry"] member.
    CargoWorkspaceSpec {
        name: "publish-registry",
        root: "[workspace]\nmembers = [\"crates/*\"]\n",
        members: &[(
            "crates/other-reg",
            "[package]\nname = \"other-reg\"\nversion = \"0.1.0\"\npublish = [\"custom-registry\"]\n",
        )],
    },
    // 15. package.metadata.halyard override (tag_style).
    CargoWorkspaceSpec {
        name: "metadata-halyard",
        root: "[workspace]\nmembers = [\"crates/*\"]\n",
        members: &[(
            "crates/meta",
            "[package]\nname = \"meta\"\nversion = \"0.1.0\"\n\n[package.metadata.halyard]\ntag_style = \"v{version}\"\n",
        )],
    },
    // 16. Nested workspace → guidance error (asserted separately).
    CargoWorkspaceSpec {
        name: "nested-workspace",
        root: "[workspace]\nmembers = [\"sub/*\"]\n",
        members: &[(
            "sub/inner",
            "[package]\nname = \"inner\"\nversion = \"0.1.0\"\n\n[workspace]\nmembers = [\".\"]\n",
        )],
    },
    // 17. Member without a version (unpublishable + warning; cargo
    // metadata reports 0.0.0).
    CargoWorkspaceSpec {
        name: "member-no-version",
        root: "[workspace]\nmembers = [\"crates/*\"]\n",
        members: &[
            ("crates/nover", "[package]\nname = \"nover\"\n"),
            (
                "crates/withver",
                "[package]\nname = \"withver\"\nversion = \"0.1.0\"\n",
            ),
        ],
    },
    // 18. Root package + workspace (root is a member).
    CargoWorkspaceSpec {
        name: "root-package-workspace",
        root: "[package]\nname = \"rootpkg\"\nversion = \"0.5.0\"\n\n[workspace]\nmembers = [\"crates/*\"]\n",
        members: &[(
            "crates/leaf",
            "[package]\nname = \"leaf\"\nversion = \"0.1.0\"\n\n[dependencies]\nrootpkg = { path = \"../..\" }\n",
        )],
    },
    // 19. Deep glob (`crates/**`).
    CargoWorkspaceSpec {
        name: "deep-glob",
        root: "[workspace]\nmembers = [\"crates/**\"]\n",
        members: &[
            (
                "crates/level0",
                "[package]\nname = \"level0\"\nversion = \"0.1.0\"\n",
            ),
            (
                "crates/nested",
                "[package]\nname = \"nested\"\nversion = \"0.1.0\"\n",
            ),
            (
                "crates/nested/level1",
                "[package]\nname = \"level1\"\nversion = \"0.1.0\"\n",
            ),
        ],
    },
    // 20. Path dependency (versionless) into the workspace.
    CargoWorkspaceSpec {
        name: "path-dep",
        root: "[workspace]\nmembers = [\"crates/*\"]\n",
        members: &[
            (
                "crates/pcore",
                "[package]\nname = \"pcore\"\nversion = \"0.1.0\"\n",
            ),
            (
                "crates/puser",
                "[package]\nname = \"puser\"\nversion = \"0.1.0\"\n\n[dependencies]\npcore = { path = \"../pcore\" }\n",
            ),
        ],
    },
    // 21. default-members present (member set still includes all).
    CargoWorkspaceSpec {
        name: "default-members",
        root: "[workspace]\nmembers = [\"crates/*\"]\ndefault-members = [\"crates/one\"]\n",
        members: &[
            (
                "crates/one",
                "[package]\nname = \"one\"\nversion = \"0.1.0\"\n",
            ),
            (
                "crates/two",
                "[package]\nname = \"two\"\nversion = \"0.1.0\"\n",
            ),
        ],
    },
    // 22. Mixed inheritance: one inherited, one local version.
    CargoWorkspaceSpec {
        name: "mixed-inheritance",
        root: "[workspace]\nmembers = [\"crates/*\"]\n\n[workspace.package]\nversion = \"3.1.4\"\n",
        members: &[
            (
                "crates/inherited",
                "[package]\nname = \"inherited\"\nversion.workspace = true\n",
            ),
            (
                "crates/local",
                "[package]\nname = \"local\"\nversion = \"9.9.9\"\n",
            ),
        ],
    },
    // 23. External registry deps only (no internal edges).
    CargoWorkspaceSpec {
        name: "external-deps",
        root: "[workspace]\nmembers = [\"crates/*\"]\n",
        members: &[(
            "crates/ext",
            "[package]\nname = \"ext\"\nversion = \"0.1.0\"\n\n[dependencies]\nserde = \"1.0\"\nlibc = { version = \"0.2\", optional = true }\n",
        )],
    },
    // 24. Dev-cycle: a dev-depends on b while b depends on a (legal in
    // cargo; §6.6 exemption).
    CargoWorkspaceSpec {
        name: "dev-cycle",
        root: "[workspace]\nmembers = [\"crates/*\"]\n",
        members: &[
            (
                "crates/cyc-a",
                "[package]\nname = \"cyc-a\"\nversion = \"0.1.0\"\n\n[dev-dependencies]\ncyc-b = { path = \"../cyc-b\" }\n",
            ),
            (
                "crates/cyc-b",
                "[package]\nname = \"cyc-b\"\nversion = \"0.1.0\"\n\n[dependencies]\ncyc-a = { path = \"../cyc-a\" }\n",
            ),
        ],
    },
    // 25. Five-crate glob: deterministic ordering + versions.
    CargoWorkspaceSpec {
        name: "glob-five",
        root: "[workspace]\nmembers = [\"crates/*\"]\n",
        members: &[
            (
                "crates/e1",
                "[package]\nname = \"e1\"\nversion = \"1.0.0\"\n",
            ),
            (
                "crates/e2",
                "[package]\nname = \"e2\"\nversion = \"2.0.0\"\n",
            ),
            (
                "crates/e3",
                "[package]\nname = \"e3\"\nversion = \"3.0.0\"\n",
            ),
            (
                "crates/e4",
                "[package]\nname = \"e4\"\nversion = \"4.0.0\"\n",
            ),
            (
                "crates/e5",
                "[package]\nname = \"e5\"\nversion = \"5.0.0\"\n",
            ),
        ],
    },
];

/// Runs discovery on a materialized fixture and compares with `cargo
/// metadata`. Returns the discovery result for extra assertions.
fn differential(spec: &CargoWorkspaceSpec) -> halyard_discover::Discovery {
    let fixture = build_cargo_fixture(spec).unwrap_or_else(|e| unreachable!("fixture builds: {e}"));
    let discovery = halyard_discover::discover_repo(fixture.root(), &DiscoverInput::default())
        .unwrap_or_else(|e| unreachable!("{}: discovery failed: {e}", spec.name));
    let expected = cargo_metadata_expectation(fixture.root())
        .unwrap_or_else(|e| unreachable!("{}: cargo metadata failed: {e}", spec.name));
    let ours = workspace_expectation(&discovery.workspace);
    assert_eq!(
        ours.members, expected.members,
        "{0}: member set mismatch (ours={1:?} cargo={2:?})",
        spec.name, ours.members, expected.members
    );
    assert_eq!(
        ours.edges, expected.edges,
        "{}: internal edge mismatch (ours={:?} cargo={:?})",
        spec.name, ours.edges, expected.edges
    );
    discovery
}

/// One fixture from the table (`unreachable!` keeps the
/// `indexing_slicing` lint intact in the test harness too).
fn fixture(index: usize) -> &'static CargoWorkspaceSpec {
    CARGO_FIXTURES
        .get(index)
        .unwrap_or_else(|| unreachable!("fixture index {index}"))
}

#[test]
fn differential_01_single_pkg() {
    differential(fixture(0));
}

#[test]
fn differential_02_simple_glob() {
    differential(fixture(1));
}

#[test]
fn differential_03_explicit_and_exclude() {
    differential(fixture(2));
}

#[test]
fn differential_04_virtual_workspace() {
    differential(fixture(3));
}

#[test]
fn differential_05_version_inheritance() {
    let discovery = differential(fixture(4));
    for pkg in discovery.workspace.packages() {
        assert!(matches!(
            pkg.version_source,
            halyard_core::package::VersionSource::WorkspaceInherited { .. }
        ));
    }
}

#[test]
fn differential_06_workspace_deps() {
    differential(fixture(5));
}

#[test]
fn differential_07_renames() {
    differential(fixture(6));
}

#[test]
fn differential_08_target_deps() {
    differential(fixture(7));
}

#[test]
fn differential_09_dev_deps() {
    differential(fixture(8));
}

#[test]
fn differential_10_build_deps() {
    differential(fixture(9));
}

#[test]
fn differential_11_optional_dep() {
    let discovery = differential(fixture(10));
    let user = discovery
        .workspace
        .find_by_name("ouser")
        .and_then(|id| discovery.workspace.package(id))
        .expect("ouser");
    assert!(
        user.deps
            .iter()
            .any(|d| matches!(d.kind, halyard_core::package::DepKind::Optional))
    );
}

#[test]
fn differential_12_dep_features() {
    differential(fixture(11));
}

#[test]
fn differential_13_publish_false() {
    let discovery = differential(fixture(12));
    let priv_pkg = discovery
        .workspace
        .find_by_name("priv")
        .and_then(|id| discovery.workspace.package(id))
        .expect("priv");
    assert!(!priv_pkg.publishable);
    let pub_pkg = discovery
        .workspace
        .find_by_name("pub")
        .and_then(|id| discovery.workspace.package(id))
        .expect("pub");
    assert!(pub_pkg.publishable);
}

#[test]
fn differential_14_publish_registry() {
    differential(fixture(13));
}

#[test]
fn differential_15_metadata_halyard() {
    let discovery = differential(fixture(14));
    let meta = discovery
        .workspace
        .find_by_name("meta")
        .and_then(|id| discovery.workspace.package(id))
        .unwrap_or_else(|| unreachable!("meta"));
    assert_eq!(meta.tag_template.as_str(), "v{version}");
}

#[test]
fn differential_16_nested_workspace_errors_with_guidance() {
    let fixture =
        build_cargo_fixture(fixture(15)).unwrap_or_else(|e| unreachable!("fixture builds: {e}"));
    let Err(err) = halyard_discover::discover_repo(fixture.root(), &DiscoverInput::default())
    else {
        unreachable!("nested workspace must error");
    };
    assert_eq!(err.code(), "E-DISC-NESTED-WS");
    assert!(err.to_string().contains("split the repo"));
}

#[test]
fn differential_17_member_no_version_warns_and_unpublishable() {
    let discovery = differential(fixture(16));
    let nover = discovery
        .workspace
        .find_by_name("nover")
        .and_then(|id| discovery.workspace.package(id))
        .expect("nover");
    assert!(nover.version.is_none());
    assert!(!nover.publishable);
    assert!(
        discovery
            .warnings
            .iter()
            .any(|w| w.code == "no-version" && w.subject == "nover")
    );
}

#[test]
fn differential_18_root_package_is_member() {
    let discovery = differential(fixture(17));
    assert!(discovery.workspace.find_by_name("rootpkg").is_some());
    assert!(discovery.workspace.find_by_name("leaf").is_some());
}

#[test]
fn differential_19_deep_glob() {
    differential(fixture(18));
}

#[test]
fn differential_20_path_dep() {
    differential(fixture(19));
}

#[test]
fn differential_21_default_members_still_full_set() {
    differential(fixture(20));
}

#[test]
fn differential_22_mixed_inheritance() {
    let discovery = differential(fixture(21));
    let inherited = discovery
        .workspace
        .find_by_name("inherited")
        .and_then(|id| discovery.workspace.package(id))
        .unwrap_or_else(|| unreachable!("inherited"));
    assert_eq!(
        inherited
            .version
            .as_ref()
            .map(ToString::to_string)
            .as_deref(),
        Some("3.1.4")
    );
    let local = discovery
        .workspace
        .find_by_name("local")
        .and_then(|id| discovery.workspace.package(id))
        .unwrap_or_else(|| unreachable!("local"));
    assert_eq!(
        local.version.as_ref().map(ToString::to_string).as_deref(),
        Some("9.9.9")
    );
}

#[test]
fn differential_23_external_deps_have_no_internal_edges() {
    let discovery = differential(fixture(22));
    let ext = discovery
        .workspace
        .find_by_name("ext")
        .and_then(|id| discovery.workspace.package(id))
        .unwrap_or_else(|| unreachable!("ext"));
    assert!(ext.deps.is_empty(), "external deps stay external");
}

#[test]
fn differential_24_dev_cycle_discovers_and_groups() {
    let discovery = differential(fixture(23));
    let sccs = discovery.graph.sccs();
    assert_eq!(sccs.len(), 1, "dev cycle is one SCC");
    assert_eq!(sccs.first().map_or(0, Vec::len), 2);
    // Topo must NOT fail: the cycle is dev-only (§6.6).
    assert!(discovery.graph.topo_order().is_ok());
}

#[test]
fn differential_25_glob_five_deterministic() {
    let first = differential(fixture(24));
    let second = differential(fixture(24));
    let dump_a = workspace_dump(&first.workspace);
    let dump_b = workspace_dump(&second.workspace);
    assert_eq!(dump_a, dump_b, "identical fixtures -> identical dumps (I3)");
}

/// Deterministic canonical dump of a workspace (I3 checks).
fn workspace_dump(ws: &halyard_core::package::Workspace) -> String {
    let interner = ws.interner();
    let mut rows: Vec<String> = Vec::new();
    for pkg in ws.packages() {
        let name = interner.get(pkg.name).unwrap_or_default();
        let deps: Vec<String> = pkg
            .deps
            .iter()
            .filter_map(|d| {
                ws.package(d.to)
                    .and_then(|t| interner.get(t.name))
                    .map(|n| format!("{n}({:?})", d.kind))
            })
            .collect();
        rows.push(format!(
            "{name}|{}|{}|{}|{}",
            pkg.root.as_str(),
            pkg.version
                .as_ref()
                .map_or("none".to_owned(), ToString::to_string),
            pkg.publishable,
            deps.join(",")
        ));
    }
    rows.join("\n")
}

#[test]
fn determinism_shuffled_insertion_order_is_identical() {
    // Discovery input order (BTreeMap) never leaks into the output: build
    // the ctx from the same file set in two orders and compare dumps.
    let spec = fixture(24);
    let _fixture = build_cargo_fixture(spec).unwrap_or_else(|e| unreachable!("fixture: {e}"));
    let mut ctx_a = halyard_core::discover::DiscoverCtx::new();
    let mut ctx_b = halyard_core::discover::DiscoverCtx::new();
    let mut files: Vec<(String, Vec<u8>)> =
        vec![("Cargo.toml".to_owned(), spec.root.as_bytes().to_vec())];
    for (dir, manifest) in spec.members {
        files.push((format!("{dir}/Cargo.toml"), manifest.as_bytes().to_vec()));
    }
    for (path, bytes) in &files {
        ctx_a.insert_file(path, bytes.clone());
    }
    // Reversed + rotated order for ctx_b.
    let mut rotated = files.clone();
    rotated.rotate_right(1);
    rotated.reverse();
    for (path, bytes) in &rotated {
        ctx_b.insert_file(path, bytes.clone());
    }
    let a = discover_from_ctx(&ctx_a, &DiscoverInput::default()).expect("a");
    let b = discover_from_ctx(&ctx_b, &DiscoverInput::default()).expect("b");
    assert_eq!(workspace_dump(&a.workspace), workspace_dump(&b.workspace));
}

#[test]
fn all_25_fixture_names_are_unique() {
    let names: BTreeSet<&str> = CARGO_FIXTURES.iter().map(|f| f.name).collect();
    assert_eq!(names.len(), CARGO_FIXTURES.len());
    assert_eq!(CARGO_FIXTURES.len(), 25);
}

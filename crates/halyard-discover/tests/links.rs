//! M1.7 polyglot-link end-to-end tests (§6.5, §17): napi-rs, wasm-pack,
//! explicit `[[link]]`, and workspace-path fixtures link the expected
//! pairs with the expected modes through the full orchestrator.

use halyard_core::discover::{DiscoverCtx, DiscoverError};
use halyard_core::link::{LinkMode, LinkRule};
use halyard_discover::discover_repo;
use halyard_discover::{DiscoverInput, discover_from_ctx};
use halyard_test_support::manifest_fixtures::NpmWorkspaceSpec;
use halyard_test_support::manifest_fixtures::build_cargo_fixture;
use halyard_test_support::manifest_fixtures::{CargoWorkspaceSpec, build_npm_fixture};

/// napi-rs layout: cdylib crate + main npm binding + platform
/// sub-packages, all discovered through the orchestrator.
#[test]
fn napi_rs_fixture_links_expected_pairs() {
    let spec = CargoWorkspaceSpec {
        name: "link-napi",
        root: "[workspace]\nmembers = [\"crates/*\"]\n",
        members: &[(
            "crates/native",
            "[package]\nname = \"native\"\nversion = \"0.1.0\"\n\n[lib]\ncrate-type = [\"cdylib\"]\n",
        )],
    };
    let npm = NpmWorkspaceSpec {
        name: "link-napi-npm",
        root_json: Some(r#"{"name":"root","private":true}"#),
        pnpm_workspace: Some("packages:\n  - 'crates/*'\n  - 'crates/*/npm/*'\n"),
        members: &[
            (
                "crates/native",
                r#"{"name":"@org/native","version":"0.1.0","napi":{"binaryName":"native"}}"#,
            ),
            (
                "crates/native/npm/linux-x64-gnu",
                r#"{"name":"@org/native-linux-x64-gnu","version":"0.1.0","private":true,"cpu":["x64"]}"#,
            ),
        ],
    };
    let mut ctx = DiscoverCtx::new();
    for fixture in [
        build_cargo_fixture(&spec).expect("cargo fx"),
        build_npm_fixture(&npm).expect("npm fx"),
    ] {
        let root = fixture.root();
        let walked =
            halyard_discover::collect_ctx(root).unwrap_or_else(|e| unreachable!("walk: {e}"));
        for path in walked.tree().map(str::to_owned) {
            let bytes =
                std::fs::read(root.join(&path)).unwrap_or_else(|e| unreachable!("read back: {e}"));
            assert!(ctx.insert_file(&path, bytes), "{path}");
        }
    }
    let d = discover_from_ctx(&ctx, &DiscoverInput::default())
        .unwrap_or_else(|e| unreachable!("discovery: {e}"));
    let id = |name: &str| {
        d.workspace
            .find_by_name(name)
            .unwrap_or_else(|| unreachable!("{name}"))
    };
    let links = d.workspace.links();
    assert_eq!(links.len(), 2, "crate→binding and binding→platform");
    assert!(links.contains(&halyard_core::link::LinkEdge {
        from: id("native"),
        to: id("@org/native"),
        mode: LinkMode::Lockstep,
        rule: LinkRule::NapiRs,
    }));
    assert!(links.contains(&halyard_core::link::LinkEdge {
        from: id("@org/native"),
        to: id("@org/native-linux-x64-gnu"),
        mode: LinkMode::Lockstep,
        rule: LinkRule::NapiRs,
    }));
}

/// wasm-pack layout: wasm-bindgen crate with a package.json beside it.
#[test]
fn wasm_pack_fixture_links_expected_pair() {
    let spec = CargoWorkspaceSpec {
        name: "link-wasm",
        root: "[workspace]\nmembers = [\"crates/*\"]\n",
        members: &[(
            "crates/engine",
            "[package]\nname = \"engine\"\nversion = \"0.2.0\"\n\n[dependencies]\nwasm-bindgen = \"0.2\"\n",
        )],
    };
    let npm = NpmWorkspaceSpec {
        name: "link-wasm-npm",
        root_json: Some(r#"{"name":"root","private":true}"#),
        pnpm_workspace: Some("packages:\n  - 'crates/*'\n"),
        members: &[(
            "crates/engine",
            r#"{"name":"engine-js","version":"0.2.0","files":["pkg"]}"#,
        )],
    };
    let mut ctx = DiscoverCtx::new();
    for fixture in [
        build_cargo_fixture(&spec).expect("cargo fx"),
        build_npm_fixture(&npm).expect("npm fx"),
    ] {
        let root = fixture.root();
        let walked =
            halyard_discover::collect_ctx(root).unwrap_or_else(|e| unreachable!("walk: {e}"));
        for path in walked.tree().map(str::to_owned) {
            let bytes =
                std::fs::read(root.join(&path)).unwrap_or_else(|e| unreachable!("read back: {e}"));
            ctx.insert_file(&path, bytes);
        }
    }
    let d = discover_from_ctx(&ctx, &DiscoverInput::default())
        .unwrap_or_else(|e| unreachable!("discovery: {e}"));
    let links = d.workspace.links();
    assert_eq!(links.len(), 1);
    let link = links.first().unwrap_or_else(|| unreachable!("one link"));
    assert_eq!(link.rule, LinkRule::WasmPack);
    assert_eq!(link.mode, LinkMode::Lockstep);
    let from = d
        .workspace
        .package(link.from)
        .unwrap_or_else(|| unreachable!("from"));
    let to = d
        .workspace
        .package(link.to)
        .unwrap_or_else(|| unreachable!("to"));
    assert_eq!(d.workspace.interner().get(from.name), Some("engine"));
    assert_eq!(d.workspace.interner().get(to.name), Some("engine-js"));
}

/// Explicit `[[link]]` (config layer mapped onto `ExplicitLinkSpec`).
#[test]
fn explicit_link_fixture_uses_configured_mode() {
    let spec = NpmWorkspaceSpec {
        name: "link-explicit",
        root_json: Some(r#"{"name":"root","private":true}"#),
        pnpm_workspace: Some("packages:\n  - 'pkgs/*'\n"),
        members: &[
            ("pkgs/one", r#"{"name":"one","version":"0.1.0"}"#),
            ("pkgs/two", r#"{"name":"two","version":"0.1.0"}"#),
        ],
    };
    let fixture = build_npm_fixture(&spec).unwrap_or_else(|e| unreachable!("fixture: {e}"));
    let input = DiscoverInput {
        links: vec![halyard_core::link::ExplicitLinkSpec {
            from: "one".to_owned(),
            to: "two".to_owned(),
            mode: Some(LinkMode::Lockstep),
        }],
    };
    let d =
        discover_repo(fixture.root(), &input).unwrap_or_else(|e| unreachable!("discovery: {e}"));
    let links = d.workspace.links();
    assert_eq!(links.len(), 1);
    let link = links.first().unwrap_or_else(|| unreachable!("one link"));
    assert_eq!(link.rule, LinkRule::Explicit);
    assert_eq!(link.mode, LinkMode::Lockstep);
}

/// Unknown `[[link]]` name errors with the stable code.
#[test]
fn explicit_link_unknown_name_errors() {
    let spec = NpmWorkspaceSpec {
        name: "link-unknown",
        root_json: Some(r#"{"name":"root","private":true}"#),
        pnpm_workspace: Some("packages:\n  - 'pkgs/*'\n"),
        members: &[("pkgs/one", r#"{"name":"one","version":"0.1.0"}"#)],
    };
    let fixture = build_npm_fixture(&spec).unwrap_or_else(|e| unreachable!("fixture: {e}"));
    let input = DiscoverInput {
        links: vec![halyard_core::link::ExplicitLinkSpec {
            from: "one".to_owned(),
            to: "ghost".to_owned(),
            mode: None,
        }],
    };
    let err = discover_repo(fixture.root(), &input).expect_err("unknown link");
    assert!(matches!(err, DiscoverError::UnknownLink { .. }));
    assert_eq!(err.code(), "E-DISC-LINK-UNKNOWN");
}

/// workspace-path rule: a `workspace:` dep onto a co-located crate
/// binding links the crate to the binding with the cascade default.
#[test]
fn workspace_path_fixture_links_colocation() {
    let spec = CargoWorkspaceSpec {
        name: "link-wsp",
        root: "[workspace]\nmembers = [\"crates/*\"]\n",
        members: &[(
            "crates/core",
            "[package]\nname = \"core\"\nversion = \"0.3.0\"\n",
        )],
    };
    let npm = NpmWorkspaceSpec {
        name: "link-wsp-npm",
        root_json: Some(r#"{"name":"root","private":true}"#),
        pnpm_workspace: Some("packages:\n  - 'crates/*'\n  - 'apps/*'\n"),
        members: &[
            ("crates/core", r#"{"name":"@org/core","version":"0.3.0"}"#),
            (
                "apps/cli",
                r#"{"name":"cli","version":"0.1.0","dependencies":{"@org/core":"workspace:^"}}"#,
            ),
        ],
    };
    let mut ctx = DiscoverCtx::new();
    for fixture in [
        build_cargo_fixture(&spec).expect("cargo fx"),
        build_npm_fixture(&npm).expect("npm fx"),
    ] {
        let root = fixture.root();
        let walked =
            halyard_discover::collect_ctx(root).unwrap_or_else(|e| unreachable!("walk: {e}"));
        for path in walked.tree().map(str::to_owned) {
            let bytes =
                std::fs::read(root.join(&path)).unwrap_or_else(|e| unreachable!("read back: {e}"));
            ctx.insert_file(&path, bytes);
        }
    }
    let d = discover_from_ctx(&ctx, &DiscoverInput::default())
        .unwrap_or_else(|e| unreachable!("discovery: {e}"));
    let id = |name: &str| {
        d.workspace
            .find_by_name(name)
            .unwrap_or_else(|| unreachable!("{name}"))
    };
    let links = d.workspace.links();
    assert_eq!(links.len(), 1);
    let link = links.first().unwrap_or_else(|| unreachable!("one link"));
    assert_eq!(link.rule, LinkRule::WorkspacePath);
    assert_eq!(link.mode, LinkMode::Cascade);
    assert_eq!(link.from, id("core"));
    assert_eq!(link.to, id("@org/core"));
}

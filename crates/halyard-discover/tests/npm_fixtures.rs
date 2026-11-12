//! M1.6 fixture tests: npm/pnpm/yarn/bun workspaces, protocols,
//! catalogs, negations, private/publishConfig (§6.4, §17).
//!
//! The corpus differential against `pnpm m ls --json --depth=-1` runs via
//! `xtask discover-diff --corpus npm` when pnpm + the corpus are present
//! (skipped honestly otherwise); these fixtures pin the structural
//! semantics the differential needs.

use halyard_core::discover::DiscoverCtx;
use halyard_core::package::{DepKind, ReqSpec, WorkspaceProtocol};
use halyard_discover::{discover_from_ctx, DiscoverInput};
use halyard_test_support::manifest_fixtures::{build_npm_fixture, NpmWorkspaceSpec};

static NPM_FIXTURES: &[NpmWorkspaceSpec] = &[
    // 1. npm workspaces (array form) + root member.
    NpmWorkspaceSpec {
        name: "npm-array",
        root_json: Some(
            r#"{"name":"root","version":"1.0.0","private":true,"workspaces":["packages/*"]}"#,
        ),
        pnpm_workspace: None,
        members: &[(
            "packages/app",
            r#"{"name":"app","version":"0.1.0","dependencies":{"root":"^1.0.0"}}"#,
        )],
    },
    // 2. yarn object form.
    NpmWorkspaceSpec {
        name: "yarn-object",
        root_json: Some(r#"{"name":"root","version":"1.0.0","workspaces":{"packages":["apps/*"],"nohoist":["**/mobile"]}}"#),
        pnpm_workspace: None,
        members: &[(
            "apps/web",
            r#"{"name":"web","version":"2.0.0"}"#,
        )],
    },
    // 3. pnpm globs with negations.
    NpmWorkspaceSpec {
        name: "pnpm-negations",
        root_json: Some(r#"{"name":"root","private":true}"#),
        pnpm_workspace: Some("packages:\n  - 'pkgs/*'\n  - '!pkgs/secret'\n"),
        members: &[
            ("pkgs/keep", r#"{"name":"keep","version":"0.1.0"}"#),
            ("pkgs/secret", r#"{"name":"secret","version":"0.1.0","private":true}"#),
        ],
    },
    // 4. pnpm catalogs (default + named) with both protocols.
    NpmWorkspaceSpec {
        name: "pnpm-catalogs",
        root_json: Some(r#"{"name":"root","private":true}"#),
        pnpm_workspace: Some("packages:\n  - 'pkgs/*'\ncatalog:\n  react: ^18.0.0\ncatalogs:\n  next:\n    react: ^19.0.0\n"),
        members: &[(
            "pkgs/ui",
            r#"{"name":"ui","version":"0.2.0","dependencies":{"react":"catalog:","preact":"catalog:next"}}"#,
        )],
    },
    // 5. All workspace: protocols.
    NpmWorkspaceSpec {
        name: "workspace-protocols",
        root_json: Some(r#"{"name":"root","workspaces":["pkgs/*"]}"#),
        pnpm_workspace: None,
        members: &[(
            "pkgs/proto",
            r#"{"name":"proto","version":"1.0.0","dependencies":{"a":"workspace:*","b":"workspace:^","c":"workspace:~","d":"workspace:1.2.3"}}"#,
        )],
    },
    // 6. file: / link: protocols.
    NpmWorkspaceSpec {
        name: "file-link",
        root_json: Some(r#"{"name":"root","workspaces":["pkgs/*"]}"#),
        pnpm_workspace: None,
        members: &[(
            "pkgs/user",
            r#"{"name":"user","version":"1.0.0","dependencies":{"filed":"file:../filed","linked":"link:../linked"}}"#,
        )],
    },
    // 7. private + publishConfig are read, not interpreted.
    NpmWorkspaceSpec {
        name: "private-publishconfig",
        root_json: Some(r#"{"name":"root","workspaces":["pkgs/*"]}"#),
        pnpm_workspace: None,
        members: &[(
            "pkgs/priv",
            r#"{"name":"priv","version":"0.1.0","private":true,"publishConfig":{"access":"public","provenance":true},"files":["dist"]}"#,
        )],
    },
    // 8. napi facet via key and via @napi-rs/cli dep.
    NpmWorkspaceSpec {
        name: "napi-facet",
        root_json: Some(r#"{"name":"root","workspaces":["pkgs/*"]}"#),
        pnpm_workspace: None,
        members: &[
            ("pkgs/keyed", r#"{"name":"keyed","version":"1.0.0","napi":{"binaryName":"keyed"}}"#),
            ("pkgs/depped", r#"{"name":"depped","version":"1.0.0","devDependencies":{"@napi-rs/cli":"^2.0.0"}}"#),
            ("pkgs/plain", r#"{"name":"plain","version":"1.0.0"}"#),
        ],
    },
    // 9. bun-style workspaces (same field as npm).
    NpmWorkspaceSpec {
        name: "bun-workspaces",
        root_json: Some(r#"{"name":"root","workspaces":["pkg/*"]}"#),
        pnpm_workspace: None,
        members: &[(
            "pkg/bun-pkg",
            r#"{"name":"bun-pkg","version":"0.3.0"}"#,
        )],
    },
    // 10. peerDependencies and devDependencies protocols.
    NpmWorkspaceSpec {
        name: "dep-fields",
        root_json: Some(r#"{"name":"root","workspaces":["pkgs/*"]}"#),
        pnpm_workspace: None,
        members: &[(
            "pkgs/all",
            r#"{"name":"all","version":"1.0.0","dependencies":{"dep":"workspace:*"},"devDependencies":{"dev":"workspace:*"},"peerDependencies":{"peer":"workspace:^"},"optionalDependencies":{"opt":"workspace:~"}}"#,
        )],
    },
];

fn discover(spec: &NpmWorkspaceSpec) -> halyard_discover::Discovery {
    let fixture = build_npm_fixture(spec).expect("npm fixture builds");
    halyard_discover::discover_repo(fixture.root(), &DiscoverInput::default())
        .unwrap_or_else(|e| panic!("{}: discovery failed: {e}", spec.name))
}

fn root_names(discovery: &halyard_discover::Discovery) -> Vec<String> {
    let interner = discovery.workspace.interner();
    discovery
        .workspace
        .packages()
        .iter()
        .map(|p| interner.get(p.name).unwrap_or_default().to_owned())
        .collect()
}

#[test]
fn npm_array_includes_root_and_members() {
    let d = discover(&NPM_FIXTURES[0]);
    assert_eq!(root_names(&d), vec!["root", "app"]);
}

#[test]
fn yarn_object_form_workspaces() {
    let d = discover(&NPM_FIXTURES[1]);
    assert_eq!(root_names(&d), vec!["root", "web"]);
}

#[test]
fn pnpm_negations_exclude_members() {
    let d = discover(&NPM_FIXTURES[2]);
    assert_eq!(root_names(&d), vec!["root", "keep"]);
    assert!(!root_names(&d).contains(&"secret".to_owned()));
}

#[test]
fn pnpm_catalogs_both_protocols() {
    let d = discover(&NPM_FIXTURES[3]);
    let ui = d
        .workspace
        .find_by_name("ui")
        .and_then(|id| d.workspace.package(id))
        .expect("ui");
    assert!(matches!(ui.deps[0].req, ReqSpec::Catalog(None)));
    assert!(matches!(ui.deps[1].req, ReqSpec::Catalog(Some(ref n)) if n == b"next"));
    // Merged DepEdges exist only for proven-internal rows, so an edge at
    // all is the internal marker (registry deps never become edges).
}

#[test]
fn all_workspace_protocols_map() {
    let d = discover(&NPM_FIXTURES[4]);
    let proto = d
        .workspace
        .find_by_name("proto")
        .and_then(|id| d.workspace.package(id))
        .expect("proto");
    let expect = [
        ("a", WorkspaceProtocol::Star),
        ("b", WorkspaceProtocol::Caret),
        ("c", WorkspaceProtocol::Tilde),
        ("d", WorkspaceProtocol::Exact),
    ];
    assert_eq!(proto.deps.len(), 4);
    for (dep, (name, protocol)) in proto.deps.iter().zip(expect) {
        assert_eq!(d.workspace.interner().get(
            d.workspace.package(dep.to).expect("target").name
        ), Some(name), "dep target {name}");
        assert!(matches!(dep.req, ReqSpec::WorkspaceProtocol(p) if p == protocol));
    }
}

#[test]
fn file_and_link_protocols() {
    let d = discover(&NPM_FIXTURES[5]);
    let user = d
        .workspace
        .find_by_name("user")
        .and_then(|id| d.workspace.package(id))
        .expect("user");
    assert!(user.deps.iter().all(|dep| matches!(dep.req, ReqSpec::PathOnly)));
}

#[test]
fn private_is_unpublishable_publishconfig_read() {
    let d = discover(&NPM_FIXTURES[6]);
    let priv_pkg = d
        .workspace
        .find_by_name("priv")
        .and_then(|id| d.workspace.package(id))
        .expect("priv");
    assert!(!priv_pkg.publishable);
    // publishConfig must not break parsing or alter the package row.
    assert!(priv_pkg.version.is_some());
}

#[test]
fn napi_facet_detected_both_ways() {
    let d = discover(&NPM_FIXTURES[7]);
    let facet = |name: &str| {
        d.workspace
            .find_by_name(name)
            .and_then(|id| d.workspace.package(id))
            .map(|p| p.facets.napi)
            .unwrap_or_default()
    };
    assert!(facet("keyed"));
    assert!(facet("depped"));
    assert!(!facet("plain"));
}

#[test]
fn bun_workspaces_same_field() {
    let d = discover(&NPM_FIXTURES[8]);
    assert_eq!(root_names(&d), vec!["root", "bun-pkg"]);
}

#[test]
fn dep_kinds_map_to_npm_fields() {
    let d = discover(&NPM_FIXTURES[9]);
    let all = d
        .workspace
        .find_by_name("all")
        .and_then(|id| d.workspace.package(id))
        .expect("all");
    let kinds: Vec<(&str, DepKind)> = all
        .deps
        .iter()
        .map(|dep| {
            let name = d
                .workspace
                .package(dep.to)
                .and_then(|t| d.workspace.interner().get(t.name))
                .unwrap_or_default();
            (name, dep.kind)
        })
        .collect();
    assert!(kinds.contains(&("dep", DepKind::Normal)));
    assert!(kinds.contains(&("dev", DepKind::Dev)));
    assert!(kinds.contains(&("peer", DepKind::Peer)));
    assert!(kinds.contains(&("opt", DepKind::Optional)));
}

#[test]
fn npm_determinism_two_runs_identical() {
    let a = discover(&NPM_FIXTURES[9]);
    let b = discover(&NPM_FIXTURES[9]);
    let dump = |d: &halyard_discover::Discovery| {
        let interner = d.workspace.interner();
        d.workspace
            .packages()
            .iter()
            .map(|p| {
                format!(
                    "{}|{}|{}",
                    interner.get(p.name).unwrap_or_default(),
                    p.version.as_ref().map_or("none".into(), |v| v.to_string()),
                    p.publishable
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    assert_eq!(dump(&a), dump(&b));
}

#[test]
fn ctx_insertion_order_is_irrelevant_for_npm() {
    let spec = &NPM_FIXTURES[9];
    let mut ctx_a = DiscoverCtx::new();
    let mut ctx_b = DiscoverCtx::new();
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    if let Some(root) = spec.root_json {
        files.push(("package.json".to_owned(), root.as_bytes().to_vec()));
    }
    if let Some(yaml) = spec.pnpm_workspace {
        files.push(("pnpm-workspace.yaml".to_owned(), yaml.as_bytes().to_vec()));
    }
    for (dir, json) in spec.members {
        files.push((format!("{dir}/package.json"), json.as_bytes().to_vec()));
    }
    for (path, bytes) in &files {
        ctx_a.insert_file(path, bytes.clone());
    }
    let mut rev = files.clone();
    rev.reverse();
    for (path, bytes) in &rev {
        ctx_b.insert_file(path, bytes.clone());
    }
    let a = discover_from_ctx(&ctx_a, &DiscoverInput::default()).expect("a");
    let b = discover_from_ctx(&ctx_b, &DiscoverInput::default()).expect("b");
    assert_eq!(root_names(&a), root_names(&b));
}

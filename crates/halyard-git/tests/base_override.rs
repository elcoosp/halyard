//! The affected-package selector's git half: the `base_override` walk
//! option (§16.3/M10, `halyard affected --base <ref>`). One explicitly
//! named ref replaces **every** package's release base for a single
//! walk, so the selector can attribute commits against any ref (the CI
//! selection case) rather than only against the release bases the tag
//! templates resolve. Fail-closed on an unresolvable ref (`E-GIT-REF`
//! naming the ref); the default release-base path is unchanged and
//! pinned by the control test.

use halyard_core::HistoryProvider;
use halyard_core::PkgId;
use halyard_core::package::{
    DepEdge, Ecosystem, GroupId, Package, PathFilter, RelPath, TagTemplate, VersionSource,
    Workspace,
};
use halyard_git::GitHistoryOptions;
use halyard_test_support::fixture::GitFixture;

type BoxError = Box<dyn std::error::Error>;

/// One Cargo package `app` rooted at `crates/app`.
fn single_package_workspace() -> Workspace {
    let mut ws = Workspace::new();
    let name_sym = ws.intern("app");
    ws.push_package(Package {
        id: PkgId::FIRST,
        name: name_sym,
        ecosystem: Ecosystem::Cargo,
        root: RelPath::new("crates/app")
            .unwrap_or_else(|| unreachable!("the package root is a valid RelPath")),
        manifest: RelPath::new("crates/app/Cargo.toml")
            .unwrap_or_else(|| unreachable!("the manifest path is a valid RelPath")),
        version: Some(semver::Version::new(0, 1, 0)),
        version_source: VersionSource::Local,
        publishable: true,
        deps: Vec::<DepEdge>::new(),
        tag_template: TagTemplate::parse("{name}-v{version}")
            .unwrap_or_else(|e| unreachable!("fixture templates parse: {e}")),
        changelog: None,
        version_site: None,
        include: PathFilter::new(Vec::new(), Vec::new()),
        group: GroupId::new(0),
        facets: halyard_core::Facets::default(),
    });
    ws
}

/// Two commits: the scaffold (tagged `app-v0.1.0`) and one later `app`
/// commit, with the branch `origin/main` pinned at the scaffold.
fn fixture(label: &str) -> Result<GitFixture, BoxError> {
    let mut fx = GitFixture::new(label)?;
    fx.write(
        "crates/app/Cargo.toml",
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\n",
    )?;
    fx.stage_all()?;
    fx.commit("feat(app): initial")?;
    fx.tag_annotated("app-v0.1.0", "app 0.1.0")?;
    fx.branch("origin/main", None)?;
    fx.write("crates/app/src/lib.rs", "pub const C: usize = 1;\n")?;
    fx.stage_all()?;
    fx.commit("feat(app): later work")?;
    Ok(fx)
}

#[test]
fn base_override_walks_from_the_given_ref() -> Result<(), BoxError> {
    let fx = fixture("git-base-override-selects")?;
    let ws = single_package_workspace();
    let history = halyard_git::GitHistory::build_with_options(
        fx.root(),
        &ws,
        GitHistoryOptions {
            base_override: Some("origin/main".to_owned()),
            ..GitHistoryOptions::default()
        },
    )?;
    let digest = history.digest();
    let base = digest
        .base_of(PkgId::FIRST)
        .expect("the override names every package's base");
    assert_eq!(base.tag.to_vec(), b"origin/main".to_vec());
    assert_eq!(
        base.oid.as_bytes(),
        fx.resolve("origin/main")?.as_bytes(),
        "the base commit is the named ref's target"
    );
    let pending = digest.commits_for(PkgId::FIRST);
    assert_eq!(
        pending.len(),
        1,
        "only the commit after the override base is attributed"
    );
    let attributed = pending
        .first()
        .and_then(|idx| history.digest().commits.get(*idx))
        .expect("the digest indexes the package's own commit");
    assert!(
        attributed
            .subject
            .starts_with(&b"feat(app): later work"[..]),
        "the attributed commit is the one after the base"
    );
    Ok(())
}

#[test]
fn base_override_unknown_ref_fails_closed() -> Result<(), BoxError> {
    let fx = fixture("git-base-override-unknown")?;
    let ws = single_package_workspace();
    let error = halyard_git::GitHistory::build_with_options(
        fx.root(),
        &ws,
        GitHistoryOptions {
            base_override: Some("does-not-exist".to_owned()),
            ..GitHistoryOptions::default()
        },
    )
    .expect_err("an unresolvable ref refuses the walk");
    assert_eq!(error.code(), "E-GIT-REF");
    assert!(
        error.to_string().contains("does-not-exist"),
        "the refusal names the ref: {error}"
    );
    Ok(())
}

#[test]
fn the_default_release_base_path_is_unchanged() -> Result<(), BoxError> {
    let fx = fixture("git-base-override-control")?;
    let ws = single_package_workspace();
    let history =
        halyard_git::GitHistory::build_with_options(fx.root(), &ws, GitHistoryOptions::default())?;
    let digest = history.digest();
    let base = digest
        .base_of(PkgId::FIRST)
        .expect("the release tag resolves");
    assert_eq!(
        base.tag.to_vec(),
        b"app-v0.1.0".to_vec(),
        "no override means the §8.2 tag-template rule governs"
    );
    assert_eq!(
        digest.commits_for(PkgId::FIRST).len(),
        1,
        "the pending window is the same shape as the override's in this fixture"
    );
    Ok(())
}

#[test]
fn the_override_run_is_deterministic() -> Result<(), BoxError> {
    let fx = fixture("git-base-override-determinism")?;
    let ws = single_package_workspace();
    let build = || {
        halyard_git::GitHistory::build_with_options(
            fx.root(),
            &ws,
            GitHistoryOptions {
                base_override: Some("origin/main".to_owned()),
                ..GitHistoryOptions::default()
            },
        )
    };
    let first = build()?;
    let second = build()?;
    assert_eq!(
        format!("{:?}", first.digest()),
        format!("{:?}", second.digest()),
        "I3: the same repo and the same ref produce the same digest"
    );
    Ok(())
}

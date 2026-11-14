//! The §8.8 smart-minimal-fetch integration tests: the fetch planner
//! (pure) and the live fetch effect, exercised against real `git`-built
//! fixtures with `git` ground truth — the same harness the tag-write and
//! tag-push suites use. Pins the contracts the shell depends on:
//!
//! * **§8.8 step 1** — per-package latest tag selected from a tag→target
//!   map (the remote listing or the local refs) through the package's
//!   tag template; foreign templates and invalid versions are ignored;
//! * **§8.8 step 2** — one minimal fetch: `--filter=blob:none --no-tags`,
//!   `--shallow-exclude` per requested tag, explicit create-only tag
//!   refspecs plus the base branch;
//! * **§8.8 step 3** — a failing minimal fetch is retried once as the
//!   documented full fetch of the same refs (no filter, no exclude);
//! * **I1 idempotence** — a tag already present locally at the probed
//!   target is not requested again; the re-run fetches nothing;
//! * **fail-closed** — a divergent local release tag is never moved
//!   (E-GIT-TAG-CONFLICT); an unreachable remote is `E-GIT-FETCH`;
//! * **I3 determinism** — the report (selection, rows, tallies, argv) is
//!   identical across independent repositories.

use std::collections::BTreeMap;

use halyard_core::PkgId;
use halyard_core::package::{
    DepEdge, Ecosystem, GroupId, Package, PathFilter, RelPath, TagTemplate, VersionSource,
    Workspace,
};
use halyard_core::planner::PlanConfig;
use halyard_git::fetch::{
    FetchCommand, FetchRow, GixTagFetcher, TagFetcher, deepen_boundaries_for, fetch_attempts,
    latest_tags_for, plan_fetch,
};
use halyard_test_support::fixture::GitFixture;

type BoxError = Box<dyn std::error::Error>;

/// Builds a two-package workspace (the shape the shell drives): `app`
/// (template `v{version}`) and `lib` (template `lib-v{version}`), both
/// Cargo packages at the repo root.
fn two_package_workspace() -> Workspace {
    let mut ws = Workspace::new();
    let specs = [("app", "v{version}"), ("lib", "lib-v{version}")];
    for (name, template) in specs {
        let name_sym = ws.intern(name);
        ws.push_package(Package {
            id: PkgId::FIRST,
            name: name_sym,
            ecosystem: Ecosystem::Cargo,
            root: RelPath::new("")
                .unwrap_or_else(|| unreachable!("the empty root is a valid RelPath")),
            manifest: RelPath::new(&format!("{name}/Cargo.toml"))
                .unwrap_or_else(|| unreachable!("the manifest path is a valid RelPath")),
            version: Some(semver::Version::new(0, 1, 0)),
            version_source: VersionSource::Local,
            publishable: true,
            deps: Vec::<DepEdge>::new(),
            tag_template: TagTemplate::parse(template)
                .unwrap_or_else(|e| unreachable!("fixture templates parse: {e}")),
            changelog: None,
            version_site: None,
            include: PathFilter::new(Vec::new(), Vec::new()),
            group: GroupId::new(0),
            facets: halyard_core::Facets::default(),
        });
    }
    ws
}

/// Builds an "origin" fixture with two commits and two annotated release
/// tags (`v0.1.0` at the first, `v0.2.0` at HEAD).
fn origin_fixture(label: &str) -> Result<GitFixture, BoxError> {
    let mut fx = GitFixture::new(label)?;
    fx.write(
        "app/Cargo.toml",
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\n",
    )?;
    fx.stage_all()?;
    fx.commit("feat(app): initial")?;
    fx.tag_annotated("v0.1.0", "app 0.1.0")?;
    fx.write(
        "app/Cargo.toml",
        "[package]\nname = \"app\"\nversion = \"0.2.0\"\n",
    )?;
    fx.stage_all()?;
    fx.commit("feat(app): next")?;
    fx.tag_annotated("v0.2.0", "app 0.2.0")?;
    Ok(fx)
}

/// A full clone of `origin` that carries **no tags** (the CI checkout
/// shape §8.8 exists for): branches fetched, tags never followed.
fn tagless_clone(origin: &GitFixture, label: &str) -> Result<GitFixture, BoxError> {
    let clone = GitFixture::new(label)?;
    clone.run_git(&["remote", "add", "origin", origin.file_uri().as_str()])?;
    clone.run_git(&[
        "fetch",
        "--quiet",
        "--no-tags",
        "origin",
        "+refs/heads/*:refs/remotes/origin/*",
    ])?;
    clone.run_git(&["reset", "--hard", "origin/main"])?;
    Ok(clone)
}

/// The remote tag→target map of `fixture` as git reports it (the probe
/// shape the live fetcher parses).
#[test]
fn fetch_selection_rides_the_probed_listing() -> Result<(), BoxError> {
    // The selection is computed from the probed listing, not from local
    // refs: a tagless clone still selects the remote's latest tags (the
    // whole §8.8 point), while a tagful clone selects nothing new.
    let origin = origin_fixture("fetch-probe-origin")?;
    let targets = {
        let mut map = BTreeMap::new();
        let output = origin.run_git(&["ls-remote", "--tags", origin.file_uri().as_str()])?;
        for line in output.lines() {
            let Some((oid, reference)) = line.split_once('\t') else {
                continue;
            };
            let Some(name) = reference.strip_prefix("refs/tags/") else {
                continue;
            };
            let name = name.strip_suffix("^{}").unwrap_or(name);
            map.insert(name.to_owned(), oid.trim().to_owned());
        }
        map
    };
    let ws = two_package_workspace();
    let selection = latest_tags_for(&ws, &PlanConfig::default_values(), &targets);
    assert_eq!(selection.get("app").map(String::as_str), Some("v0.2.0"));
    assert_eq!(selection.len(), 1, "lib has no tags on the remote");
    Ok(())
}

#[test]
fn plan_fetch_pins_the_minimal_argv() {
    // §8.8 step 2, shallow shape: `--filter=blob:none --no-tags`, one
    // `--shallow-exclude` per deepen boundary (the previous release
    // tags), the base branch first, then explicit create-only tag
    // refspecs. The argv is pinned byte-exact — the fetch effect and
    // the docs pages both derive from this shape.
    let mut selection = BTreeMap::new();
    selection.insert("app".to_owned(), "v0.2.0".to_owned());
    selection.insert("lib".to_owned(), "lib-v1.3.0".to_owned());
    let boundaries = ["v0.1.0".to_owned(), "lib-v1.2.0".to_owned()];
    let plan = plan_fetch("origin", Some("main"), &selection, &boundaries, true);
    assert_eq!(plan.remote, "origin");
    assert_eq!(plan.base_branch.as_deref(), Some("main"));
    assert_eq!(plan.tags, vec!["lib-v1.3.0", "v0.2.0"]);
    assert_eq!(plan.shallow_exclude, vec!["lib-v1.2.0", "v0.1.0"]);
    assert_eq!(
        plan.argv,
        vec![
            "fetch",
            "--filter=blob:none",
            "--no-tags",
            "--shallow-exclude=lib-v1.2.0",
            "--shallow-exclude=v0.1.0",
            "origin",
            "main",
            "refs/tags/lib-v1.3.0:refs/tags/lib-v1.3.0",
            "refs/tags/v0.2.0:refs/tags/v0.2.0",
        ]
    );
}

#[test]
fn plan_fetch_omits_the_shallow_machinery_on_complete_repositories() {
    // On a complete clone the shallow-exclude machinery is omitted even
    // when boundaries exist: git fails the whole fetch with `no commits
    // selected for shallow requests` when there is nothing to deepen
    // (probed empirically, ADR-0016).
    let mut selection = BTreeMap::new();
    selection.insert("app".to_owned(), "v0.2.0".to_owned());
    let boundaries = ["v0.1.0".to_owned()];
    let plan = plan_fetch("origin", Some("main"), &selection, &boundaries, false);
    assert_eq!(plan.shallow_exclude, Vec::<String>::new());
    assert_eq!(
        plan.argv,
        vec![
            "fetch",
            "--filter=blob:none",
            "--no-tags",
            "origin",
            "main",
            "refs/tags/v0.2.0:refs/tags/v0.2.0",
        ]
    );
}

#[test]
fn plan_fetch_without_a_base_or_tags_is_well_formed() {
    // A package-less selection with no base branch composes the flag-only
    // fetch (the effect skips running it — there is nothing to request).
    let plan = plan_fetch("origin", None, &BTreeMap::new(), &[], false);
    assert_eq!(plan.tags, Vec::<String>::new());
    assert_eq!(plan.refspecs, Vec::<String>::new());
    assert_eq!(
        plan.argv,
        vec!["fetch", "--filter=blob:none", "--no-tags", "origin"]
    );
}

#[test]
fn fetch_attempts_falls_back_to_a_full_fetch() {
    // §8.8 step 3: the primary minimal fetch is retried once as the
    // documented full fetch of the same refs — no filter, no
    // shallow-exclude, same explicit refspecs.
    let attempts = fetch_attempts(
        "origin",
        Some("main"),
        &["v0.2.0".to_owned()],
        &["v0.1.0".to_owned()],
        true,
    );
    assert_eq!(attempts.len(), 2);
    assert_eq!(
        attempts.first().map(|a| a.command),
        Some(FetchCommand::Minimal)
    );
    assert_eq!(
        attempts.first().map(|a| a.argv.as_slice()),
        Some(
            [
                "fetch",
                "--filter=blob:none",
                "--no-tags",
                "--shallow-exclude=v0.1.0",
                "origin",
                "main",
                "refs/tags/v0.2.0:refs/tags/v0.2.0",
            ]
            .map(String::from)
            .as_slice()
        )
    );
    assert_eq!(attempts.get(1).map(|a| a.command), Some(FetchCommand::Full));
    assert_eq!(
        attempts.get(1).map(|a| a.argv.as_slice()),
        Some(
            [
                "fetch",
                "--no-tags",
                "origin",
                "main",
                "refs/tags/v0.2.0:refs/tags/v0.2.0",
            ]
            .map(String::from)
            .as_slice()
        )
    );
}

#[test]
fn fetch_attempts_without_tags_fetch_only_the_base() {
    // The tags-only selection of an empty remote listing leaves the base
    // branch as the only refspec (both attempts stay well-formed; the
    // complete-repository shape carries no shallow machinery).
    let attempts = fetch_attempts("origin", Some("main"), &[], &[], false);
    assert_eq!(attempts.len(), 2);
    assert_eq!(
        attempts.first().map(|a| a.argv.as_slice()),
        Some(
            ["fetch", "--filter=blob:none", "--no-tags", "origin", "main"]
                .map(String::from)
                .as_slice()
        )
    );
}

#[test]
fn deepen_boundaries_name_the_previous_release_per_package() {
    // The §8.8 deepen boundary is each package's previous release tag:
    // strictly below the selected latest, so the boundary never swallows
    // the request; single-tag packages contribute nothing.
    let ws = two_package_workspace();
    let targets: BTreeMap<String, String> = [
        ("v0.1.0", "a1"),
        ("v0.2.0", "a2"),
        ("v0.10.0", "a10"),
        ("lib-v1.2.0", "b1"),
        ("lib-v1.3.0", "b2"),
        ("solo-v0.1.0", "c1"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v.to_owned()))
    .collect();
    let boundaries = deepen_boundaries_for(&ws, &PlanConfig::default_values(), &targets);
    // app: v0.10.0 selected ⇒ v0.2.0 is the boundary; lib: v1.3.0 ⇒
    // v1.2.0. `solo-v0.1.0` matches no workspace template.
    assert_eq!(
        boundaries,
        vec!["lib-v1.2.0".to_owned(), "v0.2.0".to_owned()]
    );
}

#[test]
fn latest_tags_for_selects_the_highest_version_per_package() {
    // §8.8 step 1: per package the highest-version tag matching its
    // template wins; version ordering is semver (0.10.0 > 0.2.0), never
    // lexicographic.
    let ws = two_package_workspace();
    let targets: BTreeMap<String, String> = [
        ("v0.1.0", "a1"),
        ("v0.2.0", "a2"),
        ("v0.10.0", "a10"),
        ("lib-v1.2.0", "b1"),
        ("lib-v1.3.0", "b2"),
        ("other-v9.9.9", "x1"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v.to_owned()))
    .collect();
    let selection = latest_tags_for(&ws, &PlanConfig::default_values(), &targets);
    assert_eq!(selection.get("app").map(String::as_str), Some("v0.10.0"));
    assert_eq!(selection.get("lib").map(String::as_str), Some("lib-v1.3.0"));
    assert_eq!(selection.len(), 2, "foreign tags select nothing");
}

#[test]
fn latest_tags_for_ignores_invalid_versions_and_foreign_templates() {
    let ws = two_package_workspace();
    let targets: BTreeMap<String, String> = [
        ("v0.1.0", "a1"),
        ("v-not-a-version", "a2"),
        ("lib-v0.1.0", "b1"),
        ("lib-not-semver", "b2"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v.to_owned()))
    .collect();
    let selection = latest_tags_for(&ws, &PlanConfig::default_values(), &targets);
    assert_eq!(selection.len(), 2);
    assert_eq!(selection.get("app").map(String::as_str), Some("v0.1.0"));
    assert_eq!(selection.get("lib").map(String::as_str), Some("lib-v0.1.0"));
}

#[test]
fn latest_tags_for_is_deterministic_in_input_order() {
    // I3: shuffled probe output cannot change the selection (BTreeMap in,
    // sorted selection out; equal inputs, different insertion order).
    let ws = two_package_workspace();
    let forward: BTreeMap<String, String> =
        [("v0.1.0", "a1"), ("v0.2.0", "a2"), ("lib-v1.0.0", "b1")]
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .collect();
    let mut reversed: Vec<(String, String)> = forward
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    reversed.reverse();
    let reversed: BTreeMap<_, _> = reversed.into_iter().collect();
    assert_eq!(
        latest_tags_for(&ws, &PlanConfig::default_values(), &forward),
        latest_tags_for(&ws, &PlanConfig::default_values(), &reversed)
    );
}

#[test]
fn fetch_pulls_missing_tags_and_the_base_branch() -> Result<(), BoxError> {
    // The CI shape: a full clone with zero tags. The fetcher probes the
    // remote, selects the per-package latest tags, and requests them plus
    // the base branch; git ground truth sees the tags afterwards.
    let origin = origin_fixture("fetch-live-origin")?;
    let clone = tagless_clone(&origin, "fetch-live-clone")?;
    assert!(
        clone.ls_tags()?.is_empty(),
        "the clone fixture starts tagless"
    );
    let ws = two_package_workspace();
    let fetcher = GixTagFetcher::open(clone.root())?;
    let report =
        fetcher.fetch_release_tags(&ws, &PlanConfig::default_values(), "origin", Some("main"))?;
    assert_eq!(report.remote.as_deref(), Some("origin"));
    assert_eq!(report.fetched, 1, "app's v0.2.0 is the only selected tag");
    assert_eq!(report.already_local, 0);
    assert_eq!(report.failed, 0);
    assert!(!report.fallback_used, "the primary fetch succeeded");
    assert_eq!(
        report.selection.get("app").map(String::as_str),
        Some("v0.2.0"),
        "the selection provenance names the tag a live run requested"
    );
    // Ground truth: git sees the tag at the remote's target, and the base
    // branch objects arrived (the opportunistic remote-tracking update).
    assert_eq!(clone.ls_tags()?, vec!["v0.2.0".to_owned()]);
    let clone_main = clone.resolve("refs/remotes/origin/main")?;
    let origin_main = origin.resolve("main")?;
    assert_eq!(clone_main.trim(), origin_main.trim());
    // The complete-clone shape carries no shallow machinery (ADR-0016).
    assert!(
        !report
            .argv
            .iter()
            .any(|arg| arg.starts_with("--shallow-exclude=")),
        "a complete clone must not pass --shallow-exclude, got {:?}",
        report.argv
    );
    Ok(())
}

#[test]
fn fetch_deepens_a_shallow_clone_bounded_by_the_previous_release() -> Result<(), BoxError> {
    // The §8.8 use case proper: a shallow CI clone missing a newer
    // release. The minimal fetch rides the shallow machinery — one
    // `--shallow-exclude` per package's previous release tag — and the
    // ground truth sees the newer tag afterwards.
    let mut origin = origin_fixture("fetch-shallow-origin")?;
    let clone = origin.shallow_clone(1)?;
    // A newer release lands after the clone: the shallow clone has
    // v0.2.0 (the depth-1 tip tag) but neither the commit nor v0.3.0.
    origin.write(
        "app/Cargo.toml",
        "[package]\nname = \"app\"\nversion = \"0.3.0\"\n",
    )?;
    origin.stage_all()?;
    origin.commit("feat(app): third")?;
    origin.tag_annotated("v0.3.0", "app 0.3.0")?;
    let ws = two_package_workspace();
    let fetcher = GixTagFetcher::open(clone.root())?;
    let report =
        fetcher.fetch_release_tags(&ws, &PlanConfig::default_values(), "origin", Some("main"))?;
    assert!(!report.fallback_used, "the minimal shallow fetch succeeded");
    assert_eq!(report.fetched, 1, "v0.3.0 is the only missing selected tag");
    assert!(
        report
            .argv
            .iter()
            .any(|arg| arg == "--shallow-exclude=v0.2.0"),
        "the deepen boundary is app's previous release tag, got {:?}",
        report.argv
    );
    let tags = clone.ls_tags()?;
    assert!(
        tags.contains(&"v0.3.0".to_owned()),
        "the fetched tag is visible to git, got {tags:?}"
    );
    Ok(())
}

#[test]
fn fetch_rerun_is_a_no_op() -> Result<(), BoxError> {
    // I1: after the first fetch every selected tag is already local at
    // the probed target — the re-run requests nothing and reports
    // AlreadyLocal rows without touching the wire.
    let origin = origin_fixture("fetch-idem-origin")?;
    let clone = tagless_clone(&origin, "fetch-idem-clone")?;
    let ws = two_package_workspace();
    let fetcher = GixTagFetcher::open(clone.root())?;
    let first =
        fetcher.fetch_release_tags(&ws, &PlanConfig::default_values(), "origin", Some("main"))?;
    assert_eq!(first.fetched, 1);
    let second =
        fetcher.fetch_release_tags(&ws, &PlanConfig::default_values(), "origin", Some("main"))?;
    assert_eq!(second.fetched, 0);
    assert_eq!(second.already_local, 1);
    assert_eq!(second.failed, 0);
    match second.rows.as_slice() {
        [FetchRow::AlreadyLocal { tag }] => assert_eq!(tag, "v0.2.0"),
        other => return Err(format!("expected one AlreadyLocal row, got {other:?}").into()),
    }
    Ok(())
}

#[test]
fn fetch_refuses_to_move_a_divergent_release_tag() -> Result<(), BoxError> {
    // A local release tag at a *different* target than the remote's is
    // fail-closed (E-GIT-TAG-CONFLICT): release tags are never moved.
    // The phase continues past it (I2) — the healthy tag still lands.
    let origin = origin_fixture("fetch-conflict-origin")?;
    let mut clone = tagless_clone(&origin, "fetch-conflict-clone")?;
    // A local-only commit carrying a conflicting `v0.2.0`.
    clone.write("local.txt", "local-only")?;
    clone.stage_all()?;
    clone.commit("chore(local): local-only commit")?;
    clone.tag_annotated("v0.2.0", "a divergent local release tag")?;
    let ws = two_package_workspace();
    let fetcher = GixTagFetcher::open(clone.root())?;
    let report =
        fetcher.fetch_release_tags(&ws, &PlanConfig::default_values(), "origin", Some("main"))?;
    assert_eq!(report.failed, 1);
    assert_eq!(report.fetched, 0, "the divergent tag is never requested");
    match report.rows.as_slice() {
        [FetchRow::Failed { tag, error }] => {
            assert_eq!(tag, "v0.2.0");
            assert_eq!(error.code(), "E-GIT-TAG-CONFLICT");
        }
        other => return Err(format!("expected one Failed row, got {other:?}").into()),
    }
    Ok(())
}

#[test]
fn fetch_without_a_configured_remote_is_an_honest_no_op() -> Result<(), BoxError> {
    // A repository without the named remote has nothing to fetch from:
    // an empty report (remote None), no subprocess, no error — the same
    // state-not-error convention the push phase uses.
    let mut fx = GitFixture::new("fetch-noremote")?;
    fx.write(
        "Cargo.toml",
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\n",
    )?;
    fx.stage_all()?;
    fx.commit("feat: initial")?;
    let ws = two_package_workspace();
    let fetcher = GixTagFetcher::open(fx.root())?;
    let report =
        fetcher.fetch_release_tags(&ws, &PlanConfig::default_values(), "origin", Some("main"))?;
    assert_eq!(report.remote, None);
    assert_eq!(report.rows, Vec::<FetchRow>::new());
    assert_eq!(report.fetched + report.already_local + report.failed, 0);
    Ok(())
}

#[test]
fn fetch_from_an_unreachable_remote_fails_closed() -> Result<(), BoxError> {
    // A configured-but-unreachable remote fails the phase with
    // E-GIT-FETCH after both attempts (never a prompt, never a partial
    // report pretending success).
    let clone = GitFixture::new("fetch-broken-remote")?;
    clone.run_git(&["remote", "add", "origin", "/nonexistent/halyard-probe-repo"])?;
    let ws = two_package_workspace();
    let fetcher = GixTagFetcher::open(clone.root())?;
    let error = fetcher
        .fetch_release_tags(&ws, &PlanConfig::default_values(), "origin", Some("main"))
        .expect_err("an unreachable remote must fail closed");
    assert_eq!(error.code(), "E-GIT-FETCH");
    Ok(())
}

#[test]
fn fetch_report_is_deterministic_across_repositories() -> Result<(), BoxError> {
    // I3: independent clones of the same origin produce identical
    // reports (selection, rows, tallies, argv) — no probe-order or
    // repository-layout leakage.
    let origin = origin_fixture("fetch-det-origin")?;
    let ws = two_package_workspace();
    let mut reports = Vec::new();
    for label in ["fetch-det-clone-a", "fetch-det-clone-b"] {
        let clone = tagless_clone(&origin, label)?;
        let fetcher = GixTagFetcher::open(clone.root())?;
        reports.push(fetcher.fetch_release_tags(
            &ws,
            &PlanConfig::default_values(),
            "origin",
            Some("main"),
        )?);
    }
    let [a, b] = reports.as_slice() else {
        return Err("two reports expected".into());
    };
    assert_eq!(a.selection, b.selection);
    assert_eq!(a.rows, b.rows);
    assert_eq!(a.fetched, b.fetched);
    assert_eq!(a.argv, b.argv);
    Ok(())
}

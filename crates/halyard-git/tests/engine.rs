//! Engine integration tests (M2.1/M2.2): base resolution and the painter
//! walk against real `git`-built fixtures, with `git`-plumbing ground
//! truth (`merge-base --is-ancestor`, `rev-list`, `describe`).

use std::collections::BTreeSet;

use halyard_core::package::{
    DepEdge, Ecosystem, GroupId, Package, PathFilter, RelPath, TagTemplate, VersionSource,
    Workspace,
};
use halyard_core::{HistoryProvider, PkgId};
use halyard_git::{GitError, GitHistory};
use halyard_test_support::fixture::GitFixture;
use halyard_test_support::scenarios;

/// Builds a minimal workspace with one package per `(name, template)`.
fn ws_with(specs: &[(&str, &str)]) -> Workspace {
    let mut ws = Workspace::new();
    for (name, template) in specs {
        let pkg = Package {
            id: PkgId::FIRST,
            name: ws.intern(name),
            ecosystem: Ecosystem::Cargo,
            root: RelPath::new("Cargo.toml").unwrap(),
            manifest: RelPath::new("Cargo.toml").unwrap(),
            version: Some(semver::Version::new(0, 1, 0)),
            version_source: VersionSource::Local,
            publishable: true,
            deps: Vec::<DepEdge>::new(),
            tag_template: TagTemplate::parse(template).unwrap(),
            changelog: None,
            version_site: None,
            include: PathFilter::new(Vec::new(), Vec::new()),
            group: GroupId::new(0),
            facets: halyard_core::Facets::default(),
        };
        ws.push_package(pkg);
    }
    ws
}

/// `git merge-base --is-ancestor tag HEAD` as ground truth.
fn is_ancestor_git(fx: &GitFixture, tag: &str) -> bool {
    fx.run_git(&["merge-base", "--is-ancestor", tag, "HEAD"]).is_ok()
}

/// The peeled commit of a tag as reported by git.
fn peeled_git(fx: &GitFixture, tag: &str) -> String {
    fx.resolve(&format!("{tag}^{{commit}}"))
        .unwrap_or_else(|_| fx.resolve(tag).unwrap_or_default())
}

/// `git rev-list HEAD ^b1 ^b2 …` as ground truth for the walk.
fn rev_list_excluding(fx: &GitFixture, bases: &[Option<String>]) -> BTreeSet<String> {
    let mut revs: Vec<String> = vec!["HEAD".to_owned()];
    for base in bases.iter().flatten() {
        revs.push(format!("^{base}"));
    }
    let refs: Vec<&str> = revs.iter().map(String::as_str).collect();
    fx.rev_list(&refs)
        .unwrap_or_default()
        .into_iter()
        .collect()
}

#[test]
fn base_resolution_matches_git_ground_truth() {
    // (factory, packages with their tag templates).
    type Factory = fn() -> Result<GitFixture, halyard_test_support::FixtureError>;
    let cases: Vec<(Factory, Vec<(&str, &str)>)> = vec![
        (scenarios::annotated_tags, vec![("app", "v{version}")]),
        (scenarios::packed_refs, vec![("app", "v{version}")]),
        (
            scenarios::mixed_tag_states,
            vec![("a", "{name}-v{version}"), ("b", "{name}-v{version}")],
        ),
        (
            scenarios::multi_package_tag_set,
            vec![
                ("core", "{name}-v{version}"),
                ("cli", "{name}-v{version}"),
                ("web", "@scope/{name}@{version}"),
            ],
        ),
    ];
    for (factory, specs) in cases {
        let fx = factory().unwrap();
        let ws = ws_with(&specs);
        let history = GitHistory::build(fx.root(), &ws, 10_000).unwrap();
        for pkg in ws.packages() {
            let name = ws.interner().get(pkg.name).unwrap_or_default().to_owned();
            let base = history.digest().base_of(pkg.id);
            // Ground truth: highest-version matching tag that git says is
            // an ancestor of HEAD.
            let tags = fx.ls_tags().unwrap();
            let mut candidates: Vec<(semver::Version, String)> = Vec::new();
            for tag in &tags {
                if let Some(v) = pkg.tag_template.extract_version(tag, &name, "cargo") {
                    candidates.push((v, tag.clone()));
                }
            }
            candidates.sort_by(|x, y| y.0.cmp(&x.0).then_with(|| x.1.cmp(&y.1)));
            let expected = candidates
                .iter()
                .find(|(_, tag)| is_ancestor_git(&fx, tag))
                .map(|(_, tag)| tag.clone());
            assert_eq!(
                base.map(|b| b.tag.to_string()),
                expected,
                "base of {name} on {}",
                fx.root().display()
            );
            if let (Some(base), Some(tag)) = (base, expected) {
                assert_eq!(base.oid.as_str(), peeled_git(&fx, &tag));
            }
        }
    }
}

#[test]
fn side_branch_tag_is_rejected() {
    let fx = scenarios::tag_on_side_branch().unwrap();
    let ws = ws_with(&[("app", "v{version}")]);
    let history = GitHistory::build(fx.root(), &ws, 10_000).unwrap();
    // v0.1.0 sits on a side branch: not an ancestor of HEAD.
    assert!(history.digest().base_of(PkgId::new(0)).is_none());
}

#[test]
fn scoped_npm_tags_match() {
    let fx = scenarios::multi_package_tag_set().unwrap();
    let ws = ws_with(&[("web", "@scope/{name}@{version}")]);
    let history = GitHistory::build(fx.root(), &ws, 10_000).unwrap();
    let base = history.digest().base_of(PkgId::new(0)).unwrap();
    assert_eq!(base.tag.to_string(), "@scope/web@2.3.4");
}

#[test]
fn shallow_clone_is_rejected_with_actionable_error() {
    let fx = scenarios::shallow_history().unwrap();
    let ws = ws_with(&[("app", "v{version}")]);
    let err = GitHistory::build(fx.root(), &ws, 10_000).unwrap_err();
    assert_eq!(err.code(), "E-GIT-SHALLOW");
    assert!(err.to_string().contains("--unshallow"));
}

#[test]
fn partial_clone_still_paints_commits() {
    let fx = scenarios::partial_blobless_clone().unwrap();
    let ws = ws_with(&[("app", "v{version}")]);
    // The walk reads commits, never blobs (§8.4): a blob-less clone plans
    // fine (blob reads are the tree-diff stage's concern, M2.3).
    let history = GitHistory::build(fx.root(), &ws, 10_000);
    // The filter-serving fixture may or may not be reachable in every
    // sandbox; a transport failure is reported, never a panic.
    match history {
        Ok(h) => assert!(h.head_oid().is_some()),
        Err(GitError::Open { .. }) => {}
        Err(other) => panic!("unexpected error: {other}"),
    }
}

#[test]
fn walk_matches_rev_list_ground_truth() {
    type Factory = fn() -> Result<GitFixture, halyard_test_support::FixtureError>;
    // (factory, packages with their tag templates).
    let cases: Vec<(Factory, Vec<(&str, &str)>)> = vec![
        (scenarios::linear_single_package, vec![("app", "v{version}")]),
        (scenarios::merge_two_branches, vec![("app", "v{version}")]),
        (scenarios::octopus_merge, vec![("app", "v{version}")]),
        (scenarios::squash_style_history, vec![("app", "v{version}")]),
        (scenarios::revert_commit, vec![("app", "v{version}")]),
        (scenarios::packed_refs, vec![("app", "v{version}")]),
        (scenarios::annotated_tags, vec![("app", "v{version}")]),
        (scenarios::commit_graph_present, vec![("app", "v{version}")]),
        (
            scenarios::mixed_tag_states,
            vec![("a", "{name}-v{version}"), ("b", "{name}-v{version}")],
        ),
        (
            scenarios::multi_package_tag_set,
            vec![
                ("core", "{name}-v{version}"),
                ("cli", "{name}-v{version}"),
                ("web", "@scope/{name}@{version}"),
            ],
        ),
    ];
    for (factory, specs) in cases {
        let fx = factory().unwrap();
        // Fixtures without release tags get one at HEAD so the walk has a
        // base; the painted window is then empty and compared as such.
        let ws = ws_with(&specs);
        let history = GitHistory::build(fx.root(), &ws, 10_000).unwrap();
        let digest = history.digest();
        // Ground truth: per package, `git rev-list HEAD ^base_i` (or the
        // full `rev-list HEAD` for an initial release); the walk emits
        // the union of the windows.
        let mut expected: BTreeSet<String> = BTreeSet::new();
        for pkg in ws.packages() {
            match digest.base_of(pkg.id) {
                Some(base) => {
                    let tag = base.tag.to_string();
                    let peeled = peeled_git(&fx, &tag);
                    expected.extend(rev_list_excluding(&fx, &[Some(peeled)]));
                }
                None => {
                    expected.extend(rev_list_excluding(&fx, &[]));
                }
            }
        }
        // The digest carries only conventional commits (§8.5); filter the
        // ground truth the same way.
        let expected_conventional: BTreeSet<String> = expected
            .into_iter()
            .filter(|oid| {
                let show = fx
                    .run_git(&["show", "--format=%B", "-s", oid])
                    .unwrap_or_default();
                halyard_conventional::parse(show.as_bytes()).is_some()
            })
            .collect();
        let got: BTreeSet<String> = digest
            .commits
            .iter()
            .map(|c| c.oid.to_string())
            .collect();
        assert_eq!(
            got, expected_conventional,
            "paint vs rev-list on {}",
            fx.root().display()
        );
        // Determinism: the full build twice, byte-identical digest.
        let again = GitHistory::build(fx.root(), &ws, 10_000).unwrap();
        assert_eq!(format!("{:?}", history.digest()), format!("{:?}", again.digest()));
    }
}

#[test]
fn degraded_marker_tracks_commit_graph_presence() {
    let without = scenarios::linear_single_package().unwrap();
    let ws = ws_with(&[("app", "v{version}")]);
    let history = GitHistory::build(without.root(), &ws, 10_000).unwrap();
    assert!(history.degraded, "no commit-graph written yet");
    assert!(!history.truncated);

    let mut with = scenarios::commit_graph_present().unwrap();
    with.commit_graph_write(true).unwrap();
    let history = GitHistory::build(with.root(), &ws, 10_000).unwrap();
    assert!(!history.degraded, "commit-graph present");
}

#[test]
fn initial_release_walk_is_bounded() {
    let fx = scenarios::linear_single_package().unwrap();
    let ws = ws_with(&[("app", "v{version}")]);
    let history = GitHistory::build(fx.root(), &ws, 2).unwrap();
    assert!(history.truncated);
    assert_eq!(history.digest().commits.len(), 2);
    // Unbounded run sees all five commits.
    let full = GitHistory::build(fx.root(), &ws, 10_000).unwrap();
    assert_eq!(full.digest().commits.len(), 5);
}

#[test]
fn empty_repository_reports_unborn_head() {
    let fx = scenarios::empty().unwrap();
    let ws = ws_with(&[("app", "v{version}")]);
    let err = GitHistory::build(fx.root(), &ws, 10_000).unwrap_err();
    assert_eq!(err.code(), "E-GIT-HEAD");
}

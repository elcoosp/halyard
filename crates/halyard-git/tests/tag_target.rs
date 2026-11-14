//! Tag-target resolution integration tests (§11.1 precision rule): the
//! commit that *introduced* the release version, found by a bounded
//! first-parent walk on manifest blob-OID change with a caller-supplied
//! version probe — and the documented HEAD fallback whenever the walk
//! cannot verify (blob-less clones, exhausted bounds, never-committed
//! versions). Exercised against real `git`-built fixtures with
//! `git`-plumbing ground truth, exactly like the write-path tests.
//!
//! Pins the contracts the release forecast and the live shell share:
//!
//! * **precision** — the newest first-parent commit whose manifest
//!   *introduced* V (its manifest carries V, its parent's does not), not
//!   the newest commit that merely touched the manifest;
//! * **honest degradation** — an unreadable manifest blob (the
//!   `--filter=blob:none` case the pinpoint was deferred for) falls back
//!   to snapshot HEAD instead of erroring or guessing;
//! * **fail-safety** — every degradation path still yields a target the
//!   tag phase can tag (HEAD exists by construction).

use halyard_core::HistoryProvider;
use halyard_core::package::{
    DepEdge, Ecosystem, GroupId, Package, PathFilter, RelPath, TagTemplate, VersionSource,
    Workspace,
};
use halyard_core::release_state::PendingRelease;
use halyard_core::{Oid, PkgId};
use halyard_git::gix::hash::ObjectId;
use halyard_git::tag_target::{
    GixTagTargetResolver, ManifestVersionProbe, TagTargetRequest, TagTargetResolver,
    TagTargetSource, directives_for_pending_resolved,
};
use halyard_git::{GitError, GitHistory, GitHistoryOptions};
use halyard_test_support::fixture::GitFixture;

type BoxError = Box<dyn std::error::Error>;

/// A probe that extracts the version from manifest *text* — the minimal
/// ecosystem grammar the resolver contract needs (`cargo`-style
/// `version = "X"` lines and `npm`-style `"version": "X"` pairs). It
/// records the ecosystem it was asked about so tests can pin the probe
/// contract (the resolver forwards the request's ecosystem); the record
/// is shared through an `Rc` so the probe can be boxed into the resolver
/// while the test keeps reading it.
struct TextProbe {
    seen: std::rc::Rc<std::cell::RefCell<Vec<&'static str>>>,
}

impl TextProbe {
    fn new() -> Self {
        Self {
            seen: std::rc::Rc::new(std::cell::RefCell::new(Vec::new())),
        }
    }
}

impl ManifestVersionProbe for TextProbe {
    fn manifest_version(&self, ecosystem: &str, manifest: &[u8]) -> Option<String> {
        self.seen.borrow_mut().push(
            // The test fixtures only ever use these two ecosystems; the
            // leak-free static str keeps the record cheap.
            match ecosystem {
                "cargo" => "cargo",
                "npm" => "npm",
                _ => "other",
            },
        );
        let text = std::str::from_utf8(manifest).ok()?;
        if ecosystem == "npm" {
            // Minimal `"version": "X"` scan (the tests' package.json
            // fixtures are single-line flat objects).
            let key = text.split("\"version\"").nth(1)?;
            let value = key.split('"').nth(1)?;
            if value.is_empty() {
                return None;
            }
            return Some(value.to_owned());
        }
        // Minimal `version = "X"` scan inside the `[package]` table (the
        // fixtures never carry another table before it).
        for line in text.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("version = \"") {
                let value = rest.strip_suffix('"')?;
                if !value.is_empty() {
                    return Some(value.to_owned());
                }
            }
            // A later table ends the package section.
            if line.starts_with('[') && !line.starts_with("[package]") {
                return None;
            }
        }
        None
    }
}

/// The resolver under test over `fx` with the text probe.
fn resolver(fx: &GitFixture) -> Result<GixTagTargetResolver, GitError> {
    GixTagTargetResolver::open(fx.root(), Box::new(TextProbe::new()))
}

fn oid(hex: &str) -> ObjectId {
    ObjectId::from_hex(hex.as_bytes())
        .unwrap_or_else(|e| unreachable!("fixture oids are valid hex: {e}"))
}

fn core_oid(hex: &str) -> Oid {
    Oid::from_hex(hex.as_bytes()).unwrap_or_else(|| unreachable!("fixture oids are valid hex"))
}

fn request(manifest: &str, version: &str, head: &str, base: Option<&str>) -> TagTargetRequest {
    TagTargetRequest {
        ecosystem: "cargo".to_owned(),
        manifest: manifest.to_owned(),
        version: version.to_owned(),
        head: core_oid(head),
        base: base.map(core_oid),
    }
}

/// Asserts the resolution is the `expected` commit under the `expected`
/// source, and that resolving twice is stable (I3: the walk cannot leak
/// request order or mutable state).
fn assert_resolves(
    resolver: &GixTagTargetResolver,
    req: &TagTargetRequest,
    expected_source: TagTargetSource,
    expected_commit: &str,
) -> Result<(), BoxError> {
    let first = resolver.resolve(req)?;
    assert_eq!(first.source, expected_source, "unexpected target source");
    assert_eq!(
        first.commit,
        oid(expected_commit),
        "unexpected target commit"
    );
    let second = resolver.resolve(req)?;
    assert_eq!(first, second, "resolution is not deterministic");
    Ok(())
}

/// The §11.1 base shape: v0.1.0 released, then bumped to 0.2.0, then an
/// unrelated source edit. Returns (fixture, tag commit, bump commit, head).
fn bumped_fixture(label: &str) -> Result<(GitFixture, String, String, String), BoxError> {
    let mut fx = GitFixture::new(label)?;
    let base = fx.commit_with(&halyard_test_support::fixture::CommitSpec::files(
        vec![
            (
                "Cargo.toml".to_owned(),
                "[package]\nname = \"app\"\nversion = \"0.1.0\"\n".to_owned(),
            ),
            (
                "src/lib.rs".to_owned(),
                "pub const A: usize = 1;\n".to_owned(),
            ),
        ],
        "feat: scaffold app",
    ))?;
    fx.tag_annotated("v0.1.0", "app 0.1.0")?;
    let bump = fx.commit_with(&halyard_test_support::fixture::CommitSpec::files(
        vec![(
            "Cargo.toml".to_owned(),
            "[package]\nname = \"app\"\nversion = \"0.2.0\"\n".to_owned(),
        )],
        "chore: release 0.2.0",
    ))?;
    fx.commit_with(&halyard_test_support::fixture::CommitSpec::files(
        vec![(
            "src/lib.rs".to_owned(),
            "pub const A: usize = 2;\n".to_owned(),
        )],
        "feat: grow app",
    ))?;
    let head = fx
        .head_oid()?
        .ok_or_else(|| "fixture without HEAD".to_owned())?;
    Ok((fx, base, bump, head))
}

#[test]
fn pinpoints_the_commit_that_introduced_the_version() -> Result<(), BoxError> {
    let (fx, base, bump, head) = bumped_fixture("pinpoint-basic")?;
    let r = resolver(&fx)?;
    assert_resolves(
        &r,
        &request("Cargo.toml", "0.2.0", &head, Some(&base)),
        TagTargetSource::ManifestPinpoint,
        &bump,
    )?;
    // The already-released version pinpoints its own release commit (the
    // walk reaches it through the same window).
    assert_resolves(
        &r,
        &request("Cargo.toml", "0.1.0", &head, None),
        TagTargetSource::ManifestPinpoint,
        &base,
    )?;
    Ok(())
}

#[test]
fn later_manifest_edits_do_not_steal_the_pinpoint() -> Result<(), BoxError> {
    let mut fx = GitFixture::new("pinpoint-later-edit")?;
    let base = fx.commit_with(&halyard_test_support::fixture::CommitSpec::files(
        vec![(
            "Cargo.toml".to_owned(),
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\n".to_owned(),
        )],
        "feat: scaffold app",
    ))?;
    fx.tag_annotated("v0.1.0", "app 0.1.0")?;
    let bump = fx.commit_with(&halyard_test_support::fixture::CommitSpec::files(
        vec![(
            "Cargo.toml".to_owned(),
            "[package]\nname = \"app\"\nversion = \"0.2.0\"\n[dependencies]\nserde = \"1\"\n"
                .to_owned(),
        )],
        "chore: release 0.2.0",
    ))?;
    // A *later* manifest edit that does not change the version: the
    // newest change event is not the introducer.
    fx.commit_with(&halyard_test_support::fixture::CommitSpec::files(
        vec![(
            "Cargo.toml".to_owned(),
            "[package]\nname = \"app\"\nversion = \"0.2.0\"\n[dependencies]\nserde = \"1\"\nlog = \"0.4\"\n"
                .to_owned(),
        )],
        "build: add log dep",
    ))?;
    let head = fx
        .head_oid()?
        .ok_or_else(|| "fixture without HEAD".to_owned())?;
    let r = resolver(&fx)?;
    assert_resolves(
        &r,
        &request("Cargo.toml", "0.2.0", &head, Some(&base)),
        TagTargetSource::ManifestPinpoint,
        &bump,
    )?;
    Ok(())
}

#[test]
fn falls_back_to_head_when_the_version_was_never_committed() -> Result<(), BoxError> {
    // The manifest only ever carried 0.1.0; asking for the not-yet-bumped
    // 0.2.0 (e.g. a worktree-only edit) cannot pinpoint anything.
    let (fx, _base, _bump, head) = bumped_fixture("pinpoint-never-committed")?;
    let r = resolver(&fx)?;
    assert_resolves(
        &r,
        &request("Cargo.toml", "0.2.3", &head, None),
        TagTargetSource::HeadFallback,
        &head,
    )?;
    Ok(())
}

#[test]
fn initial_release_pinpoints_the_bootstrap_commit() -> Result<(), BoxError> {
    // No release base: the window runs to the root, where the manifest
    // introduced 0.1.0 (the initial-release state, §8.2).
    let (fx, base, _bump, head) = bumped_fixture("pinpoint-initial")?;
    let r = resolver(&fx)?;
    assert_resolves(
        &r,
        &request("Cargo.toml", "0.1.0", &head, None),
        TagTargetSource::ManifestPinpoint,
        &base,
    )?;
    Ok(())
}

#[test]
fn blobless_clones_degrade_to_the_head_fallback() -> Result<(), BoxError> {
    // The reason the pinpoint was deferred: `--filter=blob:none` clones
    // carry no manifest blobs to verify against. The resolver must fall
    // back honestly (HEAD, no error) instead of guessing or failing.
    let (mut source, _base, _bump, head) = bumped_fixture("pinpoint-blobless-src")?;
    source.enable_filter_serving()?;
    let clone = source.partial_clone()?;
    let r = resolver(&clone)?;
    assert_resolves(
        &r,
        &request("Cargo.toml", "0.2.0", &head, None),
        TagTargetSource::HeadFallback,
        &head,
    )?;
    Ok(())
}

#[test]
fn the_walk_bound_stops_the_search() -> Result<(), BoxError> {
    // The introducer sits below the bound: only HEAD is examined, the
    // classification of HEAD (its manifest carries 0.2.0 too) needs the
    // parent, and the bound stops before a verified answer exists.
    let (fx, base, _bump, head) = bumped_fixture("pinpoint-bound")?;
    let r = resolver(&fx)?.with_max_commits(1);
    assert_resolves(
        &r,
        &request("Cargo.toml", "0.2.0", &head, Some(&base)),
        TagTargetSource::HeadFallback,
        &head,
    )?;
    // A bound that reaches the introducer pinpoints it again (the bump
    // sits one commit below HEAD).
    let r = resolver(&fx)?.with_max_commits(3);
    let bump = fx.resolve("HEAD~1")?.trim().to_owned();
    assert_resolves(
        &r,
        &request("Cargo.toml", "0.2.0", &head, Some(&base)),
        TagTargetSource::ManifestPinpoint,
        &bump,
    )?;
    Ok(())
}

#[test]
fn merge_landings_pinpoint_the_merge_commit() -> Result<(), BoxError> {
    // The bump lands on a side branch and merges `--no-ff`: on the
    // first-parent chain the manifest changes *at the merge*, which is
    // the commit the tag phase targets (the engine's diff policy walks
    // first-parent too).
    let mut fx = GitFixture::new("pinpoint-merge")?;
    let base = fx.commit_with(&halyard_test_support::fixture::CommitSpec::files(
        vec![(
            "Cargo.toml".to_owned(),
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\n".to_owned(),
        )],
        "feat: scaffold app",
    ))?;
    fx.tag_annotated("v0.1.0", "app 0.1.0")?;
    fx.checkout_new("release/0.2.0", None)?;
    fx.commit_with(&halyard_test_support::fixture::CommitSpec::files(
        vec![(
            "Cargo.toml".to_owned(),
            "[package]\nname = \"app\"\nversion = \"0.2.0\"\n".to_owned(),
        )],
        "chore: release 0.2.0",
    ))?;
    fx.checkout("main")?;
    fx.merge_no_ff("release/0.2.0", "chore: merge release 0.2.0")?;
    let merge = fx
        .head_oid()?
        .ok_or_else(|| "fixture without HEAD".to_owned())?;
    let r = resolver(&fx)?;
    assert_resolves(
        &r,
        &request("Cargo.toml", "0.2.0", &merge, Some(&base)),
        TagTargetSource::ManifestPinpoint,
        &merge,
    )?;
    Ok(())
}

#[test]
fn two_packages_pinpoint_independently() -> Result<(), BoxError> {
    let mut fx = GitFixture::new("pinpoint-two-pkgs")?;
    let base = fx.commit_with(&halyard_test_support::fixture::CommitSpec::files(
        vec![
            (
                "crates/a/Cargo.toml".to_owned(),
                "[package]\nname = \"a\"\nversion = \"0.1.0\"\n".to_owned(),
            ),
            (
                "crates/b/Cargo.toml".to_owned(),
                "[package]\nname = \"b\"\nversion = \"1.0.0\"\n".to_owned(),
            ),
        ],
        "feat: scaffold a and b",
    ))?;
    fx.tag_annotated("v0.1.0", "a 0.1.0")?;
    let bump_a = fx.commit_with(&halyard_test_support::fixture::CommitSpec::files(
        vec![(
            "crates/a/Cargo.toml".to_owned(),
            "[package]\nname = \"a\"\nversion = \"0.2.0\"\n".to_owned(),
        )],
        "chore: release a 0.2.0",
    ))?;
    let bump_b = fx.commit_with(&halyard_test_support::fixture::CommitSpec::files(
        vec![(
            "crates/b/Cargo.toml".to_owned(),
            "[package]\nname = \"b\"\nversion = \"1.1.0\"\n".to_owned(),
        )],
        "chore: release b 1.1.0",
    ))?;
    let head = fx
        .head_oid()?
        .ok_or_else(|| "fixture without HEAD".to_owned())?;
    let r = resolver(&fx)?;
    assert_resolves(
        &r,
        &request("crates/a/Cargo.toml", "0.2.0", &head, Some(&base)),
        TagTargetSource::ManifestPinpoint,
        &bump_a,
    )?;
    assert_resolves(
        &r,
        &request("crates/b/Cargo.toml", "1.1.0", &head, Some(&base)),
        TagTargetSource::ManifestPinpoint,
        &bump_b,
    )?;
    Ok(())
}

/// Builds the glue tests' two-package workspace (the same shape the
/// shell drives): `app` (tag template `v{version}`) and `lib` (template
/// `lib-v{version}`), both Cargo packages at the repo root.
fn two_package_workspace() -> Workspace {
    let mut ws = Workspace::new();
    let specs = [
        ("app", "v{version}", "0.2.0"),
        ("lib", "lib-v{version}", "1.3.0"),
    ];
    for (name, tpl, ver) in specs {
        let name_sym = ws.intern(name);
        ws.push_package(Package {
            id: PkgId::FIRST,
            name: name_sym,
            ecosystem: Ecosystem::Cargo,
            root: RelPath::new("")
                .unwrap_or_else(|| unreachable!("the empty root is a valid RelPath")),
            manifest: RelPath::new(&format!("{name}/Cargo.toml"))
                .unwrap_or_else(|| unreachable!("the manifest path is a valid RelPath")),
            version: Some(
                semver::Version::parse(ver)
                    .unwrap_or_else(|e| unreachable!("fixture versions are semver: {e}")),
            ),
            version_source: VersionSource::Local,
            publishable: true,
            deps: Vec::<DepEdge>::new(),
            tag_template: TagTemplate::parse(tpl)
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

#[test]
fn glue_targets_pinpoints_and_reports_the_source() -> Result<(), BoxError> {
    // Workspace packages app/Cargo.toml and lib/Cargo.toml; app's manifest
    // bumps in commit 2, lib's never. The glue resolves app to its bump
    // commit and lib to the documented HEAD fallback.
    let mut fx = GitFixture::new("pinpoint-glue")?;
    fx.commit_with(&halyard_test_support::fixture::CommitSpec::files(
        vec![
            (
                "app/Cargo.toml".to_owned(),
                "[package]\nname = \"app\"\nversion = \"0.1.0\"\n".to_owned(),
            ),
            (
                "lib/Cargo.toml".to_owned(),
                "[package]\nname = \"lib\"\nversion = \"1.2.0\"\n".to_owned(),
            ),
        ],
        "feat: scaffold",
    ))?;
    let bump = fx.commit_with(&halyard_test_support::fixture::CommitSpec::files(
        vec![(
            "app/Cargo.toml".to_owned(),
            "[package]\nname = \"app\"\nversion = \"0.2.0\"\n".to_owned(),
        )],
        "chore: release app 0.2.0",
    ))?;
    let head = fx
        .head_oid()?
        .ok_or_else(|| "fixture without HEAD".to_owned())?;
    let ws = two_package_workspace();
    let history = GitHistory::build_with_options(fx.root(), &ws, GitHistoryOptions::default())?;
    let pending = vec![
        PendingRelease {
            package: "app".to_owned(),
            version: semver::Version::parse("0.2.0")?,
            tag: "v0.2.0".to_owned(),
        },
        PendingRelease {
            package: "lib".to_owned(),
            version: semver::Version::parse("1.3.0")?,
            tag: "lib-v1.3.0".to_owned(),
        },
    ];
    let r = resolver(&fx)?;
    let resolved =
        directives_for_pending_resolved(&pending, &ws, history.digest(), &core_oid(&head), &r);
    assert_eq!(resolved.len(), 2, "one directive per pending release");
    let app = resolved
        .iter()
        .find(|d| d.directive.name == "v0.2.0")
        .ok_or_else(|| "missing app directive".to_owned())?;
    assert_eq!(
        app.source,
        TagTargetSource::ManifestPinpoint,
        "app pinpoints"
    );
    assert_eq!(
        app.directive.target,
        oid(&bump),
        "app targets its bump commit"
    );
    assert_eq!(
        app.directive.message, "app 0.2.0",
        "the minimal annotation is unchanged"
    );
    let lib = resolved
        .iter()
        .find(|d| d.directive.name == "lib-v1.3.0")
        .ok_or_else(|| "missing lib directive".to_owned())?;
    assert_eq!(
        lib.source,
        TagTargetSource::HeadFallback,
        "lib's version was never committed"
    );
    assert_eq!(lib.directive.target, oid(&head), "lib falls back to HEAD");
    Ok(())
}

#[test]
fn glue_falls_back_for_stale_package_names() -> Result<(), BoxError> {
    // A pending release naming a package the workspace no longer knows
    // (a stale queue): the phase still runs — the directive targets the
    // documented HEAD fallback.
    let (fx, _base, _bump, head) = bumped_fixture("pinpoint-glue-stale")?;
    let ws = two_package_workspace();
    let history = GitHistory::build_with_options(fx.root(), &ws, GitHistoryOptions::default())?;
    let pending = vec![PendingRelease {
        package: "ghost".to_owned(),
        version: semver::Version::parse("9.9.9")?,
        tag: "v9.9.9".to_owned(),
    }];
    let r = resolver(&fx)?;
    let resolved =
        directives_for_pending_resolved(&pending, &ws, history.digest(), &core_oid(&head), &r);
    assert_eq!(
        resolved.len(),
        1,
        "the stale release still becomes a directive"
    );
    let stale = resolved
        .first()
        .unwrap_or_else(|| unreachable!("one directive per pending release"));
    assert_eq!(stale.source, TagTargetSource::HeadFallback);
    assert_eq!(stale.directive.target, oid(&head));
    Ok(())
}

#[test]
fn the_probe_receives_the_request_ecosystem() -> Result<(), BoxError> {
    // The resolver forwards the request's ecosystem to the probe (the
    // shell dispatches its adapters on it). Pinned with an npm request:
    // the probe records every ecosystem it was asked about.
    let (fx, _base, _bump, head) = bumped_fixture("pinpoint-eco")?;
    let probe = TextProbe::new();
    let seen = std::rc::Rc::clone(&probe.seen);
    let r = GixTagTargetResolver::open(fx.root(), Box::new(probe))?;
    let req = TagTargetRequest {
        ecosystem: "npm".to_owned(),
        manifest: "Cargo.toml".to_owned(),
        version: "0.2.0".to_owned(),
        head: core_oid(&head),
        base: None,
    };
    // The npm probe reads the Cargo.toml text as JSON — no version comes
    // back, so the walk degrades to the fallback (the assertion here is
    // on the recorded ecosystem, not the outcome).
    let target = r.resolve(&req)?;
    assert_eq!(target.source, TagTargetSource::HeadFallback);
    let seen = seen.borrow();
    assert!(!seen.is_empty(), "the probe was never consulted");
    assert!(
        seen.iter().all(|eco| *eco == "npm"),
        "the probe saw {seen:?}, want npm only"
    );
    Ok(())
}

//! Tag-phase write-path integration tests (§11.1 step 2, git-side
//! effects): the first write path in `halyard-git`, exercised against
//! real `git`-built fixtures with `git`-plumbing ground truth — exactly
//! like the read-path engine tests. Pins the three contracts the live
//! release shell depends on:
//!
//! * **I1 idempotence** — re-running the tag phase on an already-tagged
//!   target is a no-op (`AlreadyTagged`), never a duplicate or a moved
//!   tag;
//! * **I2 resumability** — a partial failure (some tags created, some
//!   not) re-runs to completion without duplicates; a tag that exists at
//!   a *different* target fails closed (E-GIT-TAG-CONFLICT) and is never
//!   silently moved;
//! * **I3 determinism** — identical directives and identity produce a
//!   byte-identical tag object (same oid) in any repository.

use std::cell::RefCell;
use std::collections::HashSet;

use halyard_core::package::{
    DepEdge, Ecosystem, GroupId, Package, PathFilter, RelPath, TagTemplate, VersionSource,
    Workspace,
};
use halyard_core::planner::PlanConfig;
use halyard_core::{BString, Oid, PkgId};
use halyard_git::gix::hash::ObjectId;
use halyard_git::tag::{
    GixTagWriter, TagDirective, TagOutcome, TagPhaseRow, TagWriter, TaggerIdentity,
    directives_for_pending, run_tag_phase,
};
use halyard_git::{GitError, refs};
use halyard_test_support::fixture::GitFixture;

type BoxError = Box<dyn std::error::Error>;

/// The fixed tagger identity every test uses (deterministic tag objects).
fn identity() -> TaggerIdentity {
    TaggerIdentity::new("halyard", "release@halyard.test", 1_700_000_000)
}

/// A fixture with one commit; returns the fixture and HEAD's oid.
fn fixture_one_commit(
    label: &str,
) -> Result<(GitFixture, ObjectId), halyard_test_support::FixtureError> {
    let mut fx = GitFixture::new(label)?;
    fx.write(
        "Cargo.toml",
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\n",
    )?;
    fx.stage_all()?;
    let head = fx.commit("feat(app): initial")?;
    let oid = ObjectId::from_hex(head.as_bytes())
        .unwrap_or_else(|e| unreachable!("fixture oids are valid hex: {e}"));
    Ok((fx, oid))
}

/// The tag's object type as git reports it (`tag` = annotated).
fn tag_kind(fx: &GitFixture, tag: &str) -> Result<String, BoxError> {
    Ok(fx.run_git(&["cat-file", "-t", tag])?.trim().to_owned())
}

/// The peeled commit of a tag as git reports it.
fn peeled(fx: &GitFixture, tag: &str) -> Result<String, BoxError> {
    Ok(fx.resolve(&format!("{tag}^{{}}"))?.trim().to_owned())
}

/// The tag object id (for annotated tags: the tag object itself).
fn tag_object_oid(fx: &GitFixture, tag: &str) -> Result<String, BoxError> {
    Ok(fx.resolve(tag)?.trim().to_owned())
}

#[test]
fn annotated_tag_write_is_visible_to_git() -> Result<(), BoxError> {
    let (fx, head) = fixture_one_commit("tag-write-annotated")?;
    let writer = GixTagWriter::open(fx.root(), identity())?;
    let directive = TagDirective {
        name: "v0.1.0".to_owned(),
        target: head,
        message: "app 0.1.0".to_owned(),
    };
    let TagOutcome::Created { tag_oid: created } = writer.create_annotated_tag(&directive)? else {
        return Err("a fresh repository must report Created".into());
    };
    // Ground truth: git sees an annotated tag object at HEAD carrying the
    // annotation message.
    assert_eq!(tag_kind(&fx, "v0.1.0")?, "tag");
    assert_eq!(peeled(&fx, "v0.1.0")?, head.to_string());
    assert_eq!(created.to_string(), tag_object_oid(&fx, "v0.1.0")?);
    let message = fx.run_git(&["cat-file", "-p", "v0.1.0"])?;
    assert!(
        message.contains("app 0.1.0"),
        "annotation must carry the directive message, got {message:?}"
    );
    assert!(
        message.contains("halyard <release@halyard.test>"),
        "annotation must carry the tagger identity, got {message:?}"
    );
    Ok(())
}

#[test]
fn identical_directives_produce_identical_tag_objects() -> Result<(), BoxError> {
    // I3: same (name, target, message, identity) ⇒ same tag object bytes
    // ⇒ same oid, independent of the repository the write lands in.
    let (a, head_a) = fixture_one_commit("tag-write-det-a")?;
    let (b, head_b) = fixture_one_commit("tag-write-det-b")?;
    assert_eq!(
        head_a.to_string(),
        head_b.to_string(),
        "fixtures are deterministic"
    );
    let wa = GixTagWriter::open(a.root(), identity())?;
    let wb = GixTagWriter::open(b.root(), identity())?;
    let directive = TagDirective {
        name: "v0.1.0".to_owned(),
        target: head_a,
        message: "app 0.1.0".to_owned(),
    };
    for writer in [&wa, &wb] {
        let TagOutcome::Created { tag_oid } = writer.create_annotated_tag(&directive)? else {
            return Err("a fresh repository must report Created".into());
        };
        assert_eq!(tag_oid.to_string(), tag_object_oid(&a, "v0.1.0")?);
    }
    assert_eq!(
        tag_object_oid(&a, "v0.1.0")?,
        tag_object_oid(&b, "v0.1.0")?,
        "byte-identical tag objects"
    );
    Ok(())
}

#[test]
fn rerun_on_tagged_target_is_already_tagged() -> Result<(), BoxError> {
    // I1: the re-run is a no-op — no second object, no ref move.
    let (fx, head) = fixture_one_commit("tag-write-idempotent")?;
    let writer = GixTagWriter::open(fx.root(), identity())?;
    let directive = TagDirective {
        name: "v0.1.0".to_owned(),
        target: head,
        message: "app 0.1.0".to_owned(),
    };
    writer.create_annotated_tag(&directive)?;
    let before = fx.ls_tags()?;
    let outcome = writer.create_annotated_tag(&directive)?;
    assert_eq!(outcome, TagOutcome::AlreadyTagged { target: head });
    assert_eq!(fx.ls_tags()?, before, "I1: the re-run writes nothing");
    Ok(())
}

#[test]
fn lightweight_tag_at_target_is_already_tagged() -> Result<(), BoxError> {
    // A pre-existing lightweight tag pointing at the target satisfies the
    // directive (the ref is what ships releases) — no conversion rewrite.
    let (mut fx, head) = fixture_one_commit("tag-write-lightweight")?;
    fx.tag_light("v0.1.0")?;
    let writer = GixTagWriter::open(fx.root(), identity())?;
    let directive = TagDirective {
        name: "v0.1.0".to_owned(),
        target: head,
        message: "app 0.1.0".to_owned(),
    };
    let outcome = writer.create_annotated_tag(&directive)?;
    assert_eq!(outcome, TagOutcome::AlreadyTagged { target: head });
    Ok(())
}

#[test]
fn conflicting_tag_fails_closed_and_is_not_moved() -> Result<(), BoxError> {
    // I2/I7 fail-closed: a tag that exists at a *different* target is a
    // hard error — the reconciliation never moves a release tag.
    let mut fx = GitFixture::new("tag-write-conflict")?;
    fx.write(
        "Cargo.toml",
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\n",
    )?;
    let _ = fx.stage_all();
    let first = fx.commit("feat(app): initial")?;
    fx.tag_annotated("v0.1.0", "earlier release")?;
    fx.write(
        "Cargo.toml",
        "[package]\nname = \"app\"\nversion = \"0.2.0\"\n",
    )?;
    let _ = fx.stage_all();
    let head = fx.commit("feat(app): next")?;
    let writer = GixTagWriter::open(fx.root(), identity())?;
    let directive = TagDirective {
        name: "v0.1.0".to_owned(),
        target: ObjectId::from_hex(head.as_bytes())
            .unwrap_or_else(|e| unreachable!("fixture oid: {e}")),
        message: "app 0.2.0".to_owned(),
    };
    let GitError::TagConflict {
        tag,
        existing,
        requested,
    } = writer
        .create_annotated_tag(&directive)
        .expect_err("conflict must fail")
    else {
        return Err("a conflicting tag must fail with TagConflict".into());
    };
    assert_eq!(tag, "v0.1.0");
    assert_eq!(existing, first, "the conflict names the existing target");
    assert_eq!(requested, head);
    // Ground truth: the existing tag still points where it pointed before.
    assert_eq!(peeled(&fx, "v0.1.0")?, first);
    Ok(())
}

#[test]
fn invalid_tag_names_fail_closed() -> Result<(), BoxError> {
    let (fx, head) = fixture_one_commit("tag-write-names")?;
    let writer = GixTagWriter::open(fx.root(), identity())?;
    for tag in ["", "v..1", ".stealth", "v1.lock", "v@{env}"] {
        let directive = TagDirective {
            name: tag.to_owned(),
            target: head,
            message: "app 0.1.0".to_owned(),
        };
        let error = writer
            .create_annotated_tag(&directive)
            .expect_err("invalid names fail closed");
        assert_eq!(error.code(), "E-GIT-TAG", "name {tag:?}");
        assert!(
            fx.ls_tags()?.is_empty(),
            "a rejected directive must not have written anything"
        );
    }
    Ok(())
}

#[test]
fn unknown_target_fails_closed() -> Result<(), BoxError> {
    let (fx, _head) = fixture_one_commit("tag-write-unknown-target")?;
    let writer = GixTagWriter::open(fx.root(), identity())?;
    let ghost = ObjectId::from_hex(b"1111111111111111111111111111111111111111")
        .unwrap_or_else(|e| unreachable!("static hex: {e}"));
    let directive = TagDirective {
        name: "v0.1.0".to_owned(),
        target: ghost,
        message: "app 0.1.0".to_owned(),
    };
    let error = writer
        .create_annotated_tag(&directive)
        .expect_err("unknown targets fail closed");
    assert_eq!(error.code(), "E-GIT-TAG");
    assert!(
        fx.ls_tags()?.is_empty(),
        "no tag may be written against a target that does not exist"
    );
    Ok(())
}

/// A recording writer for the executor tests (calls land in invocation
/// order; `fail_on` makes named directives fail with a stable error).
struct FakeTagWriter {
    calls: RefCell<Vec<String>>,
    fail_on: HashSet<&'static str>,
}

impl FakeTagWriter {
    fn failing_on(names: &[&'static str]) -> Self {
        Self {
            calls: RefCell::new(Vec::new()),
            fail_on: names.iter().copied().collect(),
        }
    }
}

impl TagWriter for FakeTagWriter {
    fn create_annotated_tag(&self, directive: &TagDirective) -> Result<TagOutcome, GitError> {
        self.calls.borrow_mut().push(directive.name.clone());
        if self.fail_on.contains(directive.name.as_str()) {
            return Err(GitError::TagWrite {
                message: format!("injected failure for {}", directive.name),
            });
        }
        Ok(TagOutcome::Created {
            tag_oid: directive.target,
        })
    }
}

/// A directive against a static target oid (executor tests only — the
/// fake writer never resolves the target).
fn directive(tag: &str) -> TagDirective {
    TagDirective {
        name: tag.to_owned(),
        target: ObjectId::from_hex(b"2222222222222222222222222222222222222222")
            .unwrap_or_else(|e| unreachable!("static hex: {e}")),
        message: format!("{tag} annotation"),
    }
}

#[test]
fn executor_sorts_dedupes_and_reports_deterministically() {
    // The executor owns the canonical input order (I3): scrambled input
    // and duplicates collapse into one sorted, deduplicated pass.
    let writer = FakeTagWriter::failing_on(&[]);
    let directives = vec![
        directive("v0.2.0"),
        directive("v0.1.0"),
        directive("v0.2.0"),
    ];
    let report = run_tag_phase(&writer, &directives);
    assert_eq!(
        writer.calls.borrow().clone(),
        vec!["v0.1.0".to_owned(), "v0.2.0".to_owned()],
        "name-sorted, deduplicated"
    );
    assert_eq!(report.created, 2);
    assert_eq!(report.already_tagged, 0);
    assert_eq!(report.failed, 0);
    let rows: Vec<String> = report
        .rows
        .iter()
        .map(|row| match row {
            TagPhaseRow::Applied { tag, .. } | TagPhaseRow::Failed { tag, .. } => tag.clone(),
        })
        .collect();
    assert_eq!(rows, vec!["v0.1.0".to_owned(), "v0.2.0".to_owned()]);
}

#[test]
fn executor_continues_after_failure() {
    // I2: one failed directive does not abort the batch — the report
    // carries the failure and the rest still apply, so a re-run resumes.
    let writer = FakeTagWriter::failing_on(&["v0.2.0"]);
    let directives = vec![
        directive("v0.1.0"),
        directive("v0.2.0"),
        directive("v0.3.0"),
    ];
    let report = run_tag_phase(&writer, &directives);
    assert_eq!(report.created, 2);
    assert_eq!(report.failed, 1);
    let mut failed_tags = Vec::new();
    for row in &report.rows {
        if let TagPhaseRow::Failed { tag, error } = row {
            failed_tags.push(tag.clone());
            assert_eq!(error.code(), "E-GIT-TAG");
        }
    }
    assert_eq!(failed_tags, vec!["v0.2.0".to_owned()]);
}

/// One package named `name` with manifest version `ver` and template
/// `template` (repository-root layout, like the engine tests).
fn ws_with(name: &str, ver: &str, template: &str) -> Result<Workspace, BoxError> {
    let mut ws = Workspace::new();
    let pkg = Package {
        id: PkgId::FIRST,
        name: ws.intern(name),
        ecosystem: Ecosystem::Cargo,
        root: RelPath::new("").ok_or("empty root is always a valid RelPath")?,
        manifest: RelPath::new("Cargo.toml").ok_or("root manifest is a valid RelPath")?,
        version: Some(semver::Version::parse(ver)?),
        version_source: VersionSource::Local,
        publishable: true,
        deps: Vec::<DepEdge>::new(),
        tag_template: TagTemplate::parse(template).map_err(|e| format!("bad template: {e}"))?,
        changelog: None,
        version_site: None,
        include: PathFilter::new(Vec::new(), Vec::new()),
        group: GroupId::new(0),
        facets: halyard_core::Facets::default(),
    };
    ws.push_package(pkg);
    Ok(ws)
}

#[test]
fn directives_for_pending_target_head() -> Result<(), BoxError> {
    // The §11.1 fallback: without the manifest-blob pinpoint walk the
    // directive targets HEAD (documented, deterministic).
    let ws = ws_with("app", "0.1.0", "v{version}")?;
    let head = Oid::from_hex(b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").ok_or("static hex")?;
    let pending =
        halyard_core::release_state::pending_releases(&ws, &[], &PlanConfig::default_values());
    assert_eq!(pending.len(), 1, "untagged manifest version is pending");
    let directives = directives_for_pending(&pending, &head);
    let directive = directives
        .first()
        .ok_or("one pending release ⇒ one directive")?;
    assert_eq!(directive.name, "v0.1.0");
    assert_eq!(
        directive.target,
        ObjectId::from_hex(head.as_bytes()).unwrap_or_else(|e| unreachable!("roundtrip: {e}"))
    );
    assert_eq!(directive.message, "app 0.1.0");
    Ok(())
}

#[test]
fn tag_phase_over_real_repo_is_idempotent_end_to_end() -> Result<(), BoxError> {
    // The full §11.1 git-side loop against a real repository: reconcile →
    // pending → directives → create → git sees the tags; the re-run
    // reconciles to *nothing pending* (I1) and a stale directive set is
    // still a no-op (AlreadyTagged, I2 resume shape).
    let (fx, head) = fixture_one_commit("tag-write-loop")?;
    let ws = ws_with("app", "0.1.0", "v{version}")?;
    let cfg = PlanConfig::default_values();
    let writer = GixTagWriter::open(fx.root(), identity())?;

    let tag_names = |repo: &halyard_git::gix::Repository| -> Vec<BString> {
        refs::enumerate(repo)
            .unwrap_or_else(|e| unreachable!("enumerate: {e}"))
            .into_iter()
            .map(|candidate| candidate.name)
            .collect()
    };
    let repo = halyard_git::gix::open(fx.root())?;
    let head_core = Oid::from_hex(head.to_string().as_bytes()).ok_or("fixture oids are hex")?;

    // First pass: the manifest version is pending, the phase creates it.
    let pending = halyard_core::release_state::pending_releases(&ws, &tag_names(&repo), &cfg);
    assert_eq!(pending.len(), 1);
    let directives = directives_for_pending(&pending, &head_core);
    let report = run_tag_phase(&writer, &directives);
    assert_eq!(report.created, 1);
    assert_eq!(fx.ls_tags()?, vec!["v0.1.0".to_owned()]);

    // Second pass: the reconciliation is empty (the tag now carries the
    // manifest version, I1) — the tag phase owes nothing.
    let pending_after = halyard_core::release_state::pending_releases(&ws, &tag_names(&repo), &cfg);
    assert!(pending_after.is_empty(), "reconciliation must go quiet");

    // A stale directive set replayed over the tagged repo is a no-op.
    let report = run_tag_phase(&writer, &directives);
    assert_eq!(report.already_tagged, 1);
    assert_eq!(report.created, 0);
    Ok(())
}

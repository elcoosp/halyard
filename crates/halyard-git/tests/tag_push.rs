//! The tag-push effect (§11.1 step 2: "push tags (single `git push`)",
//! §11.2) — the remote half of the tag phase, over the [`TagPusher`]
//! seam. Tests run real `git` fixtures: a bare origin, local annotated
//! tags, and the empirical `git push --porcelain` wire format.

use std::collections::BTreeMap;

use halyard_git::push::{
    GixTagPusher, PreviewTagPusher, TagPushRow, TagPusher as _, parse_porcelain_push,
    parse_remote_tag_targets, refspecs_for,
};
use halyard_test_support::{CommitSpec, GitFixture};

type BoxError = Box<dyn std::error::Error>;

const SCAFFOLD: &[(&str, &str)] = &[(
    "Cargo.toml",
    "[package]\nname = \"solo\"\nversion = \"0.1.0\"\n",
)];

/// A one-commit fixture with `v0.1.0` tagged locally and a bare `origin`
/// wired as `remote.origin.url`.
fn fixture_with_origin(label: &str) -> Result<GitFixture, BoxError> {
    let mut fx = GitFixture::new(label)?;
    fx.commit_with(&CommitSpec::files(
        SCAFFOLD
            .iter()
            .map(|(name, body)| ((*name).to_owned(), (*body).to_owned()))
            .collect(),
        "feat: scaffold",
    ))?;
    fx.tag_annotated("v0.1.0", "solo v0.1.0")?;
    let origin = fx.root().join(format!("{label}-origin.git"));
    fx.run_git(&["init", "--bare", origin.to_string_lossy().as_ref()])?;
    fx.run_git(&["remote", "add", "origin", origin.to_string_lossy().as_ref()])?;
    Ok(fx)
}

/// The remote's tag names, as bare `git ls-remote` ground truth.
fn remote_tags(fx: &GitFixture) -> Result<Vec<String>, BoxError> {
    let out = fx.run_git(&["ls-remote", "--tags", "origin"])?;
    let mut names = Vec::new();
    for line in out.lines() {
        let Some((_, reference)) = line.split_once('\t') else {
            continue;
        };
        let Some(name) = reference.strip_prefix("refs/tags/") else {
            continue;
        };
        // Annotated tags list the tag object and the `^{}`-peeled commit;
        // either row names the same release ref.
        names.push(name.strip_suffix("^{}").unwrap_or(name).to_owned());
    }
    names.sort();
    names.dedup();
    Ok(names)
}

#[test]
fn refspecs_are_explicit_and_total() {
    let specs = refspecs_for(&["v0.1.0".to_owned(), "core-v0.2.0".to_owned()]);
    assert_eq!(
        specs,
        vec![
            "refs/tags/v0.1.0:refs/tags/v0.1.0",
            "refs/tags/core-v0.2.0:refs/tags/core-v0.2.0",
        ]
    );
    // Total on an empty request: no refspecs, nothing to send.
    assert_eq!(refspecs_for(&[]), Vec::<String>::new());
}

#[test]
fn parsers_pin_the_empirical_wire_format() {
    // `git ls-remote --tags`: object rows plus `^{}`-peeled rows for
    // annotated tags (probed empirically, ADR-0012).
    let ls = "5bfd25ac1ba9af8eaef496fbd53f3ee2fe0eee07\trefs/tags/v0.1.0\n\
              7c9766ceb07b9de89e64539bd5f15bf9584a8638\trefs/tags/v0.1.0^{}\n\
              e897c83f967103a924e1407aaccd8e9b8938fcc1\trefs/tags/v0.2.0\n";
    let mut expected = BTreeMap::new();
    expected.insert(
        "v0.1.0".to_owned(),
        "7c9766ceb07b9de89e64539bd5f15bf9584a8638".to_owned(),
    );
    expected.insert(
        "v0.2.0".to_owned(),
        "e897c83f967103a924e1407aaccd8e9b8938fcc1".to_owned(),
    );
    assert_eq!(parse_remote_tag_targets(ls), expected);

    // `git push --porcelain`: `<flag>\t<from>:<to>\t<summary>` rows
    // (probed empirically: `*` new tag, `=` up to date, `!` rejected).
    let out = "To ../origin.git\n\
               *\trefs/tags/v0.1.0:refs/tags/v0.1.0\t[new tag]\n\
               =\trefs/tags/v0.2.0:refs/tags/v0.2.0\t[up to date]\n\
               !\trefs/tags/v0.3.0:refs/tags/v0.3.0\t[rejected] (already exists)\n\
               Done\n";
    let rows = parse_porcelain_push(out);
    assert_eq!(rows.len(), 3);
    let [first, second, third] = rows.as_slice() else {
        panic!("three parsed rows");
    };
    assert_eq!(first.flag, '*');
    assert_eq!(first.tag, "v0.1.0");
    assert_eq!(second.flag, '=');
    assert_eq!(second.tag, "v0.2.0");
    assert_eq!(third.flag, '!');
    assert_eq!(third.summary, "[rejected] (already exists)");
}

#[test]
fn pushes_created_tags_to_a_local_remote() -> Result<(), BoxError> {
    let fx = fixture_with_origin("push-created")?;
    let pusher = GixTagPusher::open(fx.root())?;
    let report = pusher.push_tags("origin", &["v0.1.0".to_owned()])?;
    assert_eq!(report.remote.as_deref(), Some("origin"));
    assert_eq!(report.pushed, 1, "v0.1.0 lands on the remote");
    assert_eq!(report.already_on_remote, 0);
    assert_eq!(report.failed, 0);
    assert_eq!(remote_tags(&fx)?, vec!["v0.1.0".to_owned()]);
    Ok(())
}

#[test]
fn re_push_is_already_on_remote_i1() -> Result<(), BoxError> {
    let fx = fixture_with_origin("push-idempotent")?;
    let pusher = GixTagPusher::open(fx.root())?;
    let tags = ["v0.1.0".to_owned()];
    let first = pusher.push_tags("origin", &tags)?;
    assert_eq!(first.pushed, 1);
    // The re-run probes first and sends nothing (I1: the no-op path).
    let second = pusher.push_tags("origin", &tags)?;
    assert_eq!(second.already_on_remote, 1);
    assert_eq!(second.pushed, 0);
    assert_eq!(remote_tags(&fx)?, vec!["v0.1.0".to_owned()]);
    Ok(())
}

#[test]
fn remote_conflict_fails_closed_without_forcing() -> Result<(), BoxError> {
    let mut fx = fixture_with_origin("push-conflict")?;
    // Hostile pre-state: the remote carries v0.1.0 at a *different*
    // commit than the local tag (moved there by an outside actor), while
    // the local tag stays at the release commit.
    fx.commit_with(&CommitSpec::files(
        vec![(
            "Cargo.toml".to_owned(),
            "[package]\nname = \"solo\"\nversion = \"0.2.0\"\n".to_owned(),
        )],
        "chore: another commit",
    ))?;
    let release = fx.resolve("HEAD~1")?;
    fx.run_git(&["tag", "-f", "v0.1.0", "HEAD"])?;
    fx.run_git(&["push", "origin", "refs/tags/v0.1.0"])?;
    fx.run_git(&["tag", "-f", "v0.1.0", &release])?;
    let pusher = GixTagPusher::open(fx.root())?;
    let report = pusher.push_tags("origin", &["v0.1.0".to_owned()])?;
    assert_eq!(report.failed, 1, "the divergent remote tag is a failure");
    let TagPushRow::Failed { tag, error } = report.rows.first().unwrap() else {
        panic!("expected a Failed row");
    };
    assert_eq!(tag, "v0.1.0");
    assert_eq!(error.code(), "E-GIT-TAG-CONFLICT");
    // The remote still carries the tag: nothing forced, nothing moved.
    assert_eq!(remote_tags(&fx)?, vec!["v0.1.0".to_owned()]);
    Ok(())
}

#[test]
fn no_remote_is_an_honest_no_op() -> Result<(), BoxError> {
    let mut fx = GitFixture::new("push-no-remote")?;
    fx.commit_with(&CommitSpec::files(
        SCAFFOLD
            .iter()
            .map(|(name, body)| ((*name).to_owned(), (*body).to_owned()))
            .collect(),
        "feat: scaffold",
    ))?;
    fx.tag_annotated("v0.1.0", "solo v0.1.0")?;
    let pusher = GixTagPusher::open(fx.root())?;
    let report = pusher.push_tags("origin", &["v0.1.0".to_owned()])?;
    assert!(report.remote.is_none(), "no remote configured");
    assert_eq!(report.rows, Vec::<halyard_git::push::TagPushRow>::new());
    assert_eq!(report.pushed + report.already_on_remote + report.failed, 0);
    Ok(())
}

#[test]
fn missing_local_tag_fails_closed() -> Result<(), BoxError> {
    let fx = fixture_with_origin("push-missing-local")?;
    let pusher = GixTagPusher::open(fx.root())?;
    let report = pusher.push_tags("origin", &["v9.9.9".to_owned()])?;
    assert_eq!(report.failed, 1);
    let TagPushRow::Failed { tag, error } = report.rows.first().unwrap() else {
        panic!("expected a Failed row");
    };
    assert_eq!(tag, "v9.9.9");
    assert_eq!(error.code(), "E-GIT-PUSH");
    assert_eq!(remote_tags(&fx)?, Vec::<String>::new());
    Ok(())
}

#[test]
fn partial_state_resumes_to_the_uninterrupted_state_i2() -> Result<(), BoxError> {
    let mut fx = fixture_with_origin("push-resume")?;
    fx.tag_annotated("v0.2.0", "solo v0.2.0")?;
    // Pre-state: v0.2.0 already on the remote, v0.1.0 missing — the
    // shape after a partially-completed phase. The re-run pushes only
    // what is missing (I2, zero duplicates).
    fx.run_git(&["push", "origin", "refs/tags/v0.2.0"])?;
    let pusher = GixTagPusher::open(fx.root())?;
    let report = pusher.push_tags("origin", &["v0.1.0".to_owned(), "v0.2.0".to_owned()])?;
    assert_eq!(report.pushed, 1, "v0.1.0");
    assert_eq!(report.already_on_remote, 1, "v0.2.0");
    assert_eq!(report.failed, 0);
    assert_eq!(
        remote_tags(&fx)?,
        vec!["v0.1.0".to_owned(), "v0.2.0".to_owned()]
    );
    Ok(())
}

#[test]
fn preview_forecasts_would_push_without_touching_anything() -> Result<(), BoxError> {
    let fx = fixture_with_origin("push-preview")?;
    let pusher = PreviewTagPusher::open(fx.root())?;
    let report = pusher.push_tags("origin", &["v0.1.0".to_owned(), "v9.9.9".to_owned()])?;
    assert_eq!(report.remote.as_deref(), Some("origin"));
    assert_eq!(report.rows.len(), 2, "both tags forecast");
    assert!(matches!(
        report.rows.first().unwrap(),
        TagPushRow::WouldPush { tag } if tag == "v0.1.0"
    ));
    assert_eq!(report.pushed + report.already_on_remote, 0);
    // The remote stays empty: the forecast cannot write.
    assert_eq!(remote_tags(&fx)?, Vec::<String>::new());
    Ok(())
}

#[test]
fn push_report_is_deterministic_i3() -> Result<(), BoxError> {
    let fx = fixture_with_origin("push-determinism")?;
    let pusher = GixTagPusher::open(fx.root())?;
    let tags = ["v0.1.0".to_owned()];
    let _creating = pusher.push_tags("origin", &tags)?;
    // Two runs over the *same* repository state classify identically (I3;
    // the re-run itself is the I1 no-op shape).
    let first = pusher.push_tags("origin", &tags)?;
    let second = pusher.push_tags("origin", &tags)?;
    assert_eq!(first, second);
    assert_eq!(first.already_on_remote, 1);
    Ok(())
}

#[test]
fn unreachable_remote_is_a_fail_closed_push_error() -> Result<(), BoxError> {
    let fx = fixture_with_origin("push-unreachable")?;
    // Point origin at a directory that is not a repository.
    let bogus = fx.root().join("not-a-repo");
    std::fs::create_dir_all(&bogus)?;
    fx.run_git(&[
        "remote",
        "set-url",
        "origin",
        bogus.to_string_lossy().as_ref(),
    ])?;
    let pusher = GixTagPusher::open(fx.root())?;
    let error = pusher
        .push_tags("origin", &["v0.1.0".to_owned()])
        .expect_err("an unreachable remote fails the phase");
    assert_eq!(error.code(), "E-GIT-PUSH");
    Ok(())
}

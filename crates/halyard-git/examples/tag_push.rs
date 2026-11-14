//! Demos the §11.2 tag-push effect end to end: a fixture repository with
//! a pending release gets its annotated tag through the
//! [`halyard_git::tag::TagWriter`] seam, the created tag is pushed to a
//! bare `origin` through the [`halyard_git::push::TagPusher`] seam (the
//! §11.1 "push tags (single `git push`)" effect step), and the re-run
//! classifies the tag as already on the remote — the I1 no-op shape the
//! live release shell produces on every CI re-run.
//!
//! Run: `cargo run -p halyard-git --example tag_push`

use std::error::Error;

use halyard_git::push::{GixTagPusher, TagPusher};
use halyard_git::tag::{
    GixTagWriter, TagDirective, TagOutcome, TagPhaseRow, TaggerIdentity, run_tag_phase,
};
use halyard_test_support::{CommitSpec, GitFixture};
use tempfile::TempDir;

fn main() -> Result<(), Box<dyn Error>> {
    // A deterministic fixture: v0.1.0 released, manifest bumped to 0.1.1
    // on a later commit — the tag phase owes v0.1.1 (§11.1 step 2).
    let dir = TempDir::new()?;
    let mut fx = GitFixture::init_in(dir.path())?;
    let base = fx.commit_with(&CommitSpec::files(
        vec![(
            "Cargo.toml".to_owned(),
            "[package]\nname = \"solo\"\nversion = \"0.1.0\"\n".to_owned(),
        )],
        "feat: scaffold",
    ))?;
    fx.tag_annotated("v0.1.0", "solo v0.1.0")?;
    let bump = fx.commit_with(&CommitSpec::files(
        vec![(
            "Cargo.toml".to_owned(),
            "[package]\nname = \"solo\"\nversion = \"0.1.1\"\n".to_owned(),
        )],
        "chore(main): release v0.1.1",
    ))?;

    // The push target: a bare repository wired as `origin`.
    let origin = dir.path().join("origin.git");
    fx.run_git(&["init", "--bare", origin.to_string_lossy().as_ref()])?;
    fx.run_git(&["remote", "add", "origin", origin.to_string_lossy().as_ref()])?;

    // Tag phase (git side): one annotated tag, created through the seam.
    // The identity is the shell's (git config + clock); here it is
    // explicit so the example is deterministic.
    let writer = GixTagWriter::open(
        fx.root(),
        TaggerIdentity::new("halyard example", "example@example.invalid", 1_700_000_000),
    )?;
    let directive = TagDirective {
        name: "v0.1.1".to_owned(),
        target: halyard_git::gix::hash::ObjectId::from_hex(bump.as_bytes())?,
        message: "solo 0.1.1".to_owned(),
    };
    let report = run_tag_phase(&writer, &[directive]);
    let created: Vec<String> = report
        .rows
        .iter()
        .filter_map(|row| match row {
            TagPhaseRow::Applied {
                tag,
                outcome: TagOutcome::Created { .. },
            } => Some(tag.clone()),
            _ => None,
        })
        .collect();
    println!(
        "tag phase: {} created, {} already tagged, {} failed",
        report.created, report.already_tagged, report.failed
    );

    // Push phase (§11.2): the tags this run created go to `origin` in a
    // single `git push --porcelain` (argv arrays, no force, no prompt).
    let pusher = GixTagPusher::open(fx.root())?;
    let push = pusher.push_tags("origin", &created)?;
    println!(
        "push ({}): {} pushed, {} already on remote, {} failed",
        push.remote.as_deref().unwrap_or_default(),
        push.pushed,
        push.already_on_remote,
        push.failed
    );

    // Re-run: the remote probe classifies the tag as already present —
    // nothing is sent (I1), exactly the CI re-run shape.
    let rerun = pusher.push_tags("origin", &created)?;
    assert_eq!(rerun.already_on_remote, created.len());
    println!(
        "re-run: {} already on remote (I1 no-op) — base {base}, bump {bump}",
        rerun.already_on_remote
    );
    Ok(())
}

//! The §16.2 write step demo (M4.7 step 2, ADR-0023): the validated
//! draft from the migration report is planned and written to a real
//! `halyard.toml` through the [`AtomicDraftSink`] — atomic, create-only
//! — and the landed file is re-loaded through the strict loader as the
//! proof. The create-only contract is demonstrated live: the second
//! write attempt at the same target is refused (`E-CFG-MIGRATE`), never
//! a clobber.
//!
//! Run: `cargo run -p halyard-config --example migrate_write`

use std::error::Error;

use halyard_config::{
    AtomicDraftSink, DRAFT_TARGET, DraftSink, ForeignFiles, default_sources, load, migrate,
    plan_draft_write, run_draft_write,
};

fn main() -> Result<(), Box<dyn Error>> {
    // A repo migrating off release-please (the VERIFIED grammar — see
    // docs/research/migrate-config-grammars.md).
    let mut files = ForeignFiles::new();
    files.insert(
        "release-please-config.json",
        r#"{
  "packages": {
    "crates/p1": { "package-name": "p1-crate" },
    "crates/p2": {}
  },
  "bump-minor-pre-major": true,
  "include-v-in-tag": false
}"#,
    );
    files.insert(
        ".release-please-manifest.json",
        r#"{ "crates/p1": "1.2.3", "crates/p2": "0.5.0" }"#,
    );

    // Step 1: the survey (nothing written yet — the draft rides the
    // report as a validated string).
    let report = migrate(&files, &default_sources())?;

    // Step 2: the pure plan picks the draft; the runner lands it through
    // the sink. The demo writes into a temp directory (the CLI runs in
    // the repo root and always targets `halyard.toml` there).
    let plan = plan_draft_write(&report)?
        .ok_or("a configured tool with a non-empty draft is a write candidate")?;
    println!("plan:     tool={} path={}", plan.tool, plan.path);
    let dir = tempfile::tempdir()?;
    let target = dir.path().join(DRAFT_TARGET);
    // The plan's path is the repo-root-relative `halyard.toml` the CLI
    // runs against; the demo retargets it into the temp dir (plain data,
    // so the retarget is a field move — the contents are untouched).
    let plan = halyard_config::DraftWritePlan {
        path: target.display().to_string(),
        ..plan
    };
    let sink = AtomicDraftSink;
    let write = run_draft_write(Some(plan), &sink)?;
    println!(
        "written:  outcome={:?} bytes={}",
        write.outcome, write.bytes
    );

    // The landed file is exactly the reviewed draft, and the strict
    // loader accepts it — the write step cannot drift from step 1.
    let landed = std::fs::read_to_string(&target)?;
    println!("landed:   {} bytes at {}", landed.len(), target.display());
    println!(
        "reparse:  {}",
        match load(&target) {
            Ok(_) => "the strict loader accepts the written halyard.toml".to_owned(),
            Err(error) => error.to_string(),
        }
    );

    // Create-only: the re-run at the same target refuses instead of
    // clobbering (the CLI's re-run surfaces the same refusal).
    match sink.write(&target.display().to_string(), "[repo]\n") {
        Ok(()) => println!("re-write: (unreachable — the sink is create-only)"),
        Err(error) => println!("re-write: refused — {error}"),
    }
    Ok(())
}

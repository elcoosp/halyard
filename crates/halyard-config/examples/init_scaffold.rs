//! The `halyard init` engine, end to end (§16.1/§16.2, ADR-0029): the
//! shell's discovery survey becomes a create-only write plan — the
//! documented `halyard.toml` skeleton, plus the §16.1 workflow with
//! `--github` — and the runner lands it through a sink. This demo wires
//! the recording [`MemoryDraftSink`], so nothing touches the disk; run
//! `halyard init` for the real, atomic, create-only write.
//!
//! Run: `cargo run -p halyard-config --example init_scaffold`

use halyard_config::{InitSurvey, MemoryDraftSink, plan_init, run_init};

fn main() {
    // The survey the CLI builds from discovery (plain data — the engine
    // stays pure): one npm package at the repo root.
    let survey = InitSurvey {
        packages: 1,
        ecosystems: vec!["npm".to_owned()],
        single_package: true,
    };
    let plan = plan_init(&survey, true).unwrap_or_else(|error| {
        eprintln!("the survey always plans: {error}");
        std::process::exit(1);
    });
    println!("detected shape: {}", plan.shape);
    for file in &plan.files {
        println!(
            "\n=== would write `{}` ({} bytes) ===",
            file.path,
            file.contents.len()
        );
        print!("{}", file.contents);
    }
    let sink = MemoryDraftSink::new();
    let report = run_init(plan, &sink).unwrap_or_else(|error| {
        eprintln!("a fresh sink never refuses: {error}");
        std::process::exit(1);
    });
    println!("\n=== landed over the memory sink ===");
    for file in &report.files {
        println!("  wrote `{}` ({} bytes)", file.path, file.bytes);
    }
    println!(
        "\nthe memory sink records without touching the disk; the real command \
         wires AtomicScaffoldSink:\n  halyard init [--github]"
    );
}

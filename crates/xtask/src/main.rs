//! Workspace automation for HALYARD (`xtask`, §7.1).
//!
//! Subcommands (run via `cargo run -p xtask -- <subcommand>`):
//!
//! | Subcommand | Purpose |
//! |---|---|
//! | `check-layers` | Enforce the §7.2 crate dependency rules |
//! | `schema-check` | Regenerate/verify the frozen JSON schemas (M0.7) |
//! | `perf-gate` | Compare measured budgets against §14.1 limits (M2.6+) |
//! | `size-gate` | Check the release binary size budget (§14.1) |
//! | `corpus` | List/fetch the pinned corpus repositories (M0.2) |
//! | `gen-repo` | Deterministic synthetic repo generator (M0.4) |
//! | `bench-competitors` | Competitor benchmark harness (M0.3) |

mod competitors;
mod corpus;
mod genrepo;
mod layers;
mod perf;
mod schema;
mod size;

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(msg) => {
            eprintln!("xtask: {msg}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> Result<(), String> {
    let (sub, rest) = match args.split_first() {
        Some((s, r)) => (s.as_str(), r),
        None => usage_exit(),
    };
    match sub {
        "check-layers" => layers::check_layers(),
        "schema-check" => schema::schema_check(flag(rest, "--write")),
        "perf-gate" => perf::perf_gate(),
        "size-gate" => size::size_gate(value_after(rest, "--binary")?),
        "corpus" => corpus::corpus(rest),
        "gen-repo" => genrepo::gen_repo(rest),
        "bench-competitors" => competitors::bench_competitors(rest),
        "help" | "--help" | "-h" => {
            print_help();
            Ok(())
        }
        other => {
            print_help();
            Err(format!("unknown subcommand {other:?}"))
        }
    }
}

fn usage_exit() -> ! {
    eprintln!("xtask: expected a subcommand (see `cargo run -p xtask -- help`)");
    std::process::exit(2);
}

fn flag(args: &[String], name: &str) -> bool {
    args.iter().any(|a| a == name)
}

fn value_after(args: &[String], name: &str) -> Result<Option<String>, String> {
    let mut iter = args.iter();
    while let Some(a) = iter.next() {
        if a == name {
            return iter
                .next()
                .cloned()
                .map(Some)
                .ok_or_else(|| format!("{name} needs a value"));
        }
    }
    Ok(None)
}

fn print_help() {
    println!(
        "xtask — HALYARD workspace automation

USAGE:
    cargo run -p xtask -- <SUBCOMMAND> [FLAGS]

SUBCOMMANDS:
    check-layers                      enforce crate dependency layers (§7.2)
    schema-check [--write]            verify/regenerate frozen JSON schemas (M0.7)
    perf-gate                         check measured budgets vs §14.1 (M2.6+)
    size-gate [--binary PATH]         check release binary size budget (§14.1)
    corpus <list|fetch> [--shallow|--full] [--only NAME]
                                      pinned corpus repos (M0.2)
    gen-repo --packages N --commits K --shape SHAPE --seed S --out DIR
                                      deterministic synthetic repo (M0.4)
    bench-competitors [--tool T] [--fixture DIR] [--report]
                                      competitor harness (M0.3)"
    );
}

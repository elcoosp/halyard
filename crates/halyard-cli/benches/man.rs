//! The man renderer's throughput (§16.2). The renderer is a pure
//! function of a [`clap::Command`] tree, so the bench renders a mirror
//! of the CLI's tree — the same 18 commands and representative flags —
//! built in-process (the bin's derive is not a library surface). The
//! shape pins the render cost of the full page set, so a help-text or
//! flag change that suddenly re-formats every page shows up honestly.

use std::hint::black_box;

use clap::Command as ClapCommand;
use criterion::Criterion;
use halyard_cli::man;

/// The CLI-tree mirror: 18 commands with one representative option per
/// command and the root's global flag set.
fn mirror_tree() -> ClapCommand {
    let names = [
        "plan",
        "apply",
        "pr",
        "release",
        "publish",
        "migrate",
        "init",
        "explain",
        "check",
        "affected",
        "fetch",
        "status",
        "graph",
        "doctor",
        "forge-smoke",
        "schema",
        "completions",
        "man",
    ];
    let mut root = ClapCommand::new("halyard")
        .about("Polyglot release automation")
        .arg(
            clap::Arg::new("format")
                .long("format")
                .global(true)
                .value_name("FORMAT"),
        )
        .arg(clap::Arg::new("offline").long("offline").global(true));
    for name in &names {
        let sub = ClapCommand::new(*name)
            .about("rendered help text stands in for the real one")
            .arg(
                clap::Arg::new("rendered_flag")
                    .long("rendered-flag")
                    .value_name("VAL"),
            );
        root = root.subcommand(sub);
    }
    root
}

fn bench_man(c: &mut Criterion) {
    let root = mirror_tree();
    let mut group = c.benchmark_group("man");
    group.bench_function("top_page", |biter| {
        biter.iter(|| man::top_page(black_box(&root)));
    });
    group.bench_function("command_pages_18", |biter| {
        biter.iter(|| {
            names_iter()
                .filter_map(|name| man::command_page(black_box(&root), black_box(name)))
                .count()
        });
    });
    group.finish();
}

/// The mirror's subcommand names (kept in one place with the tree).
fn names_iter() -> impl Iterator<Item = &'static str> {
    [
        "plan",
        "apply",
        "pr",
        "release",
        "publish",
        "migrate",
        "init",
        "explain",
        "check",
        "affected",
        "fetch",
        "status",
        "graph",
        "doctor",
        "forge-smoke",
        "schema",
        "completions",
        "man",
    ]
    .into_iter()
}

fn main() {
    bench_man(&mut Criterion::default().configure_from_args());
}

//! Tag-push benchmark (§11.1 step 2 / §11.2): the pure seam surfaces the
//! release shell exercises per run — the explicit refspec build for the
//! batch push and the two wire-format parsers (the `git ls-remote --tags`
//! probe and the `git push --porcelain` statuses). The subprocess effects
//! themselves are not benchmarked: they are dominated by the network
//! round trip, which no in-repo budget gates.

use std::hint::black_box;
use std::sync::OnceLock;

use criterion::Criterion;

use halyard_git::push::{parse_porcelain_push, parse_remote_tag_targets, refspecs_for};

/// Tags per bench batch (the §14.1-style steady-state shape: a full
/// 40-package workspace releasing everything at once, with headroom).
const BATCH: usize = 64;

/// A deterministic tag-name batch (`v0.0.N`).
fn tag_names() -> &'static Vec<String> {
    static NAMES: OnceLock<Vec<String>> = OnceLock::new();
    NAMES.get_or_init(|| (0..BATCH).map(|n| format!("v0.0.{n}")).collect::<Vec<_>>())
}

/// A deterministic `git ls-remote --tags` transcript over the batch
/// (annotated tags carry the `^{}` peeled row — the wire format probed
/// in ADR-0012).
fn ls_remote_transcript() -> &'static String {
    static TRANSCRIPT: OnceLock<String> = OnceLock::new();
    TRANSCRIPT.get_or_init(|| {
        let mut out = String::new();
        for (n, tag) in tag_names().iter().enumerate() {
            let hex = format!("{n:040}");
            let _ = std::fmt::Write::write_fmt(
                &mut out,
                format_args!("{hex}\trefs/tags/{tag}\n{hex}\trefs/tags/{tag}^{{}}\n"),
            );
        }
        out
    })
}

/// A deterministic `git push --porcelain` transcript: half the batch
/// accepted, a quarter already up to date, a quarter rejected.
fn porcelain_transcript() -> &'static String {
    static TRANSCRIPT: OnceLock<String> = OnceLock::new();
    TRANSCRIPT.get_or_init(|| {
        let mut out = String::from("To ../origin.git\n");
        for (n, tag) in tag_names().iter().enumerate() {
            let flag = match n % 4 {
                0 | 1 => "*",
                2 => "=",
                _ => "!",
            };
            let summary = match n % 4 {
                0 | 1 => "[new tag]",
                2 => "[up to date]",
                _ => "[rejected] (already exists)",
            };
            let _ = std::fmt::Write::write_fmt(
                &mut out,
                format_args!("{flag}\trefs/tags/{tag}:refs/tags/{tag}\t{summary}\n"),
            );
        }
        out.push_str("Done\n");
        out
    })
}

fn bench_tag_push(c: &mut Criterion) {
    let mut group = c.benchmark_group("tag_push");
    group.throughput(criterion::Throughput::Elements(BATCH as u64));
    group.bench_function("refspecs_64", |b| {
        b.iter(|| {
            let specs = refspecs_for(tag_names());
            black_box(specs.len())
        });
    });
    group.bench_function("parse_remote_targets_64", |b| {
        b.iter(|| {
            let targets = parse_remote_tag_targets(ls_remote_transcript());
            black_box(targets.len())
        });
    });
    group.bench_function("parse_porcelain_64", |b| {
        b.iter(|| {
            let rows = parse_porcelain_push(porcelain_transcript());
            black_box(rows.len())
        });
    });
    group.finish();
}

fn main() {
    bench_tag_push(&mut Criterion::default().configure_from_args());
}

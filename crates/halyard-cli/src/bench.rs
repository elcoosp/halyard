//! The §16.2 `bench` command's measurement engine (ADR-0042): the pure
//! aggregation behind the built-in benchmark — per-stage wall-clock
//! samples of the planning pipeline over the current repo, reduced to
//! the nearest-rank percentiles the §14.1 budget table is written in.
//!
//! Division of labor (the shell's convention): the command loop owns
//! the pipeline passes and their timing (it is the only place the
//! `Context` pipeline is callable); this module owns the plain-data
//! reduction — grouping the samples per stage, the percentile math and
//! the deterministic stage order. The stage names ride the `--profile`
//! trace's own span vocabulary (`discover`, `config`, `history`,
//! `plan`), so a bench row and a trace span with the same name measure
//! the same phase.
//!
//! The report is a **measurement**, never a gate: the §14.1 budgets in
//! `bench/budgets.toml` are shape-bound to the synthetic fixtures the
//! xtask harness measures, so a repo-shaped bench prints its own shape
//! (packages, pending commits, cache posture) beside the timings and
//! borrows no budget. Every rendering is deterministic given the same
//! samples (I3) — the values themselves are wall-clock and change run
//! to run.

/// One recorded stage sample: the pipeline stage (the `--profile`
/// trace's span name) and one pass's wall-clock duration in
/// microseconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StageSample {
    /// The stage name (`discover`, `config`, `history`, `plan`).
    pub stage: &'static str,
    /// The pass's duration, integral microseconds (the trace's unit).
    pub duration_us: u128,
}

/// One stage's aggregated distribution: the per-pass samples reduced to
/// the nearest-rank percentiles the §14 budget table speaks (`p50`,
/// `p99`) plus the min and the integer mean. Plain data, comparable
/// across reports (I3 for the structure; the values are wall-clock).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StageStats {
    /// The stage name (the trace span's).
    pub stage: &'static str,
    /// How many passes contributed (one sample per pass).
    pub samples: usize,
    /// Nearest-rank 50th percentile, microseconds.
    pub p50_us: u128,
    /// Nearest-rank 99th percentile, microseconds.
    pub p99_us: u128,
    /// The fastest pass, microseconds.
    pub min_us: u128,
    /// The integer (floor) mean, microseconds.
    pub mean_us: u128,
}

/// The assembled bench report: the posture the caller declares (passes,
/// cache mode) plus the repo shape the timings belong to and the
/// name-sorted stage distributions. Rendered by all three formats.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BenchReport {
    /// The timed pass count (the `--iterations` value; the warmup pass
    /// is untimed and never sampled).
    pub iterations: u32,
    /// `true` = the §8.7 per-commit cache warmed the passes (the §14.1
    /// warm model); `false` = `--no-cache`, the cold measurement.
    pub cache_warm: bool,
    /// Packages the discovery found (the measured workspace's size).
    pub packages: usize,
    /// Commits the attribution walk carried (the pending history's
    /// size; zero on a fully-released repo — the no-op-warm shape).
    pub pending_commits: usize,
    /// One distribution per stage, name-sorted (I3).
    pub stages: Vec<StageStats>,
}

/// The nearest-rank percentile of an **ascending-sorted** sample list:
/// the `ceil(p/100 × n)`-th smallest value (1-based), the definition
/// the §14 budget table's p50/p99 speak. The percentile is an integer
/// percent (50, 99, 100 — the vocabulary the report renders), so the
/// rank is exact integer ceiling arithmetic, no floats. `None` only
/// for an empty sample list (a stage with no samples has no
/// distribution).
#[must_use]
pub fn nearest_rank(sorted: &[u128], percentile: u32) -> Option<u128> {
    if sorted.is_empty() {
        return None;
    }
    let len = u32::try_from(sorted.len()).ok()?;
    let rank = (percentile * len).div_ceil(100);
    let index = usize::try_from(rank.max(1)).ok()? - 1;
    sorted.get(index).copied()
}

/// Reduces the samples per stage into name-sorted
/// [`StageStats`] (I3). A stage with zero samples is omitted — a
/// distribution needs at least one pass; the caller's pass loop
/// guarantees the pipeline's four stages carry one sample each.
#[must_use]
pub fn aggregate(samples: &[StageSample]) -> Vec<StageStats> {
    let mut by_stage: Vec<(&'static str, Vec<u128>)> = Vec::new();
    for sample in samples {
        let entry = by_stage
            .iter_mut()
            .find(|(stage, _)| *stage == sample.stage);
        match entry {
            Some((_, durations)) => durations.push(sample.duration_us),
            None => by_stage.push((sample.stage, vec![sample.duration_us])),
        }
    }
    by_stage.sort_unstable_by_key(|(stage, _)| *stage);
    by_stage
        .into_iter()
        .filter_map(|(stage, mut durations)| {
            if durations.is_empty() {
                return None;
            }
            durations.sort_unstable();
            let sum: u128 = durations.iter().sum();
            let len = durations.len();
            Some(StageStats {
                stage,
                samples: len,
                p50_us: nearest_rank(&durations, 50)?,
                p99_us: nearest_rank(&durations, 99)?,
                min_us: *durations.first()?,
                mean_us: sum / u128::try_from(len.max(1)).unwrap_or(1),
            })
        })
        .collect()
}

/// Renders a microsecond duration for the human/gha tables: integral
/// microseconds below a millisecond, milliseconds with two decimals
/// below a second, seconds with two decimals above — the magnitudes a
/// planning pipeline actually spans. The decimals are exact integer
/// floor arithmetic on the remainder (no float round-trip), so the
/// rendered value is derived from the sample, not re-parsed.
#[must_use]
pub fn format_duration(micros: u128) -> String {
    if micros < 1_000 {
        format!("{micros}µs")
    } else if micros < 1_000_000 {
        format!("{}.{:02}ms", micros / 1_000, micros % 1_000 / 10)
    } else {
        format!("{}.{:02}s", micros / 1_000_000, micros % 1_000_000 / 10_000)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_rank_follows_the_ceil_definition() {
        // The §14 budget table's p50/p99 over 8 samples: p50 lands on
        // the 4th smallest (ceil(0.5 × 8) = 4), p99 on the maximum
        // (ceil(0.99 × 8) = 8).
        let samples = [3, 1, 4, 1, 5, 9, 2, 6];
        let mut sorted = samples;
        sorted.sort_unstable();
        assert_eq!(sorted, [1, 1, 2, 3, 4, 5, 6, 9]);
        assert_eq!(nearest_rank(&sorted, 50), Some(3));
        assert_eq!(nearest_rank(&sorted, 99), Some(9));
        assert_eq!(nearest_rank(&sorted, 100), Some(9));
    }

    #[test]
    fn nearest_rank_handles_one_sample_and_the_empty_list() {
        // One sample is its own distribution at every percentile; the
        // empty list has none (the honest None).
        assert_eq!(nearest_rank(&[42], 50), Some(42));
        assert_eq!(nearest_rank(&[42], 99), Some(42));
        assert_eq!(nearest_rank(&[], 50), None);
    }

    #[test]
    fn aggregate_groups_sorts_and_reduces() {
        // Two stages, deliberately out of name order; the aggregation
        // name-sorts (I3), counts the samples and reduces each to the
        // pinned percentile/min/mean.
        let samples = [
            StageSample {
                stage: "plan",
                duration_us: 100,
            },
            StageSample {
                stage: "discover",
                duration_us: 20,
            },
            StageSample {
                stage: "plan",
                duration_us: 300,
            },
            StageSample {
                stage: "discover",
                duration_us: 10,
            },
        ];
        let stats = aggregate(&samples);
        assert_eq!(
            stats,
            vec![
                StageStats {
                    stage: "discover",
                    samples: 2,
                    p50_us: 10,
                    p99_us: 20,
                    min_us: 10,
                    mean_us: 15,
                },
                StageStats {
                    stage: "plan",
                    samples: 2,
                    p50_us: 100,
                    p99_us: 300,
                    min_us: 100,
                    mean_us: 200,
                },
            ]
        );
    }

    #[test]
    fn aggregate_of_no_samples_is_empty() {
        assert_eq!(aggregate(&[]), []);
    }

    #[test]
    fn durations_render_at_their_magnitude() {
        // µs below a millisecond, ms below a second, s above — the
        // three magnitudes the pipeline stages span.
        assert_eq!(format_duration(812), "812µs");
        assert_eq!(format_duration(1_234), "1.23ms");
        assert_eq!(format_duration(987_654), "987.65ms");
        assert_eq!(format_duration(1_500_000), "1.50s");
    }
}

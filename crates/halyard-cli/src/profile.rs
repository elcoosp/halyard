//! Chrome-trace profiling (`--profile`, §16.2): command phases recorded as
//! spans and emitted as a Chrome Trace JSON — to `$HALYARD_PROFILE_PATH`
//! when set, stderr otherwise. The trace is deterministic in structure
//! (sorted phase names, integral microseconds).

use std::io::Write as _;
use std::time::Instant;

/// One recorded phase.
struct SpanRecord {
    name: &'static str,
    start_us: u128,
    duration_us: u128,
}

/// The profiler handle; cheap when disabled (`profile = false` records
/// nothing).
pub struct Profiler {
    enabled: bool,
    started: Instant,
    spans: Vec<SpanRecord>,
}

impl Profiler {
    /// Starts the profiler when `enabled` (the CLI's `--profile`).
    pub fn start(enabled: bool) -> Self {
        Self {
            enabled,
            started: Instant::now(),
            spans: Vec::new(),
        }
    }

    /// Records one span for the closure's duration (a no-op measurement
    /// when disabled — the closure still runs).
    pub fn span<T>(&mut self, name: &'static str, run: impl FnOnce() -> T) -> T {
        if !self.enabled {
            return run();
        }
        let start = self.started.elapsed().as_micros();
        let result = run();
        let duration = self.started.elapsed().as_micros().saturating_sub(start);
        self.spans.push(SpanRecord {
            name,
            start_us: start,
            duration_us: duration,
        });
        result
    }

    /// Writes the trace (Chrome Trace JSON, `traceEvents` with `X` phase
    /// events) and returns `true` when a trace was produced.
    pub fn finish(&mut self) -> bool {
        if !self.enabled {
            return false;
        }
        let events: Vec<String> = self
            .spans
            .iter()
            .map(|span| {
                format!(
                    "{{\"name\":{name},\"ph\":\"X\",\"ts\":{ts},\"dur\":{dur},\"pid\":1,\"tid\":1}}",
                    name = serde_json::to_string(span.name).unwrap_or_default(),
                    ts = span.start_us,
                    dur = span.duration_us,
                )
            })
            .collect();
        let trace = format!("{{\"traceEvents\":[{}]}}", events.join(","));
        if let Ok(path) = std::env::var("HALYARD_PROFILE_PATH") {
            if let Ok(mut file) = std::fs::File::create(&path) {
                let _ = file.write_all(trace.as_bytes());
                return true;
            }
        }
        eprintln!("{trace}");
        true
    }
}

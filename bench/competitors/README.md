# Competitor benchmark harness (M0.3)

Measures competitor release tooling against the same corpus fixtures HALYARD
will be measured on, so the differentiator claims of §1.1 become numbers
instead of marketing.

## Layout

| Path | Role |
|---|---|
| `tools.toml` | Invocation recipes (argv transcribed from each tool's `--help`; nothing invented) |
| `claims.toml` | The §1.1 claim table, one row per claim |
| `results/` | Raw JSON per (tool, fixture) run — gitignored |
| `REPORT.md` | Generated aggregate: claims status + run table — committed |

## Running

```bash
# fetch a corpus fixture (fd is the small smoke repo):
cargo run -p xtask -- corpus fetch --shallow --only fd

# run every offline-capable tool on every fetched clone:
cargo run -p xtask -- bench-competitors

# one tool, one fixture:
cargo run -p xtask -- bench-competitors --tool cocogitto --fixture corpus/clones/fd

# networked recipes (release-please needs the GitHub API and a token; child
# processes inherit the environment, so export what the tool reads, e.g.):
GH_TOKEN=… cargo run -p xtask -- bench-competitors --allow-network

# regenerate REPORT.md:
cargo run -p xtask -- bench-competitors --report
```

## Rules

1. **No invented behaviour.** A tool without an observed CLI surface has no
   entry in `tools.toml`; its claims stay `pending` in `REPORT.md`.
2. Tokens are passed via environment variables (exported before invoking
   xtask; child processes inherit them) and are never written into
   `tools.toml` or results.
3. Runs happen in disposable clones (`results/work-<tool>/`); nothing a
   competitor writes can touch the corpus clone or the workspace.
4. Every claim status in `REPORT.md` cites its result file; without a result
   file there is no status beyond `pending`.

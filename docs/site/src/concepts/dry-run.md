# Dry-run and preview everywhere

Every stage that can touch the world has a dry-run mode: `plan`, `apply`,
`pr`, `release`, and `publish` all accept `--dry`. The guarantee is simple —
**a dry run classifies and reports exactly what the live run would do, and
performs none of it**.

## Preview adapters

The dry runs are not a separate code path with guessed outputs. The same
executors run, over **preview implementations** of the effect seams:

- `PreviewForge` probes see an empty forge, mutations are forecasts marked
  with the `preview://` shape, and the call log records what the live run
  would have spent.
- `PreviewTagWriter` validates targets against the real repository — the
  classification (would-create, already-tagged, conflict) is identical to the
  live run — but writes forecasted tag objects and no refs.
- `PreviewTagPusher` reports the per-tag push outcomes a live push would
  produce, offline.
- `PreviewActionWriter` records the ordered output and summary calls a real
  GitHub Actions step would receive, so local runs observe the exact wire
  shapes.

A forecast is visibly a forecast: identifiers carry the `preview://`/`0`
shapes, and the `released` output of an action dry run is honestly `false`.

## Fail-closed, never guessed

Where a live run cannot proceed, it refuses with a stable error code instead
of approximating: missing credentials decline before any network call, a
non-resolvable registry route refuses before any spawn, an unparseable
response is a protocol error rather than a silent "not published". The
refusal paths are the same in dry mode where classification would be
required, so a green dry run means the live run's inputs are well-formed.

## Determinism

Dry-run output is byte-stable: same repository state, same bytes. That is
what makes the forecasts testable as snapshots and the JSON documents
stable across CI runs. The one deliberate exception to byte-identical
re-runs is documented in the repository's replay notes, where the
byte-gated artifact is the replay report itself.

# ADR-0029: the `halyard init` zero-config scaffold (§16.1/§16.2)

- **Status:** accepted
- **Date:** 2026-11-16
- **Milestone:** the §16.2 `init` row — the command the previous two
  sessions both deferred as the next start point ("`halyard init` …
  needs its own red→green cycle, CLI gates, docs row and an example")
- **Deciders:** autonomous implementation agent (per §0 rule 3)

## Context

Zero-config is the architecture's opening promise (§16.1): `halyard
plan` works in any Cargo/pnpm/single-package repo with no config file.
What the repo never had is the on-ramp — §16.2's `init` row ("scaffold
workflow/config (never required)") was the last wired-commands gap, and
two sessions had declared it the next start point and deferred it on
budget. The building blocks all existed: the discovery pipeline the
shell already runs, the strict loader that validates any surfaced draft
(M1.8), and the ADR-0023 write machinery — the `DraftSink` seam,
`plan_draft_write`'s validate-then-act shape and the atomic protocol
(temp sibling → fsync → rename). The capability was missing, not the
seam.

Constraints that shaped the design:

1. **The scaffold never sets keys.** Zero-config means zero-config: the
   skeleton carries the documented defaults as comments and *loading it
   yields the built-in defaults exactly*. If `init` silently pinned
   today's defaults, every future default change would strand
   scaffolded repos — a config file that claims to be optional must
   actually be optional. The key-free invariant is checked at plan time
   (`plan_init` re-loads the rendered skeleton and refuses anything
   that stops being key-free) and pinned by tests.
2. **Create-only, gated before any write.** The migration's refusal
   discipline (ADR-0023) generalizes: `run_init` checks *every*
   target's existence before the first write, so a refusal never leaves
   a half-scaffold (config without workflow, or the reverse). The sink
   re-checks at write time — the gate is a contract, not a race-free
   guarantee, and the atomic protocol keeps a failed write from
   leaving a partial file.
3. **One seam, two sinks.** The scaffold writes through the same
   `DraftSink` seam the migration uses (`MemoryDraftSink` records in
   tests), but the production sink is a distinct
   `AtomicScaffoldSink`: the migration writes `halyard.toml` into an
   existing repo root and its sink deliberately never creates
   directories, while the scaffold's `.github/workflows/release.yml`
   needs the parent directories created. Same atomic protocol, one
   documented behavioral difference, two types — the alternative
   (a flag on `AtomicDraftSink`) would have made the migration's
   never-mkdir promise conditional and easy to regress.
4. **The survey is plain data.** Discovery stays in the shell: the
   engine takes an `InitSurvey` (package count, per-package
   ecosystems, single-package shape) and never touches the
   filesystem. The shape line is the survey's pure rendering, keyed on
   the same count-based rule the planner's `auto` tag template applies
   (`effective_template`), so what `init` prints and what the tag
   phase will name can never disagree.
5. **The workflow is the §16.1 document, verbatim.** `--github`
   generates exactly the one file the architecture specifies
   (`actions/checkout@v4` at full history + `halyard/action@v1`). The
   action glue itself is a later milestone (§16.2 keeps it absent
   rather than stubbed); the workflow references the finished surface
   the way the architecture documents it, and the human render says
   so instead of hiding the dependency.

## Decision

- `halyard-config` gains an `init` module (ADR-0029): `InitSurvey` →
  `plan_init` (pure; refuses a zero-package survey with `E-CFG-INIT`) →
  `InitPlan` (the shape line + ordered `ScaffoldFile`s) → `run_init`
  (gate-then-write over `&dyn DraftSink`) → `InitReport`;
  `AtomicScaffoldSink` is the production sink.
- `ConfigError` gains the `Init` variant with the stable `E-CFG-INIT`
  code (page + index row) — the scaffold's refusals (zero packages,
  existing target, failed atomic write) fail closed like the
  migration's.
- The shell (`halyard-cli`) gains the `init [--github]` command: the
  conventional-config footgun check first (an existing
  `.config/halyard.toml` refuses — a root scaffold would shadow it,
  and `resolve_config` prefers the root file), then the survey, the
  pure plan, the effect, and the three-format render.
- The skeleton documents the v1 sections with their §16.4 defaults as
  comments; the workflow is the §16.1 YAML verbatim.

## Consequences

- **The on-ramp exists.** `halyard init` → `halyard plan` is a working
  zero-config loop (the CLI test runs both against one fixture); the
  scaffolded file can stay untouched forever without changing a single
  plan.
- **The key-free invariant is load-bearing.** Someone adding a default
  key to the skeleton template fails the plan-time check and the
  engine tests — the scaffold cannot silently grow opinions.
- **`E-CFG-*` gains its eighth code.** The page/index/producer
  discipline holds: one variant, one page, one index row, three
  producers (the engine's gate, the shell's footgun check, the sink).
- **The workflow lands before the action does.** Until the `halyard/
  action` glue milestone lands, a scaffolded workflow points at a
  reference that does not resolve on GitHub yet — stated in the human
  render and here, not stubbed around.

## Alternatives considered

- **Generate values, not comments.** Rejected (see Context 1): the
  scaffold's value is documentation of the knobs, not a second source
  of defaults that can drift from the resolver.
- **Skip the config file when nothing is pinnable, only write with
  `--github`.** Rejected: §16.2's row says "scaffold workflow/config" —
  the config skeleton *is* the discoverability surface ("what can I
  tune?"), and a comment-only file is a valid, load-bearing v1
  document.
- **A `--force` flag.** Rejected: the create-only contract is what
  makes the command safe to hand a stranger; the merge instruction in
  the refusal is the documented path (ADR-0023's reasoning, reused).

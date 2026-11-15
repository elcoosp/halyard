# ADR-0019: the publish-auth plumbing seam (M5.3 step 1, §13)

- **Status:** accepted
- **Date:** 2026-11-15
- **Milestone:** M5.3 step 1 — the `PublishAuth` seam, the
  `EnvPublishAuth` production resolution over the VERIFIED env grammar,
  the `E-PUB-AUTH` fail-closed misconfiguration and the secret
  scrubbing of surfaced tool output; the spawn effect consumes the
  seam (M5.3 step 2 lands the render/doctor observability)
- **Deciders:** autonomous implementation agent (per §0 rule 3 against
  the existing seam conventions; the wire grammar was VERIFIED per
  §0.4 — `docs/research/publish-auth-env.md` — no new external facts
  left open beyond the recorded VERIFY items)

## Context

§13 step 4's own words: the publish effect is "shell out to
`cargo publish` / `pnpm publish` — they own packaging semantics; our
job: ordering, resume, **auth plumbing**". M5.2 shipped the ordering
and the resume; the auth plumbing did not exist. The concrete gaps at
base:

- `SpawnPublishEffect` inherited the ambient environment silently — a
  token reached the tool only if the user had already exported it, and
  nothing told them which variable the run would authenticate with.
- A tool's stderr tail landed **verbatim** in the report row's message
  (`PublishError::Effect`) and from there into the JSON/human/GHA
  renderers — a token a misconfigured tool echoes leaks into every
  renderer.
- A present-but-empty token variable (the `${VAR}` auth-file
  interpolation with an unset secret) would ride the spawn as an empty
  bearer token and surface as a confusing registry 401.
- `docs/research/publish-tooling.md` had explicitly deferred
  `--registry` mapping and auth to "M5.3's auth plumbing"; PROGRESS.md
  declared M5.3 the next start point.

Three constraints shaped the design:

1. **The engine owns no I/O.** Auth resolution is a shell concern; the
   engine must not read the process environment (the functional-core
   rule every other seam follows).
2. **halyard never writes auth files.** pnpm's own docs refuse to
   expand `${…}` credentials in the *project* `.npmrc` because "a
   malicious repository could otherwise send your CI tokens to a
   registry it controls" — a tool that writes token files would
   reproduce exactly that vector (halyard runs *inside* the repo it
   publishes).
3. **Secrets must be inert data.** No `Debug`/`Display` path may
   render a token value; anything surfaced from the spawn passes the
   scrubber.

## Decision

1. **The seam lives in the engine crate, the resolution in the
   shell.** `halyard_publish::PublishAuth` (`credentials(&target) →
   Result<AuthOutcome, PublishError>` + `secrets() → Vec<String>`) is
   object-safe and `Send + Sync` (the bounded within-layer parallelism
   shares the effect, and the effect shares the provider). The shell
   captures `EnvPublishAuth::capture()` once per run and hands it to
   `SpawnPublishEffect` — the same "the shell owns the map" shape as
   the `RegistryRouter`.
2. **Absent ≠ error; empty = fail-closed.** No documented candidate
   present ⇒ `AuthOutcome::Absent` — the tools' own credential stores
   (`cargo login`, the user-level auth file) govern and halyard cannot
   see them; pretending otherwise would be a guess. A present-but-empty
   candidate ⇒ `PublishError::Auth` (`E-PUB-AUTH`) naming the variable,
   refused before any spawn (the capture refuses at the CLI level for
   run-level honesty; the seam refuses per-target for scripted
   providers).
3. **The grammar is the tools' own, VERIFIED** (`variables_for`, the
   documented candidate lists in resolution order):
   cargo → `CARGO_REGISTRY_TOKEN` (crates.io; the
   `CARGO_REGISTRIES_CRATES_IO_TOKEN` mapping is the documented
   fallback), `CARGO_REGISTRIES_<NAME>_TOKEN` for other registries
   (NAME uppercased, dots/dashes → underscores — the config
   reference's env rule); npm → `NPM_TOKEN`, then `NODE_AUTH_TOKEN`
   (the user-level auth-file interpolation contract; the project-file
   interpolation is deliberately absent per pnpm's anti-exfiltration
   rule). Unknown ecosystems resolve to `Absent` — no invented
   grammar.
4. **Env injection is not the point — the contracts around it are.**
   For environment sources the spawned child inherits the parent's
   environment anyway; the seam's net-new capability is (a) resolution
   and provenance before the run (doctor/M5.3 step 2), (b) the
   fail-closed empty-token refusal, (c) the scrubbing contract, and
   (d) the injection point a non-env provider (secret store, M5.3
   later steps) fills without touching the effect again. The ADR
   records this honestly so nobody mistakes the plumbing for a token
   *transport*.
5. **Scrubbing is a pure, engine-side helper.**
   `scrub_secrets(text, secrets)` replaces every occurrence of every
   non-empty resolved value with the `[secret]` marker; the spawn
   effect runs the stderr tail through it before wrapping it in
   `PublishError::Effect`. The provider's `secrets()` list (sorted by
   variable name — deterministic, I3) is the contract: a value
   `credentials` can resolve is a value `secrets` lists.
6. **The stderr tail picks the last non-empty line** (the M5.2
   `next_back()` on a blank-line-terminated stderr produced an empty
   diagnosis — micro-debt repaired in the same lines, recorded
   separately in the PR body).

## Consequences

- `E-PUB-AUTH` joins the taxonomy (page + index row); `PublishError`
  gains the structural `Auth` variant (never retryable).
- The engine crate stays std-only; no layer-rule change.
- The dry forecast resolves and injects credentials identically to the
  live run (the forecast cannot disagree with the live run) — and the
  capture-level refusal (empty token) fails both modes before any
  spawn.
- OIDC/trusted publishing stays M5.3-later (its own VERIFY: the M0.6
  research note's requirements — `id-token: write`, hosted runners —
  are CI-runtime facts this sandbox cannot exercise); the `--registry`
  flag mapping stays deferred (the plan's §13 registry name maps to
  the token env var, but a cargo `--registry` config-name mapping is a
  user-config guess).

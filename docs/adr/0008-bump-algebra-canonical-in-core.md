# ADR-0008: The bump/rewrite algebra's canonical implementation moves into `halyard-core`

- **Status:** accepted
- **Date:** 2026-11-12
- **Milestone:** M1 (slice C: M1.9)
- **Deciders:** autonomous implementation agent (deviation from the §7.1
  crate split → documented here per §0 rule 3)

## Context

§7.1 assigns the "Bump algebra" to `halyard-version`, and §7.2 forbids
`halyard-core` workspace dependencies. The M1.9 planner lives in
`halyard-core::planner` (§4) and needs exactly that algebra: `apply_bump`
(with the pre-major policy), the commit-type mapping and the requirement
rewriter (`rewrite_requirement`, appendix A.2) to emit the plan's
dependent-requirement edits. Slice A shipped the algebra in
`halyard-version`; Slice B's discovery meanwhile confirmed (ADR-0006) that
requirement *text* is the rewriter's input, so the planner must be able to
call it directly. With the layer rules as written, core cannot call
`halyard_version::*`.

## Decision

The **pure, canonical implementations** of the algebra move into core:

- `halyard_core::bump` — `apply_bump`, `PreMajorPolicy`,
  `CommitBumpMapping` (the bodies from `halyard-version`'s `algebra.rs`).
- `halyard_core::rewrite` — `rewrite_requirement`, `UpperBoundPolicy`,
  `RewriteOutcome`, `UnchangedReason`, `UnrewritableReason`, `VersionError`
  (the bodies from `halyard-version`'s `rewrite.rs`).

`halyard-version` keeps every public path and signature: its `algebra.rs`
and `rewrite.rs` become `pub use halyard_core::…` re-export shims, so all
Slice A call sites and tests compile and run unchanged. Prerelease
channels (`PrereleaseChannel`, `advance_prerelease`) are release-time
logic with no planner consumer and stay in `halyard-version` untouched.

Core's external dependency set does not change (the algebra only uses
`semver`, already sanctioned). `xtask check-layers` continues to enforce
`halyard-version → halyard-core` only.

## Consequences

- The planner and the rewriter share one implementation: appendix A.2's
  golden vectors are now executed by both the version-crate tests (via the
  re-exports) and the planner's edit-emission path — no drift is possible.
- `halyard-version` remains the *policy home* (0.x policy documentation,
  prerelease channels) and the ergonomic import path for CLI code; core is
  the *mechanism home*.
- The move is behavior-neutral: the moved bodies are byte-identical, and
  `halyard-version`'s existing test suite (≈90 cases incl. proptests)
  guards the re-export path.

## Alternatives considered

1. **Duplicate the algebra in core** — rejected: two implementations of
   the A.2 table is exactly the drift the golden vectors exist to prevent.
2. **Move the planner to a new crate between version and config** —
   rejected: §4 fixes `halyard-core::planner`, and the plan hash (I8)
   belongs next to the canonical-JSON emitter.
3. **Dependency-inject the algebra as closures** — rejected: data knobs
   (ADR-0007) keep `plan()` deterministic and inspectable; closures would
   make golden plans unverifiable from the inputs alone.

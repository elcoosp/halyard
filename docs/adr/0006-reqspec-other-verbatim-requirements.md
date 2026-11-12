# ADR-0006: `ReqSpec::Other` carries ecosystem-specific requirement text

- **Status:** accepted
- **Date:** 2026-11-12
- **Milestone:** M1 (slice B: M1.5/M1.6)
- **Deciders:** autonomous implementation agent (deviation from §5.3 →
  documented here per §0 rule 3)

## Context

§5.3 enumerates `ReqSpec` variants (`VersionReq`, `WorkspaceProtocol`,
`Catalog`, `PathOnly`). Real manifests carry requirement grammars outside
that set: npm ranges such as `>=8 <10` or `1.x` (node-semver unions and
bare-x ranges), `git:`/`http:` URLs. A strict enum forces discovery to
either fail the whole run on one unparseable string (unacceptable for
corpus parity) or silently drop the requirement (worse: the version
rewriter needs to know an edit site exists).

## Decision

`ReqSpec` gains a fifth variant, `Other { raw: BString }`: the requirement
text is carried verbatim with its normal `ManifestSite` edit site.
Discovery never errors on it; the version rewriter (M1.3 `rewrite.rs`)
treats it as unrewritable (`E-EDIT-UNREWRITABLE` semantics). The
differential comparators normalize it away because package managers
resolve these requirements outside the workspace graph.

## Consequences

- §5.3's variant list is amended to five; the wire format of
  `halyard-plan/v1` is unaffected (an `Other` req renders through its
  raw text when a plan references the site).
- The `halyard-version` rewriter needs a match arm for `Other` when it
  learns `ReqSpec` (slice C task, noted in PROGRESS.md).
- Adapters construct it through `halyard_core::BString` (the re-export)
  so `bstr` stays out of the adapter dependency set.

## Alternatives considered

1. **Error on unparseable requirements** — rejected: one npm range would
   fail discovery of a whole repo.
2. **Add one variant per grammar** (`NodeSemver`, `GitUrl`, …) — rejected:
   premature; parsing foreign grammars properly is ecosystem work that
   belongs in the adapters when the rewriter (M3) needs it.
3. **Drop the requirement silently** — rejected: loses the edit site and
   hides information from the plan.

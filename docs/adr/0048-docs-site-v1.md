# ADR-0048: the docs site v1 — mdBook, with the link gate as a workspace gate

- **Status:** accepted
- **Date:** 2026-11-24
- **Milestone:** M4.8 — "Docs site v1 (mdBook/Starlight): quickstart,
  concepts, reference, errors | link-checked in CI" (the §17 milestone
  table)
- **Deciders:** autonomous implementation agent (per §0 rule 3)

## Context

M4.8 names two renderer options (mdBook/Starlight) and an acceptance
half ("link-checked in CI") that has no obvious owner: the §15.6 CI
battery hosts every other acceptance gate as an `xtask` subcommand, but
a renderer's own link checker is an external plugin with its own
maintenance posture, and it would not see repo-escaping links (site
pages pointing at canonical repository documents) the same way the
rendered output does. The site also has a content-sync hazard the
render cannot catch: the `reference/errors.md` page mirrors the
`docs/errors/README.md` index, and nothing stops the two from drifting.

The tree, at decision time, had no site: no `book.toml`, no renderer
configuration, no docs-site CI job (the M4.8 row was the only `mdBook`
mention in the repository). The renderer choice is otherwise
constrained by the workspace itself: the repository is deliberately
JS-free, the toolchain is Rust, and the CI installs tools through
`taiki-e/install-action`.

## Decision

- **mdBook** is the renderer: a Rust-native tool, installed in CI the
  same way every other gate tool is, with zero Node toolchain. The book
  lives at `docs/site` (`book.toml` + `src/`), the mdBook-conventional
  layout, so the renderer needs no configuration beyond the title and
  the repository links.
- **The link check is a workspace gate, not a plugin**: `xtask
  check-docs-site` validates the summary grammar (every entry's file
  exists, no duplicates, no pure-anchor entries), page reachability (no
  orphan `.md` under the book `src` — mdBook silently drops unlisted
  files), every relative link (page-relative → book → repository root
  for targets that escape the book), `file#anchor` targets against
  GitHub-style heading slugs, `{{#include}}` directives, and the
  error-table mirror. External `http(s)` targets are counted, never
  fetched — the gate stays offline and deterministic (I5). The CI
  `docs-site` job runs the gate and then a real `mdbook build`, so a
  book that no longer renders cannot hide behind a structurally valid
  tree.
- **The errors table is generated** from the `docs/errors/README.md`
  index, and the gate enforces set-equality in both directions: a code
  on the site without a page, or an index row missing from the site,
  fails the gate.
- **The pages are hand-authored summaries** of the canonical
  in-repository documents (README, architecture document, CLI README,
  error pages); each page names its canonical source and links to it.
  Content duplication is bounded by keeping the site pages
  short-form/pointer-shaped, with the generated errors table as the one
  mechanically-mirrored surface.

## Consequences

- Easier: the M4.8 acceptance ("link-checked in CI") is satisfied by a
  gate that runs identically locally and in CI; broken cross-references
  — including repo-escaping ones and anchor drift — fail a PR.
- Easier: a new error code cannot ship without its site row (the gate
  names the missing code), closing the mirror-drift hazard.
- Harder: the site's hand-authored pages can drift from their canonical
  sources; the gate cannot prove semantic freshness, only structural
  health plus the errors mirror. Page updates must ride the same change
  as the canonical-document change (the contributing page says so).
- Harder: the anchor slug check is an over-approximation of GitHub's
  slug algorithm (plain ASCII prose scope, documented in the gate
  module); a heading that slugs differently on GitHub would need the
  link rewritten.
- Revisit: deployment (GitHub Pages or similar) is not part of M4.8;
  when it lands, the repo-escaping links in `contributing.md` and the
  architecture page need deployment-rewritten targets, and the gate's
  escape-fallback rule stays the correctness anchor.

## Alternatives considered

- **Starlight (Astro)**: richer theming and search, but it drags a Node
  toolchain and a build stack into a deliberately JS-free Rust
  repository, and its link checker has the same repo-escaping blind
  spot. Lost on toolchain alignment.
- **Renderer's own link checker only (`mdbook-linkcheck`)**: would
  validate the rendered book, but not the summary grammar, orphan pages,
  the errors mirror, or repo-escaping targets consistently, and it adds
  an external plugin dependency to CI. Lost on the §15.6 "the gates are
  workspace gates" convention.
- **No site until deployment is decided**: defers a plan milestone
  indefinitely; M4.8's acceptance is in-tree (content + CI check), and
  deployment is explicitly not in the row. Lost — the milestone is
  shippable without the deployment decision.

# Research: `--shallow-exclude`, partial clone & forge support (VERIFY, M0.6)

**Checked:** 2026-11-12 · **Source:** git-scm.com, git-fetch(1) for git
2.56.0 <https://git-scm.com/docs/git-fetch> (fetched live)

## Verified facts (client-side)

- `--shallow-exclude=<ref>`: "Deepen or shorten the history of a shallow
  repository to exclude commits reachable from a specified remote branch or
  tag." Repeatable; refs resolve against the **remote** being fetched; it
  modifies an already-shallow repo's boundary rather than being a standalone
  mode. Composes with `--depth` / `--deepen` / `--shallow-since`.
- **Tag caveat (documented):** "Tags for the deepened commits are not
  fetched." Default tag auto-following ("any tag that points into the
  histories being fetched is also fetched") therefore does not cover the
  deepened window — tag-bound fetches must fetch tags explicitly.
- `--filter=<spec>` uses **partial clone** over protocol v2 promisor
  remotes; the filter spec must be advertised by the server
  (`promisor.acceptFromServer`, `--filter=auto`). Filtered blobs are fetched
  lazily ("`--filter=blob:none` will filter out all blobs … until needed by
  Git").
- `git fetch` refuses by default to update `.git/shallow` when fetching from
  a shallow source (`--update-shallow` overrides).
- Local-path clones imply `--local`; `file://` URLs take the real transport
  — which is why `GitFixture` forces `file://` + `--no-local` (ADR-0003).

## Open VERIFY items

- **Forge server-side matrix** (GitHub/GitLab/Gitea: `uploadpack.allowFilter`,
  `uploadpack.allowReachableSHA1InWant`, `allowAnySHA1InWant`): git-upload-pack
  documents the knobs but per-forge enablement is not documented here — must
  be probed per forge before M8/M9 (smart fetch). Do **not** assume GitHub
  behaviour from folklore.
- Observed in this workspace (2026-11-12): `git clone --depth 1` fetches
  **no tags** (the fd corpus clone has an empty tag set), which broke the
  tag-anchored `cog bump --auto` competitor run — recorded in
  `bench/competitors/REPORT.md`. Consequence: tag-dependent measurement or
  base-resolution runs need full (`--full`) or `--tags` fetches.

## Design consequences (feeds M2/M8)

- The §8.1 tag-bound minimal fetch is supported exactly as specified:
  shallow start + `git fetch --depth=N --shallow-exclude=<old-tag> <remote>`,
  followed by an explicit tag fetch for the window.
- Partial clones are a first-class input: byte-offset painting must force
  materialization of manifest paths before scanning (blob-lazy objects).

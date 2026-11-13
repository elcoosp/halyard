# Error pages

Stable error codes (§15.4): every user-facing error variant carries a code
of the form `E-<AREA>-<NAME>` and gets a page in this directory with:

1. **What it means** — the exact condition.
2. **Cause** — why it happens.
3. **Fix** — what the user can do.
4. **Command** — a copy-pasteable command to diagnose or recover.

The rendered output goes through `miette` with the code as the report code;
the page is linked from the variant's rustdoc (`#[doc = include_str!(…)]`
pattern from M1 onward where useful).

## Index

| Code | Page | Area |
|---|---|---|
| `E-CFG-UNKNOWN-KEY` | [E-CFG-UNKNOWN-KEY.md](E-CFG-UNKNOWN-KEY.md) | config loader (M1.8) |
| `E-CFG-TAG-STYLE` | [E-CFG-TAG-STYLE.md](E-CFG-TAG-STYLE.md) | config tag template (M1.8) |
| `E-CFG-TOML` | [E-CFG-TOML.md](E-CFG-TOML.md) | config read/parse (M1.8) |
| `E-CFG-ENV` | [E-CFG-ENV.md](E-CFG-ENV.md) | config env layer (M1.8) |
| `E-EDIT-PATCHES` | [E-EDIT-PATCHES.md](E-EDIT-PATCHES.md) | edit patch validation (M1.4) |
| `E-EDIT-VERIFY` | [E-EDIT-VERIFY.md](E-EDIT-VERIFY.md) | edit re-parse verification (M1.4) |
| `E-EDIT-IO` | [E-EDIT-IO.md](E-EDIT-IO.md) | edit atomic write (M1.4) |
| `E-EDIT-UNREWRITABLE` | [E-EDIT-UNREWRITABLE.md](E-EDIT-UNREWRITABLE.md) | version requirement rewriter (M1.3, §A.2) |
| `E-TEST-FIXTURE` | [E-TEST-FIXTURE.md](E-TEST-FIXTURE.md) | test-support fixture failures |
| `E-DISC-IO` | [E-DISC-IO.md](E-DISC-IO.md) | discovery filesystem walk (M1.5) |
| `E-DISC-MANIFEST` | [E-DISC-MANIFEST.md](E-DISC-MANIFEST.md) | discovery manifest parse (M1.5) |
| `E-DISC-NESTED-WS` | [E-DISC-NESTED-WS.md](E-DISC-NESTED-WS.md) | discovery nested workspaces (M1.5) |
| `E-DISC-GLOB` | [E-DISC-GLOB.md](E-DISC-GLOB.md) | discovery member globs (M1.5) |
| `E-DISC-NO-PACKAGE` | [E-DISC-NO-PACKAGE.md](E-DISC-NO-PACKAGE.md) | discovery empty repo (M1.5) |
| `E-DISC-DUPLICATE` | [E-DISC-DUPLICATE.md](E-DISC-DUPLICATE.md) | discovery duplicate members (M1.5) |
| `E-DISC-LINK-UNKNOWN` | [E-DISC-LINK-UNKNOWN.md](E-DISC-LINK-UNKNOWN.md) | discovery polyglot links (M1.7) |
| `E-GRAPH-CYCLE` | [E-GRAPH-CYCLE.md](E-GRAPH-CYCLE.md) | workspace graph cycles (M1.7) |
| `E-GIT-OPEN` | [E-GIT-OPEN.md](E-GIT-OPEN.md) | git engine repo open (M2) |
| `E-GIT-HEAD` | [E-GIT-HEAD.md](E-GIT-HEAD.md) | git engine unborn HEAD (M2) |
| `E-GIT-SHALLOW` | [E-GIT-SHALLOW.md](E-GIT-SHALLOW.md) | git engine shallow clone (M2) |
| `E-GIT-REF` | [E-GIT-REF.md](E-GIT-REF.md) | git engine ref database (M2) |
| `E-GIT-OBJECT` | [E-GIT-OBJECT.md](E-GIT-OBJECT.md) | git engine object reads (M2) |
| `E-PLAN-CYCLE` | [E-PLAN-CYCLE.md](E-PLAN-CYCLE.md) | planner cycle (M1.9) |
| `E-PLAN-RELEASE-AS` | [E-PLAN-RELEASE-AS.md](E-PLAN-RELEASE-AS.md) | planner Release-As footer (M1.9) |
| `E-PLAN-UNREWRITABLE` | [E-PLAN-UNREWRITABLE.md](E-PLAN-UNREWRITABLE.md) | planner cascade rewriter (M1.9, A.2) |
| `E-LOCK-PARSE` | [E-LOCK-PARSE.md](E-LOCK-PARSE.md) | lockfile patch seam (M3.2, §9.2 · M3.4, §9.3) |
| `E-LOCK-NATIVE` | [E-LOCK-NATIVE.md](E-LOCK-NATIVE.md) | native lockfile refresh fallback (M3.2, §9.2 rule 4 · M3.4, §9.3) |
| `E-CACHE-OPEN` | [E-CACHE-OPEN.md](E-CACHE-OPEN.md) | cache open (M2.5) |
| `E-CACHE-HEADER` | [E-CACHE-HEADER.md](E-CACHE-HEADER.md) | cache header validation (M2.5) |
| `E-CACHE-IO` | [E-CACHE-IO.md](E-CACHE-IO.md) | cache read/write/fsync (M2.5) |
| `E-CACHE-LOCK` | [E-CACHE-LOCK.md](E-CACHE-LOCK.md) | cache advisory lock (M2.5) |
| `E-CHLOG-STYLE` | [E-CHLOG-STYLE.md](E-CHLOG-STYLE.md) | changelog style (M3.x) |
| `E-CHLOG-FILE` | [E-CHLOG-FILE.md](E-CHLOG-FILE.md) | changelog existing file (M3.x) |
| `E-CHLOG-PRBODY` | [E-CHLOG-PRBODY.md](E-CHLOG-PRBODY.md) | Release-PR body / embedded plan (M4.3a, §11.3) |
| `E-CLI-CWD` | [E-CLI-CWD.md](E-CLI-CWD.md) | CLI -C handling (M1.10) |
| `E-CLI-RENDER` | [E-CLI-RENDER.md](E-CLI-RENDER.md) | CLI stable output rendering (M1.10) |
| `E-CLI-NO-PACKAGE` | [E-CLI-NO-PACKAGE.md](E-CLI-NO-PACKAGE.md) | CLI explain target (M1.10) |
| `E-CLI-TITLE` | [E-CLI-TITLE.md](E-CLI-TITLE.md) | CLI PR-title lint (M4.3 `check`, §11.6) |
| `E-CLI-PR` | [E-CLI-PR.md](E-CLI-PR.md) | CLI live Release-PR decline (M4.2b `pr --dry`) |
| `E-FORGE-AUTH` | [E-FORGE-AUTH.md](E-FORGE-AUTH.md) | forge credentials (M4.1a, §11.4) |
| `E-FORGE-RATE-LIMIT` | [E-FORGE-RATE-LIMIT.md](E-FORGE-RATE-LIMIT.md) | forge throttling (M4.1a, exit code 5) |
| `E-FORGE-CONFLICT` | [E-FORGE-CONFLICT.md](E-FORGE-CONFLICT.md) | forge CAS precondition (M4.1a, §11.2) |
| `E-FORGE-REQUEST` | [E-FORGE-REQUEST.md](E-FORGE-REQUEST.md) | forge spec validation (M4.1a) |
| `E-FORGE-UNSUPPORTED` | [E-FORGE-UNSUPPORTED.md](E-FORGE-UNSUPPORTED.md) | forge capability decline (M4.1a) |

## Extension policy

- New user-facing error variants **must** land with their page in the same
  PR (checked by review — the same discipline as the layer table, which
  errors an unknown crate: an unknown code fails the review pass through
  the index above, which is kept bidirectionally exact against the
  `code()` implementations).
- Test-infrastructure errors (like `E-TEST-FIXTURE`) are outside §15.4's
  user taxonomy but still carry stable codes so CI logs stay greppable.
- Codes are never reused or renumbered; a removed variant's page stays as
  "retired".

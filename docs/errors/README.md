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
| `E-DISC-IO` | [E-DISC-IO.md](E-DISC-IO.md) | edit |
| `E-DISC-MANIFEST` | [E-DISC-MANIFEST.md](E-DISC-MANIFEST.md) | edit |
| `E-DISC-NESTED-WS` | [E-DISC-NESTED-WS.md](E-DISC-NESTED-WS.md) | edit |
| `E-DISC-GLOB` | [E-DISC-GLOB.md](E-DISC-GLOB.md) | edit |
| `E-DISC-NO-PACKAGE` | [E-DISC-NO-PACKAGE.md](E-DISC-NO-PACKAGE.md) | edit |
| `E-DISC-DUPLICATE` | [E-DISC-DUPLICATE.md](E-DISC-DUPLICATE.md) | edit |
| `E-DISC-LINK-UNKNOWN` | [E-DISC-LINK-UNKNOWN.md](E-DISC-LINK-UNKNOWN.md) | edit |
| `E-GRAPH-CYCLE` | [E-GRAPH-CYCLE.md](E-GRAPH-CYCLE.md) | edit |

| `E-GIT-OPEN` | [E-GIT-OPEN.md](E-GIT-OPEN.md) | git engine repo open (M2) |
| `E-GIT-HEAD` | [E-GIT-HEAD.md](E-GIT-HEAD.md) | git engine unborn HEAD (M2) |
| `E-GIT-SHALLOW` | [E-GIT-SHALLOW.md](E-GIT-SHALLOW.md) | git engine shallow clone (M2) |
| `E-GIT-REF` | [E-GIT-REF.md](E-GIT-REF.md) | git engine ref database (M2) |
| `E-GIT-OBJECT` | [E-GIT-OBJECT.md](E-GIT-OBJECT.md) | git engine object reads (M2) |
| `E-CACHE-OPEN` | [E-CACHE-OPEN.md](E-CACHE-OPEN.md) | cache open (M2.5) |
| `E-CACHE-HEADER` | [E-CACHE-HEADER.md](E-CACHE-HEADER.md) | cache header validation (M2.5) |
| `E-CACHE-IO` | [E-CACHE-IO.md](E-CACHE-IO.md) | cache read/write/fsync (M2.5) |
| `E-CACHE-LOCK` | [E-CACHE-LOCK.md](E-CACHE-LOCK.md) | cache advisory lock (M2.5) |
| `E-CHLOG-STYLE` | [E-CHLOG-STYLE.md](E-CHLOG-STYLE.md) | changelog style (M3.x) |
| `E-CHLOG-FILE` | [E-CHLOG-FILE.md](E-CHLOG-FILE.md) | changelog existing file (M3.x) |
| `E-CLI-CWD` | [E-CLI-CWD.md](E-CLI-CWD.md) | CLI -C handling (M1.10) |
| `E-CLI-RENDER` | [E-CLI-RENDER.md](E-CLI-RENDER.md) | CLI stable output rendering (M1.10) |
| `E-CLI-NO-PACKAGE` | [E-CLI-NO-PACKAGE.md](E-CLI-NO-PACKAGE.md) | CLI explain target (M1.10) |

## Extension policy

- New user-facing error variants **must** land with their page in the same
  PR (checked by review; enforced mechanically from M1.10 via
  `xtask check-errors` once the taxonomy exists in code).
- Test-infrastructure errors (like `E-TEST-FIXTURE`) are outside §15.4's
  user taxonomy but still carry stable codes so CI logs stay greppable.
- Codes are never reused or renumbered; a removed variant's page stays as
  "retired".

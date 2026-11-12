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
| `E-TEST-FIXTURE` | [E-TEST-FIXTURE.md](E-TEST-FIXTURE.md) | test-support fixture failures |

## Extension policy

- New user-facing error variants **must** land with their page in the same
  PR (checked by review; enforced mechanically from M1.10 via
  `xtask check-errors` once the taxonomy exists in code).
- Test-infrastructure errors (like `E-TEST-FIXTURE`) are outside §15.4's
  user taxonomy but still carry stable codes so CI logs stay greppable.
- Codes are never reused or renumbered; a removed variant's page stays as
  "retired".

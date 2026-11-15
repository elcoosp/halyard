# Publish auth env grammar (M5.3 step 1) — VERIFIED

Status: **VERIFIED live 2026-11-15** (per §0.4: the pages below were
fetched and the quotes are verbatim; search snippets only pointed at the
pages, every load-bearing quote below comes from a fetched page).

The auth plumbing's job (ADR-0019) is to resolve the publish tool's own
token variables — never to invent a credential mechanism. The grammar:

## cargo (Cargo Book — doc.rust-lang.org/cargo/reference/config.html)

- crates.io: "The token for crates.io may be specified with the
  **CARGO_REGISTRY_TOKEN** environment variable." (singular `REGISTRY`,
  not the registries form)
- Other registries: "Tokens for other registries may be specified with
  environment variables of the form **CARGO_REGISTRIES_<name>_TOKEN**
  where <name> is the name of the registry in all capital letters."
  (registry-authentication.html), combined with the config reference's
  general rule: "For each configuration key of the form foo.bar the
  environment variable CARGO_FOO_BAR can also be used to define the
  value. Keys are converted to uppercase, dots and dashes are converted
  to underscores." — so a registry named `my-corps.index` maps to
  `CARGO_REGISTRIES_MY_CORPS_INDEX_TOKEN`.
- The general mapping also covers crates.io itself
  (`registries.crates-io.token` → `CARGO_REGISTRIES_CRATES_IO_TOKEN`);
  halyard keeps it as the documented fallback after the singular form.
- Credentials also live in Cargo's credentials file (`cargo login`,
  stored "unencrypted in plain text"). halyard cannot see that file —
  an absent environment variable is therefore `AuthOutcome::Absent`
  (the tool's own store governs), never an error.

## npm (the pnpm spawn, §13.4) — pnpm.io/npmrc

- The bearer token setting: `_authToken` — "The bearer token for a
  registry:" `//registry.npmjs.org/:_authToken=xxxxxxxx-…`.
- Interpolation: "User-level auth files, <pnpm config>/auth.ini and
  ~/.npmrc, can reference environment variables as ${NAME}:"
  `//registry.npmjs.org/:_authToken=${NPM_TOKEN}`.
- **The project `.npmrc` deliberately does not interpolate credentials**
  — "pnpm does not expand environment variables in the project .npmrc
  for: … credentials: _authToken, _auth, _password, username, …" and
  "pnpm ignores such a setting and prints a warning", because "a
  malicious repository could otherwise send your CI tokens to a
  registry it controls."
- Consequence (ADR-0019): halyard injects `NPM_TOKEN` (the canonical
  candidate; `NODE_AUTH_TOKEN` is the actions/setup-node convention and
  the second candidate) and **never writes auth files** — the env var
  rides the user-level file contract the tool already documents.
  halyard-side files could only reproduce the leak vector pnpm's
  warning exists to prevent.

## Precedence and refusal

- Candidates resolve in the documented order above; the first
  non-empty value wins (all candidates listed in
  `halyard_publish::variables_for`).
- A token that is present but **empty** is refused fail-closed with
  `E-PUB-AUTH` naming the variable: with a `${VAR}` interpolation an
  empty value becomes an empty bearer token and a confusing 401 — the
  misconfiguration must be named before the spawn, not diagnosed from
  the registry's rejection.

## Remaining VERIFY items (honest open list)

- `CARGO_REGISTRIES_<NAME>_TOKEN` for an *alias* vs the *actual
  registry name* (config-defined aliasing) — halyard maps the plan's
  registry name verbatim; a config alias must be handled by the user's
  config, not guessed here.
- npm scoped-registry variables (`//<scope>.registry/:_authToken` env
  forms) — the packument probe side is covered by the registry crate;
  a publish-side scope grammar is a future VERIFY (no scope support in
  the plan's publish contract yet).

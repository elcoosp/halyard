# E-CFG-INIT

**Code:** `E-CFG-INIT` · **Area:** config (§16.1/§16.2, ADR-0029) ·
**Raised by:** `halyard_config::plan_init`, `halyard_config::run_init`,
`halyard_config::AtomicScaffoldSink`, the shell's conventional-config
footgun check (`halyard init`)

## What it means

The `halyard init` scaffold refused. The scaffold is create-only and
fail-closed: it never overwrites an existing file, never lands a
half-scaffold, and never guesses. The refusal names the target (or `.`
for a survey-level refusal) and the rule that fired.

## Cause

- **Zero packages:** discovery found no `Cargo.toml`, `package.json` or
  `pnpm-workspace.yaml` in the working directory — there is nothing to
  scaffold for (the repo may be empty, or you are not at the root).
- **An existing target:** `halyard.toml` or
  `.github/workflows/release.yml` already exists (the gate runs before
  *any* write, so nothing was written at all).
- **The shadow footgun:** `.config/halyard.toml` exists — a root
  scaffold would shadow it, because the config resolution prefers the
  root file.
- **A failed atomic write:** the temp-sibling → fsync → rename protocol
  failed; no partial file is left behind.

## Fix

Pick the branch that matches the refusal message:

- empty tree → run `halyard init` from the repository root, after the
  first manifest exists;
- existing config → the scaffold never clobbers: merge the commented
  skeleton into your `halyard.toml` by hand (or delete it first — your
  call, never halyard's);
- existing workflow → compare it with the §16.1 document
  (`halyard init --github` prints what it would land in the report)
  and keep yours;
- `.config/halyard.toml` → point `--config` at the existing file
  instead, or move it to the root before scaffolding.

## Command

```bash
# what discovery sees (and what init would survey):
halyard status
# what the scaffold would land, without writing:
halyard init --format json
# where the conventional configs live:
ls halyard.toml .config/halyard.toml .github/workflows/release.yml 2>/dev/null
```

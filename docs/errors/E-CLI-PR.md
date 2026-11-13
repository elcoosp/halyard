# E-CLI-PR

**Code:** `E-CLI-PR` · **Area:** CLI (M4.2b, §11.1) · **Raised by:**
`halyard pr` (without `--dry`)

## What it means

The live Release-PR mode was requested, but no forge adapter exists yet:
the M4.1b GitHub REST adapter (ETag conditionals, backoff, App-JWT auth)
has not landed, and the shell declines rather than pretending to write to
a forge.

## Cause

- `halyard pr` was run without `--dry` — the only implemented mode is the
  offline prepare-phase forecast over the dry-run adapter
  (`halyard-forge::PreviewForge`);
- the §11.1 machine is live, its forge side is not.

## Fix

Use the offline forecast while M4.1b is pending:

```bash
halyard pr --dry
halyard --format json pr --dry | jq '.calls, .effects'
```

The forecast renders the §11.3 body, the branch
(`halyard/release/<base>`, §11.2) and the §11.5 call budget the live run
would spend. No file is written and no network is touched (I5).

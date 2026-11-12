# E-EDIT-VERIFY

**Code:** `E-EDIT-VERIFY` · **Area:** edit (M1.4) · **Raised by:**
`apply_to_file_verified`, `verify_outputs`

## What it means

The re-parse/semantic verification of the patched content failed before
anything was written (§9.1 apply protocol): the file on disk is untouched.

## Cause

- the patched bytes no longer parse as the manifest format the verifier
  expects (e.g. a patch target inside a comment);
- a semantic check rejected the result (version not found after re-read).

## Fix

Regenerate the plan (stale spans are the usual cause). If the verifier is
yours, confirm the semantic check matches the format version.

## Command

```bash
cargo run -p halyard-edit --example apply_patch
```

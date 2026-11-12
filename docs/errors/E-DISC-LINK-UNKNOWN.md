# E-DISC-LINK-UNKNOWN

**Code:** `E-DISC-LINK-UNKNOWN` · **Area:** edit (M1 slice B) · **Raised by:** the
discovery orchestrator / graph module

## What it means

A `[[link]]` entry in `halyard.toml` names a package discovery did not find.

## Cause

- the package name is misspelled; - the package lives outside the discovered workspace.

## Fix

Fix `from`/`to` in `halyard.toml`, or add the missing package to the workspace.

## Command

```bash
halyard graph --format text
python3 -c "import tomllib;print(tomllib.load(open('halyard.toml','rb'))['link'])"
```

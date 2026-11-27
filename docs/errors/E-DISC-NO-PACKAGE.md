# E-DISC-NO-PACKAGE

**Code:** `E-DISC-NO-PACKAGE` · **Area:** edit (M1 slice B) · **Raised by:** the
discovery orchestrator / graph module

## What it means

The repository contains neither a workspace nor a package: no root `[package]`/`[workspace]` in `Cargo.toml`, no root `package.json`, no root `pyproject.toml`, no root `go.mod`/`go.work`, no root `pom.xml`, no root SDK-style `.csproj`, no root `pubspec.yaml` and no root `.gemspec`.

## Cause

- discovery was pointed at the wrong directory; - manifests are gitignored or deleted.

## Fix

Run in the repository root, or add a root manifest. `halyard plan` needs at least one package.

## Command

```bash
ls Cargo.toml package.json pyproject.toml go.mod go.work pom.xml '*.csproj' pubspec.yaml '*.gemspec'
pwd
```

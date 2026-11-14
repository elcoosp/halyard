# E-GIT-CACHE

**Code:** `E-GIT-CACHE` · **Area:** git engine persistent-cache
durability (§8.7) · **Raised by:** the history build
([`GitHistory::build_with_options`])

[`GitHistory::build_with_options`]: https://docs.rs/halyard-git/latest/halyard_git/struct.GitHistory.html#method.build_with_options

## What it means

The persistent per-commit cache (§8.7) could not be **updated** during
the history build: a record insert or the final flush failed (unwritable
`$GIT_DIR`, disk full, an I/O error). The in-memory digest is
unaffected — §8.7 makes the cache an optimization, never a correctness
surface — but the durability failure is its own stable code, not an
object-read error (`E-GIT-OBJECT`, which this failure was previously
mapped onto; the cache neither reads nor parses objects, so that code
was misleading).

A cache that cannot be *opened* degrades to an uncached run with a
warning (the CLI shell's §8.7 policy); a cache that cannot be *written
back* after the walk completed fails the build with `E-GIT-CACHE` —
silently dropping the append would hide a broken `$GIT_DIR` the next
run would pay for again.

## Cause

- the `$GIT_DIR` filesystem is full or read-only;
- the cache file was removed or locked between the open and the flush
  (a concurrent process; the per-op advisory lock serializes writers,
  §8.7);
- an I/O error while appending (failing disk).

## Fix

The cache is safe to delete and will be rebuilt from scratch (§8.7);
nothing else depends on it:

```bash
# Remove the cache (any release-planning state is unaffected)
rm -rf "$(git rev-parse --git-dir)/halyard/cache"
# Or skip it for one run
halyard plan --no-cache
```

If the failure persists, check the filesystem holding `$GIT_DIR`
(`df -h`, mount ro flag); the underlying I/O error is quoted in the
message.

## Command

```bash
# Where does the cache live here?
git -C <repo-root> rev-parse --git-dir
ls -la "$(git -C <repo-root> rev-parse --git-dir)/halyard/cache/v1/"
```

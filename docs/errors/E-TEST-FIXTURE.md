# E-TEST-FIXTURE

**Code:** `E-TEST-FIXTURE` · **Area:** `halyard-test-support` (test
infrastructure) · **Kinds:** `git_invocation`, `git_output`, `io`,
`scenario`

## What it means

A `GitFixture` scenario failed to build or inspect a git repository. The
message contains the exact argv that ran and git's stderr.

## Cause

One of:

- a `git` subcommand exited non-zero (missing flag on old git versions,
  ref name conflicts, dirty tree during merge);
- a command succeeded but its output was not interpretable (no OID where
  one was required);
- a filesystem operation failed (permissions, disk full);
- the DSL was used in an invalid order (e.g. octopus merge with fewer than
  two heads, worktree on a non-owning fixture).

## Fix

- Re-read the failing argv in the message; reproduce by hand with the same
  arguments if unclear.
- `git_invocation` with `unknown option` usually means a git version below
  the documented floor — check `git --version` (CI floor: whatever GitHub
  runner images ship; local development: any git ≥ 2.28 for `-b main`).
- `scenario` errors are caller bugs in the test using the DSL, not fixture
  bugs.

## Command

```bash
# run one scenario's tests with output:
cargo test -p halyard-test-support --test scenarios -- --nocapture

# inspect a leftover fixture (temp dirs are cleaned on drop; disable by
# keeping the process alive with a breakpoint or setting a watchpoint):
git -C /tmp/halyard-fixture-<label>-*/repo log --oneline --graph
```

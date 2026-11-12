# Research: Cargo.lock grammar & semantics (VERIFY, M0.6)

**Checked:** 2026-11-12 · **Sources:**
- Cargo Book guide, "Cargo.toml vs Cargo.lock"
  <https://doc.rust-lang.org/cargo/guide/cargo-toml-vs-cargo-lock.html> (fetched live)
- Local observation: this workspace's `Cargo.lock` (v4) and `cargo --version`
  (1.99.1)

## Verified facts

- Division of labour: "Cargo.toml is about describing your dependencies in a
  broad sense, and is written by you. … Cargo.lock contains exact information
  about your dependencies. It is maintained by Cargo and **should not be
  manually edited**."
- The lock records exact versions and, for git sources, the exact revision:
  `source = "git+https://…#<40-hex sha>"`.
- Committing guidance: "When in doubt, check Cargo.lock into the version
  control system."
- Update paths: `cargo update` (all), `cargo update <spec>` (one package) —
  the lockfile is rewritten only by cargo, never by hand.
- Local observation (cargo 1.99.1, this workspace's `Cargo.lock`): the file
  begins with two `#`-comment lines, a `version = 4` top-level key, then
  `[[package]]` tables with fields `name`, `version`, `source` (e.g.
  `registry+https://github.com/rust-lang/crates.io-index`), an inline
  `checksum` (lower-case hex SHA-256), and a `dependencies` array listing
  plain `"name"` entries. **The full v4 grammar (git-source package fields,
  dependency spec syntax like `"name version source"`, patch sections, and
  what cargo tolerates on round-trip) is VERIFY-pending against cargo's
  source (`src/cargo/util/lockfile.rs`) before the M3 lockfile patcher is
  written.** The M3 differential test (`cargo update -w --offline`
  equivalence) is the arbiter.

## Design consequences (feeds M3.2)

- HALYARD's lockfile patcher must **edit only the version/checksum/dependency
  lines of released packages**, preserving all other bytes (format
  preservation, I4) — including indentation, ordering and any fields cargo
  added that HALYARD does not understand.
- Validity gate: patched lock must satisfy `cargo metadata --offline` (or
  `cargo tree`) before an EditSet ships; the differential harness compares
  HALYARD's patch against `cargo update -w --offline` output on the corpus.

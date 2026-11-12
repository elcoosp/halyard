# Research: crates.io publishing & trusted publishing (VERIFY, M0.6)

**Checked:** 2026-11-12 · **Source:** Cargo Book, "Publishing on crates.io"
<https://doc.rust-lang.org/cargo/reference/publishing.html> (fetched live)

## Verified facts

- Auth is GitHub-login based ("required for now") plus a verified email; API
  tokens are created at `https://crates.io/settings/tokens` and stored by
  `cargo login` in `~/.cargo/credentials.toml`. Token leakage ⇒ revoke.
- `cargo publish` pipeline (page's own list): verification checks → compress
  `.crate` → extract + compile verification → upload → registry-side checks.
- `cargo publish --dry-run` (equivalent to `cargo package`) runs only steps
  1–3; `cargo package --list` inspects contents. `.crate` size limit: 10 MB.
- Publishing is immutable: "The version can never be overwritten, and the
  code cannot be deleted." `cargo yank` only blocks new dependents.
- Required metadata: `license`/`license-file`, `description`, `homepage`,
  `repository`, `readme`.
- The page explicitly recommends per-release automation: changelog entry +
  git tag pointing at the published commit (names cargo-release,
  cargo-smart-release, release-plz as precedents).

## Open VERIFY items (must not be assumed)

1. **Trusted publishing / OIDC for crates.io:** the publishing page documents
   no tokenless path. Whether crates.io supports OIDC trusted publishing
   (à la npm/PyPI) must be checked against crates.io's own docs/blog before
   designing the publish adapter (M13). Design until then: token-based
   `cargo publish` only.
2. **Index propagation delay:** nothing on the page quantifies the delay
   between a successful publish and sparse-index visibility. HALYARD's
   `publish.cargo.wait_for_index` default (`90s`) is an **assumption** to be
   measured empirically (M13.4), not documented behaviour.

## Design consequences (feeds M13)

- The publish adapter shells out to `cargo publish --dry-run` first, then the
  real publish, with the token injected via env (`CARGO_REGISTRY_TOKEN`),
  never argv (I7: untrusted text never shell-interpolated; argv arrays only).
- Dependent publishing must await index visibility — poll the sparse index
  endpoint until the new version appears, bounded by `wait_for_index`.

# Reference

The three pages in this section are the operator-facing surface:

1. [CLI commands](cli.md) — every wired `halyard` command, its purpose, and
   the global flags and output formats.
2. [Configuration](config.md) — the `halyard.toml` key reference and the
   resolution order across defaults, files, package metadata, environment
   variables, and CLI flags.
3. [Error codes](errors.md) — the stable error taxonomy, the exit codes, and
   the full table mapping every code to its explanation page in the
   repository.

The pages mirror the repository's own documentation: the command rows come
from the CLI README, the configuration reference from the architecture
document's configuration section, and the error table is generated from the
`docs/errors/` index (the `check-docs-site` gate enforces that the table
stays in exact set-equality with that index, so a code cannot appear on the
site without a page, or lose its page silently).

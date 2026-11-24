# Channels and maintenance trains

Halyard supports prerelease channels and maintenance-branch release trains
without a state file: the counters are derived from the tags the repository
already carries.

## Prerelease channels

With `[versioning].channel = "rc"` in `halyard.toml`, every planned release
becomes `{target}-{rc}.{n}` — `1.3.0-rc.1`, `1.3.0-rc.2`, and so on, with the
counter derived from the repository's existing tags. Non-breaking steps on an
active train are absorbed into the train's committed triple (`1.3.0-rc.1` +
a fix → `1.3.0-rc.2`, never `1.3.1`); a breaking commit opens a new base.
Removing the key promotes a leftover prerelease manifest to its own stable
triple. `Release-As` targets are exempt. An invalid channel name fails
closed (`E-CFG-CHANNEL`).

## Branch-bound channels

`[versioning].channel_branches = { next = "rc" }` binds a channel to a
branch: HEAD on `next` plans `rc` prereleases, every other branch keeps the
unbound default (or stays stable). The binding rides the persistent cache
key, so planning `next` and planning `main` never share attributions.

## Per-package trains

`[[package]] channel = "rc"` names that one package's train — the most
explicit policy, winning over the default and the branch bindings. Every
other package keeps the repo-wide (or stable) derivation, and promotion
splits per package: a trained package continues its train while a
channel-less package promotes. An override requires a package `name`
(path-glob overrides refine include/exclude only).

## Maintenance lines

`[versioning].maintenance_branches = ["1.x"]` pins the release line of a
maintenance branch: `MAJOR.x` / `MAJOR.MINOR.x` forms are parsed and matched
against the final target. Planning `1.3.1` on the `1.x` branch succeeds;
planning `1.4.0` or `2.0.0` there refuses with `E-PLAN-MAINTENANCE`, naming
the package, the line, and the offending target — backports cannot
accidentally drift into a feature release. The 0.x pre-major remap applies
before the guard, so `0.1.x` refuses a remapped `0.2.0` target the same way.

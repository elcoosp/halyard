# E-PUB-ORDER

**Code:** `E-PUB-ORDER` · **Area:** publish engine layer planning (M5.2,
§13 step 1) · **Raised by:** [`plan_publication`]

[`plan_publication`]: https://docs.rs/halyard-publish/latest/halyard_publish/fn.plan_publication.html

## What it means

The publish targets contain a dependency **cycle over non-dev edges**,
so topological publish layers are undefined: no order publishes both
sides of the cycle while satisfying "dependents publish after their
dependencies" (§13 step 1). The message renders the cycle path, e.g.
`a -> b -> a`.

Dev-only cycles are legal (Cargo allows them, §6.6) and never reach
this error — only edges that make a dependent *embed* the dependency
(normal/build/peer/optional) constrain the publish order.

## Cause

A normal dependency cycle among publishable workspace members. Cargo
refuses to build such a cycle for ordinary crates; npm workspaces allow
it, which is where this error realistically fires.

## Fix

Break the cycle — extract the shared code into a lower package, or
exclude one member of the cycle from publishing:

```toml
# halyard.toml — keep the versioning, drop the publishing (§16.4)
[[package]]
name = "<one-member-of-the-cycle>"
publish = false
```

## Command

```bash
# The dependency edges halyard sees (the cycle is visible here)
halyard graph
```

# Go module grammars — VERIFIED reference

Status: VERIFIED live against the official Go references on 2026-11-26
(the M8 **Go modules** cell, ADR-0057). Quotes are verbatim from the
pages listed per section; the grammar work rides only these.

## 1. `go.mod` — `https://go.dev/ref/mod`

The directive set:

> Keywords distinguish different kinds of directives in a go.mod file.
> Allowed keywords are **module, go, require, replace, exclude, and
> retract**.

The EBNF (the same page; Go 1.24 adds `tool` and `ignore`, both
recognized-and-skipped by halyard's lexer):

> `GoMod = { Directive } .`
> `Directive = ModuleDirective | GoDirective | ToolDirective | IgnoreDirective | RequireDirective | ExcludeDirective | ReplaceDirective | RetractDirective .`

The module directive carries the path, never a version:

> A module directive defines the main module's path. A go.mod file must
> contain exactly one module directive.

The **tag-only** fact the cell is built on — the go.mod grammar has no
field for the module's own version; versions live in the repository:

> A module is identified by a module path, which is declared in a
> go.mod file, together with information about the module's
> dependencies.

(The version's source is the repository's semantic version tags; the
same page's Versions section:)

> To summarize, a semantic version consists of three non-negative
> integers (the major, minor, and patch versions, from left to right)
> separated by dots. The patch version may be followed by an optional
> pre-release string starting with a hyphen. The pre-release string or
> patch version may be followed by a build metadata string starting
> with a plus. For example, v0.0.0, v1.12.134, v8.0.5-pre, and
> v2.0.9+meta are valid versions.

The require/replace rows halyard parses (the page's example):

```go.mod
module example.com/my/thing

go 1.23.0

require example.com/other/thing v1.0.2
require example.com/new/thing/v2 v2.3.4
exclude example.com/old/thing v1.2.3
replace example.com/bad/thing v1.4.5 => example.com/good/thing v1.4.5
retract [v1.9.0, v1.9.5]
```

## 2. The major version suffix — `https://go.dev/ref/mod`

The v2+ path rule the planner's release guard enforces:

> Starting with major version 2, module paths must have a major version
> suffix like /v2 that matches the major version. For example, if a
> module has the path example.com/mod at v1.0.0, it must have the path
> example.com/mod/v2 at version v2.0.0.

> Major version suffixes are not allowed at major versions v0 or v1.
> There is no need to change the module path between v0 and v1 because
> v0 versions are unstable and have no compatibility guarantee.

The path-element constraints (the page's lexical rules):

> For a final path element of the form /vN where N looks numeric (ASCII
> digits and dots), N must not begin with a leading zero, must not be
> /v1, and must not contain any dots.

And the module-path↔repository-subdirectory mapping that makes the
subdir tag shape derivable:

> If the module is released at major version 2 or higher, the module
> path must end with a major version suffix like /v2. This may or may
> not be part of the subdirectory name. For example, the module with
> path golang.org/x/repo/sub/v2 could be in the /sub or /sub/v2
> subdirectory of the repository golang.org/x/repo.

## 3. `go.work` — `https://go.dev/ref/mod` (go-work file section)

The directive set and `use` semantics:

> A `use` adds a module on disk to the set of main modules in a
> workspace. Its argument is a relative path to the directory
> containing the module's go.mod file. A `use` directive does not add
> modules contained in subdirectories of its argument directory. Those
> modules may be added by the directory containing their go.mod file in
> separate use directives.

The block form (the page's example):

```
use (
./my/first/thing
./my/second/thing
)
```

The replace grammar (same section):

> `ReplaceSpec = ModulePath [ Version ] "=>" FilePath newline`
> `| ModulePath [ Version ] "=>" ModulePath Version newline .`

The single-module posture halyard mirrors when no `go.work` exists:

> When no go.work file exists, the workspace consists of the single
> module containing the current directory.

## 4. Publishing by tag — `https://go.dev/doc/modules/publishing`

The tag shape at the module root (step 4 of the documented publish flow):

```sh
$ git commit -m "mymodule: changes for v0.1.0"
$ git tag v0.1.0
$ git push origin v0.1.0
```

> Note: Don't change a tagged version of a module after publishing it.
> For developers using the module, Go tools authenticate a downloaded
> module against the first downloaded copy.

The **subdir tag shape** (a module in a subdirectory tags
`subdir/vX.Y.Z`) follows §2's module-path↔subdirectory mapping plus
§4's root shape: the tag is the module's repo-relative directory, then
`/vX.Y.Z` — for a `/v2` module the version itself carries the major
(`libs/metrics/v2.1.0`). Halyard renders both shapes through the shared
template grammar (architecture appendix A.3), never a Go-specific
template syntax.

## 5. What halyard does NOT verify live

- No `go` toolchain run: the adapter is a pure parser (I6 — no process
  spawn in discovery); the go tool's own resolution (pseudo-versions,
  minimal version selection) is never re-implemented.
- `GOPROXY`/proxy.golang.org existence probes (the registry seam's Go
  arm) stay unstarted — a tag-only ecosystem has no publish step and no
  resume probe to run; recorded-open in ADR-0057.
- `retract`/`exclude` rows are recognized and skipped (they constrain
  go's own resolution, not halyard's release planning); `tool`/`ignore`
  (Go 1.24) likewise.

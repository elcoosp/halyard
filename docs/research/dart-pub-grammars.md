# Dart/pub grammars — VERIFIED reference

Status: VERIFIED live against the official Dart documentation on
2026-11-27 (the M8 **Dart/pub** cell, ADR-0060). Quotes are verbatim
from the pages listed per section; the grammar work rides only these.

## 1. The pubspec fields — `https://dart.dev/tools/pub/pubspec`

The package identity (the adapter's row name rides this):

> **name** — Required for every package. ... Every package needs a name.
> It's how other packages refer to yours, and how it appears to the
> world. ... The name should be all lowercase, with underscores to
> separate words, just_like_this. ... Use only basic Latin letters and
> Arabic digits: [a-z0-9_].

The version field — the whole grammar the adapter plans:

> **version** — Required for packages that are hosted on the pub.dev
> site. A version number is required to host your package on the pub.dev
> site, but can be omitted for local-only packages. If you omit it, your
> package is implicitly versioned 0.0.0.
>
> A version number is three numbers separated by dots, like 0.2.43. It
> can also optionally have a build (+1, +2, +hotfix.oopsie) or prerelease
> (-dev.4, -alpha.12, -beta.7, -rc.5) suffix. When you select a version,
> follow semantic versioning.

and the published-version immutability rule (why a rewrite must
read-back verify and never touch a non-canonical spelling):

> When you've published a package at some version, consider it hermetically
> sealed: you can't touch it anymore.

The closed field list (everything else is consumed unmodeled):

> Pub ignores all other fields.

The dependency sections (the edge grammar the adapter walks):

> Regular dependencies are listed under `dependencies`—these are packages
> that anyone using your package will also need. Dependencies that are
> only needed in the development phase of the package itself are listed
> under `dev_dependencies`. During the development process, you might
> need to temporarily override a dependency. You can do so using
> `dependency_overrides`.

## 2. Workspaces — `https://dart.dev/tools/pub/workspaces`

The workspace declaration (the aggregator grammar the adapter walks,
exactly like `<modules>` for Maven and `use` for Go):

> To create a workspace: Add a pubspec.yaml at the repository root
> directory with a `workspace` entry enumerating the paths to the
> packages of the repository (the workspace packages):
>
> ```yaml
> name: _
> publish_to: none
> environment:
>   sdk: ^3.6.0
> workspace:
>   - packages/shared
>   - packages/client_package
>   - packages/server_package
> ```

The member marker (the gate a member must carry):

> For each of the existing pubspec.yaml files, make sure their SDK
> constraint is at least ^3.6.0 and add a `resolution` entry:
>
> ```yaml
> environment:
>   sdk: ^3.6.0
> resolution: workspace
> ```

The version the feature landed in:

> **Version note** — Support for pub workspaces was introduced in Dart
> 3.6.0. To use pub workspaces, all your workspace packages (but not
> your dependencies) must have an SDK version constraint of ^3.6.0 or
> higher.

Nested workspaces (the recursive walk):

> You can nest workspaces to organize packages hierarchically in large
> repositories. A workspace member can declare its own `workspace` field,
> just like the root pubspec.yaml. ... Pub discovers auth and api through
> the workspace entry of the server package and includes them in the
> single, shared dependency resolution.

Glob entries (the shipped subset — a single `*` as a whole path segment,
per the documented example; anything else is refused fail-closed):

> **Version note** — Glob pattern support requires Dart 3.11 or later. If
> you use an earlier version, use explicit paths instead. The `workspace`
> entry supports glob patterns to automatically include packages:
>
> ```yaml
> workspace:
>   - packages/*
> ```
>
> This includes all subdirectories in packages/ that contain a
> pubspec.yaml file, eliminating the need to manually list each package.

The stray-file rule (pub's own fail-closed posture, mirrored by the
adapter's unclaimed-pubspec refusal):

> If any directory between the workspace root and a workspace package
> contains a "stray" pubspec.yaml file that is not member of the
> workspace, pub get will report an error and fail to resolve. This is
> because resolving such a pubspec.yaml would create a
> .dart_tool/package_config.json file that shadows the one at the root.

The internal-edge rule (name-proven, regardless of the dep's declared
source — this is why a dependency *naming* a member is the proven edge):

> **Interdependencies between workspace packages** — If any of the
> workspace packages depend on each other, they will automatically
> resolve to the one in the workspace, regardless of the source. Eg.
> packages/client_package/pubspec.yaml might depend on `shared`:
>
> ```yaml
> dependencies:
>   shared: ^2.3.0
> ```
>
> When resolved inside the workspace, the local version of `shared` will
> be used. The local version of `shared` would still have to match the
> constraint (^2.3.0) though. But when the package is consumed as a
> dependency without being part of the workspace, the original source
> (here implicitly hosted) is used.

The single shared lockfile (the lockfile half's decline posture):

> Create a single pubspec.lock next to the root pubspec.yaml that
> contains the resolution of all the dependencies and dev_dependencies
> of all the workspace packages. ... Delete any other existing
> pubspec.lock and .dart_tool/package_config.json files next to workspace
> packages.

The root row (the root pubspec is itself a workspace package — `dart pub
workspace list` prints it):

> ```
> $ dart pub workspace list
> Package         Path
> _               ./
> client_package  packages/client_package/
> server_package  packages/server_package/
> shared          packages/shared/
> ```

## 3. Dependency sources — `https://dart.dev/tools/pub/dependencies`

The path grammar and its relative resolution (the second internal-edge
proof, alongside the name match):

> To handle that, pub supports path dependencies.
>
> ```yaml
> dependencies:
>   transmogrify:
>     path: /Users/me/transmogrify
> ```
>
> This says the root directory for transmogrify is
> /Users/me/transmogrify. ... Relative paths are allowed and are
> considered relative to the directory containing your pubspec.

The publish-blocking posture of path and git deps (why a path dep is
consumed verbatim and never rewritten — it is not a version requirement
at all):

> Path dependencies are useful for local development, but do not work
> when sharing code with the outside world—not everyone can get to your
> file system. Because of this, you cannot upload a package to the
> pub.dev site if it has any path dependencies in its pubspec.
>
> Git dependencies are not allowed as dependencies for packages uploaded
> to pub.dev.

## 4. Publishing — `https://dart.dev/tools/pub/publishing`

The argv grammar (verified and recorded; the engine's spawn arm stays a
recorded-open remainder — see §5):

> The pub package manager isn't just for using other people's packages.
> It also allows you to share your packages with the world. If you have a
> useful project and you want others to be able to use it, use the
> `dart pub publish` command.

The opt-out field (the workspace-root convention, warned at discovery):

> **Note** — To publish to a location other than pub.dev, or to prevent
> publication anywhere, use the `publish_to` field, as defined in the
> pubspec.

## 5. Environment variables — `https://dart.dev/tools/pub/environment-variables`

The complete documented environment surface (`PUB_CACHE`, `PUB_HOSTED_URL`
— **no publish-token variable exists**; pub.dev publishing rides the pub
client's own OAuth2 credentials flow, not an env-var token the publish
engine's auth seam could carry):

> **PUB_CACHE** — Some of pub's dependencies are downloaded to the pub
> cache. By default, this directory is located under `$HOME/.pub-cache`
> (on macOS and Linux), or in `%LOCALAPPDATA%\Pub\Cache` (on Windows). ...
> You can use the `PUB_CACHE` environment variable to specify another
> location.
>
> **PUB_HOSTED_URL** — Pub downloads dependencies from the pub.dev site.
> To specify the location of a particular mirror server, use the
> `PUB_HOSTED_URL` environment variable.

## 6. Adapter decisions (the fail-closed set)

- Package identity: the `name` field ([a-z0-9_], so the shared
  appendix-A.3 tag grammar renders legal git refnames). A pubspec
  without a `name` refuses (`E-DISC-MANIFEST` — *"Required for every
  package"*).
- Version strategy: the three-part semver grammar is the plan contract's
  own type — every pub spelling that parses as semver (`1.2.3`,
  `1.2.3-dev.4`, `1.2.3+build.5`) is **canonical** and plans with an
  editable site; two-component spellings (`1.2` — *"three numbers
  separated by dots"*) and quoted/null/absent values are warned,
  unpublishable, no edit site, never rewritten (the ADR-0056 discipline).
- Internal edges, two proofs: (1) a dependency whose **name** matches a
  discovered member (*"they will automatically resolve to the one in the
  workspace, regardless of the source"*); (2) a `path:` dependency whose
  resolved path (relative to the pubspec's directory) names a member
  manifest (*"considered relative to the directory containing your
  pubspec"*). `ReqSpec::Other` verbatim per ADR-0006 — pub constraints
  (`^2.3.0`, ranges) are never interpreted.
- Workspace membership: a root pubspec with a `workspace` entry is the
  aggregator; every member must carry `resolution: workspace` (fail
  closed, *"add a resolution entry"*); nested `workspace` entries walk
  recursively; a single-`*` glob segment expands over the committed tree
  (`packages/*` — the documented example); any other glob shape refuses.
  With a workspace root present, an unclaimed pubspec.yaml is the
  documented stray-file refusal; without one, unclaimed pubspecs are
  standalone packages (the orphan-manifest posture).
- Publish posture: `dart pub publish` is the verified argv, but pub.dev
  publishing documents **no token env var** (§5 — only `PUB_CACHE` and
  `PUB_HOSTED_URL`), so rows carry `publishable = false` (the engine's
  disabled shape, the Maven D5/NuGet posture) until the auth arm lands;
  the planner's registry arm names `pub.dev`. `publish_to: none` is
  warned (the documented *"prevent publication anywhere"* posture).
  The path/git-dep publish blockers (§3) are recorded in ADR-0060.
- Lockfile half: the single root `pubspec.lock` is pub-generated and
  pub-owned (*"Delete any other existing pubspec.lock"* — pub get
  regenerates it), so the adapter rides the trait's documented
  `NeedsNative`/`Unsupported` declines (the PyPI cell's uv.lock shape);
  the `dart pub get` native-refresh arm stays a recorded-open remainder.

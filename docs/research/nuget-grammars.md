# NuGet / MSBuild project grammars (M8 NuGet cell)

Verified 2026-11-26 against `learn.microsoft.com` (the `nuget` reference
pages, the `.NET project SDK overview`, and the MSBuild
`customize-by-directory` page). VERBATIM quotes below carry the page and
section; every grammar rule the `halyard-eco-nuget` adapter implements
points at one of them. The discipline is the Maven cell's
(`docs/research/maven-pom-grammars.md`): a rule without a quote does not
ship.

## 1. The project file and the SDK attribute

The manifest is the MSBuild project file (`.csproj` for C# projects):
*"Project files, which have extensions like .csproj for C# projects and
.fsproj for F# projects, are in XML format. The root element of an MSBuild
project file is the Project element."* (`.NET project SDK overview`,
"Project files").

The adapter plans **SDK-style** projects only. The SDK is declared by the
`Sdk` attribute — *"The `Project` element has an optional `Sdk` attribute
that specifies which SDK (and version) to use"* — or by the top-level
`Sdk` element — *"Another way to specify the SDK is with the top-level
`Sdk` element"* (`<Project> <Sdk Name="Microsoft.NET.Sdk" /> ...`), both
`.NET project SDK overview`. The examples on the same page are
`Microsoft.NET.Sdk`, `Microsoft.NET.Sdk.Web`, `Microsoft.NET.Sdk.Razor`,
`Microsoft.NET.Sdk.BlazorWebAssembly`, `Microsoft.NET.Sdk.Worker`,
`Aspire.AppHost.Sdk`, `MSTest.Sdk`; a versioned form
(`MSBuild.Sdk.Extras/2.0.54`) is documented too. The adapter accepts any
SDK name (the `Sdk` value is consumed unmodeled — the grammar plans
version rows, not SDK variants) and refuses a `.csproj` whose root
`Project` element carries neither the attribute nor the child element:
a non-SDK (classic) project's metadata may live outside the file
(`packages.config` / `.nuspec`), and planning it would guess.

## 2. Package identity

The pack target's property table (`msbuild-targets`, "pack target",
row `Id`) — `PackageId`'s documented default is **`$(AssemblyName)`**:

> | `Id` | `PackageId` | `$(AssemblyName)` | `$(AssemblyName)` from MSBuild |

and the inputs table:

> `PackageId` — Specifies the name for the resulting package. If not
> specified, the `pack` operation will default to using the
> `AssemblyName` or directory name as the name of the package.

The adapter's identity chain is therefore `<PackageId>` →
`<AssemblyName>` → the manifest file's stem (the documented
`AssemblyName` default when the property is unset). The chain is
evaluated per project file; a `PackageId` or `AssemblyName` inside a
conditioned `PropertyGroup` (§5) is consumed unmodeled.

## 3. The version: `PackageVersion` overrides `Version`

The pack target's property table (`msbuild-targets`, "pack target", row
`Version`):

> | `Version` | `PackageVersion` | Version | This is semver compatible,
> for example `1.0.0`, `1.0.0-beta`, or `1.0.0-beta-00345`. Defaults to
> `Version` if not set. |

The inputs table pins the resolution:

> `PackageVersion` — Specifies the version that the resulting package
> will have. Accepts all forms of NuGet version string. Default is the
> value of `$(Version)`, that is, of the property `Version` in the
> project.

So the adapter's version resolution is `PackageVersion` → `Version`:
the element pack actually consumes is the edit site (the plan's
version patch lands on `PackageVersion` when present, else on
`Version`).

The prefix/suffix pair exists but is composite evaluation semantics:

> | `VersionPrefix` | `VersionPrefix` | empty | Setting `PackageVersion`
> overwrites `VersionPrefix` |
> | `VersionSuffix` | `VersionSuffix` | empty | Setting `PackageVersion`
> overwrites `VersionSuffix` |

A project carrying only `VersionPrefix`/`VersionSuffix` (no
`PackageVersion`, no `Version`) resolves its version through the
`$(VersionPrefix)$(VersionSuffix)` composition — an evaluated value a
pure discovery pass does not compute. The adapter warns
(`non-canonical-version`) and plans no version for that shape, never
rewrites it (the Maven `${...}` posture, ADR-0058).

## 4. NuGet version grammar: SemVer 2.0.0 with non-canonical spellings

`package-versioning` pins the standard:

> The following document follows the Semantic Versioning 2.0.0 standard,
> supported by NuGet 4.3.0+ and Visual Studio 2017 version 15.3+.

and the form:

> A specific version number is in the form *Major.Minor.Patch[-Suffix]*.

The adapter applies the ADR-0056/ADR-0058 discipline: only spellings
that parse as the plan contract's semver are canonical (planned with an
editable site). NuGet's own grammar admits spellings semver cannot
render, documented in the same page:

- *"NuGetVersion only requires the major segment to be defined. All
  others are optional... This means that `1`, `1.0`, `1.0.0`, and
  `1.0.0.0` are all accepted and equal."* (two-component and
  four-component forms);
- *"Leading zeroes are removed from version numbers: 1.00 is treated as
  1.0"* (normalization);
- *"SemVer 2.0.0 build metadata is removed: 1.0.7+r3456 is treated as
  1.0.7"* (build metadata is legal but stripped on resolve).

Each is **recognized-but-non-canonical**: warned, unpublishable, no edit
site, never rewritten. The prerelease form (`-beta.1`, dot-separated) is
SemVer 2.0.0 proper (*"The pre-release label is dot-separated, for
example, 1.0.0-alpha.1"*) and plans verbatim.

## 5. Conditions are build evaluation semantics

PackageReference conditions are documented
(`package-references-in-project-files`, "Adding a PackageReference
condition"):

> You can use a condition to control whether a package is included...
> Conditions can also be applied at the `ItemGroup` level and will apply
> to all children `PackageReference` elements.

MSBuild conditions are allowed on any element; a conditioned value may
not be the pack-time value. The adapter consumes a conditioned
`PropertyGroup`/`Sdk`/item **unmodeled with a warning** (a property or
reference inside a conditioned element never plans a version row or an
edge; a conditioned version element is tainted like the pom scanner's
comment-inside-value taint). A `<PackageReference>`/`<ProjectReference>`
carrying its own `Condition` attribute is likewise consumed unmodeled.

## 6. Property evaluation: later definitions overwrite earlier ones

`customize-by-directory` ("Troubleshooting — Property not taking
effect"):

> 1. **Check import order**: The property might be getting overwritten
>    later.

and ("Import order"):

> Properties that are set in *Directory.Build.props* can be overridden
> elsewhere in the project file or in imported files, so you should
> think of the settings in *Directory.Build.props* as specifying the
> defaults for your projects.

The adapter's scan is a top-to-bottom read of the project file; when the
same version property is declared twice, **the last declaration wins**
(the overwritten-earlier rule above), and the row's edit site follows
the winning element.

## 7. Shared property files are consumed unmodeled

> When MSBuild runs, *Microsoft.Common.props* searches your directory
> structure for the *Directory.Build.props* file. If it finds one, it
> imports the file and reads the properties defined within it.
> (`customize-by-directory`, "Directory.Build.props and
> Directory.Build.targets")

A `Directory.Build.props` / `Directory.Build.targets` anywhere in the
committed tree means a project's effective properties may come from an
imported file (the targets file is imported late and *"can override
properties and targets defined in most of the build logic"*). The
adapter emits one `ignored-section` warning per project when either
file exists in the tree: the project file's own `<PackageVersion>` /
`<Version>` remains the plan source (§6's override rule), but a project
carrying none is warned `no-version` and never inherits an imported
value the scanner cannot evaluate (the go/maven no-version posture).

## 8. Dependencies

Package references (`package-references-in-project-files`, "Adding a
PackageReference"):

> Add a dependency in your project file using the following syntax:
> `<PackageReference Include="Contoso.Utility.UsefulStuff" Version="3.6.0" />`

The `Version` attribute carries a NuGet **range** (`6.1`, `[6.1.0]`,
`3.6.*` floating — *"Floating versions are supported with
PackageReference"*), not the plan contract's requirement grammar: rows
ride `ReqSpec::Other` verbatim with the attribute's span, exactly like
Maven's `ReqSpec::Other` rows. A `PackageReference` without a `Version`
attribute is a corrupt row per the best-practice rule (*"Always specify
a version or version range for package dependencies..."*) — warned and
consumed unmodeled. Central-package-management rows (version supplied by
`Directory.Packages.props`) are that shape — consumed unmodeled, never
resolved.

Project references (`msbuild-targets`, "Project to project references"):

> Project to project references are considered by default as NuGet
> package references. For example:
> `<ProjectReference Include="..\UwpLibrary2\UwpLibrary2.csproj"/>`

The `Include` path **names the referenced project file** — the
proven-internal edge: the adapter normalizes the path
(backslashes→slashes, project-directory-relative) and the edge lands
when it matches a discovered member's manifest path. A `ProjectReference`
naming a file the tree does not contain is a fail-closed
`E-DISC-GLOB` (the Maven `<module>` posture). There is no documented
dev/test scope grammar on `PackageReference`/`ProjectReference` — all
internal edges are normal edges (Maven's `<scope>test</scope>` mapping
has no NuGet counterpart; inventing one would guess).

## 9. Publish posture

The M8 gate row is *"each: discover/edit/publish/resume tests + replay on
2 real repos"*. The engine's publish effect spawns the ecosystem's own
tool; the argv tables are verified against the **installed** tool
(`uv 0.13.9 --help` for PyPI, ADR-0056) or the tool's own reference
documentation (`dotnet`, §11 below — Microsoft Learn, fetched live) —
no flag is ever invented. As-built (ADR-0059 continued): the `dotnet`
argv arm landed (§11), the nuget.org flat-container probe landed (§11),
the `NUGET_API_KEY` auth arm landed (§11); rows carry
`publishable = true` at the documented packability gate (§10) and the
planner's registry arm names `nuget.org` — the engine routes the plan
contract's registry name to the probe. Recorded-open: the corpus-lane
replay on 2 real repos (token/network-bound, the M7 lane discipline).

## 10. Packability

> `IsPackable` — A Boolean value that specifies whether the project can
> be packed. The default value is `true`. (`msbuild-targets`, "pack
> target inputs")

A project declaring `<IsPackable>false</IsPackable>` cannot pack: the
row keeps its version (a release tag still plans) but is
unpublishable.

## 11. The publish half — Microsoft Learn, fetched live (2026-11-27)

The §17 M8 `NuGet` cell's recorded-open argv/probe/auth arms, verified
against the tool's own reference pages (fetched 2026-11-27): the
quickstart (*create-and-publish-a-package-using-the-dotnet-cli*), the
`dotnet nuget push` CLI reference, and the NuGet API's
`PackageBaseAddress` resource page.

### The two-step flow (the push reference's own statement)

> The command pushes an existing package. It doesn't create a package.
> To create a package, use `dotnet pack`.

### The pack step (the quickstart)

> To build a NuGet package or *.nupkg* file from the project, run the
> [dotnet pack](/en-us/dotnet/core/tools/dotnet-pack) command, which
> also builds the project automatically.

> ```dotnetcli
> dotnet pack
> ```

The sample output documents where the artifact lands:

> Successfully created package
> 'C:\Users\myname\source\repos\AppLogger\bin\Debug\Contoso.08.28.22.001.Test.1.0.0.nupkg'.

(the default Debug configuration; `<id>.<version>.nupkg` under
`bin/Debug/` — the push path the grammar builds.)

### The push step (the quickstart + the CLI reference)

> ```dotnetcli
> dotnet nuget push <package-file> --api-key <API-key> --source
> https://api.nuget.org/v3/index.json
> ```

> `-s|--source <SOURCE>` — Specifies the server URL. [...] Starting
> with NuGet 3.4.2, this is a mandatory parameter unless the NuGet
> config file specifies a `DefaultPushSource` value.

(the documented official-server URL rides the argv; the credential does
not — see below.)

### The auth arm (the quickstart's own tip)

> Starting with NuGet 7.6 (.NET SDK 10.0.300), you can set the
> `NUGET_API_KEY` environment variable instead of passing `--api-key`
> on the command line. This is useful in CI/CD scenarios where you
> don't want to pass the key as a command-line argument.

(the recency is recorded honestly: NuGet 7.6 / SDK 10.0.300+; older
SDKs keep the tool's own NuGet.config credential store as the absence
posture — the auth seam injects nothing when the variable is absent.)

### The probe arm (the `PackageBaseAddress` resource)

> This resource is commonly referred to as either the "package base
> address" or as the "flat container".

> ```
> GET {@id}/{LOWER_ID}/index.json
> ```

> If the package source has no versions of the provided package ID, a
> 404 status code is returned.

> The strings in the `versions` array are all lowercased, normalized
> NuGet version strings.

> ```
> GET https://api.nuget.org/v3-flatcontainer/owin/index.json
> ```

(the sample base `https://api.nuget.org/v3-flatcontainer/` is the
client's default; `LOWER_ID` — *"The package ID, lowercased using the
rules implemented by .NET's `System.String.ToLowerInvariant()`"*. The
plan contract's canonical `X.Y.Z` rows compare lowercased — build
metadata never reaches a row, the csproj strategy refuses
non-canonical spellings at discovery.)

### Consequences recorded (ADR-0059 continued)

- The spawn is the two documented steps: bare `dotnet pack`, then
  `dotnet nuget push bin/Debug/<id>.<version>.nupkg --source
  https://api.nuget.org/v3/index.json` — no flag invented (the
  ADR-0033 unset-is-semantic rule at dotnet's grammar).
- The dry forecast runs the packaging half only: `dotnet pack` is
  local-only (no registry contact), but the push carries **no
  documented dry form** — a forecast never invents one; the live run
  spawns both steps.
- Rows are `publishable` at the packability gate (§10); the registry
  arm names `nuget.org`, which routes to the flat-container probe.

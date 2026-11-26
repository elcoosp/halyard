# Maven POM grammars — VERIFIED reference

Status: VERIFIED live against the official Maven references on 2026-11-26
(the M8 **Maven/Gradle** cell, ADR-0058). Quotes are verbatim from the
pages listed per section; the grammar work rides only these.

## 1. Coordinates — `https://maven.apache.org/pom.html`

The three-coordinate identity (the adapter's package identity rides this):

> The POM defined above is the bare minimum that Maven allows.
> groupId:artifactId:version are all required fields (although, groupId
> and version do not need to be explicitly defined if they are inherited
> from a parent - more on inheritance later). The three fields act much
> like an address and timestamp in one. This marks a specific place in a
> repository, acting like a coordinate system for Maven projects

## 2. Inheritance — the same page

What a child inherits (the parent-POM inheritance the adapter plans as
the workspace-inherited posture, §6.3):

> Now we may add values to the parent POM, which will be inherited by
> its children. Most elements from the parent POM are inherited by its
> children, including:
>
> - groupId
> - version
> - description
> - url
> ...
> - dependencies

and what is *not* inherited:

> Notable elements which are not inherited include:
>
> - artifactId
> - name
> - prerequisites
> - profiles (but the effects of active profiles from parent POMs are)

The `parent` element's `relativePath` default — the rule the adapter's
member lookup follows (the parent POM must be the aggregator whose
coordinates match, else fail closed):

> Notice the <relativePath> element. It is not required, but may be used
> as a signifier to Maven to first search the path given for this
> project's parent, before searching the local and then remote
> repositories. Without it, Maven looks in ../pom.xml, and uses the POM
> it finds there only if its coordinates match the <parent> element.
> With an empty <relativePath/>, Maven 3 skips the file system and
> searches the repositories; Maven 4 also finds the parent among the
> projects of the build.

## 3. Aggregation (multi-module) — the same page

The module-listing grammar (member discovery rides exactly this; a
module entry is a relative directory or POM-file path):

> A project with modules is known as a multi-module, or aggregator
> project. Modules are projects that this POM lists, and are executed as
> a group. A pom packaged project may aggregate the build of a set of
> projects by listing them as modules, which are relative paths to the
> directories or the POM files of those projects.

The aggregator's packaging is fixed:

> The <packaging> type is required to be pom for parent and aggregation
> (multi-module) projects.

The default packaging (a single `pom.xml` with no `<packaging>` and no
`<modules>` is the single-package posture, §6.1):

> When no packaging is declared, Maven assumes the packaging is the
> default: jar.

## 4. Version comparison — `maven-artifact` `ComparableVersion` javadoc, `https://maven.apache.org/ref/3.9.11/maven-artifact/apidocs/org/apache/maven/artifact/versioning/ComparableVersion.html`

Maven versions are **arbitrary strings under Maven's own comparator** —
not semantic versions. The class-level javadoc, verbatim:

> Generic implementation of version comparison.
>
> Features:
>
> - mixing of '-' (hyphen) and '.' (dot) separators,
> - transition between characters and digits also constitutes a
>   separator: `1.0alpha1 => [1, 0, alpha, 1]`
> - unlimited number of version components,
> - version components in the text can be digits or strings,
> - strings are checked for well-known qualifiers and the qualifier
>   ordering is used for version ordering.
>   Well-known qualifiers (case insensitive) are:
>
>   - alpha or a
>   - beta or b
>   - milestone or m
>   - rc or cr
>   - snapshot
>   - (the empty string) or ga or final
>   - sp

with the tail:

> Unknown qualifiers are considered after known qualifiers, with lexical
> order (always case insensitive)

This is the grammar fact the version strategy stands on: a `<version>`
admits spellings (`1.0`, `1.0-SNAPSHOT`, `1.0-alpha-1`, `2022.01.1`,
property references) that Maven itself orders by `ComparableVersion`.
Halyard's plan contract is semver-typed (`semver::Version`, §5.5), so the
adapter applies the **PEP 440 discipline of ADR-0056**: only
semver-parseable spellings are canonical (planned, editable);
Maven-specific or property-reference spellings are
recognized-but-non-canonical — **warned, unpublishable, never rewritten**
(fail closed at discovery, no edit site). No Maven-version rewrite is
invented.

## 5. Decisions recorded

The semver-canonical line: Maven qualifier forms that parse as semver
(`1.2.3-SNAPSHOT`, `1.0-alpha-1` — prerelease identifiers admit
`[0-9A-Za-z-]` per the semver grammar) are **canonical** and plan
verbatim; the fail-closed set is the spellings the plan contract cannot
type — two-component `1.0` (never padded), date forms, property
references.

- Package identity: the `artifactId` (colon-free, so the shared
  appendix-A.3 tag grammar renders legal git refnames — a colon is not
  legal in a git refname). Duplicate `artifactId`s within one discovered
  maven ecosystem are the existing `E-DISC-DUPLICATE` refusal (the
  name-keyed group/tag grammar requires artifactId uniqueness in one
  workspace, the cargo posture).
- Internal edges: a `<dependency>` whose (groupId, artifactId) pair
  names a discovered member (`ReqSpec::Other` verbatim per ADR-0006 —
  Maven ranges/properties are never interpreted).
- Module entries naming a nested POM *file* (`third-project/pom-example.xml`)
  or an absolute path, and `settings.xml`-level source overrides, are
  out of the shipped grammar (fail closed) — the recorded-open
  remainders live in ADR-0058.

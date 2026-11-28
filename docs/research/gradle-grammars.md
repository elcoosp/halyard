# Gradle build-file grammars — VERIFIED reference

> The grammar gate ADR-0058's Consequences named for the Gradle half:
> *"a `build.gradle(.kts)` version is arbitrary Groovy/Kotlin build
> code (an assignment, a closure, a `gradle.properties` lookup, a plugin
> convention); extracting one without an evaluated, VERIFIED grammar
> would guess, and halyard's discipline forbids guessing."* This note is
> the evaluated, VERIFIED grammar that lifts the gate — the
> deterministic-specs literal-assignment subset, exactly the discipline
> ADR-0061's gemspec scanner established (a computed or conditionally
> guarded `version` row is consumed unmodeled, never a slot).

## 1. The `version` property — `https://docs.gradle.org/current/dsl/org.gradle.api.Project.html`

The Project API documents `version` as a writable `Object` property of
every project (fetched 2026-11-27):

> *"The version of this project. Gradle always uses the `toString()`
> value of the version. The version defaults to `unspecified`."*

The property's type is `Object` (not read-only); the page's scope rules
state the Project object *"includes any property getters and setters
declared by the `Project` implementation class"* and that these *"are
readable or writable depending on the presence of the corresponding
getter and setter method."* Build scripts delegate to the Project
object — the page's Dynamic Project Properties section explains that
*"any property or method which your script uses is delegated through to
the associated `Project` object,"* so `version = '1.0.0'` in a build
script delegates to the Project's version setter.

The decisive consequence for halyard: `version` is a plain writable
property set by **assignment** (`version = …`), and **the last
assignment in document order wins** — each assignment calls the setter
and overwrites the prior value, exactly the property-setter semantics
the page documents (the scope search *"sets the property in the first
scope it finds the property in"*). There is no override *rule* to model
(no documented "last label wins" like the Dockerfile `LABEL` override);
there is only ordinary property overwrite. The scanner records every
`version = …` row and the adapter takes the last — quietly, no
superseded warning (a conditional `version = …` is normal Groovy/Kotlin
control flow, not a surprising override).

## 2. The assignment syntax — `https://docs.gradle.org/current/userguide/building_java_projects.html`

The Building Java Projects guide documents the canonical top-level
string assignment in both DSLs (fetched 2026-11-27). The introduction
shows, alongside *"The simplest build script for a Java project applies
the Java Library Plugin and optionally sets the project version and
selects the Java toolchain to use"*:

- Kotlin DSL:
  ```kotlin
  version = "1.2.1"
  ```
- Groovy DSL:
  ```groovy
  version = '1.2.1'
  ```

The Uber JAR section repeats the same pair (`version = "1.0.0"` /
`version = '1.0.0'`). The guide's note: *"Although the properties in
the example are optional, we recommend that you specify them in your
projects."* and *"The version string is important for tracking the
progression of the project. The project version is also used in archive
names by default."*

Only the `version = <string-literal>` assignment form is VERIFIED at
this grammar — a direct string literal on the right-hand side of `=`,
at any point in the script (the property setter is in scope wherever
the build script's top-level delegation runs; a `version = …` inside a
configured block is the same setter call). The right-hand side is a
**literal** string:

- Groovy single-quoted `'1.2.1'` — a `java.lang.String` literal (the
  Groovy language: single-quoted strings are literal, no
  interpolation);
- Groovy/Kotlin double-quoted `"1.2.1"` — a literal **only when it
  carries no template** (`$name` / `${expr}` in either DSL is
  interpolation: a Groovy `GString` or a Kotlin string template). A `$`
  inside a double-quoted value is the non-literal signal — the version
  is computed, not a literal.

Every other right-hand side is **non-literal** and the row is consumed
unmodeled (the fail-closed discipline): a property reference
(`version = appVersion`), a `gradle.properties` lookup
(`version = findProperty('appVersion')` / `version = property('v')`),
a version-catalog accessor (`version = libs.versions.app.get()`), a
plugin convention, a closure, or the method-call form
(`version '1.0.0'` without `=` — valid Groovy, never the documented
assignment). These are recognized — a `version = …` row exists — but
the value is not a literal string, so there is no byte span to edit and
the row warns `non-canonical-version` (not `no-version`).

## 3. Comments and string literals — the scanner's lexical substrate

The scanner hand-rolls the lexical substrate the assignment grammar
rides (no parser dependency, the ADR-0001 JSON-scanner precedent shared
with the gomod/gemspec/Dockerfile lexers):

- **Line comments** `//` to end of line — both DSLs.
- **Block comments** `/* … */` (not nested) — both DSLs.
- **String literals**: single-quoted `'…'` (Groovy literal; the closing
  quote ends the literal, a backslash escapes the next byte) and
  double-quoted `"…"` (the closing quote ends the literal; a `$` inside
  flags interpolation → non-literal; a backslash escapes the next byte).
  Triple-quoted forms (`'''…'''` / `"""…"""`) are out of the
  deterministic-specs subset — a `version = """…"""` row is consumed
  unmodeled (the literal-assignment grammar declines the multi-line
  form rather than guessing its escape rules).

The span the scanner captures is the **inner bytes** of the literal
(the bytes between the quotes) in the ORIGINAL file coordinates, so the
edit seam swaps exactly those bytes and nothing else moves (invariant
I4). A literal whose bytes the plan contract's semver cannot parse is
recognized-but-non-canonical — warned, unpublishable, never rewritten
(the ADR-0056 discipline shared with every M8 cell).

## 4. Identity — the package name

A Gradle build is one project per `build.gradle` / `build.gradle.kts`
file: the project is the package, the manifest is the build file (the
per-file shape the Dockerfile and gemspec cells established — every
manifest in the committed tree is a standalone package row). The
project's name rides `rootProject.name` in a sibling
`settings.gradle` / `settings.gradle.kts` (the Gradle authoring
convention) — a grammar the deterministic-specs subset does not yet
cover, so the identity chain falls back to the containing directory's
name (the documented build context, the Dockerfile cell's chain) and
then the manifest path. A root build file with no containing directory
keeps the manifest path as its name (the Dockerfile cell's `Dockerfile`
fallback) — the scanner never guesses a name. The
`settings.gradle`/`include` grammar, the `project(":sub")` dependency
edge, and the multi-project aggregator shape stay the recorded-open
remainders (the pom.xml cell's `<modules>` aggregator is Maven's own;
Gradle's `include` grammar is its own research gate). *(Lifted
2026-11-28: both remainders landed at the VERIFIED grammar — §6 is the
settings gate's evaluation, §7 the project edge; the identity chain
gains the settings provenance in front of the directory chain.)*

## 5. Decisions recorded

- **D1** — the scanner hand-rolls the `version = <string-literal>`
  assignment at the VERIFIED grammar (Groovy `'1.2.1'`, Kotlin
  `"1.2.1"`), offset-preserving like the sibling lexers; the right-hand
  side is a literal only when it is a single-quoted string or a
  double-quoted string carrying no `$` (interpolation is non-literal).
- **D2** — the last `version = …` assignment in document order wins
  (the Project API's property-setter overwrite; no override *rule*, no
  superseded warning). Every other right-hand side is recognized but
  non-canonical — `non-canonical-version`, no site, never rewritten.
- **D3** — the version strategy is the ADR-0056 fail-closed discipline:
  only semver-parseable literal values are canonical (planned with an
  edit site on the literal's inner-byte span); every other spelling is
  warned and unpublishable. Gradle publishing rides the Maven cell's
  disabled shape (`maven-central`, `publishable = false`, no `gradle`
  argv arm — the credential surface is `~/.gradle/gradle.properties` +
  the `publishing` block, no documented env-var grammar).
- **D4** — the identity chain: the containing directory's name → the
  manifest path (the `rootProject.name`/`include` grammar is the
  recorded-open remainder). Internal edges are none (the
  `project(":sub")` dependency grammar is a recorded-open remainder;
  cross-package cascade rides the explicit `[[link]]` config surface,
  the Dockerfile cell's posture).
- **D5** — `build.gradle` and `build.gradle.kts` are the two manifest
  names the walk reads (the documented build-file names; a
  `buildSrc`/`settings.gradle`/`init.gradle` grammar is out of the
  subset). The Gradle half rides `Ecosystem::Maven` (the Maven/Gradle
  cell's single ecosystem — both publish to Maven Central; the
  `Ecosystem::Maven` doc names the cell's shipped half, this note names
  the Gradle half), with a new `ManifestFormat::GradleBuild` variant
  naming the grammar in discovery errors.

## 6. The settings-file grammar — the multi-project aggregator shape
(VERIFIED 2026-11-28)

The recorded-open remainder the Gradle half landed with ("the
`settings.gradle` `rootProject.name` / `include` grammar — the
multi-project aggregator shape") lifts here, VERIFIED against
`https://docs.gradle.org/current/dsl/org.gradle.api.initialization.Settings.html`
(fetched 2026-11-28) and the `org.gradle.api.initialization.ProjectDescriptor`
javadoc (`https://docs.gradle.org/current/javadoc/org/gradle/api/initialization/ProjectDescriptor.html`):

- `rootProject` is the Settings API's read-only property — *"The root
  project of the build"* — typed `ProjectDescriptor`, whose `name` is a
  writable property (the javadoc's `setName(String)`: *"Sets the name
  of this project"*; `getName()`: *"Returns the name of this
  project. Never returns null."*). The settings script delegates to the
  Settings object (the DSL page's script-delegation rules), so the
  `rootProject.name = '…'` row is the descriptor's name setter.
- The documented default: *"The root project's name defaults to the
  name of the directory containing the settings file."* — the fallback
  halyard applies when no literal row applies. At the repository root
  the committed tree cannot know the checkout directory's own name, so
  the chain falls to the manifest path (the Dockerfile cell's chain);
  a nested settings directory falls to its own directory name.
- The last `rootProject.name = <literal>` assignment in document order
  wins — the same property-setter overwrite the `version` row rides
  (each assignment calls `setName`); no override *rule* exists.
- `include` — the Settings API's *"Adds the given projects to the
  build"* — carries the two documented overloads
  (`include(String... projectPaths)` /
  `include(Iterable<String> projectPaths)`), callable unparenthesized
  (Groovy) or parenthesized (Kotlin and Groovy), comma-separated:
  *"these path are not file paths, but instead specify the location of
  the new project in the project hierarchy"*; *"the supplied paths
  must use the `:` character as separator (and NOT `/`)"*; *"The last
  element of the supplied path is used as the project name"*; *"The
  supplied path is converted to a project directory relative to the
  root project directory"*; and the page's own example — *"the path
  `a:b` adds a project with path `:a:b`, name `b` and project
  directory `$rootDir/a/b`"* — with *"It also adds the parent `:a`
  project if it doesn't already exist"* (the implicit-parent rule the
  discovery governance applies). A leading `:` is Gradle's own
  normalization (the page's example writes the bare `a:b` form), so
  both spellings parse.
- A non-literal argument (a `$` template, a property reference) is
  recognized but non-canonical: that member is not proven, the row
  warns, nothing is guessed.
- Both settings spellings in one directory (`settings.gradle` +
  `settings.gradle.kts`) have no documented precedence — the
  fail-closed discipline declines all provenance and warns
  (`ambiguous-settings`).
- The governance shape: a settings file governs its own directory
  subtree — the root build file (named by `rootProject.name` when a
  literal row applies) and the include-proven member directories (named
  by the path's last element). A build file the include set does not
  prove keeps the standalone row with a warning
  (`unincluded-gradle-build`) — the per-file shape never silently
  shrinks. A proven member without a build file is Gradle's
  build-less-project shape: no row exists, and the root row warns
  (`include-without-manifest`). Nested settings files (composite
  builds) govern their own subtrees; the nearest settings directory
  wins for the files it governs. The `includeBuild` /
  `pluginManagement` / `dependencyResolutionManagement` surfaces are
  different grammars — consumed unmodeled. A settings file is never a
  package manifest (it carries no version slot).

## 7. The project dependency edge — `project(':path')` (VERIFIED
2026-11-28)

The second recorded-open remainder (the `project(":sub")` dependency
edge) lifts at the same page set: the Settings page's path rules
(*"the supplied paths must use the `:` character as separator"*; *"The
supplied path is converted to a project directory relative to the root
project directory"*) govern the `project(':path')` argument the same
way they govern `include`. The capture:

- only the **top-level** `dependencies { … }` block is the owning
  project's dependency statement; a block nested inside
  `allprojects`/`subprojects` configures OTHER projects and is consumed
  unmodeled (not the owner's edge, no warning — the subset boundary);
- a `project(…)` call OUTSIDE a `dependencies` block
  (`evaluationDependsOn(':lib')`) is a build-order declaration, not a
  version-cascade dependency — unmodeled;
- the captured shape is `<configuration> project(':path')` — the
  configuration identifier before the call (`implementation`, `api`,
  `testImplementation`, …); the Kotlin parenthesized form
  `implementation(project(":path"))` is the same shape;
- only the leading-`:` root-relative literal is the proven shape — a
  path resolved relative to the owning project (`project('lib')`) and
  a `$` template are recognized but non-canonical (warned, never an
  edge — the fail-closed discipline);
- the requirement rides `ReqSpec::PathOnly`: a project dependency
  carries no version constraint — nothing to rewrite, no edit site;
  the resolution matches the include-proven member by the row name the
  include path's last element names, the orchestrator's
  `resolve_dep` matches a same-ecosystem member by name, and the edge
  joins the workspace graph (`halyard graph`, the `affected`
  closure, the plan's topological publish order);
- the cascade policy is the planner's own (a `PathOnly` edge never
  mismatches — §10.4's `when-unsatisfied` yields no forced bump);
  `[[link]]` remains the explicit forced-cascade/lockstep surface;
- a `test*` configuration is the dev edge (the maven cell's
  `<scope>test</scope>` mapping, §6.3's dev-does-not-cascade default).

## 8. Decisions recorded (2026-11-28, the settings/edges session)

- **D6** — the settings grammar is the multi-project aggregator shape:
  the governed root named by `rootProject.name` (last literal
  assignment wins), the include-proven members named by their path's
  last element (implicit parents included), the unincluded build files
  keeping the standalone rows (warned), the ambiguous settings pair
  declining all provenance (warned). The identity chain of §4/D4 gains
  the settings provenance in front of the directory chain.
- **D7** — the dependency edge is the top-level-block-only,
  leading-`:`-only, `PathOnly` shape of §7: internal when the path
  resolves to an include-proven member, warned-external otherwise.
- **D8** — the publish posture is unchanged (D5): no `gradle` argv
  arm exists (the credential surface is `~/.gradle/gradle.properties`
  + the `publishing` block, no documented env-var grammar), rows stay
  `publishable = false`, and the `gradle.properties` version site, the
  `mvn deploy` argv arm and the corpus-lane replay stay the Maven/Gradle
  cell's recorded-open remainders.

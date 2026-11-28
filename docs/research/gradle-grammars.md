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
Gradle's `include` grammar is its own research gate).

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

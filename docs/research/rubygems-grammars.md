# RubyGems grammars — VERIFIED reference

Status: VERIFIED live against the official RubyGems sources on
2026-11-27 (the M8 **RubyGems** cell, ADR-0061). Quotes are verbatim
from the pages listed per section; the grammar work rides only these.

## 1. The gemspec shape — `https://guides.rubygems.org/specification-reference/`

The manifest shape the adapter scans (a `.gemspec` file carrying a
`Gem::Specification.new` block whose attribute rows are assignments):

> Specifications are typically defined in a .gemspec file or a Rakefile,
> and looks like this:
>
> ```ruby
> Gem::Specification.new do |s|
>   s.name = 'example'
>   s.version = '0.1.0'
>   # ... licenses, summary, description, authors, files, homepage, metadata
> end
> ```
>
> Specifications must be deterministic, as in the example above.

The deterministic rule is why the adapter pairs only **literal string
assignments**: a conditionally-defined attribute is documented invalid,
and a version computed from Ruby build code has no honest byte span.

The required-attribute list (the adapter's identity and version fields
are required rows):

> Required gemspec attributes: authors=, files, name, summary, version

- name: "This gem's name."
- version: "This gem's version." — "The version string can contain
  numbers and periods, such as 1.0.0." and "A gem is a 'prerelease' gem
  if the version has a letter in it, such as 1.0.0.pre."

The dependency methods (the edge grammar the adapter walks):

> **add_dependency(gem, \*requirements)** — Adds a runtime dependency
> named gem with requirements to this gem. Also known as:
> add_runtime_dependency
>
> **add_development_dependency(gem, \*requirements)** — Adds a
> development dependency named gem with requirements to this gem.

> Development dependencies aren't installed by default and aren't
> activated when a gem is required.

Requirements are extra string arguments at the call site, e.g.
`spec.add_dependency 'example', '>= 1.1.4', '< 2'` (the pessimistic
`~>` operator rides verbatim — a RubyGems requirement is not semver, so
the row is a foreign-grammar constraint per ADR-0006 and is never
rewritten).

## 2. Gem::Version — `lib/rubygems/version.rb` (rubygems/rubygems, master)

The version grammar the version row is validated against:

> ```ruby
> VERSION_PATTERN = '[0-9]+(?>\.[0-9a-zA-Z]+)*(-[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*)?'
> ANCHORED_VERSION_PATTERN = /\A\s*(#{VERSION_PATTERN})?\s*\z/
> ```

> If any part contains letters (currently only a-z are supported) then
> that version is considered prerelease.

> Versions shall be represented by three non-negative integers,
> separated by periods (e.g. 3.1.4). The first number is the "major
> version". The second is the "minor version". The third is the "patch
> version".

> Malformed version number string #{version} (the `ArgumentError`
> raised on input the pattern does not accept).

The plan-contract pairing discipline (ADR-0056): only spellings the
`semver` crate parses are canonical (`1.2.3`, the dash-suffix form
`1.2.3-rc.1` — the pattern's `(-[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*)?`
suffix — and letter-segment prereleases like `1.2.3.pre.1`). Every
other Gem::Version spelling (`1.2`, the four-segment `1.2.3.1`,
`1.2.3-pre` — a semver-invalid hyphen segment) is
recognized-but-non-canonical: warned, unpublishable, no edit site, never
rewritten.

## 3. Gem names — `https://guides.rubygems.org/name-your-gem/`

> If a class or module has multiple words, use underscores to separate
> them.

> If you're adding functionality to another gem, use a dash.

> **Don't use UPPERCASE letters** — OS X and Windows have
> case-insensitive filesystems by default.

## 4. Publishing — `https://github.com/rubygems/guides/blob/main/publishing.md`

The publish argv and the artifact it consumes (the build-artifact step
is why no argv arm exists yet — publishing a gem pushes a built `.gem`,
not a manifest):

> $ gem push squid-utils-0.1.0.gem
>
> Successfully registered gem: squid-utils (0.1.0)

The credential surface (a local credentials file maintained by an
interactive sign-in — the publishing guide names no token environment
variable the auth seam's env capture could carry):

> Enter your RubyGems.org credentials.
>
> RubyGems saves the credentials in ~/.gem/credentials for you so you
> only need to log in once.

The artifact naming convention (implied by the documented example:
name, hyphen, version, `.gem`): `squid-utils-0.1.0.gem`.

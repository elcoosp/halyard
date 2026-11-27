//! The M8 `RubyGems` cell's discovery contract (`docs/HALYARD_ARCHITECTURE.md`
//! §17 YEAR 2, M8 row — *"More ecosystems"*, the `RubyGems` cell — the
//! next listed cell after the shipped `Dart/pub` half (ADR-0060), gate
//! *"each: discover/edit/publish/resume tests + replay on 2 real
//! repos"*; VERIFIED grammar in `docs/research/rubygems-grammars.md`).
//!
//! This is the plan's acceptance criterion pinned as code: `*.gemspec`
//! manifests discover as rubygems rows — the identity is the `name`
//! attribute (the specification reference's *"Required gemspec
//! attributes: authors=, files, name, summary, version"*), the version
//! rides the `Gem::Version` grammar (*"Versions shall be represented by
//! three non-negative integers, separated by periods (e.g. 3.1.4)."* —
//! only semver-parseable spellings are canonical per ADR-0056's
//! discipline), a dependency *naming* another discovered gemspec is the
//! proven-internal edge, every `RubyGems` requirement (`~>`, `>=`) rides
//! verbatim (ADR-0006), and every gem-legal spelling the plan contract's
//! semver cannot render (a two-component `1.2`) is warned, unpublishable,
//! never rewritten. The specification reference's *"Specifications must
//! be deterministic"* pins the literal-assignment grammar: a conditionally
//! defined version is consumed unmodeled, never rewritten.

use halyard_core::EcosystemAdapter;
use halyard_core::discover::{DiscoverCtx, DiscoverError, ManifestFormat, RawVersionSource};
use halyard_core::package::{DepKind, Ecosystem, ReqSpec};
use halyard_eco_rubygems::RubyGemsAdapter;

/// Builds a context from `(path, body)` fixture files.
fn ctx_with(files: &[(&str, &str)]) -> DiscoverCtx {
    let mut ctx = DiscoverCtx::new();
    for (path, body) in files {
        assert!(ctx.insert_file(path, body.as_bytes().to_vec()), "{path}");
    }
    ctx
}

/// The canonical root gemspec: `Gem::Specification.new do |spec|` with a
/// literal `name` and a literal `version` — the specification
/// reference's own documented shape.
const SHARED_GEMSPEC: &str = r"# frozen_string_literal: true

Gem::Specification.new do |spec|
  spec.name = 'shared'
  spec.version = '2.3.0'
  spec.summary = 'the shared library'
  spec.authors = ['Someone']
end
";

/// The canonical root: `shared` + `app` (both inserted by the tests that
/// use it).
fn canonical_pair() -> Vec<(&'static str, &'static str)> {
    vec![
        ("gems/shared/shared.gemspec", SHARED_GEMSPEC),
        (
            "gems/app/app.gemspec",
            r#"# frozen_string_literal: true

Gem::Specification.new do |spec|
  spec.name = "app"
  spec.version = "1.4.0"
  spec.summary = 'the application'
  spec.add_dependency 'shared', '~> 2.3'
  spec.add_dependency 'json', '>= 2.0'
  spec.add_development_dependency 'rake', '~> 13.0'
end
"#,
        ),
    ]
}

#[test]
fn a_literal_gemspec_discovers_as_a_rubygems_row() {
    let ctx = ctx_with(&[("shared.gemspec", SHARED_GEMSPEC)]);
    let rows = RubyGemsAdapter::new().discover(&ctx).expect("discovers");
    assert_eq!(rows.len(), 1);
    let row = rows.first().expect("the single row");
    assert_eq!(row.name, "shared");
    assert_eq!(row.ecosystem, Ecosystem::RubyGems);
    assert_eq!(row.manifest, "shared.gemspec");
    assert_eq!(row.root, "");
    assert_eq!(
        row.version.as_ref().map(std::string::ToString::to_string),
        Some("2.3.0".to_owned())
    );
    assert_eq!(row.version_source, RawVersionSource::Local);
    // The honest publish posture (ADR-0061): no argv arm yet.
    assert!(!row.publishable);
    // The edit site: the version literal's inner bytes (quotes never
    // join the span).
    let site = row.version_site.as_ref().expect("an edit site");
    assert_eq!(site.file.as_str(), "shared.gemspec");
    let text = std::str::from_utf8(ctx.file("shared.gemspec").expect("fixture")).expect("utf-8");
    let span = site.spans.first().expect("the edit span");
    let span_text = &text[span.start as usize..span.end as usize];
    assert_eq!(span_text, "2.3.0", "the span points at the inner bytes");
}

#[test]
fn a_dependency_naming_a_member_is_the_proven_internal_edge() {
    let ctx = ctx_with(&canonical_pair());
    let rows = RubyGemsAdapter::new().discover(&ctx).expect("discovers");
    assert_eq!(rows.len(), 2);
    let app = rows
        .iter()
        .find(|row| row.name == "app")
        .expect("the app row");
    let shared = app
        .deps
        .iter()
        .find(|dep| dep.name == "shared")
        .expect("the member edge");
    assert!(shared.internal, "a member-naming dependency is internal");
    assert_eq!(shared.kind, DepKind::Normal);
    // The pessimistic operator rides verbatim (ADR-0006 — a RubyGems
    // requirement is not semver).
    assert_eq!(
        shared.req,
        ReqSpec::Other {
            raw: "~> 2.3".to_owned().into()
        }
    );
    let json = app
        .deps
        .iter()
        .find(|dep| dep.name == "json")
        .expect("the hosted dep");
    assert!(!json.internal, "an unrelated gem stays registry-resolved");
    let rake = app
        .deps
        .iter()
        .find(|dep| dep.name == "rake")
        .expect("the dev dep");
    assert_eq!(rake.kind, DepKind::Dev);
}

#[test]
fn nested_gemspecs_are_standalone_rows_with_their_own_roots() {
    let ctx = ctx_with(&canonical_pair());
    let rows = RubyGemsAdapter::new().discover(&ctx).expect("discovers");
    for row in &rows {
        // The root is the gemspec's directory (the manifest's parent).
        let expected = row
            .manifest
            .rsplit_once('/')
            .map_or(String::new(), |(dir, _)| dir.to_owned());
        assert_eq!(row.root, expected, "the root is the gemspec's directory");
    }
}

#[test]
fn a_two_component_version_is_recognized_but_never_canonical() {
    let loose = r"# frozen_string_literal: true

Gem::Specification.new do |spec|
  spec.name = 'loose'
  spec.version = '1.2'
end
";
    let ctx = ctx_with(&[("loose.gemspec", loose)]);
    let rows = RubyGemsAdapter::new().discover(&ctx).expect("discovers");
    let row = rows.first().expect("the single row");
    assert!(
        row.version.is_none(),
        "a non-semver Gem::Version never pairs"
    );
    assert!(
        row.version_site.is_none(),
        "no edit site for a non-canonical"
    );
    assert!(
        row.warnings
            .iter()
            .any(|warning| warning.code == "non-canonical-version"),
        "warned: {:?}",
        row.warnings
    );
}

#[test]
fn a_non_literal_version_is_consumed_unmodeled_and_never_a_slot() {
    let computed = r#"# frozen_string_literal: true

require_relative "lib/shared/version"

Gem::Specification.new do |spec|
  spec.name = 'shared'
  spec.version = Shared::VERSION
end
"#;
    let ctx = ctx_with(&[("shared.gemspec", computed)]);
    let rows = RubyGemsAdapter::new().discover(&ctx).expect("discovers");
    let row = rows.first().expect("the single row");
    assert!(row.version.is_none());
    assert!(row.version_site.is_none());
    assert!(
        row.warnings
            .iter()
            .any(|warning| warning.code == "non-canonical-version"),
        "the computed version is warned: {:?}",
        row.warnings
    );
}

#[test]
fn a_missing_version_row_warns_no_version() {
    let bare = r"
Gem::Specification.new do |spec|
  spec.name = 'bare'
  spec.summary = 'no version row'
end
";
    let ctx = ctx_with(&[("bare.gemspec", bare)]);
    let rows = RubyGemsAdapter::new().discover(&ctx).expect("discovers");
    let row = rows.first().expect("the single row");
    assert!(row.version.is_none());
    assert!(
        row.warnings
            .iter()
            .any(|warning| warning.code == "no-version"),
        "warned: {:?}",
        row.warnings
    );
}

#[test]
fn a_missing_name_row_refuses() {
    let nameless = r"
Gem::Specification.new do |spec|
  spec.version = '1.0.0'
end
";
    let ctx = ctx_with(&[("nameless.gemspec", nameless)]);
    let error = RubyGemsAdapter::new()
        .discover(&ctx)
        .expect_err("the identity has no documented default");
    assert!(
        error.to_string().contains("no `name`"),
        "names the missing identity: {error}"
    );
}

#[test]
fn a_non_literal_name_refuses() {
    let computed = r"# frozen_string_literal: true

Gem::Specification.new do |spec|
  spec.name = File.basename(__FILE__, '.gemspec')
  spec.version = '1.0.0'
end
";
    let ctx = ctx_with(&[("shared.gemspec", computed)]);
    let error = RubyGemsAdapter::new()
        .discover(&ctx)
        .expect_err("the identity must be a literal");
    assert!(
        error.to_string().contains("spec.name"),
        "names the offending attribute: {error}"
    );
}

#[test]
fn an_unclosed_gem_specification_block_refuses() {
    let unclosed = r"
Gem::Specification.new do |spec|
  spec.name = 'unclosed'
  spec.version = '1.0.0'
";
    let ctx = ctx_with(&[("unclosed.gemspec", unclosed)]);
    let error = RubyGemsAdapter::new()
        .discover(&ctx)
        .expect_err("never a guess");
    assert!(
        error.to_string().contains("never closes"),
        "the refusal names the block: {error}"
    );
}

#[test]
fn a_second_gem_specification_block_refuses() {
    let doubled = r"
Gem::Specification.new do |spec|
  spec.name = 'first'
  spec.version = '1.0.0'
end

Gem::Specification.new do |spec|
  spec.name = 'second'
  spec.version = '1.0.0'
end
";
    let ctx = ctx_with(&[("doubled.gemspec", doubled)]);
    let error = RubyGemsAdapter::new()
        .discover(&ctx)
        .expect_err("one spec block per gemspec file");
    assert!(
        error.to_string().contains("one Gem::Specification.new"),
        "the refusal names the subset: {error}"
    );
}

#[test]
fn conditional_rows_inside_the_block_are_consumed_unmodeled() {
    let conditional = r"
Gem::Specification.new do |spec|
  spec.name = 'cond'
  if RUBY_ENGINE == 'jruby'
    spec.version = '9.9.9'
  end
  spec.version = '1.0.0'
end
";
    let ctx = ctx_with(&[("cond.gemspec", conditional)]);
    let rows = RubyGemsAdapter::new().discover(&ctx).expect("discovers");
    let row = rows.first().expect("the single row");
    // The deterministic rule ("Specifications must be deterministic") —
    // the conditional row is consumed unmodeled; the unconditional row
    // plans.
    assert_eq!(
        row.version.as_ref().map(std::string::ToString::to_string),
        Some("1.0.0".to_owned())
    );
    assert!(
        row.warnings
            .iter()
            .any(|warning| warning.code == "conditional-attribute"),
        "warned: {:?}",
        row.warnings
    );
}

#[test]
fn heredoc_rows_are_consumed_and_the_version_still_captures() {
    let described = r"# frozen_string_literal: true

Gem::Specification.new do |spec|
  spec.name = 'described'
  spec.version = '1.1.0'
  spec.description = <<~DESC
    a long description
    spanning several lines
  DESC
  spec.summary = 'short'
end
";
    let ctx = ctx_with(&[("described.gemspec", described)]);
    let rows = RubyGemsAdapter::new().discover(&ctx).expect("discovers");
    let row = rows.first().expect("the single row");
    assert_eq!(
        row.version.as_ref().map(std::string::ToString::to_string),
        Some("1.1.0".to_owned())
    );
}

#[test]
fn the_single_line_brace_form_discovers() {
    let brace = r"
Gem::Specification.new { |s| s.name = 'braced' }
";
    let ctx = ctx_with(&[("braced.gemspec", brace)]);
    let rows = RubyGemsAdapter::new().discover(&ctx).expect("discovers");
    let row = rows.first().expect("the single row");
    assert_eq!(row.name, "braced");
    assert!(row.version.is_none(), "no version row in the fixture");
    assert!(
        row.warnings
            .iter()
            .any(|warning| warning.code == "no-version"),
        "warned: {:?}",
        row.warnings
    );
}

#[test]
fn quoted_escapes_and_comments_never_join_the_captured_value() {
    let annotated = r"# frozen_string_literal: true

Gem::Specification.new do |spec|
  spec.name = 'annotated' # the identity
  spec.version = '1.2.3' # the release row
  spec.summary = 'a summary'
end
";
    let ctx = ctx_with(&[("annotated.gemspec", annotated)]);
    let rows = RubyGemsAdapter::new().discover(&ctx).expect("discovers");
    let row = rows.first().expect("the single row");
    assert_eq!(row.name, "annotated");
    assert_eq!(
        row.version.as_ref().map(std::string::ToString::to_string),
        Some("1.2.3".to_owned())
    );
    let site = row.version_site.as_ref().expect("an edit site");
    let text = std::str::from_utf8(ctx.file("annotated.gemspec").expect("fixture")).expect("utf-8");
    let span = site.spans.first().expect("the edit span");
    let span_text = &text[span.start as usize..span.end as usize];
    assert_eq!(span_text, "1.2.3", "the trailing comment never joins");
}

#[test]
fn the_gemspec_format_names_the_scanner_in_manifest_errors() {
    // A hard refusal (missing name) carries the gemspec manifest format.
    let nameless = r"
Gem::Specification.new do |spec|
  spec.version = '1.0.0'
end
";
    let ctx = ctx_with(&[("nameless.gemspec", nameless)]);
    let error = RubyGemsAdapter::new().discover(&ctx).expect_err("refuses");
    match error {
        DiscoverError::Manifest { format, .. } => assert_eq!(format, ManifestFormat::Gemspec),
        other => panic!("expected a Manifest refusal, got: {other}"),
    }
}

#[test]
fn an_empty_gemspec_file_refuses() {
    let ctx = ctx_with(&[("empty.gemspec", "# only a comment\n")]);
    let error = RubyGemsAdapter::new()
        .discover(&ctx)
        .expect_err("no Gem::Specification.new — not a gemspec");
    assert!(
        error.to_string().contains("Gem::Specification"),
        "names the missing block: {error}"
    );
}

/// The empty-row assertion (the pedantic `assert_is_empty` shape).
fn assert_empty<T>(rows: &[T]) {
    assert!(rows.is_empty(), "expected no rows");
}

#[test]
fn a_directory_without_gemspecs_discovers_no_rows() {
    let ctx = ctx_with(&[("README.md", "not a manifest\n")]);
    let rows = RubyGemsAdapter::new().discover(&ctx).expect("discovers");
    assert_empty(&rows);
}

//! Bench: the `RubyGems` gemspec scan (§6.1's hot path at the
//! `.gemspec` grammar, M8 `RubyGems` cell, ADR-0061). Measures the
//! hand-rolled scanner + row assembly over a representative gem
//! repository (two gemspecs — one with hosted/name-matched deps, one
//! with a dev dep and a heredoc description — comments, quoted values,
//! and canonical `version` rows).

use criterion::Criterion;
use halyard_core::EcosystemAdapter;
use halyard_core::discover::DiscoverCtx;
use halyard_eco_rubygems::RubyGemsAdapter;

const SHARED_GEMSPEC: &str = r"# frozen_string_literal: true

Gem::Specification.new do |spec|
  spec.name = 'shared'
  spec.version = '2.3.0' # the release row
  spec.summary = 'the shared library'
  spec.description = <<~DESC
    A long description spanning
    several heredoc lines.
  DESC
  spec.authors = ['Someone']
  spec.files = Dir['lib/**/*.rb']
end
";

const APP_GEMSPEC: &str = r"# frozen_string_literal: true

Gem::Specification.new do |spec|
  spec.name = 'app'
  spec.version = '1.4.0'
  spec.summary = 'the application'
  spec.add_dependency('shared', '~> 2.3')
  spec.add_dependency 'json', '>= 2.0', '< 3.0'
  spec.add_development_dependency 'rake', '~> 13.0'
  spec.add_development_dependency 'rspec', '~> 3.12'
end
";

fn ctx() -> DiscoverCtx {
    let mut ctx = DiscoverCtx::new();
    ctx.insert_file(
        "gems/shared/shared.gemspec",
        SHARED_GEMSPEC.as_bytes().to_vec(),
    );
    ctx.insert_file("gems/app/app.gemspec", APP_GEMSPEC.as_bytes().to_vec());
    ctx
}

fn scan() {
    let ctx = ctx();
    // The fixture is a constant that must parse; a parse failure exits
    // the bench loudly (bench fns are not `#[test]`, so the test-lint
    // allowance does not reach them — the sibling nuget bench posture).
    let rows = match RubyGemsAdapter::new().discover(&ctx) {
        Ok(rows) => rows,
        Err(error) => {
            eprintln!("the bench fixture must parse: {error}");
            std::process::exit(101);
        }
    };
    assert_eq!(rows.len(), 2);
}

fn bench(c: &mut Criterion) {
    c.bench_function("rubygems_gemspec_scan_2_packages", |b| b.iter(scan));
}

/// The bench entry point (the criterion macros generate undocumented
/// items — the workspace's missing-docs gate forbids them; the sibling
/// benches call the group directly).
fn main() {
    bench(&mut Criterion::default().configure_from_args());
}

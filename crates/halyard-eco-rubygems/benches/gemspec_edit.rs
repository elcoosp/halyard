//! Bench: the `RubyGems` gemspec edit (§9.1's semantic patch at the
//! `.gemspec` grammar, M8 `RubyGems` cell, ADR-0061). Measures the
//! validate → splice → read-back-verify cycle over the version row of a
//! representative gemspec (comments, a heredoc description, quoted
//! values).

use criterion::Criterion;
use halyard_core::edit::EditOutcome;
use halyard_core::package::RelPath;
use halyard_core::plan::PlanPatch;
use halyard_eco_rubygems::edit::edit_versions;

const GEMSPEC: &str = r"# frozen_string_literal: true

Gem::Specification.new do |spec|
  spec.name = 'app'
  spec.version = '1.4.0' # the release row
  spec.summary = 'the application'
  spec.description = <<~DESC
    A long description spanning
    several heredoc lines.
  DESC
  spec.add_dependency 'json', '>= 2.0'
end
";

fn patch() -> (RelPath, Vec<PlanPatch>) {
    // The fixture is a constant that must carry the version row; the
    // bench-lint allowance does not reach bench targets, so the failure
    // exits loudly instead of panicking (the sibling dart bench posture).
    let fail = |what: &str| -> ! {
        eprintln!("the bench fixture must {what}");
        std::process::exit(101);
    };
    let file = RelPath::new("app.gemspec").unwrap_or_else(|| fail("carry a normalized path"));
    let start = GEMSPEC
        .find("1.4.0")
        .and_then(|idx| u32::try_from(idx).ok())
        .unwrap_or_else(|| fail("carry the version row"));
    let patches = vec![PlanPatch {
        start,
        end: start + 5,
        text: "1.5.0".to_owned(),
    }];
    (file, patches)
}

fn edit() {
    let (file, patches) = patch();
    // The fixture is a constant that must apply; a failure exits the
    // bench loudly (bench fns are not `#[test]`, so the test-lint
    // allowance does not reach them — the sibling nuget bench posture).
    let out: EditOutcome = match edit_versions(&file, GEMSPEC.as_bytes(), &patches) {
        Ok(out) => out,
        Err(error) => {
            eprintln!("the bench fixture must apply: {error}");
            std::process::exit(101);
        }
    };
    assert_eq!(out.applied, 1);
}

fn bench(c: &mut Criterion) {
    c.bench_function("rubygems_gemspec_edit_one_row", |b| b.iter(edit));
}

/// The bench entry point (the criterion macros generate undocumented
/// items — the workspace's missing-docs gate forbids them; the sibling
/// benches call the group directly).
fn main() {
    bench(&mut Criterion::default().configure_from_args());
}

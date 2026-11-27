//! Example: discover a `RubyGems` repository and inspect the rows (M8,
//! the §17 **`RubyGems`** cell — ADR-0061).
//!
//! Run: `cargo run -p halyard-eco-rubygems --example scan_gemspec`
//!
//! Every `.gemspec` in the committed tree discovers as a standalone
//! rubygems row (the gem is the package; the manifest is the gemspec
//! file), the identity is the `name` attribute, only semver-parseable
//! `Gem::Version` spellings pair with the plan contract (a two-component
//! `1.2` and a computed `Shared::VERSION` are warned and never
//! rewritten), a dependency **naming** another discovered gemspec is the
//! proven-internal edge, and every requirement (`~>`, `>=`) rides
//! verbatim (ADR-0006).

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
  spec.files = Dir['lib/**/*.rb']
end
";

const APP_GEMSPEC: &str = r"# frozen_string_literal: true

Gem::Specification.new do |spec|
  spec.name = 'app'
  spec.version = '1.4.0'
  spec.summary = 'the application'
  spec.add_dependency 'shared', '~> 2.3'
  spec.add_dependency 'json', '>= 2.0', '< 3.0'
  spec.add_development_dependency 'rake', '~> 13.0'
end
";

const LOOSE_GEMSPEC: &str = r"# frozen_string_literal: true

Gem::Specification.new do |spec|
  spec.name = 'loose'
  spec.version = '1.2' # two components — not the canonical three-number form
end
";

const COMPUTED_GEMSPEC: &str = r#"# frozen_string_literal: true

require_relative "lib/loose/version"

Gem::Specification.new do |spec|
  spec.name = 'computed'
  spec.version = Loose::VERSION
end
"#;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut ctx = DiscoverCtx::new();
    ctx.insert_file(
        "gems/shared/shared.gemspec",
        SHARED_GEMSPEC.as_bytes().to_vec(),
    );
    ctx.insert_file("gems/app/app.gemspec", APP_GEMSPEC.as_bytes().to_vec());
    ctx.insert_file(
        "gems/loose/loose.gemspec",
        LOOSE_GEMSPEC.as_bytes().to_vec(),
    );
    ctx.insert_file(
        "gems/computed/computed.gemspec",
        COMPUTED_GEMSPEC.as_bytes().to_vec(),
    );

    let rows = RubyGemsAdapter::new().discover(&ctx)?;
    println!("discovered {} rubygems rows:\n", rows.len());
    for row in &rows {
        println!(
            "  {} @ {} (manifest {})",
            row.name,
            if row.root.is_empty() {
                "<root>"
            } else {
                &row.root
            },
            row.manifest
        );
        println!(
            "    version: {:?} (source {:?})",
            row.version.as_ref().map(std::string::ToString::to_string),
            row.version_source
        );
        if let Some(site) = &row.version_site {
            println!("    edit site: {} spans {:?}", site.file, site.spans);
        }
        println!(
            "    publishable: {} (no gem build/push argv arm yet — ADR-0061)",
            row.publishable
        );
        for dep in &row.deps {
            println!(
                "    dependency {:?} {} internal={}",
                dep.name,
                String::from_utf8_lossy(&dep.raw),
                dep.internal
            );
        }
        for warning in &row.warnings {
            println!("    warning [{}]: {}", warning.code, warning.message);
        }
        println!();
    }
    Ok(())
}

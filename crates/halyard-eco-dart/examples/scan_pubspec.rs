//! Example: discover a Dart/pub workspace and inspect the rows (M8, the
//! §17 **Dart/pub** cell — ADR-0060).
//!
//! Run: `cargo run -p halyard-eco-dart --example scan_pubspec`
//!
//! The root's `workspace` entries discover as dart rows: the root itself
//! is a package (the documented `dart pub workspace list` prints `_ ./`),
//! every member carries `resolution: workspace`, a dependency **naming**
//! a member is the proven-internal edge (workspace packages resolve to
//! each other regardless of the declared source), a `path:` dependency
//! naming a member manifest is the second proof, and two-component
//! version spellings fail closed (warned, unpublishable, never
//! rewritten).

use halyard_core::EcosystemAdapter;
use halyard_core::discover::DiscoverCtx;
use halyard_eco_dart::DartAdapter;

const ROOT_PUBSPEC: &str = r"# the pub workspace root (the documented shape)
name: _
publish_to: none
environment:
  sdk: ^3.6.0
workspace:
  - packages/shared
  - packages/app
";

const SHARED_PUBSPEC: &str = r"name: shared
version: 2.3.0
resolution: workspace
dependencies:
  http: ^1.2.0
  meta: ^1.15.0
";

const APP_PUBSPEC: &str = r"name: app
version: 1.4.0 # the release row
resolution: workspace
dependencies:
  shared: ^2.3.0
  shared_ui:
    path: ../shared
  flutter_driver:
    sdk: flutter
dev_dependencies:
  test: ^1.25.0
dependency_overrides:
  meta: ^1.15.0
";

const LOOSE_PUBSPEC: &str = r"name: loose
version: 1.2 # two components — not the documented three-number grammar
resolution: workspace
";

const STRAY_PUBSPEC: &str = r"name: stray
version: 1.0.0
";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut ctx = DiscoverCtx::new();
    ctx.insert_file("pubspec.yaml", ROOT_PUBSPEC.as_bytes().to_vec());
    ctx.insert_file(
        "packages/shared/pubspec.yaml",
        SHARED_PUBSPEC.as_bytes().to_vec(),
    );
    ctx.insert_file("packages/app/pubspec.yaml", APP_PUBSPEC.as_bytes().to_vec());
    // A stray pubspec in the intermediate directory between the root and
    // the members rides along: with a workspace root present, that is
    // pub's own resolution error ("pub get will report an error and fail
    // to resolve"). The example shows the refusal honestly instead of
    // guessing.
    ctx.insert_file("packages/pubspec.yaml", STRAY_PUBSPEC.as_bytes().to_vec());

    let discovered = DartAdapter::new().discover(&ctx);
    match discovered {
        Ok(rows) => {
            println!("discovered {} dart rows:\n", rows.len());
            print_rows(&rows);
        }
        Err(error) => {
            println!("discovery refused the fixture workspace:\n  {error}\n");
            // The same fixture without the stray pubspec plans. The
            // `loose` member shows the two-component fail-closed posture:
            // pub's documented grammar is three numbers separated by
            // dots; the plan contract cannot render anything else.
            let mut ctx = DiscoverCtx::new();
            ctx.insert_file("pubspec.yaml", ROOT_PUBSPEC.as_bytes().to_vec());
            ctx.insert_file(
                "packages/shared/pubspec.yaml",
                SHARED_PUBSPEC.as_bytes().to_vec(),
            );
            ctx.insert_file("packages/app/pubspec.yaml", APP_PUBSPEC.as_bytes().to_vec());
            ctx.insert_file(
                "packages/loose/pubspec.yaml",
                LOOSE_PUBSPEC.as_bytes().to_vec(),
            );
            let rows = DartAdapter::new().discover(&ctx)?;
            println!("without the stray pubspec, {} dart rows:\n", rows.len());
            print_rows(&rows);
        }
    }
    Ok(())
}

fn print_rows(rows: &[halyard_core::discover::RawPackage]) {
    for row in rows {
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
            "    publishable: {} (no pub.dev publish auth grammar yet — ADR-0060)",
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
}

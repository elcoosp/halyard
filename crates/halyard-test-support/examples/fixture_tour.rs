//! Fixture tour: builds a representative set of scenarios and prints their
//! shape. Run with:
//!
//! ```text
//! cargo run -p halyard-test-support --example fixture_tour
//! ```

use halyard_test_support::{FixtureError, GitFixture};

/// `(name, builder)` entry of the tour.
type TourEntry = (&'static str, fn() -> Result<GitFixture, FixtureError>);

/// Print one fixture's shape.
fn show(name: &str, fx: &GitFixture) -> Result<(), FixtureError> {
    let head = fx.head_oid()?.unwrap_or_else(|| "(unborn)".to_owned());
    let commits = fx.rev_list(&["HEAD"])?.len();
    let tags = fx.ls_tags()?.join(", ");
    println!("{name}:");
    println!("  head:    {head}");
    println!("  commits: {commits}");
    println!(
        "  tags:    {}",
        if tags.is_empty() {
            "(none)".to_owned()
        } else {
            tags
        }
    );
    Ok(())
}

fn main() -> Result<(), FixtureError> {
    let tour: Vec<TourEntry> = vec![
        (
            "linear_single_package",
            halyard_test_support::scenarios::linear_single_package,
        ),
        (
            "merge_two_branches",
            halyard_test_support::scenarios::merge_two_branches,
        ),
        (
            "multi_package_tag_set",
            halyard_test_support::scenarios::multi_package_tag_set,
        ),
        (
            "annotated_tags",
            halyard_test_support::scenarios::annotated_tags,
        ),
        (
            "unicode_subjects",
            halyard_test_support::scenarios::unicode_subjects,
        ),
    ];
    for (name, build) in tour {
        let fixture = build()?;
        show(name, &fixture)?;
    }
    Ok(())
}

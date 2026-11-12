//! Version algebra demo (M1.3): the appendix A.2 rewriter rows, the
//! Cargo-compatible 0.x policy and a prerelease channel advancing from
//! existing tags.
//!
//! Run: `cargo run -p halyard-version --example bump_algebra`

use std::error::Error;

use halyard_core::Bump;
use halyard_version::{
    CommitBumpMapping, PreMajorPolicy, PrereleaseChannel, RewriteOutcome, UpperBoundPolicy,
    advance_prerelease, apply_bump, rewrite_requirement,
};
use semver::Version;

fn v(text: &str) -> Version {
    Version::parse(text).unwrap_or_else(|_| Version::new(0, 0, 0))
}

fn main() -> Result<(), Box<dyn Error>> {
    let policy = PreMajorPolicy::CARGO_COMPATIBLE;

    println!("== appendix A.2 requirement rewriter ==");
    let rows: &[(&str, &str)] = &[
        ("0.4", "0.5.0"),
        ("^0.4.1", "0.5.0"),
        ("=0.4.1", "0.4.2"),
        ("~1.2.3", "1.3.0"),
        (">=1.0, <2", "1.4.0"),
        (">=1.0, <2", "2.0.0"),
        ("workspace:^", "9.9.9"),
    ];
    for (req, target) in rows {
        let outcome = rewrite_requirement(req, &v(target), UpperBoundPolicy::Widen);
        let text = match outcome {
            Ok(RewriteOutcome::Unchanged { because }) => format!("unchanged ({because:?})"),
            Ok(RewriteOutcome::Rewritten(text)) => format!("rewritten to {text:?}"),
            Err(err) => format!("error {} ({err})", err.code()),
        };
        println!("  {req:>12} + {target:<6} -> {text}");
    }

    println!("\n== 0.x policy (Cargo-compatible: breaking -> minor, feat -> patch) ==");
    for (from, bump) in [
        ("0.4.1", Bump::Major),
        ("0.4.1", Bump::Minor),
        ("1.2.3", Bump::Major),
    ] {
        println!(
            "  {from} + {bump:?} -> {}",
            apply_bump(&v(from), bump, &policy)
        );
    }

    println!("\n== commit mapping ==");
    let mapping = CommitBumpMapping::new();
    for (kind, breaking) in [
        ("feat", false),
        ("fix", false),
        ("docs", false),
        ("chore", true),
    ] {
        println!(
            "  {kind} (breaking={breaking}) -> {:?}",
            mapping.bump_for(kind, breaking)
        );
    }

    println!("\n== prerelease channel (counter from existing tags) ==");
    let channel = PrereleaseChannel::new("rc").ok_or("channel 'rc' must validate")?;
    let existing = [v("1.2.0-rc.1"), v("1.2.0-rc.2")];
    println!(
        "  base 1.2.0 with tags [1.2.0-rc.1, 1.2.0-rc.2] -> {}",
        advance_prerelease(&v("1.2.0"), &channel, &existing)
    );
    println!(
        "  base 1.3.0 with the same tags -> {}",
        advance_prerelease(&v("1.3.0"), &channel, &existing)
    );
    Ok(())
}

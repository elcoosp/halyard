//! Conventional-commit parsing demo (M1.2): parse a handful of real-shaped
//! commit messages and print the structured result, the reason-relevant
//! flags (breaking, fixup) and the extracted PR number.
//!
//! Run: `cargo run -p halyard-conventional --example parse_commit`

use bstr::ByteSlice as _;
use halyard_conventional::{extract_pr_number, is_fixup_or_squash, parse};

const MESSAGES: &[&[u8]] = &[
    b"feat: allow provided config to extend other configs",
    b"feat(api)!: send an email to the customer when a product is shipped",
    b"fix(parser): handle refs (#412)",
    b"chore: drop support for Node 6\n\nBREAKING CHANGE: drop support for Node 6.",
    b"fix: correct minor typo\n\nHappy path now is correct\n\nReviewed-by: Z\nRefs: #133",
    b"fixup! feat: something earlier",
    b"not a conventional commit at all",
];

fn main() {
    for message in MESSAGES {
        println!("subject:   {:?}", String::from_utf8_lossy(message));
        if is_fixup_or_squash(message) {
            println!("  -> autosquash commit (fixup!/squash!), ignored by the planner");
            continue;
        }
        match parse(message) {
            Some(cc) => {
                println!("  kind:        {}", cc.kind.to_str_lossy());
                println!(
                    "  scope:       {}",
                    cc.scope
                        .as_ref()
                        .map_or_else(|| "(none)".into(), |s| s.to_str_lossy())
                );
                println!("  breaking:    {}", cc.breaking);
                println!("  description: {}", cc.description.to_str_lossy());
                if let Some(body) = &cc.body {
                    println!("  body:        {} bytes", body.len());
                }
                for footer in &cc.footers {
                    println!(
                        "  footer:      {} = {}",
                        footer.key.to_str_lossy(),
                        footer.value.to_str_lossy()
                    );
                }
                println!(
                    "  pr:          {:?} (standalone scan: {:?})",
                    cc.pr,
                    extract_pr_number(message)
                );
            }
            None => println!("  -> not conventional"),
        }
        println!();
    }
}

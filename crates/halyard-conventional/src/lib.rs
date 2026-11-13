//! HALYARD conventional-commit parser (M1.2, §8.5).
//!
//! Spec: Conventional Commits 1.0.0
//! (<https://www.conventionalcommits.org/en/v1.0.0/>). The subject-line
//! grammar (type, scope, `!`, `:`, description) is expressed with `winnow`
//! combinators; the escape-aware scope scan and the paragraph/trailer
//! splitting are hand-written byte scans (the trailer grammar follows git
//! trailers: `token: value` / `token #value`, continuation lines start
//! with whitespace, and `BREAKING CHANGE` is the one token that may
//! contain a space, with `BREAKING-CHANGE` as its alias).
//!
//! Parsing is *total*: `parse` returns `None` for anything that is not a
//! conventional commit and never panics (proptest-enforced). Emitted
//! fields are owned `BString`s so parsed commits outlive the input
//! buffer, and everything is byte-exact (no Unicode normalization).

#![forbid(unsafe_code)]

mod parser;

pub use parser::{
    ConventionalCommit, Trailer, extract_pr_number, is_fixup_or_squash, parse, revert_target,
    trailing_pr_number,
};

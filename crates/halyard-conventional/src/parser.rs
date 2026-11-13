//! The parser itself (see crate docs for the grammar and conventions).
//!
//! Zero-copy strategy: the subject line and scope are scanned as borrowed
//! slices; only the emitted fields are copied into owned `BString`s.

use bstr::BString;
use winnow::Parser;
use winnow::combinator::opt;
use winnow::error::ContextError;
use winnow::token::{take_till, take_while};

/// One parsed conventional commit (Conventional Commits 1.0.0).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConventionalCommit {
    /// Commit type (`feat`, `fix`, …) exactly as written.
    pub kind: BString,
    /// Scope exactly as written (`None` when absent; backslash escapes are
    /// preserved byte-for-byte).
    pub scope: Option<BString>,
    /// Breaking via a `!` before the colon **or** a `BREAKING CHANGE` /
    /// `BREAKING-CHANGE` footer.
    pub breaking: bool,
    /// Description after the colon (trailing spaces stripped; never empty
    /// for accepted commits).
    pub description: BString,
    /// Body paragraphs between the subject and the footer section
    /// (LF-normalized; `None` when the message has no body).
    pub body: Option<BString>,
    /// Footers/trailers in message order (continuation lines folded into
    /// their value with `\n`).
    pub footers: Vec<Trailer>,
    /// PR number from a trailing `(#NNN)` in the subject (§8.5).
    pub pr: Option<u64>,
}

impl ConventionalCommit {
    /// True when any footer is a `BREAKING CHANGE` / `BREAKING-CHANGE`
    /// footer (case-insensitive, like git trailer tooling).
    pub fn has_breaking_footer(&self) -> bool {
        self.footers.iter().any(is_breaking_footer)
    }
}

/// A `Key: value` commit-message footer (git trailer).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Trailer {
    /// Trailer key exactly as written (`BREAKING CHANGE` keeps its space).
    pub key: BString,
    /// Trailer value; multi-line values are joined with `\n` (continuation
    /// lines lose their leading whitespace, like git trailers).
    pub value: BString,
}

/// Kind characters after the first letter: letters, digits, `-`, `_`.
fn is_kind_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'-' || c == b'_'
}

/// Scope bytes that make the subject invalid when unescaped.
fn is_forbidden_scope_byte(c: u8) -> bool {
    c.is_ascii_whitespace() || c.is_ascii_control() || c == b'(' || c == b')'
}

/// Parses one commit message; `None` when the message is not a
/// conventional commit (including `fixup!`/`squash!` autosquash commits,
/// §8.5).
pub fn parse(input: &[u8]) -> Option<ConventionalCommit> {
    let line_end = input
        .iter()
        .position(|&b| b == b'\n')
        .unwrap_or(input.len());
    let subject = strip_trailing_cr(input.get(..line_end).unwrap_or(&[]));
    if is_fixup_or_squash(subject) {
        return None;
    }
    let (kind, scope, bang, description) = parse_subject(subject)?;
    // §8.5: the PR number is a byte scan from the end of the *subject
    // line* (not the truncated description — a stray `\r` inside the line
    // must not change the outcome).
    let pr = extract_pr_number(subject);
    let (body, footers) = split_body_and_footers(
        input
            .get(line_end.saturating_add(1).min(input.len())..)
            .unwrap_or(&[]),
    );
    let breaking = bang || footers.iter().any(is_breaking_footer);
    Some(ConventionalCommit {
        kind,
        scope,
        breaking,
        description,
        body,
        footers,
        pr,
    })
}

/// True when the subject is an autosquash commit (`fixup!`/`squash!`
/// prefix, §8.5: such commits are ignored by the planner).
pub fn is_fixup_or_squash(subject: &[u8]) -> bool {
    subject.starts_with(b"fixup!") || subject.starts_with(b"squash!")
}

/// The `git revert` trailer key (§8.5 revert cancellation).
const REVERT_KEY: &[u8] = b"This reverts commit";

/// Extracts the reverted commit from a `git revert` message (§8.5): the
/// 40-char (or 64-char, SHA-256) hex OID after `This reverts commit` in
/// the body. Scans the *whole message* byte-wise (revert trailers sit at
/// the bottom; nothing is allocated unless a candidate is found). Returns
/// the lowercase hex bytes; `None` when the message does not carry the
/// trailer.
///
/// The accepted trailer shape is git's own output:
/// `This reverts commit <40-hex>.` — case-sensitive key, optional trailing
/// period, the OID on the same line. A `revert:`-prefixed *subject* is
/// deliberately not accepted here (it carries no OID to cancel).
pub fn revert_target(message: &[u8]) -> Option<BString> {
    // Line-wise scan (total, allocation-free): the trailer is git's own
    // body rendering, so the accepted line is
    // `This reverts commit <40-hex>` (optionally trailing `.`).
    let mut start = 0usize;
    for i in 0..=message.len() {
        if i == message.len() || message.get(i) == Some(&b'\n') {
            let line = message.get(start..i).unwrap_or(&[]);
            if let Some(found) = line_revert_target(line) {
                return Some(found);
            }
            start = i + 1;
        }
    }
    None
}

/// Matches one line against `This reverts commit <hex>` (optionally
/// followed by a period or trailing whitespace).
fn line_revert_target(line: &[u8]) -> Option<BString> {
    let line = strip_trailing_cr(line);
    // Find the key anywhere in the line (leading whitespace tolerated).
    let pos = line.windows(REVERT_KEY.len()).position(|w| w == REVERT_KEY)?;
    let rest = line.get(pos + REVERT_KEY.len()..)?;
    let rest = rest.strip_prefix(b" ")?;
    let end = rest
        .iter()
        .position(|&b| b.is_ascii_whitespace() || b == b'.')
        .unwrap_or(rest.len());
    let hex = rest.get(..end)?;
    if hex.len() != 40 && hex.len() != 64 {
        return None;
    }
    if !hex.iter().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    Some(BString::from(hex.to_ascii_lowercase()))
}

/// Extracts a trailing `(#NNN)` PR number from a subject line (§8.5,
/// byte scan from the end, regex-free). Alias of
/// [`extract_pr_number`] with the protocol-facing name.
pub fn trailing_pr_number(subject: &[u8]) -> Option<u64> {
    extract_pr_number(subject)
}

/// Extracts a trailing `(#NNN)` PR number from a subject line (byte scan
/// from the end, §8.5 — regex-free).
pub fn extract_pr_number(subject: &[u8]) -> Option<u64> {
    let trimmed_end = subject.iter().rposition(|b| !b.is_ascii_whitespace());
    let end = trimmed_end.map_or(subject.len(), |pos| pos + 1);
    let trimmed = subject.get(..end).unwrap_or(&[]);
    if trimmed.last() != Some(&b')') {
        return None;
    }
    let mut cursor = end.checked_sub(1)?;
    // digits run (at least one)
    let digits_end = cursor;
    while cursor > 0 {
        match trimmed.get(cursor - 1) {
            Some(b) if b.is_ascii_digit() => cursor -= 1,
            _ => break,
        }
    }
    if cursor == digits_end {
        return None; // no digits between `#` and `)`
    }
    // `#`
    cursor = cursor.checked_sub(1)?;
    if trimmed.get(cursor) != Some(&b'#') {
        return None;
    }
    // `(`
    cursor = cursor.checked_sub(1)?;
    if trimmed.get(cursor) != Some(&b'(') {
        return None;
    }
    let digits = trimmed.get(cursor + 2..digits_end).unwrap_or(&[]);
    std::str::from_utf8(digits)
        .ok()
        .and_then(|d| d.parse::<u64>().ok())
}

/// Subject grammar: `<type>[scope][!]: <description>`.
fn parse_subject(subject: &[u8]) -> Option<(BString, Option<BString>, bool, BString)> {
    let mut rest: &[u8] = subject;

    // type: one letter, then kind characters — both runs are adjacent and
    // borrowed from the subject, so the kind is the union of their lengths.
    let (head, tail) = (
        take_while::<_, &[u8], ContextError>(1.., |c: u8| c.is_ascii_alphabetic()),
        take_while::<_, &[u8], ContextError>(0.., is_kind_char),
    )
        .parse_next(&mut rest)
        .ok()?;
    let kind_len = head.len() + tail.len();
    let kind = subject.get(..kind_len)?;

    // scope: `(…)` with backslash-escaped bytes preserved (spec FAQ:
    // parentheses may appear in a scope when escaped).
    let scope = opt(scope_group).parse_next(&mut rest).ok()?;

    // `!` marks breaking even without a footer.
    let bang = opt::<&[u8], u8, ContextError, _>(b'!')
        .parse_next(&mut rest)
        .ok()?
        .is_some();

    // Required terminal colon and space.
    if rest.starts_with(b": ") {
        rest = rest.get(2..).unwrap_or(&[]);
    } else {
        return None;
    }

    let description = take_till::<_, &[u8], ContextError>(0.., |c: u8| c == b'\n' || c == b'\r')
        .parse_next(&mut rest)
        .ok()?;
    let description = trim_trailing_spaces(description);
    if description.is_empty() {
        return None; // "a description MUST immediately follow the colon and space"
    }
    Some((
        BString::from(kind),
        scope.map(BString::from),
        bang,
        BString::from(description),
    ))
}

/// `(scope)` group parser: returns the raw inner bytes.
fn scope_group<'i>(input: &mut &'i [u8]) -> Result<&'i [u8], ContextError> {
    if input.first() != Some(&b'(') {
        return Err(ContextError::new());
    }
    let mut i = 1usize;
    while let Some(&b) = input.get(i) {
        match b {
            b'\\' => {
                if input.get(i + 1).is_none() {
                    return Err(ContextError::new());
                }
                i += 2;
            }
            b')' => {
                if i == 1 {
                    return Err(ContextError::new()); // empty scope
                }
                let inner = input.get(1..i).unwrap_or(&[]);
                *input = input.get(i + 1..).unwrap_or(&[]);
                return Ok(inner);
            }
            c if is_forbidden_scope_byte(c) => return Err(ContextError::new()),
            _ => i += 1,
        }
    }
    Err(ContextError::new())
}

/// Splits the post-subject remainder into body and footers: the footer
/// section is the last non-empty paragraph, separated by a blank line,
/// where every line parses as a trailer (continuation lines folded).
fn split_body_and_footers(rest: &[u8]) -> (Option<BString>, Vec<Trailer>) {
    let lines: Vec<&[u8]> = rest.split(|&b| b == b'\n').map(strip_trailing_cr).collect();
    // Ignore trailing blank lines (messages commonly end with `\n`).
    let mut end = lines.len();
    while end > 0 && lines.get(end - 1).is_some_and(|l| l.is_empty()) {
        end -= 1;
    }
    // Locate the last paragraph: the run of non-blank lines ending at `end`.
    let mut start = end;
    while start > 0 && !lines.get(start - 1).is_some_and(|l| l.is_empty()) {
        start -= 1;
    }
    if start == end {
        return (None, Vec::new());
    }
    let paragraph: Vec<&[u8]> = lines.get(start..end).unwrap_or(&[]).to_vec();

    // The paragraph is a footer section only when a blank line separates it
    // from the body region and every line is a trailer.
    let separated = start > 0;
    if let Some(footers) = separated.then(|| parse_trailers(&paragraph)).flatten() {
        let body = join_lines(lines.get(..start - 1).unwrap_or(&[]));
        return (body, footers);
    }
    (join_lines(lines.get(..end).unwrap_or(&[])), Vec::new())
}

/// Parses a paragraph into trailers, or `None` when any line does not fit
/// the trailer grammar.
fn parse_trailers(lines: &[&[u8]]) -> Option<Vec<Trailer>> {
    let mut out: Vec<Trailer> = Vec::new();
    for line in lines {
        if line.is_empty() {
            return None; // paragraphs never contain blank lines here
        }
        if line.first().is_some_and(|&b| b == b' ' || b == b'\t') {
            // Continuation line: folds into the previous trailer's value.
            let previous = out.last_mut()?;
            let continuation = trim_leading_spaces(line);
            previous.value.push(b'\n');
            previous.value.extend_from_slice(continuation);
            continue;
        }
        let trailer = parse_trailer_line(line)?;
        out.push(trailer);
    }
    Some(out)
}

/// Parses one `token: value` / `token #value` line.
fn parse_trailer_line(line: &[u8]) -> Option<Trailer> {
    // Find the earliest separator: `: ` or ` #`.
    let mut sep = None;
    let mut i = 0usize;
    while i < line.len() {
        let two = line.get(i..i + 2).unwrap_or(&[]);
        if two == b": " || two == b" #" {
            sep = Some(i);
            break;
        }
        i += 1;
    }
    let sep = sep?;
    let key = line.get(..sep).unwrap_or(&[]);
    if !is_valid_trailer_token(key) {
        return None;
    }
    let value = line.get(sep + 2..).unwrap_or(&[]);
    Some(Trailer {
        key: BString::from(key),
        value: BString::from(value),
    })
}

/// Trailer tokens: letters/digits/`-` (git trailer rules), plus the one
/// spec exception `BREAKING CHANGE` which contains a space.
fn is_valid_trailer_token(key: &[u8]) -> bool {
    if key.is_empty() {
        return false;
    }
    is_breaking_change_key(key) || key.iter().all(|&b| b.is_ascii_alphanumeric() || b == b'-')
}

/// `BREAKING CHANGE` / `BREAKING-CHANGE`, case-insensitive like git
/// trailer tooling.
fn is_breaking_change_key(key: &[u8]) -> bool {
    key.eq_ignore_ascii_case(b"BREAKING CHANGE") || key.eq_ignore_ascii_case(b"BREAKING-CHANGE")
}

fn is_breaking_footer(trailer: &Trailer) -> bool {
    is_breaking_change_key(&trailer.key)
}

/// Joins borrowed lines back into one owned buffer with `\n`, dropping
/// blank lines at the start (the line right after the subject). Returns
/// `None` when nothing non-blank remains.
fn join_lines(lines: &[&[u8]]) -> Option<BString> {
    let content: Vec<&[u8]> = lines.iter().copied().skip_while(|l| l.is_empty()).collect();
    if content.is_empty() {
        return None;
    }
    let total: usize = content.iter().map(|l| l.len() + 1).sum();
    let mut out = Vec::with_capacity(total.saturating_sub(1));
    let mut first = true;
    for line in &content {
        if !first {
            out.push(b'\n');
        }
        out.extend_from_slice(line);
        first = false;
    }
    Some(BString::from(out))
}

fn strip_trailing_cr(line: &[u8]) -> &[u8] {
    match line.strip_suffix(b"\r") {
        Some(stripped) => stripped,
        None => line,
    }
}

fn trim_trailing_spaces(line: &[u8]) -> &[u8] {
    let end = line.iter().rposition(|&b| b != b' ' && b != b'\t');
    let end = end.map_or(0, |pos| pos + 1);
    line.get(..end).unwrap_or(&[])
}

fn trim_leading_spaces(line: &[u8]) -> &[u8] {
    let start = line.iter().position(|&b| b != b' ' && b != b'\t');
    let start = start.unwrap_or(line.len());
    line.get(start..).unwrap_or(&[])
}

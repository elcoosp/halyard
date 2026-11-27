//! The `.gemspec` subset scanner (M8 ADR-0061), hand-rolled over the
//! VERIFIED reference (`docs/research/rubygems-grammars.md`, from
//! `guides.rubygems.org/specification-reference/` — the
//! `Gem::Specification.new do |s|` block shape and the required
//! attribute list — `guides.rubygems.org/name-your-gem/`,
//! `rubygems/rubygems`'s `lib/rubygems/version.rb` `Gem::Version`
//! pattern, and the rubygems publishing guide).
//!
//! Handled grammar (everything else is a refusal or a warning, never a
//! guess):
//!
//! ```ruby
//! # frozen_string_literal: true
//! Gem::Specification.new do |spec|
//!   spec.name = 'shared'          # or "shared" (double quotes)
//!   spec.version = '2.3.0'        # the release row; `.freeze` tolerated
//!   spec.summary = 'a summary'
//!   spec.description = <<~DESC    # heredoc bodies consumed unmodeled
//!     a long description
//!   DESC
//!   spec.add_dependency 'json', '>= 2.0', '< 3.0'
//!   spec.add_development_dependency('rake', '~> 13.0')
//! end
//! ```
//!
//! The specification reference's *"Specifications must be deterministic"*
//! is the grammar's fence: only **literal string assignments** to the
//! block variable's `name`/`version` attributes capture (an
//! interpolated, computed, or conditionally defined value can never
//! carry an honest byte span). `do |var|` and `{ |var| }` block forms
//! are recognized; `#` comments (outside quotes), blank lines, CRLF and
//! one-line `;` statements are handled; nesting is tracked quote-aware
//! (`do`/`{` open anywhere, statement-position
//! `if`/`unless`/`while`/`until`/`case`/`begin`/`def`/`class`/`module`/
//! `for` open, `end`/`}` close), so conditional rows are recognized and
//! consumed unmodeled with a warning. Total on arbitrary input: errors
//! carry a line number instead of panicking.

use halyard_core::package::Span;

/// One parsed `.gemspec` file (the planning-relevant subset).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Gemspec {
    /// The `name` attribute — the specification reference lists `name`
    /// among the *"Required gemspec attributes"*. `None` when the row is
    /// absent (the adapter refuses; the identity has no documented
    /// default).
    pub name: Option<ValueField>,
    /// The `version` attribute — *"This gem's version"* (*"The version
    /// string can contain numbers and periods, such as 1.0.0."*). `None`
    /// when the row is absent or not a literal string.
    pub version: Option<ValueField>,
    /// A `version` row exists (literal or not) — the adapter's
    /// `no-version` warning fires only when this is `false`.
    pub version_row_present: bool,
    /// `add_dependency` / `add_runtime_dependency` rows in document
    /// order.
    pub deps: Vec<GemDep>,
    /// `add_development_dependency` rows in document order (*"Development
    /// dependencies aren't installed by default"*).
    pub dev_deps: Vec<GemDep>,
    /// Non-fatal observations, in document order (1-based line numbers).
    pub warnings: Vec<ScanWarning>,
}

/// One captured string literal with its byte span in the original text
/// (the inner content — quotes excluded, comments excluded).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValueField {
    /// The literal content, as written (inner bytes verbatim).
    pub text: String,
    /// Byte span of the inner content in the original file (the edit
    /// site the plan contract carries).
    pub span: Span,
}

/// One dependency row (from `add_dependency`, `add_runtime_dependency`
/// or `add_development_dependency`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GemDep {
    /// The dependency name — the first literal argument.
    pub name: String,
    /// The requirement literals — the remaining string arguments
    /// (`>= 1.1.4`, `~> 13.0`), in call order. A `RubyGems` requirement is
    /// not semver: the rows ride verbatim (ADR-0006).
    pub requirements: Vec<ValueField>,
}

/// One non-fatal scan observation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScanWarning {
    /// 1-based line the observation was made on.
    pub line: usize,
    /// Stable warning code (`non-canonical-version`,
    /// `conditional-attribute`, `unmodeled-dep`).
    pub code: &'static str,
    /// What was observed (includes the line number).
    pub message: String,
}

/// Parse error with the offending line number (1-based).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GemspecError {
    /// 1-based line the error was detected on.
    pub line: usize,
    /// What went wrong.
    pub message: String,
}

impl std::fmt::Display for GemspecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "invalid .gemspec at line {}: {}",
            self.line, self.message
        )
    }
}

impl std::error::Error for GemspecError {}

/// One lexed token with its absolute byte span in the original text.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Tok {
    /// An identifier / keyword word (`spec`, `version`, `do`, `end`).
    Word(String),
    /// A single- or double-quoted string literal's inner content.
    StrLit {
        /// The literal text (inner bytes).
        text: String,
        /// Absolute span of the inner bytes.
        span: Span,
        /// `true` when the double-quoted literal carried `#{...}`
        /// interpolation (never a capture candidate).
        interpolated: bool,
    },
    /// A heredoc opener (`<<~DESC`); the body ends at the terminator.
    Heredoc {
        /// The name the heredoc body ends with.
        terminator: String,
    },
    /// Any other character (`{ } ( ) [ ] , = | . : < > ...`).
    Punct(u8),
}

/// The statement-position block openers (a `do` or `{` opens anywhere;
/// these keywords open only when they start a statement — a modifier
/// `if`/`unless`/`while`/`until` never opens a region).
const STATEMENT_OPENERS: [&str; 10] = [
    "if", "unless", "while", "until", "case", "begin", "def", "class", "module", "for",
];

/// The byte offset (within `raw`) where a `#` comment starts — outside
/// quotes. `None` when the line carries no comment. (Inside a string
/// literal `#` is text; the lexer tracks both quote styles.)
fn comment_start(raw: &str) -> Option<usize> {
    let bytes = raw.as_bytes();
    let mut quote: Option<u8> = None;
    let mut idx = 0usize;
    while let Some(&byte) = bytes.get(idx) {
        match quote {
            Some(q) => {
                if byte == b'\\' {
                    idx += 2;
                    continue;
                }
                if byte == q {
                    quote = None;
                }
            }
            None => {
                if byte == b'\'' || byte == b'"' {
                    quote = Some(byte);
                } else if byte == b'#' {
                    return Some(idx);
                }
            }
        }
        idx += 1;
    }
    None
}

/// Splits one raw line into `;`-separated statements (quote-aware — a
/// `;` inside a string literal never splits), each with its absolute
/// byte offset.
fn split_statements(line_offset: usize, content: &str) -> Vec<(usize, &str)> {
    let bytes = content.as_bytes();
    let mut out = Vec::new();
    let mut quote: Option<u8> = None;
    let mut start = 0usize;
    let mut idx = 0usize;
    while let Some(&byte) = bytes.get(idx) {
        match quote {
            Some(q) => {
                if byte == b'\\' {
                    idx += 2;
                    continue;
                }
                if byte == q {
                    quote = None;
                }
            }
            None => {
                if byte == b'\'' || byte == b'"' {
                    quote = Some(byte);
                } else if byte == b';' {
                    out.push((line_offset + start, &content[start..idx]));
                    start = idx + 1;
                }
            }
        }
        idx += 1;
    }
    out.push((line_offset + start, &content[start..]));
    out
}

/// Lexes one statement into tokens with absolute byte spans. `base` is
/// the statement's first byte in the original text; `line` is the
/// 1-based line number errors carry. The input is comment-stripped and
/// trimmed.
fn lex(base: usize, stmt: &str, line: usize) -> Result<Vec<Tok>, GemspecError> {
    let bytes = stmt.as_bytes();
    let mut toks = Vec::new();
    let mut i = 0usize;
    while let Some(&byte) = bytes.get(i) {
        match byte {
            b' ' | b'\t' | b'\r' => {
                i += 1;
            }
            b'\'' => {
                let (tok, next) = lex_single(base, stmt, i, line)?;
                toks.push(tok);
                i = next;
            }
            b'"' => {
                let (tok, next) = lex_double(base, stmt, i, line)?;
                toks.push(tok);
                i = next;
            }
            b'<' => {
                // `<<` heredoc openers: `<<~X`, `<<-X`, `<<X`, `<<'X'`,
                // `<<"X"`. A bitshift never sits flush against a word
                // in the handled rows; anything else lexes as puncts.
                if let Some((terminator, next)) = heredoc_terminator(stmt, i) {
                    toks.push(Tok::Heredoc { terminator });
                    i = next;
                } else {
                    toks.push(Tok::Punct(b'<'));
                    i += 1;
                }
            }
            _ => {
                if is_word_byte(byte) {
                    let start = i;
                    while bytes.get(i).copied().is_some_and(is_word_byte) {
                        i += 1;
                    }
                    toks.push(Tok::Word(stmt[start..i].to_owned()));
                } else {
                    toks.push(Tok::Punct(byte));
                    i += 1;
                }
            }
        }
    }
    Ok(toks)
}

/// Lexes one single-quoted literal starting at `at` (the quote byte).
/// Returns the token and the index past the closing quote.
fn lex_single(
    base: usize,
    stmt: &str,
    at: usize,
    line: usize,
) -> Result<(Tok, usize), GemspecError> {
    let bytes = stmt.as_bytes();
    let mut i = at + 1;
    while let Some(&byte) = bytes.get(i) {
        match byte {
            b'\\' => {
                i += 2;
            }
            b'\'' => {
                return Ok((
                    Tok::StrLit {
                        text: stmt[at + 1..i].to_owned(),
                        span: span_of(base + at + 1, base + i),
                        interpolated: false,
                    },
                    i + 1,
                ));
            }
            _ => {
                i += 1;
            }
        }
    }
    Err(GemspecError {
        line,
        message: "an unterminated single-quoted string — the file is not valid Ruby".to_owned(),
    })
}

/// Lexes one double-quoted literal starting at `at` (the quote byte),
/// flagging `#{...}` interpolation. Returns the token and the index
/// past the closing quote.
fn lex_double(
    base: usize,
    stmt: &str,
    at: usize,
    line: usize,
) -> Result<(Tok, usize), GemspecError> {
    let bytes = stmt.as_bytes();
    let mut i = at + 1;
    let mut interpolated = false;
    while let Some(&byte) = bytes.get(i) {
        match byte {
            b'\\' => {
                i += 2;
            }
            b'#' if bytes.get(i + 1) == Some(&b'{') => {
                interpolated = true;
                i += 2;
            }
            b'"' => {
                return Ok((
                    Tok::StrLit {
                        text: stmt[at + 1..i].to_owned(),
                        span: span_of(base + at + 1, base + i),
                        interpolated,
                    },
                    i + 1,
                ));
            }
            _ => {
                i += 1;
            }
        }
    }
    Err(GemspecError {
        line,
        message: "an unterminated double-quoted string — the file is not valid Ruby".to_owned(),
    })
}

/// Recognizes the heredoc opener at `at` (which points at the first
/// `<`). Returns the terminator and the index past the opener.
fn heredoc_terminator(stmt: &str, at: usize) -> Option<(String, usize)> {
    let bytes = stmt.as_bytes();
    if bytes.get(at) != Some(&b'<') || bytes.get(at + 1) != Some(&b'<') {
        return None;
    }
    let mut i = at + 2;
    // `~` / `-` squiggly/dash prefixes.
    if matches!(bytes.get(i), Some(b'~' | b'-')) {
        i += 1;
    }
    if let Some(quote) = bytes.get(i).copied().filter(|b| *b == b'\'' || *b == b'"') {
        let start = i + 1;
        let mut end = start;
        while bytes.get(end).copied().is_some_and(|b| b != quote) {
            end += 1;
        }
        bytes.get(end)?;
        let name = stmt.get(start..end).filter(|s| !s.is_empty())?;
        return Some((name.to_owned(), end + 1));
    }
    let start = i;
    while bytes
        .get(i)
        .copied()
        .is_some_and(|b| b.is_ascii_alphanumeric() || b == b'_')
    {
        i += 1;
    }
    if i == start {
        // `<<` with no identifier: a bitshift or an operator — not a
        // heredoc.
        return None;
    }
    let name = stmt.get(start..i)?;
    if name.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        // A digit-leading name is a shift amount (`x<<2`), never a
        // heredoc (identifiers do not start with digits).
        return None;
    }
    Some((name.to_owned(), i))
}

fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'?' || byte == b'!'
}

fn span_of(start: usize, end: usize) -> Span {
    Span {
        start: u32::try_from(start).unwrap_or(u32::MAX),
        end: u32::try_from(end).unwrap_or(u32::MAX),
    }
}

/// The scanner state: which container the walk sits in.
struct State {
    /// The `Gem::Specification.new` block's variable (`None` = preamble,
    /// before the block opens).
    block_var: Option<String>,
    /// Nested-region depth inside the current container (the
    /// `Gem::Specification` block or the preamble).
    depth: usize,
    /// The pending heredoc terminator (the body is consumed unmodeled).
    heredoc: Option<String>,
    /// The `Gem::Specification.new` block has closed — a second block is
    /// the documented refusal.
    closed: bool,
}

/// Parses one `.gemspec` file's text.
pub fn parse_gemspec(text: &str) -> Result<Gemspec, GemspecError> {
    let mut out = Gemspec::default();
    let mut state = State {
        block_var: None,
        depth: 0,
        heredoc: None,
        closed: false,
    };

    let mut offset = 0usize;
    for (idx, raw_line) in text.split('\n').enumerate() {
        let line_no = idx + 1;
        let line_offset = offset;
        offset += raw_line.len() + 1;

        // The heredoc body is consumed unmodeled; only the terminator
        // line resumes the walk.
        if let Some(terminator) = state.heredoc.clone() {
            if raw_line.trim() == terminator {
                state.heredoc = None;
            }
            continue;
        }

        // CRLF: the trailing `\r` is whitespace to the lexer; a `#`
        // outside quotes starts a comment.
        let content = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        let content = match comment_start(content) {
            Some(idx) => &content[..idx],
            None => content,
        };
        for (stmt_offset, stmt) in split_statements(line_offset, content) {
            let trimmed = stmt.trim_start();
            if trimmed.is_empty() {
                continue;
            }
            let base = stmt_offset + (stmt.len() - trimmed.len());
            let toks = lex(base, trimmed, line_no)?;
            walk_statement(&toks, line_no, &mut state, &mut out)?;
        }
    }

    let eof_line = text.split('\n').count();
    if state.heredoc.is_some() {
        return Err(GemspecError {
            line: eof_line,
            message: "the heredoc body never terminates — the file is not valid Ruby".to_owned(),
        });
    }
    if state.block_var.is_some() {
        return Err(GemspecError {
            line: eof_line,
            message:
                "the Gem::Specification.new block never closes (the matching `end` is missing)"
                    .to_owned(),
        });
    }
    if !state.closed {
        return Err(GemspecError {
            line: 1,
            message: "no Gem::Specification.new do |var| ... end block — not a gemspec file"
                .to_owned(),
        });
    }
    Ok(out)
}

/// Processes one statement's tokens (the ordered walk: block openers
/// and closers fire anywhere they legally can; attribute rows capture
/// only while the walk sits at depth 0 inside the `Gem::Specification`
/// block).
fn walk_statement(
    toks: &[Tok],
    line_no: usize,
    state: &mut State,
    out: &mut Gemspec,
) -> Result<(), GemspecError> {
    let mut i = 0usize;
    while let Some(tok) = toks.get(i) {
        match tok {
            Tok::Heredoc { terminator } => {
                state.heredoc = Some(terminator.clone());
                i += 1;
            }
            Tok::Word(word) if word == "end" => {
                step_close(state, line_no)?;
                i += 1;
            }
            Tok::Punct(b'}') => {
                step_close(state, line_no)?;
                i += 1;
            }
            Tok::Word(word) if i == 0 && STATEMENT_OPENERS.contains(&word.as_str()) => {
                state.depth += 1;
                i += 1;
            }
            Tok::Word(word) if word == "do" => {
                state.depth += 1;
                i += 1;
            }
            Tok::Punct(b'{') => {
                state.depth += 1;
                i += 1;
            }
            Tok::Word(word) if word == "Gem" => {
                // The `Gem::Specification.new` block entry (a preamble
                // statement only — a nested entry is the multiple-block
                // refusal).
                if let Some(entry) = spec_block_entry(toks, i) {
                    if state.closed {
                        return Err(GemspecError {
                            line: line_no,
                            message: "one Gem::Specification.new block per .gemspec file is \
                                      the supported subset — a second block cannot share the \
                                      file's row identity"
                                .to_owned(),
                        });
                    }
                    if state.block_var.is_some() {
                        return Err(GemspecError {
                            line: line_no,
                            message: "a nested Gem::Specification.new call is outside the \
                                      supported gemspec subset"
                                .to_owned(),
                        });
                    }
                    state.block_var = Some(entry.var);
                    i = entry.rest;
                    continue;
                }
                i += 1;
            }
            Tok::Word(word) => {
                // `var.attr = value` and `var.add_* args` rows — only
                // inside the block at depth 0, and only when `word` is
                // the block variable. The row consumes its own header
                // (and a captured literal's bytes); the walk resumes
                // after it so trailing closers (the single-line brace
                // form's `}`) still fire.
                if state.block_var.as_deref() == Some(word.as_str())
                    && let Some(event) = row_event(toks, i)
                {
                    i = apply_row(toks, &event, line_no, state, out)?;
                    continue;
                }
                i += 1;
            }
            Tok::StrLit { .. } | Tok::Punct(_) => {
                i += 1;
            }
        }
    }
    Ok(())
}

/// The `end`/`}` closer step: a nested region closes at depth > 0; the
/// `Gem::Specification` block closes at depth 0; anything deeper is a
/// stray closer (not valid Ruby).
fn step_close(state: &mut State, line_no: usize) -> Result<(), GemspecError> {
    if state.depth > 0 {
        state.depth -= 1;
        return Ok(());
    }
    if state.block_var.is_some() {
        state.block_var = None;
        state.closed = true;
        return Ok(());
    }
    Err(GemspecError {
        line: line_no,
        message: "an `end` outside the Gem::Specification.new block — the file is not valid Ruby"
            .to_owned(),
    })
}

/// The `Gem::Specification.new` block entry parsed from the token
/// stream at position `at` (the `Gem` word):
/// `Gem :: Specification . new do |var|` / `{ |var|`. Returns the block
/// variable and the index of the first token after the `|var|` header.
fn spec_block_entry(toks: &[Tok], at: usize) -> Option<BlockEntry> {
    let is_word =
        |idx: usize, expected: &str| matches!(toks.get(idx), Some(Tok::Word(w)) if w == expected);
    let is_punct =
        |idx: usize, expected: u8| matches!(toks.get(idx), Some(Tok::Punct(p)) if *p == expected);
    let mut i = at;
    if !is_word(i, "Gem") || !is_punct(i + 1, b':') || !is_punct(i + 2, b':') {
        return None;
    }
    i += 3;
    if !is_word(i, "Specification") || !is_punct(i + 1, b'.') || !is_word(i + 2, "new") {
        return None;
    }
    i += 3;
    // `do |var|` or `{ |var|` (the position-args form
    // `Gem::Specification.new 'name', '1.0' do |s|` is outside the
    // subset — the identity must be a `spec.name` row).
    match toks.get(i) {
        Some(Tok::Word(w)) if w == "do" => i += 1,
        Some(Tok::Punct(b'{')) => i += 1,
        _ => return None,
    }
    if !is_punct(i, b'|') {
        return None;
    }
    i += 1;
    let Tok::Word(var) = toks.get(i)? else {
        return None;
    };
    let var = var.clone();
    i += 1;
    if !is_punct(i, b'|') {
        return None;
    }
    i += 1;
    Some(BlockEntry { var, rest: i })
}

struct BlockEntry {
    /// The block variable name.
    var: String,
    /// Index of the first token after the `|var|` header.
    rest: usize,
}

/// The row shape at `at`: `var . attr` — followed by either `= value`
/// (an assignment) or an argument list (a method call).
struct RowEvent {
    /// The attribute (`name`, `version`) or method
    /// (`add_dependency`, `add_runtime_dependency`,
    /// `add_development_dependency`).
    attr: String,
    /// `true` when the row is an assignment (`var.attr = ...`).
    assignment: bool,
    /// Index of the first value/argument token.
    args: usize,
}

/// Matches the `var.attr` row shape at `at` (the block variable's
/// word): an assignment carries a single `=` (never `==`, `+=`,
/// `<=`...); a method call starts its arguments directly.
fn row_event(toks: &[Tok], at: usize) -> Option<RowEvent> {
    if !matches!(toks.get(at + 1), Some(Tok::Punct(b'.'))) {
        return None;
    }
    let Tok::Word(attr) = toks.get(at + 2)? else {
        return None;
    };
    let attr = attr.clone();
    let assignment = matches!(toks.get(at + 3), Some(Tok::Punct(b'=')))
        && !matches!(toks.get(at + 4), Some(Tok::Punct(b'=')));
    Some(RowEvent {
        attr,
        assignment,
        args: if assignment { at + 4 } else { at + 3 },
    })
}

/// Applies one matched row (attribute or dependency) at depth 0 — or
/// warns when the walk sits inside a conditional region (the
/// specification reference's *"Specifications must be deterministic"*:
/// conditionally defined attributes are invalid, so the row is consumed
/// unmodeled, never captured). Returns the index the walk resumes at
/// (past a captured literal, or past the row header — the value tokens
/// keep walking so openers/closers/heredocs still fire).
fn apply_row(
    toks: &[Tok],
    event: &RowEvent,
    line_no: usize,
    state: &mut State,
    out: &mut Gemspec,
) -> Result<usize, GemspecError> {
    if state.depth > 0 {
        out.warnings.push(ScanWarning {
            line: line_no,
            code: "conditional-attribute",
            message: format!(
                "line {line_no}: the `{}` row sits inside a conditional region — \
                 \"Specifications must be deterministic\" (guides.rubygems.org), so the row \
                 is consumed unmodeled and never rewritten",
                event.attr
            ),
        });
        return Ok(event.args);
    }
    match event.attr.as_str() {
        "name" | "version" if event.assignment => apply_attribute_row(toks, event, line_no, out),
        "add_dependency" | "add_runtime_dependency" | "add_development_dependency"
            if !event.assignment =>
        {
            Ok(apply_dep_row(toks, event, line_no, out))
        }
        // Every other row (summary, authors, files, ..., or the getter
        // form of the handled rows) consumes silently — the handled
        // subset carries the identity, the version and the dependency
        // edges only.
        _ => Ok(event.args),
    }
}

/// Applies a `name`/`version` assignment row: a plain string literal
/// (the no-op `.freeze` idiom tolerated) captures with its inner span; a
/// modifier-guarded literal and a non-literal value are the
/// deterministic rule's unmodeled rows.
fn apply_attribute_row(
    toks: &[Tok],
    event: &RowEvent,
    line_no: usize,
    out: &mut Gemspec,
) -> Result<usize, GemspecError> {
    let literal = match toks.get(event.args) {
        Some(Tok::StrLit {
            text,
            span,
            interpolated: false,
        }) => Some((text, span)),
        _ => None,
    };
    let Some((text, span)) = literal else {
        // Not a literal string (interpolation, a computed value, a
        // heredoc opener — the pending body is already armed by the
        // walk): the value has no honest byte span.
        if event.attr == "name" {
            return Err(GemspecError {
                line: line_no,
                message: "`spec.name` must be a literal string — the identity cannot be \
                          computed (the specification reference lists `name` among the \
                          required gemspec attributes)"
                    .to_owned(),
            });
        }
        out.version_row_present = true;
        out.warnings.push(ScanWarning {
            line: line_no,
            code: "non-canonical-version",
            message: format!(
                "line {line_no}: the `version` row is not a literal string — the value has \
                 no honest byte span, so the package is treated as unpublishable and never \
                 rewritten"
            ),
        });
        return Ok(event.args);
    };
    let mut next = event.args + 1;
    // The no-op `.freeze` idiom on the captured literal.
    if matches!(toks.get(next), Some(Tok::Punct(b'.')))
        && matches!(toks.get(next + 1), Some(Tok::Word(w)) if w == "freeze")
    {
        next += 2;
    }
    // A modifier `if`/`unless`/`while`/`until` after the value defines
    // the row conditionally — the deterministic rule consumes it
    // unmodeled.
    if matches!(
        toks.get(next),
        Some(Tok::Word(w)) if matches!(w.as_str(), "if" | "unless" | "while" | "until")
    ) {
        out.version_row_present = event.attr == "version";
        out.warnings.push(ScanWarning {
            line: line_no,
            code: "conditional-attribute",
            message: format!(
                "line {line_no}: the `{}` row is guarded by a modifier condition — \
                 \"Specifications must be deterministic\" (guides.rubygems.org), so the row \
                 is consumed unmodeled and never rewritten",
                event.attr
            ),
        });
        return Ok(next + 1);
    }
    let value = ValueField {
        text: text.clone(),
        span: *span,
    };
    if event.attr == "name" {
        if value.text.is_empty() {
            return Err(GemspecError {
                line: line_no,
                message: "`spec.name` is an empty literal — the identity must name the gem"
                    .to_owned(),
            });
        }
        out.name = Some(value);
    } else {
        out.version = Some(value);
        out.version_row_present = true;
    }
    Ok(next)
}

/// Applies an `add_*` call row: the arguments are literal strings —
/// the first names the dependency, the rest ride verbatim as its
/// requirements.
fn apply_dep_row(toks: &[Tok], event: &RowEvent, line_no: usize, out: &mut Gemspec) -> usize {
    let Some((dep, consumed)) = toks.get(event.args..).and_then(dep_row) else {
        out.warnings.push(ScanWarning {
            line: line_no,
            code: "unmodeled-dep",
            message: format!(
                "line {line_no}: the `{}` call's arguments are outside the supported subset \
                 (literal string arguments only) — the row is consumed unmodeled",
                event.attr
            ),
        });
        return event.args;
    };
    if event.attr == "add_development_dependency" {
        out.dev_deps.push(dep);
    } else {
        out.deps.push(dep);
    }
    event.args + consumed
}

/// A dependency row: the arguments are literal strings —
/// `('name', 'req', 'req')` or `'name', 'req'` — ending at the closing
/// paren or the first token an argument list cannot continue with.
/// Returns the row and the number of tokens consumed. `None` = outside
/// the subset (a malformed argument list).
fn dep_row(toks: &[Tok]) -> Option<(GemDep, usize)> {
    let mut paren = false;
    let mut closed = false;
    let mut i = 0usize;
    if matches!(toks.first(), Some(Tok::Punct(b'('))) {
        paren = true;
        i = 1;
    }
    let mut items: Vec<ValueField> = Vec::new();
    let mut expect_value = true;
    loop {
        match toks.get(i) {
            Some(Tok::StrLit {
                text,
                span,
                interpolated: false,
            }) if expect_value => {
                items.push(ValueField {
                    text: text.clone(),
                    span: *span,
                });
                expect_value = false;
                i += 1;
            }
            Some(Tok::Punct(b'.')) if !expect_value => {
                // The no-op `.freeze` idiom on an argument literal.
                if matches!(toks.get(i + 1), Some(Tok::Word(w)) if w == "freeze") {
                    i += 2;
                } else {
                    break;
                }
            }
            Some(Tok::Punct(b',')) if !expect_value => {
                expect_value = true;
                i += 1;
            }
            Some(Tok::Punct(b')')) if paren && !expect_value => {
                closed = true;
                i += 1;
                break;
            }
            // End of tokens (the space-call form) or any token an
            // argument list cannot continue with.
            _ => break,
        }
    }
    if expect_value || items.is_empty() || (paren && !closed) {
        return None;
    }
    let name = items.remove(0).text;
    if name.is_empty() {
        return None;
    }
    Some((
        GemDep {
            name,
            requirements: items,
        },
        i,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spans_point_into_the_original_text_across_crlf_and_comments() {
        let text = "# frozen_string_literal: true\r\nGem::Specification.new do |spec|\r\n  spec.name = 'shared' # the identity\r\n  spec.version = '2.3.0' # the release row\r\nend\r\n";
        let parsed = parse_gemspec(text).expect("parses");
        let version = parsed.version.expect("the version row");
        assert_eq!(version.text, "2.3.0");
        let slice = &text[version.span.start as usize..version.span.end as usize];
        assert_eq!(slice, "2.3.0", "the span is the literal's inner bytes");
        let name = parsed.name.expect("the name row");
        let slice = &text[name.span.start as usize..name.span.end as usize];
        assert_eq!(slice, "shared");
    }

    #[test]
    fn interpolation_is_never_a_literal() {
        let text = "Gem::Specification.new do |spec|\n  spec.name = 'x'\n  spec.version = \"1.#{n}\"\nend\n";
        let parsed = parse_gemspec(text).expect("parses");
        assert!(
            parsed.version.is_none(),
            "an interpolated value never pairs"
        );
        assert!(parsed.version_row_present, "the row exists");
        assert!(
            parsed
                .warnings
                .iter()
                .any(|warning| warning.code == "non-canonical-version"),
            "warned: {:?}",
            parsed.warnings
        );
    }

    #[test]
    fn the_paren_call_form_captures_requirements() {
        let text = "Gem::Specification.new do |spec|\n  spec.name = 'x'\n  spec.version = '1.0.0'\n  spec.add_dependency('json', '>= 2.0', '< 3.0')\nend\n";
        let parsed = parse_gemspec(text).expect("parses");
        assert_eq!(parsed.deps.len(), 1);
        let dep = parsed.deps.first().expect("the dep row");
        assert_eq!(dep.name, "json");
        assert_eq!(dep.requirements.len(), 2);
        assert_eq!(
            dep.requirements.first().expect("the first req").text,
            ">= 2.0"
        );
        assert_eq!(
            dep.requirements.get(1).expect("the second req").text,
            "< 3.0"
        );
    }

    #[test]
    fn a_nested_do_region_stays_balanced() {
        let text = "Gem::Specification.new do |spec|\n  spec.name = 'x'\n  spec.version = '1.0.0'\n  spec.files = Dir['lib/**/*.rb'].map do |f|\n    f\n  end\n  spec.summary = 'ok'\nend\n";
        let parsed = parse_gemspec(text).expect("parses");
        assert_eq!(parsed.name.as_ref().expect("name").text, "x");
        assert_eq!(parsed.version.as_ref().expect("version").text, "1.0.0");
        assert!(
            parsed.warnings.is_empty(),
            "no conditional warnings: {:?}",
            parsed.warnings
        );
    }

    #[test]
    fn a_modifier_if_row_is_conditional_and_never_captures() {
        let text = "Gem::Specification.new do |spec|\n  spec.name = 'x'\n  spec.version = '1.0.0' if RUBY_VERSION >= '3.0'\nend\n";
        // The modifier condition defines the row conditionally — the
        // deterministic rule consumes it unmodeled, never a slot.
        let parsed = parse_gemspec(text).expect("parses");
        assert!(parsed.version.is_none(), "never captured");
        assert!(parsed.version_row_present, "the row exists");
        assert!(
            parsed
                .warnings
                .iter()
                .any(|warning| warning.code == "conditional-attribute"),
            "warned: {:?}",
            parsed.warnings
        );
    }

    #[test]
    fn an_unterminated_string_refuses_with_a_line_number() {
        let text = "Gem::Specification.new do |spec|\n  spec.name = 'unterminated\nend\n";
        let error = parse_gemspec(text).expect_err("refuses");
        assert_eq!(error.line, 2);
    }

    #[test]
    fn a_blockless_specification_call_refuses() {
        let text = "spec = Gem::Specification.new\nspec.name = 'x'\n";
        let error = parse_gemspec(text).expect_err("no block — not the supported shape");
        assert!(
            error.message.contains("no Gem::Specification.new"),
            "{error}"
        );
    }
}

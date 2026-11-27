//! The `Dockerfile` grammar (M8 ADR-0063), hand-rolled over the VERIFIED
//! reference (`docs/research/docker-image-tags.md`, from
//! `docs.docker.com/reference/dockerfile/` and the OCI Image Spec's
//! annotations table):
//!
//! - the format: *"# Comment INSTRUCTION arguments"* — *"The instruction
//!   is not case-sensitive. However, convention is for them to be
//!   UPPERCASE"* — and *"Comment lines are removed before the Dockerfile
//!   instructions are executed"*;
//! - the LABEL instruction: *"The LABEL instruction adds metadata to an
//!   image. A LABEL is a key-value pair"* with the syntax
//!   `LABEL <key>=<value> [<key>=<value>...]` — *"An image can have more
//!   than one label. You can specify multiple labels on a single
//!   line"* (the reference's own examples carry spaces around `=` and
//!   quoted keys: `LABEL "com.example.vendor" = "ACME Incorporated"`);
//! - values: *"To include spaces within a LABEL value, use quotes and
//!   backslashes as you would in command-line parsing"* and *"Be sure to
//!   use double quotes and not single quotes"*;
//! - the override rule: *"If a label already exists but with a different
//!   value, the most-recently-applied value overrides any
//!   previously-set value"* — the LAST version label is the row's site;
//! - `FROM`: *"The FROM instruction initializes a new build stage and
//!   sets the base image"* — its tag is the base image's version, never
//!   this package's (consumed unmodeled; not a version site);
//! - the version annotation: `org.opencontainers.image.version` —
//!   *"version of the packaged software"*, *"version MAY be Semantic
//!   versioning-compatible"* (the OCI Image Spec's annotations table).
//!
//! The scanner is offset-preserving like the sibling hand-written
//! lexers (ADR-0001's JSON scanner, ADR-0057's go.mod lexer, ADR-0061's
//! gemspec lexer): spans point into the ORIGINAL bytes — a value carried
//! over a line continuation keeps the byte offsets of the file it lives
//! in (invariant I4). Escaped characters inside quoted values ride
//! verbatim (a value the plan contract's semver cannot parse is never
//! rewritten).

use halyard_core::package::Span;

/// Parse error with the offending 1-based physical line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DockerfileError {
    /// 1-based line the error was detected on.
    pub line: usize,
    /// What went wrong.
    pub message: String,
}

impl std::fmt::Display for DockerfileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "invalid Dockerfile at line {}: {}",
            self.line, self.message
        )
    }
}

impl std::error::Error for DockerfileError {}

/// One parsed `LABEL` pair: the key, the value's text (the quoted
/// value's inner bytes or the bare token) and the value's absolute
/// byte span in the original content.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LabelPair {
    /// The label key (`org.opencontainers.image.version`).
    pub key: String,
    /// The value's text (the inner bytes of a quoted value, the bare
    /// token otherwise).
    pub value: String,
    /// Absolute span of the value's inner bytes.
    pub span: Span,
}

/// One parsed `Dockerfile` (the planning-relevant subset).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Dockerfile {
    /// Every `LABEL` pair in document order across all `LABEL`
    /// instructions (the Dockerfile reference's documented multi-pair
    /// form included). Empty for a Dockerfile without metadata rows.
    pub labels: Vec<LabelPair>,
    /// The number of `FROM` instructions (multi-stage builds included —
    /// the reference's *"FROM can appear multiple times within a single
    /// Dockerfile"*). Consumed unmodeled: a `FROM` row's tag is the
    /// base image's version, never this package's.
    pub from_rows: usize,
}

/// One byte of the flattened logical line: its original-file offset and
/// the byte itself. The space a joined continuation contributes carries
/// [`NO_ORIGIN`]; it can never sit inside a captured span (a canonical
/// semver value cannot span a continuation).
type FlatByte = (usize, u8);

/// The sentinel original offset of a joiner-contributed byte.
const NO_ORIGIN: usize = usize::MAX;

/// Parses the planning-relevant subset of `text` at the VERIFIED
/// grammar: comment lines and line continuations follow the documented
/// format rules, `LABEL` pairs are captured with their value spans,
/// `FROM` rows are counted and otherwise consumed.
pub fn parse_dockerfile(text: &str) -> Result<Dockerfile, DockerfileError> {
    let mut out = Dockerfile::default();
    let bytes = text.as_bytes();
    let mut at = 0usize;
    let mut next_line_no = 1usize;
    while at < bytes.len() {
        // One physical line (the comment rule: "Comment lines are
        // removed before the Dockerfile instructions are executed").
        let line_start = at;
        let mut line_end = at;
        while line_end < bytes.len() && bytes.get(line_end) != Some(&b'\n') {
            line_end += 1;
        }
        let raw = text.get(line_start..line_end).unwrap_or_default();
        let line_no = next_line_no;
        next_line_no += 1;
        at = if line_end < bytes.len() {
            line_end + 1
        } else {
            line_end
        };
        let trimmed = raw.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        // Flatten the logical line (continuations joined, the
        // documented format's trailing-`\` rule) while keeping every
        // carried byte's original offset.
        let mut flat: Vec<FlatByte> = Vec::new();
        let mut segment = raw;
        let mut segment_start = line_start;
        let mut last_line_no = line_no;
        loop {
            let trimmed_end = segment.trim_end();
            let continues = trimmed_end.ends_with('\\');
            let carried = if continues {
                trimmed_end.strip_suffix('\\').unwrap_or(trimmed_end)
            } else {
                segment
            };
            let carried_start = segment_start + offset_in(segment, carried);
            for (index, byte) in carried.as_bytes().iter().enumerate() {
                flat.push((carried_start + index, *byte));
            }
            if !continues {
                break;
            }
            if at >= bytes.len() {
                return Err(DockerfileError {
                    line: last_line_no,
                    message: "the line continuation runs past the end of the file".to_owned(),
                });
            }
            let continuation_start = at;
            let mut continuation_end = at;
            while continuation_end < bytes.len() && bytes.get(continuation_end) != Some(&b'\n') {
                continuation_end += 1;
            }
            let next_segment = text
                .get(continuation_start..continuation_end)
                .unwrap_or_default();
            let leading = next_segment.len() - next_segment.trim_start().len();
            at = if continuation_end < bytes.len() {
                continuation_end + 1
            } else {
                continuation_end
            };
            last_line_no += 1;
            next_line_no = last_line_no + 1;
            flat.push((NO_ORIGIN, b' '));
            segment = next_segment.trim_start();
            segment_start = continuation_start + leading;
        }
        parse_instruction(&flat, line_no, &mut out)?;
    }
    Ok(out)
}

/// The byte offset of `needle` inside `whole` (both borrowed from the
/// same allocation).
fn offset_in(whole: &str, needle: &str) -> usize {
    needle.as_ptr() as usize - whole.as_ptr() as usize
}

/// Parses one logical instruction line (the flattened byte vector): the
/// instruction word (not case-sensitive — the documented format), then
/// the `LABEL` pair list when the row is a LABEL.
fn parse_instruction(
    flat: &[FlatByte],
    line: usize,
    out: &mut Dockerfile,
) -> Result<(), DockerfileError> {
    let word_end = flat
        .iter()
        .position(|(_, byte)| byte.is_ascii_whitespace())
        .unwrap_or(flat.len());
    let word: Vec<u8> = flat
        .get(..word_end)
        .unwrap_or_default()
        .iter()
        .map(|&(_, byte)| byte)
        .collect();
    let instruction = String::from_utf8_lossy(&word).to_string();
    if !instruction.eq_ignore_ascii_case("LABEL") {
        // A `FROM` row counts (multi-stage builds included); every
        // other instruction is consumed unmodeled.
        if instruction.eq_ignore_ascii_case("FROM") {
            out.from_rows += 1;
        }
        return Ok(());
    }
    let mut cursor = word_end;
    skip_whitespace(flat, &mut cursor);
    while cursor < flat.len() {
        let key = parse_token(flat, &mut cursor, line)?;
        if key.is_empty() {
            return Err(DockerfileError {
                line,
                message: "the LABEL pair carries no key".to_owned(),
            });
        }
        skip_whitespace(flat, &mut cursor);
        match flat.get(cursor).map(|&(_, byte)| byte) {
            Some(b'=') => cursor += 1,
            _ => {
                return Err(DockerfileError {
                    line,
                    message: format!("the LABEL pair for {key:?} carries no `=` separator"),
                });
            }
        }
        skip_whitespace(flat, &mut cursor);
        let value = parse_value(flat, &mut cursor, line)?;
        out.labels.push(LabelPair {
            key,
            value: value.text,
            span: value.span,
        });
        skip_whitespace(flat, &mut cursor);
    }
    Ok(())
}

/// Advances `*cursor` past ASCII whitespace.
fn skip_whitespace(flat: &[FlatByte], cursor: &mut usize) {
    while flat
        .get(*cursor)
        .is_some_and(|&(_, byte)| byte.is_ascii_whitespace())
    {
        *cursor += 1;
    }
}

/// The value shape: the inner text plus the absolute span.
struct LabelValue {
    text: String,
    span: Span,
}

/// Consumes one key token (bare to whitespace/`=` or double-quoted —
/// the reference's `LABEL "com.example.vendor" = "ACME Incorporated"`
/// form) starting at `*cursor`, advancing it.
fn parse_token(
    flat: &[FlatByte],
    cursor: &mut usize,
    line: usize,
) -> Result<String, DockerfileError> {
    if flat.get(*cursor).is_some_and(|&(_, byte)| byte == b'"') {
        return Ok(parse_quoted(flat, cursor, line)?.text);
    }
    let start = *cursor;
    while flat
        .get(*cursor)
        .is_some_and(|&(_, byte)| !byte.is_ascii_whitespace() && byte != b'=')
    {
        *cursor += 1;
    }
    Ok(flat_text(flat, start, *cursor))
}

/// Consumes one `value` token starting at `*cursor`: a double-quoted
/// literal (the documented *"use quotes … as you would in command-line
/// parsing"* form; *"Be sure to use double quotes"*) or a bare token to
/// the next whitespace. The span covers the value's inner bytes in the
/// ORIGINAL coordinates.
fn parse_value(
    flat: &[FlatByte],
    cursor: &mut usize,
    line: usize,
) -> Result<LabelValue, DockerfileError> {
    if flat.get(*cursor).is_some_and(|&(_, byte)| byte == b'"') {
        return parse_quoted(flat, cursor, line);
    }
    // A bare token: to the next whitespace. Backslash escapes join the
    // token (the command-line parsing posture; a canonical semver value
    // carries none).
    let start = *cursor;
    while let Some(&(offset, byte)) = flat.get(*cursor) {
        if byte == b'\\' && *cursor + 1 < flat.len() {
            *cursor += 2;
            let _ = offset;
            continue;
        }
        if byte.is_ascii_whitespace() {
            break;
        }
        *cursor += 1;
    }
    let span = span_of(flat, start, *cursor, line)?;
    Ok(LabelValue {
        text: flat_text(flat, start, *cursor),
        span,
    })
}

/// Consumes a double-quoted literal starting at `*cursor` (the opening
/// quote included), advancing past the closing quote. Returns the inner
/// text and its absolute span; escapes ride verbatim.
fn parse_quoted(
    flat: &[FlatByte],
    cursor: &mut usize,
    line: usize,
) -> Result<LabelValue, DockerfileError> {
    let open = *cursor;
    let mut at = open.saturating_add(1);
    while let Some(&(_, byte)) = flat.get(at) {
        match byte {
            b'\\' => at = at.saturating_add(2),
            b'"' => {
                let text = flat_text(flat, open.saturating_add(1), at);
                let span = span_of(flat, open.saturating_add(1), at, line)?;
                *cursor = at.saturating_add(1);
                return Ok(LabelValue { text, span });
            }
            _ => at += 1,
        }
    }
    Err(DockerfileError {
        line,
        message: "the quoted value carries no closing quote".to_owned(),
    })
}

/// The text of `flat[start..end]`.
fn flat_text(flat: &[FlatByte], start: usize, end: usize) -> String {
    let bytes: Vec<u8> = flat
        .get(start..end)
        .unwrap_or_default()
        .iter()
        .map(|&(_, byte)| byte)
        .collect();
    String::from_utf8_lossy(&bytes).to_string()
}

/// Builds a span from the flattened line's original offsets, refusing
/// boundaries that fall outside the line, on joiner-contributed bytes
/// (a value spanning a continuation would not survive an in-place edit
/// byte-untouched) or past the plan contract's coordinate range.
fn span_of(
    flat: &[FlatByte],
    start: usize,
    end: usize,
    line: usize,
) -> Result<Span, DockerfileError> {
    let map = |index: usize, what: &str| -> Result<u32, DockerfileError> {
        let offset = flat
            .get(index)
            .map(|&(offset, _)| offset)
            .ok_or_else(|| DockerfileError {
                line,
                message: format!("the {what} boundary falls outside the line"),
            })?;
        if offset == NO_ORIGIN {
            return Err(DockerfileError {
                line,
                message: format!(
                    "the {what} spans a line continuation — the value's bytes would not \
                     survive an in-place edit untouched"
                ),
            });
        }
        u32::try_from(offset).map_err(|_| DockerfileError {
            line,
            message: "the value span exceeds the plan contract's coordinate range".to_owned(),
        })
    };
    Ok(Span {
        start: map(start, "value start")?,
        end: map(end, "value end")?,
    })
}

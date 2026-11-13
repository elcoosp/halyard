//! Lossless JSON span scanner (ADR-0001, §6.4): a hand-written,
//! allocation-conscious parser that pairs every value with its **byte
//! span** so version edits are pure byte-range patches.
//!
//! Contract (proptested and fuzzed):
//!
//! * `parse` never panics — arbitrary bytes produce either a document or
//!   a [`JsonError`] with a byte position;
//! * [`JsonDoc::render`] reproduces the input **byte for byte** when no
//!   edit was applied;
//! * [`JsonDoc::set_string`] rewrites only the inner span of one string
//!   value — indentation, key order, escapes elsewhere, comments-free
//!   whitespace and the trailing newline all survive (invariant I4).
//!
//! Escapes (`\uXXXX`, surrogate pairs) are decoded for value access while
//! the spans keep the original encoding untouched.

use halyard_core::package::Span;

/// A parsed JSON value paired with its source span.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpannedValue {
    /// The decoded value.
    pub value: JsonValue,
    /// Full span of the literal (quotes included for strings).
    pub span: Span,
}

/// Decoded JSON value (key order preserved for objects — invariant I3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JsonValue {
    /// `null`.
    Null,
    /// `true` / `false`.
    Bool(bool),
    /// Number literal kept verbatim (no float round-trips).
    Number(String),
    /// String value: decoded text plus the **inner** span (between the
    /// quotes) — the patch target for version edits (§9.1).
    String {
        /// Decoded text (escapes resolved).
        decoded: String,
        /// Inner byte span between the quotes.
        inner: Span,
    },
    /// Object with entries in document order.
    Object(Vec<JsonEntry>),
    /// Array with elements in document order.
    Array(Vec<SpannedValue>),
}

/// One object entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsonEntry {
    /// Decoded key text.
    pub key: String,
    /// Span of the key literal (quotes included).
    pub key_span: Span,
    /// The value.
    pub value: SpannedValue,
}

/// Scan/parse error with the offending byte position.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsonError {
    /// Byte offset where the error was detected.
    pub position: usize,
    /// What went wrong (rendered with source context by the CLI).
    pub message: String,
}

impl std::fmt::Display for JsonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "invalid JSON at byte {}: {}",
            self.position, self.message
        )
    }
}

impl std::error::Error for JsonError {}

/// A parsed document: original bytes + the spanned tree.
#[derive(Clone, Debug)]
pub struct JsonDoc {
    original: Vec<u8>,
    root: SpannedValue,
    /// Recorded `(start, end, replacement)` byte patches, applied
    /// right-to-left by [`JsonDoc::render`].
    edits: Vec<(u32, u32, String)>,
}

impl JsonDoc {
    /// Parses `input` (UTF-8, optional BOM tolerated). Total: returns
    /// [`JsonError`] instead of panicking on malformed input.
    pub fn parse(input: &[u8]) -> Result<Self, JsonError> {
        let body = input.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(input);
        let mut parser = Scanner {
            bytes: body,
            pos: 0,
        };
        parser.skip_ws();
        let root = parser.parse_value()?;
        parser.skip_ws();
        if parser.pos != parser.bytes.len() {
            return Err(parser.err("trailing data after the JSON value"));
        }
        let bom_offset = input.len() - body.len();
        let root = if bom_offset == 0 {
            root
        } else {
            shift_span(root, u32::try_from(bom_offset).unwrap_or(u32::MAX))
        };
        Ok(Self {
            original: input.to_vec(),
            root,
            edits: Vec::new(),
        })
    }

    /// The spanned root value.
    pub fn root(&self) -> &SpannedValue {
        &self.root
    }

    /// Decodes a top-level object entry by key.
    pub fn get(&self, key: &str) -> Option<&SpannedValue> {
        match &self.root.value {
            JsonValue::Object(entries) => entries.iter().find(|e| e.key == key).map(|e| &e.value),
            _ => None,
        }
    }

    /// Looks up a nested key path through objects only.
    pub fn get_path(&self, path: &[&str]) -> Option<&SpannedValue> {
        let mut cur = self.get(path.first()?)?;
        for key in path.get(1..).unwrap_or(&[]) {
            cur = match &cur.value {
                JsonValue::Object(entries) => {
                    entries.iter().find(|e| e.key == *key).map(|e| &e.value)?
                }
                _ => return None,
            };
        }
        Some(cur)
    }

    /// Plans a byte patch replacing the inner text of one string value
    /// addressed by an object key path. Returns the `(start, end)` span
    /// the caller must overwrite with `new` — or applies it through
    /// [`JsonDoc::render`].
    pub fn set_string(&mut self, path: &[&str], new: &str) -> Result<Span, JsonError> {
        let span = match &self.root.value {
            JsonValue::Object(_) => {
                let target = self.get_path(path).ok_or(JsonError {
                    position: 0,
                    message: format!("key path {path:?} does not resolve to a value"),
                })?;
                match &target.value {
                    JsonValue::String { inner, .. } => *inner,
                    _ => {
                        return Err(JsonError {
                            position: target.span.start as usize,
                            message: "target value is not a string".to_owned(),
                        });
                    }
                }
            }
            _ => {
                return Err(JsonError {
                    position: self.root.span.start as usize,
                    message: "document root is not an object".to_owned(),
                });
            }
        };
        self.replace_span(span, new)?;
        Ok(span)
    }

    /// Records a byte-span replacement of one string's **inner** text
    /// (between the quotes). Span-addressed variant of
    /// [`JsonDoc::set_string`] for callers that already resolved the edit
    /// site by walking the spanned tree (M3.3 patcher): duplicate keys in
    /// one object cannot be addressed by key path, but every entry has a
    /// unique span.
    ///
    /// # Errors
    ///
    /// [`JsonError`] when `span` is out of the document's bounds — the
    /// caller's span must come from the same parsed document.
    pub fn replace_span(&mut self, span: Span, new: &str) -> Result<(), JsonError> {
        let (start, end) = (span.start, span.end);
        if start > end || end > u32::try_from(self.original.len()).unwrap_or(u32::MAX) {
            return Err(JsonError {
                position: usize::try_from(start).unwrap_or(usize::MAX),
                message: format!("span {start}..{end} is outside the document"),
            });
        }
        self.edits.push((start, end, new.to_owned()));
        Ok(())
    }

    /// Renders the document: original bytes with the recorded patches
    /// applied right-to-left. With no edits, the output is byte-identical
    /// to the input (round-trip safety, proptest-covered).
    pub fn render(&self) -> Vec<u8> {
        let mut out = self.original.clone();
        for (start, end, text) in self.edits.iter().rev() {
            let (s, e) = (*start as usize, *end as usize);
            if s <= e && e <= out.len() {
                out.splice(s..e, text.bytes());
            }
        }
        out
    }
}

/// Shifts every span of a parsed tree by `by` (the BOM offset).
fn shift_span(mut value: SpannedValue, by: u32) -> SpannedValue {
    value.span = Span {
        start: value.span.start + by,
        end: value.span.end + by,
    };
    match &mut value.value {
        JsonValue::String { inner, .. } => {
            inner.start += by;
            inner.end += by;
        }
        JsonValue::Object(entries) => {
            for e in entries.iter_mut() {
                e.key_span = Span {
                    start: e.key_span.start + by,
                    end: e.key_span.end + by,
                };
                let child = std::mem::replace(
                    &mut e.value,
                    SpannedValue {
                        value: JsonValue::Null,
                        span: Span { start: 0, end: 0 },
                    },
                );
                e.value = shift_span(child, by);
            }
        }
        JsonValue::Array(items) => {
            for item in items.iter_mut() {
                let child = std::mem::replace(
                    item,
                    SpannedValue {
                        value: JsonValue::Null,
                        span: Span { start: 0, end: 0 },
                    },
                );
                *item = shift_span(child, by);
            }
        }
        _ => {}
    }
    value
}

struct Scanner<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl Scanner<'_> {
    fn err(&self, msg: &str) -> JsonError {
        JsonError {
            position: self.pos,
            message: msg.to_owned(),
        }
    }

    /// The current position as a u32 span coordinate. A position that
    /// does not fit `u32` (impossible for real package manifests — the
    /// scanner works on in-memory files) renders as `u32::MAX`, which
    /// only ever reads as "past the end of the document".
    fn pos32(&self) -> u32 {
        u32::try_from(self.pos).unwrap_or(u32::MAX)
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn skip_ws(&mut self) {
        while let Some(b) = self.peek() {
            if b == b' ' || b == b'\t' || b == b'\n' || b == b'\r' {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn parse_value(&mut self) -> Result<SpannedValue, JsonError> {
        let start = self.pos32();
        let value = match self.peek() {
            None => return Err(self.err("unexpected end of input")),
            Some(b'{') => return self.parse_object(start),
            Some(b'[') => return self.parse_array(start),
            Some(b'"') => {
                let (decoded, inner) = self.parse_string()?;
                SpannedValue {
                    value: JsonValue::String { decoded, inner },
                    span: Span {
                        start,
                        end: self.pos32(),
                    },
                }
            }
            Some(b't') => self.parse_keyword("true", JsonValue::Bool(true))?,
            Some(b'f') => self.parse_keyword("false", JsonValue::Bool(false))?,
            Some(b'n') => self.parse_keyword("null", JsonValue::Null)?,
            Some(b'-' | b'0'..=b'9') => self.parse_number()?,
            Some(_) => return Err(self.err("unexpected character")),
        };
        Ok(value)
    }

    fn parse_keyword(&mut self, word: &str, value: JsonValue) -> Result<SpannedValue, JsonError> {
        let start = self.pos32();
        let bytes = word.as_bytes();
        for expected in bytes {
            if self.peek() != Some(*expected) {
                return Err(self.err("invalid literal"));
            }
            self.pos += 1;
        }
        Ok(SpannedValue {
            value,
            span: Span {
                start,
                end: self.pos32(),
            },
        })
    }

    fn parse_number(&mut self) -> Result<SpannedValue, JsonError> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        // int part
        match self.peek() {
            Some(b'0') => self.pos += 1,
            Some(b'1'..=b'9') => {
                while matches!(self.peek(), Some(b'0'..=b'9')) {
                    self.pos += 1;
                }
            }
            _ => return Err(self.err("invalid number")),
        }
        // frac
        if self.peek() == Some(b'.') {
            self.pos += 1;
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(self.err("digits required after decimal point"));
            }
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.pos += 1;
            }
        }
        // exp
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(self.err("digits required in exponent"));
            }
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.pos += 1;
            }
        }
        let text =
            String::from_utf8_lossy(self.bytes.get(start..self.pos).unwrap_or(&[])).into_owned();
        Ok(SpannedValue {
            value: JsonValue::Number(text),
            span: Span {
                start: u32::try_from(start).unwrap_or(u32::MAX),
                end: self.pos32(),
            },
        })
    }

    /// Parses a quoted string; returns decoded text + inner span.
    fn parse_string(&mut self) -> Result<(String, Span), JsonError> {
        // Caller guarantees the opening quote.
        self.pos += 1;
        let inner_start = self.pos;
        let mut decoded = String::new();
        loop {
            let Some(b) = self.peek() else {
                return Err(self.err("unterminated string"));
            };
            match b {
                b'"' => {
                    let inner = Span {
                        start: u32::try_from(inner_start).unwrap_or(u32::MAX),
                        end: self.pos32(),
                    };
                    self.pos += 1;
                    return Ok((decoded, inner));
                }
                b'\\' => {
                    self.pos += 1;
                    let Some(esc) = self.peek() else {
                        return Err(self.err("unterminated escape"));
                    };
                    self.pos += 1;
                    match esc {
                        b'"' => decoded.push('"'),
                        b'\\' => decoded.push('\\'),
                        b'/' => decoded.push('/'),
                        b'b' => decoded.push('\u{0008}'),
                        b'f' => decoded.push('\u{000C}'),
                        b'n' => decoded.push('\n'),
                        b'r' => decoded.push('\r'),
                        b't' => decoded.push('\t'),
                        b'u' => {
                            let hi = self.parse_hex4()?;
                            let ch = if (0xD800..0xDC00).contains(&hi) {
                                // Surrogate pair: \uXXXX\uYYYY.
                                if self.peek() != Some(b'\\') {
                                    return Err(self.err("lone high surrogate"));
                                }
                                self.pos += 1;
                                if self.peek() != Some(b'u') {
                                    return Err(self.err("expected low surrogate escape"));
                                }
                                self.pos += 1;
                                let lo = self.parse_hex4()?;
                                if !(0xDC00..0xE000).contains(&lo) {
                                    return Err(self.err("invalid low surrogate"));
                                }
                                let combined = 0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00);
                                char::from_u32(combined)
                                    .ok_or_else(|| self.err("invalid surrogate pair"))?
                            } else if (0xDC00..0xE000).contains(&hi) {
                                return Err(self.err("lone low surrogate"));
                            } else {
                                char::from_u32(hi).ok_or_else(|| self.err("invalid \\u escape"))?
                            };
                            decoded.push(ch);
                        }
                        _ => return Err(self.err("unknown escape")),
                    }
                }
                0x00..=0x1F => return Err(self.err("unescaped control character in string")),
                _ => {
                    // Copy the UTF-8 sequence verbatim.
                    let len = utf8_len(b);
                    let end = self.pos + len;
                    if end > self.bytes.len() {
                        return Err(self.err("truncated UTF-8 sequence"));
                    }
                    let chunk = self.bytes.get(self.pos..end).unwrap_or(&[]);
                    match std::str::from_utf8(chunk) {
                        Ok(s) => decoded.push_str(s),
                        Err(_) => return Err(self.err("invalid UTF-8 in string")),
                    }
                    self.pos = end;
                }
            }
        }
    }

    fn parse_hex4(&mut self) -> Result<u32, JsonError> {
        let mut value = 0u32;
        for _ in 0..4 {
            let Some(b) = self.peek() else {
                return Err(self.err("unterminated \\u escape"));
            };
            let digit = match b {
                b'0'..=b'9' => u32::from(b - b'0'),
                b'a'..=b'f' => u32::from(b - b'a') + 10,
                b'A'..=b'F' => u32::from(b - b'A') + 10,
                _ => return Err(self.err("invalid hex digit in \\u escape")),
            };
            value = value * 16 + digit;
            self.pos += 1;
        }
        Ok(value)
    }

    fn parse_object(&mut self, start: u32) -> Result<SpannedValue, JsonError> {
        self.pos += 1; // '{'
        let mut entries: Vec<JsonEntry> = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Ok(SpannedValue {
                value: JsonValue::Object(entries),
                span: Span {
                    start,
                    end: self.pos32(),
                },
            });
        }
        loop {
            self.skip_ws();
            if self.peek() != Some(b'"') {
                return Err(self.err("expected object key string"));
            }
            let (key, key_span) = self.parse_string()?;
            self.skip_ws();
            if self.peek() != Some(b':') {
                return Err(self.err("expected ':' after object key"));
            }
            self.pos += 1;
            self.skip_ws();
            let value = self.parse_value()?;
            entries.push(JsonEntry {
                key,
                key_span,
                value,
            });
            self.skip_ws();
            match self.peek() {
                Some(b',') => {
                    self.pos += 1;
                    // Lenient real-world manifests: a trailing comma
                    // before `}` is tolerated (the lossless editor must
                    // parse what npm's lenient tooling accepts).
                    self.skip_ws();
                    if self.peek() == Some(b'}') {
                        self.pos += 1;
                        return Ok(SpannedValue {
                            value: JsonValue::Object(entries),
                            span: Span {
                                start,
                                end: self.pos32(),
                            },
                        });
                    }
                }
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(SpannedValue {
                        value: JsonValue::Object(entries),
                        span: Span {
                            start,
                            end: self.pos32(),
                        },
                    });
                }
                _ => return Err(self.err("expected ',' or '}' in object")),
            }
        }
    }

    fn parse_array(&mut self, start: u32) -> Result<SpannedValue, JsonError> {
        self.pos += 1; // '['
        let mut items: Vec<SpannedValue> = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Ok(SpannedValue {
                value: JsonValue::Array(items),
                span: Span {
                    start,
                    end: self.pos32(),
                },
            });
        }
        loop {
            self.skip_ws();
            items.push(self.parse_value()?);
            self.skip_ws();
            match self.peek() {
                Some(b',') => {
                    self.pos += 1;
                    // Trailing comma before `]` is tolerated (see the
                    // object parser note).
                    self.skip_ws();
                    if self.peek() == Some(b']') {
                        self.pos += 1;
                        return Ok(SpannedValue {
                            value: JsonValue::Array(items),
                            span: Span {
                                start,
                                end: self.pos32(),
                            },
                        });
                    }
                }
                Some(b']') => {
                    self.pos += 1;
                    return Ok(SpannedValue {
                        value: JsonValue::Array(items),
                        span: Span {
                            start,
                            end: self.pos32(),
                        },
                    });
                }
                _ => return Err(self.err("expected ',' or ']' in array")),
            }
        }
    }
}

/// Expected length of the UTF-8 sequence starting with byte `b`.
fn utf8_len(b: u8) -> usize {
    match b {
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xF7 => 4,
        // ASCII and continuation bytes / invalid leads: treated as 1 so
        // the caller reports invalid UTF-8 instead of running past the
        // end.
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_preserves_bytes() {
        let src = b"{\r\n  \"name\": \"@scope/x\",\r\n\t\"version\": \"1.0.0\",\r\n  \"n\": 42,\r\n  \"list\": [1, 2.5e3, true, null],\r\n}";
        let doc = JsonDoc::parse(src).unwrap();
        assert_eq!(doc.render(), src.to_vec());
    }

    #[test]
    fn spans_point_at_literals() {
        let src = b"{ \"version\": \"1.2.3\" }";
        let doc = JsonDoc::parse(src).unwrap();
        let version = doc.get("version").unwrap();
        match &version.value {
            JsonValue::String { inner, .. } => {
                assert_eq!(
                    src.get(inner.start as usize..inner.end as usize),
                    Some(&b"1.2.3"[..])
                );
            }
            other => panic!("expected string, got {other:?}"),
        }
    }

    #[test]
    fn set_string_edits_only_the_inner_span() {
        let src = b"{\n  \"name\": \"x\",\n  \"version\": \"^1.2.3\",\n  \"keep\": [1, 2]\n}\n";
        let mut doc = JsonDoc::parse(src).unwrap();
        doc.set_string(&["version"], "^1.2.4").unwrap();
        let out = doc.render();
        let expected =
            b"{\n  \"name\": \"x\",\n  \"version\": \"^1.2.4\",\n  \"keep\": [1, 2]\n}\n";
        assert_eq!(out, expected.to_vec());
    }

    #[test]
    fn errors_carry_positions_and_never_panic() {
        for bad in [
            &b"{"[..],
            b"{ \"a\" }",
            b"[1,",
            b"\"\\u12\"",
            b"{\"a\": 1}x",
            b"tru",
            b"01",
            b"1.",
            b"\"\\x\"",
        ] {
            assert!(
                JsonDoc::parse(bad).is_err(),
                "{:?} should fail",
                String::from_utf8_lossy(bad)
            );
        }
    }

    #[test]
    fn unicode_escapes_and_surrogates_decode() {
        let doc = JsonDoc::parse(b"{ \"emoji\": \"\\ud83d\\ude00\", \"e\": \"\\u00e9\" }").unwrap();
        let emoji = doc.get("emoji").unwrap();
        match &emoji.value {
            JsonValue::String { decoded, .. } => assert_eq!(decoded, "\u{1F600}"),
            _ => panic!("expected string"),
        }
    }

    #[test]
    fn bom_is_tolerated_and_spans_shift() {
        let src = b"\xEF\xBB\xBF{ \"v\": \"1.0.0\" }";
        let doc = JsonDoc::parse(src).unwrap();
        let v = doc.get("v").unwrap();
        match &v.value {
            JsonValue::String { inner, .. } => {
                assert_eq!(
                    src.get(inner.start as usize..inner.end as usize),
                    Some(&b"1.0.0"[..])
                );
            }
            _ => panic!("expected string"),
        }
        assert_eq!(doc.render(), src.to_vec());
    }

    #[test]
    fn missing_key_path_errors() {
        let mut doc = JsonDoc::parse(b"{ \"a\": \"x\" }").unwrap();
        let err = doc.set_string(&["b"], "y").unwrap_err();
        assert!(err.message.contains("does not resolve"));
    }
}

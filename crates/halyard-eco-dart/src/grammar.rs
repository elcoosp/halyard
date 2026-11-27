//! The `pubspec.yaml` subset scanner (M8 ADR-0060), hand-rolled over the
//! VERIFIED reference (`docs/research/dart-pub-grammars.md`, from
//! `dart.dev/tools/pub`: the pubspec field reference, the pub-workspaces
//! page, the dependencies page, the publishing page and the pub
//! environment-variables page).
//!
//! Handled grammar (everything else is an error, never a guess):
//!
//! ```yaml
//! # comment
//! name: my_pkg
//! version: 1.2.3          # trailing comments never join the value
//! publish_to: none
//! resolution: workspace
//! workspace:
//!   - packages/shared
//!   - packages/*
//! dependencies:
//!   shared: ^2.3.0
//!   transmogrify:
//!     path: ../transmogrify
//!     version: ^1.0.0
//!   flutter_driver:
//!     sdk: flutter
//! dev_dependencies:
//!   test: ^1.25.0
//! dependency_overrides:
//!   shared:
//!     path: ../shared
//! ```
//!
//! Top-level scalar keys (`name`, `version`, `publish_to`,
//! `resolution`), one-level block lists (`workspace:`), two-level block
//! maps (`dependencies:` entries with one nested source map), single- and
//! double-quoted scalars, `#` comments, blank lines, CRLF — and the byte
//! offsets of every captured value in the ORIGINAL text (I4: spans point
//! into the original bytes, so an edit never moves untouched text). Tabs
//! in indentation are rejected (YAML forbids them); flow style
//! (`{`/`[`), block scalars (`|`/`>`), anchors/aliases (`&`/`*`), multi-
//! document markers (`---`), and quoted-escape sequences refuse
//! fail-closed. Total on arbitrary input: errors carry a line number
//! instead of panicking.

use halyard_core::package::Span;

/// One parsed `pubspec.yaml` (the planning-relevant subset).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Pubspec {
    /// The `name` field — *"Required for every package"*. `None` when
    /// absent (the adapter refuses the row; the identity has no
    /// documented default).
    pub name: Option<ValueField>,
    /// The `version` field — *"A version number is three numbers
    /// separated by dots"*. `None` when absent (or a null `version:` —
    /// pub's *"implicitly versioned 0.0.0"* local-only posture).
    pub version: Option<ValueField>,
    /// The `publish_to` field carries the documented opt-out value
    /// `none` (*"to prevent publication anywhere"*).
    pub publish_to_none: bool,
    /// The `resolution` field (`"workspace"` marks a pub-workspace
    /// member).
    pub resolution: Option<String>,
    /// The `workspace` entries in document order (paths or glob
    /// patterns, as written).
    pub workspace: Vec<WorkspaceEntry>,
    /// `dependencies:` rows in document order.
    pub deps: Vec<PubDep>,
    /// `dev_dependencies:` rows in document order.
    pub dev_deps: Vec<PubDep>,
    /// `dependency_overrides:` row names, in document order (the section
    /// is consumed unmodeled — overrides pin local sources and carry no
    /// published requirement).
    pub overrides: Vec<String>,
}

/// One captured scalar with its byte span in the original text (the
/// inner content — quotes excluded, trailing comments excluded).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValueField {
    /// The scalar content, as written (quotes stripped; escapes refuse).
    pub text: String,
    /// Byte span of the content in the original file (the edit site the
    /// plan contract carries).
    pub span: Span,
    /// The scalar was quoted (single or double) in the source.
    pub quoted: bool,
}

/// One `workspace:` list entry (a path or glob pattern, as written).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceEntry {
    /// The entry text (quotes stripped).
    pub path: String,
    /// The entry was quoted in the source.
    pub quoted: bool,
}

/// One dependency row (from `dependencies:` or `dev_dependencies:`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PubDep {
    /// The dependency name (the map key).
    pub name: String,
    /// The hosted constraint — the inline scalar (`shared: ^2.3.0`) or
    /// the nested `version:` row. `None` for source-keyed rows without a
    /// version constraint.
    pub constraint: Option<ValueField>,
    /// The nested `path:` value (the path source; *"Relative paths are
    /// allowed and are considered relative to the directory containing
    /// your pubspec"*).
    pub path: Option<String>,
    /// The nested `sdk:` value (`sdk: flutter`).
    pub sdk: Option<String>,
    /// The nested `git:` key (any git source row — the value shape is
    /// consumed unmodeled).
    pub git: bool,
}

/// Parse error with the offending line number (1-based).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct YamlError {
    /// 1-based line the error was detected on.
    pub line: usize,
    /// What went wrong.
    pub message: String,
}

impl std::fmt::Display for YamlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "invalid pubspec.yaml at line {}: {}",
            self.line, self.message
        )
    }
}

impl std::error::Error for YamlError {}

/// One prepared line: its first byte's offset in the original text and
/// its comment-stripped, end-trimmed content. The content is a byte-exact
/// prefix of the raw line from `offset` on (comment stripping and the end
/// trim only remove a suffix), so every index into `content` maps
/// directly to `offset + index` in the original text.
struct Line<'a> {
    /// Byte offset of the line's first byte in the original text.
    offset: usize,
    /// The comment-stripped, end-trimmed content (no newline).
    content: &'a str,
}

/// The byte offset (within `raw`) where a `#` comment starts — a `#` at
/// the content start or preceded by whitespace, outside quotes. `None`
/// when the line carries no comment.
fn comment_start(raw: &str) -> Option<usize> {
    let bytes = raw.as_bytes();
    let mut quote: Option<u8> = None;
    for (idx, byte) in bytes.iter().copied().enumerate() {
        match quote {
            Some(q) => {
                // A backslash still keeps the quote state here (the value
                // capture is what reports an unsupported escape).
                if byte == b'\\' {
                    continue;
                }
                if byte == q {
                    quote = None;
                }
            }
            None => {
                if byte == b'\'' || byte == b'"' {
                    quote = Some(byte);
                } else if byte == b'#'
                    && (idx == 0
                        || bytes
                            .get(idx - 1)
                            .is_some_and(|p| *p == b' ' || *p == b'\t'))
                {
                    return Some(idx);
                }
            }
        }
    }
    None
}

/// Splits a content line into `(indent, rest)`; a tab anywhere in the
/// indentation is the documented refusal (YAML forbids tabs).
fn split_indent(content: &str) -> Result<(usize, &str), ()> {
    let mut idx = 0usize;
    for byte in content.as_bytes() {
        match byte {
            b' ' => idx += 1,
            b'\t' => return Err(()),
            _ => break,
        }
    }
    Ok((idx, &content[idx..]))
}

/// Splits `key: value` into the key and the value part (`None` when the
/// line is not a `key:` mapping — a `- item` or a bare scalar).
fn split_key(rest: &str) -> Option<(&str, &str)> {
    let idx = rest.find(':')?;
    let key = &rest[..idx];
    if key.is_empty() || key.contains(' ') || key.starts_with('-') {
        return None;
    }
    let value_idx = idx + 1;
    let value = rest[value_idx..].trim_start();
    Some((key, value))
}

/// Captures one scalar value. `content_offset` is the line content's
/// first byte in the original text; `value_idx` is the byte index within
/// `content` where the value starts (the caller trims leading spaces).
fn capture_scalar(
    content_offset: usize,
    content: &str,
    value_idx: usize,
    line_no: usize,
) -> Result<ValueField, YamlError> {
    let span_err = || YamlError {
        line: line_no,
        message: "file too large for the plan contract's byte spans".to_owned(),
    };
    let to_u32 = |value: usize| u32::try_from(value).map_err(|_| span_err());
    let value_part = &content[value_idx..];
    let start = content_offset + value_idx;
    let first = value_part.as_bytes().first().copied();
    match first {
        Some(b'{' | b'[') => Err(YamlError {
            line: line_no,
            message: "flow-style YAML ({} / []) is outside the supported pubspec subset — \
                      write the block form"
                .to_owned(),
        }),
        Some(b'|' | b'>') => Err(YamlError {
            line: line_no,
            message: "block scalars (| / >) are outside the supported pubspec subset".to_owned(),
        }),
        Some(b'&' | b'*' | b'!') => Err(YamlError {
            line: line_no,
            message: "YAML anchors/aliases/tags are outside the supported pubspec subset"
                .to_owned(),
        }),
        Some(quote @ (b'\'' | b'"')) => {
            let inner_start = start + 1;
            let bytes = value_part.as_bytes();
            let mut idx = 1usize;
            let mut closed = false;
            while let Some(byte) = bytes.get(idx).copied() {
                if byte == b'\\' {
                    return Err(YamlError {
                        line: line_no,
                        message: "quoted scalar escapes are outside the supported pubspec \
                                  subset — write the plain scalar"
                            .to_owned(),
                    });
                }
                // A doubled single-quote (`''`) is an escaped quote, not
                // a close.
                if byte == b'\'' && quote == b'\'' && bytes.get(idx + 1) == Some(&b'\'') {
                    return Err(YamlError {
                        line: line_no,
                        message: "single-quote escapes ('') are outside the supported pubspec \
                                  subset — write the plain scalar"
                            .to_owned(),
                    });
                }
                if byte == quote {
                    closed = true;
                    break;
                }
                idx += 1;
            }
            if !closed {
                return Err(YamlError {
                    line: line_no,
                    message: "unterminated quoted scalar".to_owned(),
                });
            }
            let inner_end = inner_start + idx - 1;
            Ok(ValueField {
                text: value_part[1..idx].to_owned(),
                span: Span {
                    start: to_u32(inner_start)?,
                    end: to_u32(inner_end)?,
                },
                quoted: true,
            })
        }
        Some(_) => {
            // A plain scalar: `content` is comment-stripped and
            // end-trimmed, so the value runs to the content's end.
            let end = content_offset + content.len();
            Ok(ValueField {
                text: value_part.to_owned(),
                span: Span {
                    start: to_u32(start)?,
                    end: to_u32(end)?,
                },
                quoted: false,
            })
        }
        None => Err(YamlError {
            line: line_no,
            message: "expected a value after the colon".to_owned(),
        }),
    }
}

/// The plain-text content of a nested scalar value (fail on flow style).
fn scalar_text(value_part: &str, line_no: usize) -> Result<String, YamlError> {
    let first = value_part.as_bytes().first().copied();
    if matches!(first, Some(b'{' | b'[')) {
        return Err(YamlError {
            line: line_no,
            message: "flow-style YAML ({} / []) is outside the supported pubspec subset — \
                      write the block form"
                .to_owned(),
        });
    }
    let text = value_part.trim_end();
    let unquoted = match (
        text.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')),
        text.strip_prefix('"').and_then(|s| s.strip_suffix('"')),
    ) {
        (Some(inner), _) | (_, Some(inner)) => inner,
        (None, None) => text,
    };
    Ok(unquoted.to_owned())
}

/// Parses the restricted subset. Total: returns [`YamlError`] instead of
/// panicking.
pub fn parse_pubspec(text: &str) -> Result<Pubspec, YamlError> {
    let (text, bom_offset) = match text.strip_prefix('\u{feff}') {
        Some(rest) => (rest, 3usize),
        None => (text, 0usize),
    };
    let mut out = Pubspec::default();
    let lines: Vec<Line<'_>> = text
        .split('\n')
        .scan(0usize, |offset, raw| {
            let line_offset = *offset;
            *offset += raw.len() + 1;
            let stripped = raw.strip_suffix('\r').unwrap_or(raw);
            let stripped_offset = line_offset + if line_offset == 0 { bom_offset } else { 0 };
            let content = match comment_start(stripped) {
                Some(idx) => stripped[..idx].trim_end(),
                None => stripped.trim_end(),
            };
            Some(Line {
                offset: stripped_offset,
                content,
            })
        })
        .collect();
    let mut i = 0usize;
    let mut seen: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    while let Some(line) = lines.get(i) {
        let content = line.content;
        i += 1;
        if content.trim().is_empty() {
            continue;
        }
        let (indent, rest) = split_indent(content).map_err(|()| tab_error(i))?;
        if indent != 0 {
            return Err(YamlError {
                line: i,
                message: "unexpectedly indented line at top level".to_owned(),
            });
        }
        if rest == "---" || rest == "..." {
            return Err(YamlError {
                line: i,
                message: "multi-document YAML is outside the supported pubspec subset".to_owned(),
            });
        }
        let Some((key, value)) = split_key(rest) else {
            return Err(YamlError {
                line: i,
                message: format!("expected `key:` at top level, found {rest:?}"),
            });
        };
        // The byte index within `content` where the value starts (the
        // caller-facing span anchor; `content` is prefix-aligned with the
        // raw line).
        let value_idx = content.len() - value.len();
        match key {
            "name" | "version" | "publish_to" | "resolution" => {
                if !seen.insert(key) {
                    return Err(YamlError {
                        line: i,
                        message: format!("duplicate `{key}` key"),
                    });
                }
                apply_scalar_key(&mut out, key, value, line, value_idx, i)?;
            }
            "workspace" => {
                if !seen.insert(key) {
                    return Err(YamlError {
                        line: i,
                        message: "duplicate `workspace` key".to_owned(),
                    });
                }
                if !value.is_empty() {
                    return Err(inline_block_error("workspace", i));
                }
                let (entries, next) = parse_block_list(&lines, i)?;
                out.workspace = entries;
                i = next;
            }
            "dependencies" | "dev_dependencies" | "dependency_overrides" => {
                if !seen.insert(key) {
                    return Err(YamlError {
                        line: i,
                        message: format!("duplicate `{key}` key"),
                    });
                }
                if !value.is_empty() {
                    return Err(inline_block_error(key, i));
                }
                let (rows, next) = parse_dep_block(&lines, i)?;
                match key {
                    "dependencies" => out.deps = rows,
                    "dev_dependencies" => out.dev_deps = rows,
                    _ => out.overrides = rows.into_iter().map(|row| row.name).collect(),
                }
                i = next;
            }
            _other => {
                // "Pub ignores all other fields" — skip the key and its
                // indented block.
                i = skip_block(&lines, i);
            }
        }
    }
    Ok(out)
}

/// The tab-indentation error (YAML forbids tabs).
fn tab_error(line: usize) -> YamlError {
    YamlError {
        line,
        message: "tab used for indentation (YAML forbids tabs)".to_owned(),
    }
}

/// The error for an inline value where the documented form is a block.
fn inline_block_error(key: &str, line: usize) -> YamlError {
    YamlError {
        line,
        message: format!(
            "`{key}` expects a block form (the documented shape) — an inline value is \
             outside the supported pubspec subset"
        ),
    }
}

/// Applies one top-level scalar key (`name`, `version`, `publish_to`,
/// `resolution`) to the output.
fn apply_scalar_key(
    out: &mut Pubspec,
    key: &str,
    value: &str,
    line: &Line<'_>,
    value_idx: usize,
    line_no: usize,
) -> Result<(), YamlError> {
    match key {
        "name" => {
            if value.is_empty() {
                return Err(YamlError {
                    line: line_no,
                    message: "`name` expects an inline scalar (it is required for every \
                              package)"
                        .to_owned(),
                });
            }
            out.name = Some(capture_scalar(
                line.offset,
                line.content,
                value_idx,
                line_no,
            )?);
        }
        "version" => {
            // A null value (`version:`) is pub's omitted posture — the
            // implicitly-0.0.0 local-only shape, no site.
            if !value.is_empty() {
                out.version = Some(capture_scalar(
                    line.offset,
                    line.content,
                    value_idx,
                    line_no,
                )?);
            }
        }
        "publish_to" => {
            out.publish_to_none = value == "none";
        }
        _ => {
            out.resolution = Some(value.to_owned());
        }
    }
    Ok(())
}

/// Skips an unknown top-level key and its indented block ("Pub ignores
/// all other fields"); returns the index of the next top-level line.
fn skip_block(lines: &[Line<'_>], start: usize) -> usize {
    let mut i = start;
    while let Some(next) = lines.get(i) {
        let blank = next.content.trim().is_empty();
        let indented = split_indent(next.content)
            .is_ok_and(|(n_indent, n_rest)| n_indent > 0 && !n_rest.is_empty());
        if blank || indented {
            i += 1;
        } else {
            break;
        }
    }
    i
}

/// Parses a block list of scalar entries (`- item` lines at one
/// consistent indentation). Returns the entries and the index of the
/// first line after the block.
fn parse_block_list(
    lines: &[Line<'_>],
    start: usize,
) -> Result<(Vec<WorkspaceEntry>, usize), YamlError> {
    let mut entries: Vec<WorkspaceEntry> = Vec::new();
    let mut item_indent: Option<usize> = None;
    let mut i = start;
    while let Some(line) = lines.get(i) {
        let content = line.content;
        if content.trim().is_empty() {
            i += 1;
            continue;
        }
        let (indent, rest) = split_indent(content).map_err(|()| tab_error(i + 1))?;
        if indent == 0 {
            break;
        }
        let line_no = i + 1;
        let Some(item) = rest.strip_prefix("- ") else {
            return Err(YamlError {
                line: line_no,
                message: "`workspace` expects a block list (`- path` entries)".to_owned(),
            });
        };
        let item_start = indent + 2;
        match item_indent {
            Some(expected) if item_start != expected => {
                return Err(YamlError {
                    line: line_no,
                    message: "inconsistent list indentation".to_owned(),
                });
            }
            None => item_indent = Some(item_start),
            _ => {}
        }
        if item.trim_end().is_empty() {
            return Err(YamlError {
                line: line_no,
                message: "an empty `workspace` entry cannot name a member".to_owned(),
            });
        }
        let field = capture_scalar(line.offset, content, item_start, line_no)?;
        entries.push(WorkspaceEntry {
            path: field.text,
            quoted: field.quoted,
        });
        i += 1;
    }
    if entries.is_empty() {
        return Err(YamlError {
            line: start + 1,
            message: "`workspace` declares no entries — an empty list cannot name members"
                .to_owned(),
        });
    }
    Ok((entries, i))
}

/// Parses a block map of dependency entries (one indentation level; each
/// entry carries an inline constraint or one nested source map). Returns
/// the rows and the index of the first line after the block.
fn parse_dep_block(lines: &[Line<'_>], start: usize) -> Result<(Vec<PubDep>, usize), YamlError> {
    let mut rows: Vec<PubDep> = Vec::new();
    let mut entry_indent: Option<usize> = None;
    let mut i = start;
    while let Some(line) = lines.get(i) {
        let content = line.content;
        if content.trim().is_empty() {
            i += 1;
            continue;
        }
        let (indent, rest) = split_indent(content).map_err(|()| tab_error(i + 1))?;
        if indent == 0 {
            break;
        }
        let line_no = i + 1;
        if let Some(expected) = entry_indent {
            if indent > expected {
                return Err(YamlError {
                    line: line_no,
                    message: "unexpectedly deep indentation in the dependency block".to_owned(),
                });
            }
            if indent < expected {
                break;
            }
        } else {
            entry_indent = Some(indent);
        }
        let (dep, next) = parse_dep_entry(lines, i, rest)?;
        if rows.iter().any(|row| row.name == dep.name) {
            return Err(YamlError {
                line: line_no,
                message: format!("duplicate dependency name {:?}", dep.name),
            });
        }
        rows.push(dep);
        i = next;
    }
    Ok((rows, i))
}

/// Parses one dependency entry at `indent` (its `key: value` rest
/// pre-split by the caller): an inline constraint or a nested source
/// map. Returns the row and the index of the next line.
fn parse_dep_entry(
    lines: &[Line<'_>],
    start: usize,
    rest: &str,
) -> Result<(PubDep, usize), YamlError> {
    let Some(line) = lines.get(start) else {
        return Err(YamlError {
            line: start + 1,
            message: "the dependency block ends mid-entry".to_owned(),
        });
    };
    let line_no = start + 1;
    let Some((name, value)) = split_key(rest) else {
        return Err(YamlError {
            line: line_no,
            message: format!("expected `name:` dependency entries, found {rest:?}"),
        });
    };
    let mut dep = PubDep {
        name: name.to_owned(),
        constraint: None,
        path: None,
        sdk: None,
        git: false,
    };
    if value.is_empty() {
        // The nested source map: `path:`, `version:`, `sdk:`, `git:`
        // keys one level deeper.
        let entry_indent = split_indent(line.content).map_or(0, |(indent, _)| indent);
        let next = parse_dep_source_map(lines, start + 1, entry_indent, &mut dep)?;
        return Ok((dep, next));
    }
    // The inline hosted constraint (`shared: ^2.3.0`).
    let value_idx = line.content.len() - value.len();
    dep.constraint = Some(capture_scalar(
        line.offset,
        line.content,
        value_idx,
        line_no,
    )?);
    Ok((dep, start + 1))
}

/// Parses one dependency entry's nested source map (one indentation
/// level under the entry key). Returns the index of the next line.
fn parse_dep_source_map(
    lines: &[Line<'_>],
    start: usize,
    entry_indent: usize,
    dep: &mut PubDep,
) -> Result<usize, YamlError> {
    let mut nested_indent: Option<usize> = None;
    let mut i = start;
    while let Some(line) = lines.get(i) {
        let content = line.content;
        if content.trim().is_empty() {
            i += 1;
            continue;
        }
        let (n_indent, n_rest) = split_indent(content).map_err(|()| tab_error(i + 1))?;
        if n_indent == 0 || n_indent <= entry_indent {
            break;
        }
        let line_no = i + 1;
        match nested_indent {
            Some(expected) if n_indent != expected => {
                return Err(YamlError {
                    line: line_no,
                    message: "inconsistent dependency-map indentation".to_owned(),
                });
            }
            None => nested_indent = Some(n_indent),
            _ => {}
        }
        let Some((n_key, n_value)) = split_key(n_rest) else {
            return Err(YamlError {
                line: line_no,
                message: format!("expected `key:` in the dependency map, found {n_rest:?}"),
            });
        };
        let n_value_idx = content.len() - n_value.len();
        match n_key {
            "path" => dep.path = Some(scalar_text(n_value, line_no)?),
            "sdk" => dep.sdk = Some(scalar_text(n_value, line_no)?),
            "version" => {
                if n_value.is_empty() {
                    return Err(YamlError {
                        line: line_no,
                        message: "`version` expects an inline constraint".to_owned(),
                    });
                }
                dep.constraint = Some(capture_scalar(line.offset, content, n_value_idx, line_no)?);
            }
            "git" => dep.git = true,
            // `url` / `ref` / anything else in the source map is consumed
            // unmodeled (the git shape details).
            _ => {}
        }
        i += 1;
    }
    Ok(i)
}

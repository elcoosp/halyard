//! The `go.mod` / `go.work` line grammar (M8 ADR-0057), hand-rolled over
//! the VERIFIED reference (`docs/research/go-module-grammars.md`, from
//! `go.dev/ref/mod`):
//!
//! - go.mod directives: `module`, `go`, `require`, `replace`, `exclude`,
//!   `retract` (plus Go 1.24's `tool`/`ignore` — recognized, skipped):
//!   *"Keywords distinguish different kinds of directives in a go.mod
//!   file."*
//! - go.work directives: `go`, `toolchain`, `use`, `replace`; a `use`
//!   *"adds a module on disk to the set of main modules in a workspace.
//!   Its argument is a relative path to the directory containing the
//!   module's go.mod file."*
//! - Versions: *"a semantic version consists of three non-negative
//!   integers … the patch version may be followed by an optional
//!   pre-release string starting with a hyphen."*
//!
//! The lexer blanks comments in place (`//` to end of line, `/* */`
//! spans) — the blanked text keeps its byte offsets, so every span this
//! module reports points into the ORIGINAL bytes. `exclude`, `retract`,
//! `toolchain`, `tool` and `ignore` rows are recognized and consumed but
//! not modeled: they constrain go's own resolution or carry no planning
//! input (an excluded/retracted version never becomes a release base in
//! halyard's grammar — release bases are this repo's own tags, §8.2).

use halyard_core::discover::{DiscoverError, ManifestFormat};
use halyard_core::package::Span;

/// One parsed `go.mod` (the planning-relevant subset).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GoMod {
    /// The `module` directive's path (`go.dev/ref/mod`: *"A go.mod file
    /// must contain exactly one module directive."*).
    pub module: String,
    /// The `go` directive's version, as written (`None` when absent).
    pub go: Option<String>,
    /// `require` rows in document order.
    pub requires: Vec<RequireRow>,
    /// `replace` rows in document order (workspace-local `Path` targets
    /// prove internal edges).
    pub replaces: Vec<ReplaceRow>,
}

/// One `require` row: `ModulePath Version` (block or single-line form).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequireRow {
    /// The required module path, as written (quotes stripped).
    pub path: String,
    /// The required version, as written (`v1.2.3`, a pseudo-version, …).
    pub version: String,
    /// Byte span of the version token in the original file — the
    /// requirement site the plan contract carries.
    pub version_span: Span,
}

/// One `replace` row: `ModulePath [Version] => (ModulePath Version |
/// FilePath)`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplaceRow {
    /// The replaced module path.
    pub old: String,
    /// The replaced version (`None` = every version of the path).
    pub old_version: Option<String>,
    /// The replacement target.
    pub target: ReplaceTarget,
}

/// The right-hand side of a `replace` row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReplaceTarget {
    /// Another module (`ModulePath Version`).
    Module {
        /// The replacement module path.
        path: String,
        /// The replacement version.
        version: String,
    },
    /// A local directory (`FilePath`, `./vendor/sdk`-style) — the
    /// workspace-proven internal edge.
    Path(String),
}

/// One parsed `go.work` (the planning-relevant subset).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GoWork {
    /// The `go` directive's version, as written (`None` when absent).
    pub go: Option<String>,
    /// `use` arguments: module directories, as written (`./api`, `.`).
    pub uses: Vec<String>,
    /// `replace` rows in document order (same grammar as go.mod's).
    pub replaces: Vec<ReplaceRow>,
}

/// Blanks comments in place: `//` to end of line and `/* */` spans become
/// spaces of the same byte length, so offsets into the returned text are
/// offsets into the original bytes. (go.mod/go.work comments are
/// semantically inert for planning — `// indirect` is informational.)
fn blank_comments(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = text.as_bytes().to_vec();
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes.get(i) {
            Some(b'/') if bytes.get(i + 1) == Some(&b'/') => {
                while i < out.len() {
                    let Some(&byte) = out.get(i) else { break };
                    if byte == b'\n' {
                        break;
                    }
                    if let Some(slot) = out.get_mut(i) {
                        *slot = b' ';
                    }
                    i += 1;
                }
            }
            Some(b'/') if bytes.get(i + 1) == Some(&b'*') => {
                if let Some(slot) = out.get_mut(i) {
                    *slot = b' ';
                }
                if let Some(slot) = out.get_mut(i + 1) {
                    *slot = b' ';
                }
                i += 2;
                while i < out.len() {
                    if bytes.get(i) == Some(&b'*') && bytes.get(i + 1) == Some(&b'/') {
                        if let Some(slot) = out.get_mut(i) {
                            *slot = b' ';
                        }
                        if let Some(slot) = out.get_mut(i + 1) {
                            *slot = b' ';
                        }
                        i += 2;
                        break;
                    }
                    if let Some(slot) = out.get_mut(i)
                        && *slot != b'\n'
                    {
                        *slot = b' ';
                    }
                    i += 1;
                }
            }
            _ => i += 1,
        }
    }
    String::from_utf8(out).unwrap_or_default()
}

/// One lexical token: text plus its byte span in the original file.
type Token = (String, Span);

/// Splits one line into tokens: bare idents end at whitespace; `"…"`
/// interpreted strings carry `\"` escapes; `` `…` `` raw strings span
/// literally. Unterminated quotes are a grammar error.
fn tokens(line: &str, line_start: usize, file: &str) -> Result<Vec<Token>, DiscoverError> {
    let mut out = Vec::new();
    let bytes = line.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes.get(i).copied() {
            Some(b' ' | b'\t' | b'\r') => i += 1,
            Some(quote @ (b'"' | b'`')) => {
                let start = i;
                i += 1;
                let mut closed = false;
                while i < bytes.len() {
                    if bytes.get(i) == Some(&b'\\') && quote == b'"' && i + 1 < bytes.len() {
                        i += 2;
                        continue;
                    }
                    if bytes.get(i) == Some(&quote) {
                        closed = true;
                        i += 1;
                        break;
                    }
                    i += 1;
                }
                if !closed || i > bytes.len() {
                    return Err(line_error(line, line_start, file, "unterminated string"));
                }
                let text = line
                    .get(start + 1..i - 1)
                    .ok_or_else(|| line_error(line, line_start, file, "unterminated string"))?;
                let text = if quote == b'"' {
                    unescape_interpreted(text)
                } else {
                    text.to_owned()
                };
                out.push((text, span_of(start, i, line_start)));
            }
            _ => {
                let start = i;
                while i < bytes.len()
                    && !matches!(bytes.get(i).copied(), Some(b' ' | b'\t' | b'\r'))
                {
                    i += 1;
                }
                let text = line.get(start..i).unwrap_or_default();
                out.push((text.to_owned(), span_of(start, i, line_start)));
            }
        }
    }
    Ok(out)
}

/// `\"`-style escapes in interpreted strings (go.dev/ref/mod: string
/// literals follow Go's own rules; planning only needs the plain path).
fn unescape_interpreted(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('"') => out.push('"'),
                // `\` escapes itself; a trailing lone `\` rides verbatim
                // (the same one-backslash output).
                Some('\\') | None => out.push('\\'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn span_of(start: usize, end: usize, line_start: usize) -> Span {
    Span {
        start: u32::try_from(line_start + start).unwrap_or(u32::MAX),
        end: u32::try_from(line_start + end.saturating_sub(1)).unwrap_or(u32::MAX),
    }
}

fn line_error(line: &str, line_start: usize, file: &str, message: &str) -> DiscoverError {
    DiscoverError::Manifest {
        file: file.to_owned(),
        format: ManifestFormat::Gomod,
        message: format!("{message} (at byte {line_start}: {line:?})"),
        span: None,
    }
}

/// A parsed directive line/block: keyword + rows of tokens.
struct Directive {
    keyword: String,
    keyword_span: Span,
    /// One row per line (single-line directives carry one row).
    rows: Vec<Vec<Token>>,
}

/// Splits blanked text into directives: keyword lines, parenthesized
/// blocks folded into their opening keyword's rows. Walks an indexed
/// line list (not an iterator) so a consumed block's lines are skipped.
fn directives(blanked: &str, file: &str) -> Result<Vec<Directive>, DiscoverError> {
    let lines: Vec<(usize, &str)> = blanked
        .split_inclusive('\n')
        .scan(0usize, |off, line| {
            let start = *off;
            *off += line.len();
            Some((start, line))
        })
        .collect();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < lines.len() {
        let Some((line_start, raw)) = lines.get(i) else {
            break;
        };
        let (line_start, raw) = (*line_start, *raw);
        i += 1;
        let line = raw.strip_suffix('\n').unwrap_or(raw);
        let trimmed_start = line.trim_start();
        let trimmed = trimmed_start.trim_end();
        if trimmed.is_empty() {
            continue;
        }
        let lead = line.len() - trimmed_start.len();
        let toks = tokens(trimmed, line_start + lead, file)?;
        if toks.is_empty() {
            continue;
        }
        let Some((keyword, keyword_span)) = toks.first().cloned() else {
            continue;
        };
        // A trailing `(` opens a block: fold the following lines into
        // this directive until the matching `)`. (The tokenizer yields
        // the paren as its own token; the grammar puts it directly after
        // the keyword on the opening line.)
        let opens_block = trimmed.ends_with('(') && toks.last().is_some_and(|(tok, _)| tok == "(");
        if opens_block {
            if toks.len() > 2 {
                return Err(DiscoverError::Manifest {
                    file: file.to_owned(),
                    format: ManifestFormat::Gomod,
                    message: "a block opens with `keyword (` on one line".to_owned(),
                    span: Some(keyword_span),
                });
            }
            let mut rows = Vec::new();
            let mut closed = false;
            while i < lines.len() {
                let Some((block_start, block_raw)) = lines.get(i) else {
                    break;
                };
                let (block_start, block_raw) = (*block_start, *block_raw);
                i += 1;
                let block_line = block_raw.strip_suffix('\n').unwrap_or(block_raw);
                let bt = block_line.trim();
                if bt == ")" {
                    closed = true;
                    break;
                }
                if bt.is_empty() {
                    continue;
                }
                let btrim = block_line.trim_start();
                let blead = block_line.len() - btrim.len();
                rows.push(tokens(btrim, block_start + blead, file)?);
            }
            if !closed {
                return Err(DiscoverError::Manifest {
                    file: file.to_owned(),
                    format: ManifestFormat::Gomod,
                    message: "unterminated ( block".to_owned(),
                    span: Some(keyword_span),
                });
            }
            out.push(Directive {
                keyword,
                keyword_span,
                rows,
            });
            continue;
        }
        out.push(Directive {
            keyword,
            keyword_span,
            rows: vec![toks.get(1..).unwrap_or_default().to_vec()],
        });
    }
    Ok(out)
}

/// Parses a `require` row: `ModulePath Version` (2 tokens; anything else
/// fails closed).
fn require_row(row: &[Token], file: &str, keyword_span: Span) -> Result<RequireRow, DiscoverError> {
    if row.len() != 2 {
        return Err(DiscoverError::Manifest {
            file: file.to_owned(),
            format: ManifestFormat::Gomod,
            message: "a require row is `ModulePath Version`".to_owned(),
            span: Some(keyword_span),
        });
    }
    // The shape check above guards the token reads; `get` keeps them
    // panic-free.
    let Some(path) = row.first() else {
        return Err(DiscoverError::Manifest {
            file: file.to_owned(),
            format: ManifestFormat::Gomod,
            message: "a require row is `ModulePath Version`".to_owned(),
            span: Some(keyword_span),
        });
    };
    let Some(version) = row.get(1) else {
        return Err(DiscoverError::Manifest {
            file: file.to_owned(),
            format: ManifestFormat::Gomod,
            message: "a require row is `ModulePath Version`".to_owned(),
            span: Some(keyword_span),
        });
    };
    Ok(RequireRow {
        path: path.0.clone(),
        version: version.0.clone(),
        version_span: version.1,
    })
}

/// Parses a `replace` row: `ModulePath [Version] => (ModulePath Version |
/// FilePath)`.
fn replace_row(row: &[Token], file: &str, keyword_span: Span) -> Result<ReplaceRow, DiscoverError> {
    let arrow = row.iter().position(|(tok, _)| tok == "=>");
    let Some(arrow) = arrow else {
        return Err(DiscoverError::Manifest {
            file: file.to_owned(),
            format: ManifestFormat::Gomod,
            message: "a replace row needs a `=>`".to_owned(),
            span: Some(keyword_span),
        });
    };
    let (lhs, rhs) = row.split_at(arrow);
    let rhs = rhs.get(1..).unwrap_or_default();
    if lhs.is_empty() || rhs.is_empty() {
        return Err(DiscoverError::Manifest {
            file: file.to_owned(),
            format: ManifestFormat::Gomod,
            message: "a replace row needs a module path on both sides of `=>`".to_owned(),
            span: Some(keyword_span),
        });
    }
    // The length match guards the token reads; `get`/`first` keep them
    // panic-free.
    let Some(lhs_path) = lhs.first() else {
        return Err(DiscoverError::Manifest {
            file: file.to_owned(),
            format: ManifestFormat::Gomod,
            message: "the replace target is `ModulePath [Version]`".to_owned(),
            span: Some(keyword_span),
        });
    };
    let (old, old_version) = match lhs.len() {
        1 => (lhs_path.0.clone(), None),
        2 => {
            let Some(version) = lhs.get(1) else {
                return Err(DiscoverError::Manifest {
                    file: file.to_owned(),
                    format: ManifestFormat::Gomod,
                    message: "the replace target is `ModulePath [Version]`".to_owned(),
                    span: Some(keyword_span),
                });
            };
            (lhs_path.0.clone(), Some(version.0.clone()))
        }
        _ => {
            return Err(DiscoverError::Manifest {
                file: file.to_owned(),
                format: ManifestFormat::Gomod,
                message: "the replace target is `ModulePath [Version]`".to_owned(),
                span: Some(keyword_span),
            });
        }
    };
    let Some(rhs_path) = rhs.first() else {
        return Err(DiscoverError::Manifest {
            file: file.to_owned(),
            format: ManifestFormat::Gomod,
            message: "the replacement is `ModulePath Version` or a `FilePath`".to_owned(),
            span: Some(keyword_span),
        });
    };
    let target = match rhs.len() {
        1 => ReplaceTarget::Path(rhs_path.0.clone()),
        2 => {
            let Some(version) = rhs.get(1) else {
                return Err(DiscoverError::Manifest {
                    file: file.to_owned(),
                    format: ManifestFormat::Gomod,
                    message: "the replacement is `ModulePath Version` or a `FilePath`".to_owned(),
                    span: Some(keyword_span),
                });
            };
            ReplaceTarget::Module {
                path: rhs_path.0.clone(),
                version: version.0.clone(),
            }
        }
        _ => {
            return Err(DiscoverError::Manifest {
                file: file.to_owned(),
                format: ManifestFormat::Gomod,
                message: "the replacement is `ModulePath Version` or a `FilePath`".to_owned(),
                span: Some(keyword_span),
            });
        }
    };
    Ok(ReplaceRow {
        old,
        old_version,
        target,
    })
}

/// Parses one `go.mod` (VERIFIED grammar, research note §1). `file` names
/// the manifest in errors; the bytes must be UTF-8 (the caller validates
/// and reports its own error otherwise).
pub fn parse_gomod(text: &str, file: &str) -> Result<GoMod, DiscoverError> {
    let blanked = blank_comments(text);
    let mut parsed = GoMod::default();
    let mut seen_module = false;
    for directive in directives(&blanked, file)? {
        match directive.keyword.as_str() {
            "module" => {
                if seen_module {
                    return Err(DiscoverError::Manifest {
                        file: file.to_owned(),
                        format: ManifestFormat::Gomod,
                        message: "a go.mod carries exactly one module directive".to_owned(),
                        span: Some(directive.keyword_span),
                    });
                }
                seen_module = true;
                let paths: Vec<&Token> = directive
                    .rows
                    .iter()
                    .flatten()
                    .filter(|(tok, _)| tok != "(" && tok != ")")
                    .collect();
                if paths.len() != 1 {
                    return Err(DiscoverError::Manifest {
                        file: file.to_owned(),
                        format: ManifestFormat::Gomod,
                        message: "the module directive carries exactly one path".to_owned(),
                        span: Some(directive.keyword_span),
                    });
                }
                let Some(path) = paths.first() else {
                    return Err(DiscoverError::Manifest {
                        file: file.to_owned(),
                        format: ManifestFormat::Gomod,
                        message: "the module directive carries exactly one path".to_owned(),
                        span: Some(directive.keyword_span),
                    });
                };
                parsed.module.clone_from(&path.0);
            }
            "go" => parsed.go = Some(directive_value(&directive, file, "the go directive")?),
            "toolchain" | "tool" | "ignore" | "exclude" | "retract" => {
                // Recognized, consumed, not modeled (see module docs).
            }
            "require" => {
                for row in &directive.rows {
                    parsed
                        .requires
                        .push(require_row(row, file, directive.keyword_span)?);
                }
            }
            "replace" => {
                for row in &directive.rows {
                    parsed
                        .replaces
                        .push(replace_row(row, file, directive.keyword_span)?);
                }
            }
            other => {
                return Err(DiscoverError::Manifest {
                    file: file.to_owned(),
                    format: ManifestFormat::Gomod,
                    message: format!("unknown directive {other:?}"),
                    span: Some(directive.keyword_span),
                });
            }
        }
    }
    if !seen_module {
        return Err(DiscoverError::Manifest {
            file: file.to_owned(),
            format: ManifestFormat::Gomod,
            message: "a go.mod must contain exactly one module directive".to_owned(),
            span: None,
        });
    }
    Ok(parsed)
}

/// Parses one `go.work` (VERIFIED grammar, research note §2).
pub fn parse_gowork(text: &str, file: &str) -> Result<GoWork, DiscoverError> {
    let blanked = blank_comments(text);
    let mut parsed = GoWork::default();
    for directive in directives(&blanked, file)? {
        match directive.keyword.as_str() {
            "go" => parsed.go = Some(directive_value(&directive, file, "the go directive")?),
            "toolchain" => {}
            "use" => {
                for row in &directive.rows {
                    if row.len() != 1 {
                        return Err(DiscoverError::Manifest {
                            file: file.to_owned(),
                            format: ManifestFormat::Gomod,
                            message: "a use row carries exactly one directory".to_owned(),
                            span: Some(directive.keyword_span),
                        });
                    }
                    let Some(directory) = row.first() else {
                        return Err(DiscoverError::Manifest {
                            file: file.to_owned(),
                            format: ManifestFormat::Gomod,
                            message: "a use row carries exactly one directory".to_owned(),
                            span: Some(directive.keyword_span),
                        });
                    };
                    parsed.uses.push(directory.0.clone());
                }
            }
            "replace" => {
                for row in &directive.rows {
                    parsed
                        .replaces
                        .push(replace_row(row, file, directive.keyword_span)?);
                }
            }
            other => {
                return Err(DiscoverError::Manifest {
                    file: file.to_owned(),
                    format: ManifestFormat::Gomod,
                    message: format!("unknown go.work directive {other:?}"),
                    span: Some(directive.keyword_span),
                });
            }
        }
    }
    Ok(parsed)
}

/// The single value token of a single-row directive (`go 1.23.0`).
fn directive_value(directive: &Directive, file: &str, what: &str) -> Result<String, DiscoverError> {
    let row = directive
        .rows
        .first()
        .ok_or_else(|| DiscoverError::Manifest {
            file: file.to_owned(),
            format: ManifestFormat::Gomod,
            message: format!("{what} carries exactly one value"),
            span: Some(directive.keyword_span),
        })?;
    if row.len() != 1 {
        return Err(DiscoverError::Manifest {
            file: file.to_owned(),
            format: ManifestFormat::Gomod,
            message: format!("{what} carries exactly one value"),
            span: Some(directive.keyword_span),
        });
    }
    let Some(value) = row.first() else {
        return Err(DiscoverError::Manifest {
            file: file.to_owned(),
            format: ManifestFormat::Gomod,
            message: format!("{what} carries exactly one value"),
            span: Some(directive.keyword_span),
        });
    };
    Ok(value.0.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn err_of(text: &str) -> String {
        match parse_gomod(text, "go.mod") {
            Ok(_) => panic!("expected a grammar error"),
            Err(e) => e.to_string(),
        }
    }

    #[test]
    fn parses_the_documented_example() {
        let text = "module example.com/my/thing\n\ngo 1.23.0\n\nrequire example.com/other/thing v1.0.2\nrequire example.com/new/thing/v2 v2.3.4\nexclude example.com/old/thing v1.2.3\nreplace example.com/bad/thing v1.4.5 => example.com/good/thing v1.4.5\n";
        let parsed = parse_gomod(text, "go.mod").expect("the documented example parses");
        assert_eq!(parsed.module, "example.com/my/thing");
        assert_eq!(parsed.go.as_deref(), Some("1.23.0"));
        assert_eq!(parsed.requires.len(), 2);
        assert_eq!(parsed.requires[0].version, "v1.0.2");
        assert_eq!(parsed.replaces.len(), 1);
    }

    #[test]
    fn block_forms_and_comments_keep_spans() {
        let text = "module example.com/m // the path\n\nrequire (\n\texample.com/a v1.2.3\n\texample.com/b v0.1.0 // indirect\n)\n";
        let parsed = parse_gomod(text, "go.mod").expect("block forms parse");
        assert_eq!(parsed.requires.len(), 2);
        // The version span points at `v1.2.3` in the original bytes.
        let span = parsed.requires[0].version_span;
        let bytes = text.as_bytes();
        assert_eq!(&bytes[span.start as usize..=span.end as usize], b"v1.2.3");
        // The indirect marker is inert (a comment).
        assert_eq!(
            &bytes[parsed.requires[1].version_span.start as usize
                ..=parsed.requires[1].version_span.end as usize],
            b"v0.1.0"
        );
    }

    #[test]
    fn local_path_replace_is_recognized() {
        let text = "module example.com/m\n\nreplace example.com/a => ./vendor/a\nreplace example.com/b v1.0.0 => example.com/c v1.0.1\n";
        let parsed = parse_gomod(text, "go.mod").expect("replaces parse");
        assert_eq!(
            parsed.replaces[0].target,
            ReplaceTarget::Path("./vendor/a".to_owned())
        );
        assert_eq!(
            parsed.replaces[1].target,
            ReplaceTarget::Module {
                path: "example.com/c".to_owned(),
                version: "v1.0.1".to_owned()
            }
        );
        assert_eq!(parsed.replaces[1].old_version.as_deref(), Some("v1.0.0"));
    }

    #[test]
    fn quoted_module_paths_unescape() {
        let text = "module \"example.com/m\"\n\nrequire \"example.com/a\" v1.0.0\n";
        let parsed = parse_gomod(text, "go.mod").expect("quoted paths parse");
        assert_eq!(parsed.module, "example.com/m");
        assert_eq!(parsed.requires[0].path, "example.com/a");
    }

    #[test]
    fn a_missing_module_directive_fails_closed() {
        let message = err_of("go 1.23.0\nrequire example.com/a v1.0.0\n");
        assert!(
            message.contains("exactly one module directive"),
            "{message}"
        );
    }

    #[test]
    fn an_unknown_directive_fails_closed() {
        let message = err_of("module example.com/m\n\nfrobnicate example.com/a v1.0.0\n");
        assert!(message.contains("unknown directive"), "{message}");
    }

    #[test]
    fn an_unterminated_block_fails_closed() {
        let message = err_of("module example.com/m\n\nrequire (\n\texample.com/a v1.0.0\n");
        assert!(message.contains("unterminated"), "{message}");
    }

    #[test]
    fn go_work_uses_and_replaces_parse() {
        let text = "go 1.23.0\n\nuse (\n\t.\n\t./api\n)\n\nuse ./web // a trailing use\n\nreplace example.com/a => ./vendor/a\n";
        let parsed = parse_gowork(text, "go.work").expect("go.work parses");
        assert_eq!(parsed.uses, vec![".", "./api", "./web"]);
        assert_eq!(parsed.replaces.len(), 1);
        assert_eq!(parsed.go.as_deref(), Some("1.23.0"));
    }

    #[test]
    fn block_comments_blank_without_shifting_offsets() {
        let text = "module example.com/m /* inline\nblock */ \n\nrequire example.com/a v1.0.0\n";
        let parsed = parse_gomod(text, "go.mod").expect("block comments parse");
        assert_eq!(parsed.requires[0].version, "v1.0.0");
        let bytes = text.as_bytes();
        let span = parsed.requires[0].version_span;
        assert_eq!(&bytes[span.start as usize..=span.end as usize], b"v1.0.0");
    }
}

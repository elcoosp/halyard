//! The Gradle **settings-file** grammar (M8, the Maven/Gradle cell's
//! Gradle half, ADR-0058 continued — the recorded-open remainder the
//! Gradle-half note names: *"the `settings.gradle` `rootProject.name` /
//! `include` grammar (the multi-project aggregator shape)"*), hand-
//! rolled over the VERIFIED reference (`docs/research/gradle-grammars.md`
//! §6, from `docs.gradle.org/current/dsl/org.gradle.api.initialization.Settings.html`
//! and the `org.gradle.api.initialization.ProjectDescriptor` javadoc):
//!
//! - `rootProject` is the Settings API's read-only property *"The root
//!   project of the build"* — a `ProjectDescriptor` whose `name` is a
//!   writable property (`setName`: *"Sets the name of this project"*);
//!   the settings script delegates to the Settings object, so
//!   `rootProject.name = '…'` is the descriptor's name setter. The
//!   documented default: *"The root project's name defaults to the name
//!   of the directory containing the settings file."* — the fallback
//!   halyard applies when no literal row (or no canonical row) exists;
//! - the last `rootProject.name = <literal>` assignment in document
//!   order wins (the descriptor setter's ordinary overwrite — the
//!   `version` row's own rule, no override *rule*);
//! - a non-literal right-hand side (a property reference, a `$`
//!   template) is recognized but non-canonical: the documented
//!   directory default governs and the row warns;
//! - `include` — *"Adds the given projects to the build"* — carries
//!   `String…` / `Iterable<String>` project paths: *"these path are not
//!   file paths, but instead specify the location of the new project in
//!   the project hierarchy"*, *"the supplied paths must use the `:`
//!   character as separator"*, *"The last element of the supplied path
//!   is used as the project name"*, and *"the supplied path is
//!   converted to a project directory relative to the root project
//!   directory"* — `the path a:b adds a project with path :a:b, name b
//!   and project directory $rootDir/a/b`. It *"also adds the parent :a
//!   project if it doesn't already exist"* — the implicit-parent rule
//!   the discovery governance applies;
//! - an include argument whose bytes are not a plain path literal (a
//!   `$` template, a property reference) is recognized but
//!   non-canonical — the member is unproven and the row warns.
//!
//! The scanner is offset-preserving like the build-file grammar (the
//! literal's inner-byte span points into the ORIGINAL bytes) and shares
//! the lexical substrate with `crate::gradle`: `//` line and `/* */`
//! block comments skipped in place, string literals that are not a
//! captured right-hand side skipped in place. Triple-quoted forms stay
//! out of the deterministic-specs subset. The Gradle *included-build*
//! surface (`includeBuild`, a `pluginManagement`/`dependencyResolution-
//! Management` block) is a different grammar — consumed unmodeled; an
//! `include` word inside a string or comment is skipped in place and
//! never captured.

use halyard_core::package::Span;

use crate::gradle::{
    GradleError, scan_double_quoted, scan_single_quoted, skip_block_comment, skip_line_comment,
    skip_string, skip_ws, word_at,
};

/// The right-hand side of one `rootProject.name = …` row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NameRhs {
    /// A literal string assignment — the Groovy single-quoted or the
    /// Kotlin/Groovy double-quoted (no `$` template) name. Carries the
    /// inner bytes and their absolute span.
    Literal {
        /// The literal's inner bytes (escapes ride verbatim).
        value: String,
        /// Absolute span of the inner bytes.
        span: Span,
    },
    /// A non-literal right-hand side (a property reference, a `$`
    /// template). Recognized — a name row exists — but non-canonical:
    /// the documented directory default governs.
    NonLiteral,
}

/// One `rootProject.name = …` assignment in document order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NameAssignment {
    /// The right-hand side shape.
    pub rhs: NameRhs,
}

/// One argument of one `include` call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IncludeArg {
    /// The argument's shape.
    pub shape: IncludeShape,
}

/// The shape of one `include` argument.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IncludeShape {
    /// A plain path literal (the documented `':'`-separated project
    /// path; the leading `:` is Gradle's own normalization — *"the path
    /// `a:b` adds a project with path `:a:b`"*). Carries the path text
    /// and the literal's inner-byte span.
    Path {
        /// The path text as written.
        path: String,
        /// Absolute span of the literal's inner bytes.
        span: Span,
    },
    /// A non-literal argument (a `$` template, a property reference).
    /// Recognized but non-canonical — the member is unproven.
    NonLiteral,
}

/// One `include` call in document order (the arguments accumulate —
/// *"Adds the given projects to the build"*).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IncludeCall {
    /// The call's arguments, left to right.
    pub args: Vec<IncludeArg>,
}

/// One parsed settings file (the planning-relevant subset).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SettingsFile {
    /// Every `rootProject.name = …` assignment in document order (the
    /// adapter applies the last-assignment-wins rule).
    pub root_names: Vec<NameAssignment>,
    /// Every `include` call in document order.
    pub includes: Vec<IncludeCall>,
}

/// Parses the planning-relevant subset of a `settings.gradle` /
/// `settings.gradle.kts` at the VERIFIED grammar: comments and sibling
/// strings are skipped in place, `rootProject.name = …` assignments and
/// `include` calls are captured with their literal spans.
pub fn parse_settings(text: &str) -> Result<SettingsFile, GradleError> {
    let bytes = text.as_bytes();
    let mut out = SettingsFile::default();
    let mut at = 0usize;
    let mut line = 1usize;
    while let Some(byte) = bytes.get(at).copied() {
        match byte {
            b'\n' => {
                line += 1;
                at += 1;
            }
            b'/' if bytes.get(at + 1) == Some(&b'/') => {
                // A `//` line comment: skip to end of line (the byte
                // itself is consumed by the newline arm next loop).
                at = skip_line_comment(bytes, at);
            }
            b'/' if bytes.get(at + 1) == Some(&b'*') => {
                // A `/* … */` block comment (not nested — the subset's
                // discipline; the build-file grammar shares it).
                (at, line) = skip_block_comment(bytes, at, line)?;
            }
            b'\'' | b'"' => {
                // A string literal that is not a captured right-hand
                // side (the capture arms consume their own literals):
                // skipped in place.
                at = skip_string(bytes, at, byte, line)?;
            }
            b'r' if word_at(bytes, at, b"rootProject") => {
                // The `rootProject` accessor at a word boundary, not
                // preceded by `.` — the Settings script's root-project
                // descriptor reference.
                let mut cursor = at + b"rootProject".len();
                skip_ws(bytes, &mut cursor, &mut line);
                if bytes.get(cursor) != Some(&b'.') {
                    // `rootProject` without a `.name` follow-up (a
                    // different member access): consumed unmodeled.
                    at += b"rootProject".len();
                    continue;
                }
                cursor += 1;
                skip_ws(bytes, &mut cursor, &mut line);
                // The `name` member of the descriptor — it legitimately
                // follows the `.` of the access chain, so only the word
                // text and its following boundary are checked here.
                if bytes.get(cursor..cursor + b"name".len()) != Some(b"name".as_slice())
                    || !crate::gradle::is_ident_boundary(bytes.get(cursor + b"name".len()).copied())
                {
                    // `rootProject.<other>`: consumed unmodeled.
                    at += b"rootProject".len();
                    continue;
                }
                cursor += b"name".len();
                skip_ws(bytes, &mut cursor, &mut line);
                if bytes.get(cursor) != Some(&b'=') {
                    // A read of `rootProject.name`, not an assignment:
                    // consumed unmodeled.
                    at += b"rootProject".len();
                    continue;
                }
                cursor += 1;
                skip_ws(bytes, &mut cursor, &mut line);
                let (rhs, advanced) = literal_rhs(bytes, cursor, line)?;
                line = advanced.1;
                out.root_names.push(NameAssignment { rhs });
                at = advanced.0;
            }
            b'i' if word_at(bytes, at, b"include") => {
                let (call, advanced, advanced_line) =
                    parse_include(bytes, at + b"include".len(), line)?;
                line = advanced_line;
                out.includes.push(call);
                at = advanced;
            }
            _ => at += 1,
        }
    }
    Ok(out)
}

/// Advances `*cursor` past ASCII whitespace on the SAME logical line
/// (never a newline — the unparenthesized Groovy `include` call ends at
/// the newline that follows its last argument).
fn skip_ws_inline(bytes: &[u8], cursor: &mut usize) {
    while let Some(byte) = bytes.get(*cursor).copied() {
        if byte == b' ' || byte == b'\t' || byte == b'\r' {
            *cursor += 1;
        } else {
            break;
        }
    }
}

/// The right-hand side of a captured assignment: a literal string (the
/// inner-byte span preserved) or a recognized-but-non-canonical shape.
fn literal_rhs(
    bytes: &[u8],
    at: usize,
    line: usize,
) -> Result<(NameRhs, (usize, usize)), GradleError> {
    match bytes.get(at).copied() {
        Some(b'\'') => {
            let (value, span, end, end_line) = scan_single_quoted(bytes, at, line)?;
            Ok((NameRhs::Literal { value, span }, (end, end_line)))
        }
        Some(b'"') => {
            let (value, span, end, end_line, templated) = scan_double_quoted(bytes, at, line)?;
            if templated {
                Ok((NameRhs::NonLiteral, (end, end_line)))
            } else {
                Ok((NameRhs::Literal { value, span }, (end, end_line)))
            }
        }
        Some(_) => {
            // A property reference, a closure, … — recognized but
            // non-canonical; the main scan consumes the rest.
            Ok((NameRhs::NonLiteral, (at + 1, line)))
        }
        None => Ok((NameRhs::NonLiteral, (at, line))),
    }
}

/// Parses one `include` call's arguments (the word is already consumed):
/// the parenthesized form (`include(':a', ':b')` — the Kotlin DSL's
/// documented call shape, legal Groovy too) or the unparenthesized
/// Groovy form (`include ':a', ':b'`), both comma-separated. A literal
/// argument is a proven path; every other argument shape is recognized
/// but non-canonical. Returns the call, the cursor past it, and the
/// carried line counter.
fn parse_include(
    bytes: &[u8],
    mut at: usize,
    mut line: usize,
) -> Result<(IncludeCall, usize, usize), GradleError> {
    let mut call = IncludeCall { args: Vec::new() };
    skip_ws(bytes, &mut at, &mut line);
    let parenthesized = bytes.get(at) == Some(&b'(');
    if parenthesized {
        at += 1;
        loop {
            skip_ws(bytes, &mut at, &mut line);
            match bytes.get(at).copied() {
                Some(quote @ (b'\'' | b'"')) => {
                    let (shape, advanced, advanced_line) = include_arg(bytes, at, quote, line)?;
                    line = advanced_line;
                    call.args.push(IncludeArg { shape });
                    at = advanced;
                }
                Some(b',') => at += 1,
                Some(b')') => {
                    at += 1;
                    break;
                }
                Some(_) => {
                    // A non-literal argument shape: consume to the
                    // argument boundary, recognized but non-canonical.
                    let (advanced, advanced_line) = skip_to_arg_boundary(bytes, at, line, true);
                    line = advanced_line;
                    call.args.push(IncludeArg {
                        shape: IncludeShape::NonLiteral,
                    });
                    at = advanced;
                }
                None => break,
            }
        }
        return Ok((call, at, line));
    }
    // The unparenthesized Groovy form: arguments ride the logical line,
    // comma-separated; a newline without a comma ends the call (the
    // inline whitespace skip never crosses the newline).
    loop {
        skip_ws_inline(bytes, &mut at);
        match bytes.get(at).copied() {
            Some(quote @ (b'\'' | b'"')) => {
                let (shape, advanced, advanced_line) = include_arg(bytes, at, quote, line)?;
                line = advanced_line;
                call.args.push(IncludeArg { shape });
                at = advanced;
            }
            Some(b',') => at += 1,
            Some(b'\n') | None => break,
            Some(_) => {
                let (advanced, advanced_line) = skip_to_arg_boundary(bytes, at, line, false);
                line = advanced_line;
                call.args.push(IncludeArg {
                    shape: IncludeShape::NonLiteral,
                });
                at = advanced;
            }
        }
    }
    Ok((call, at, line))
}

/// One include argument's literal scan: a single-quoted (Groovy) or a
/// double-quoted literal whose `$` template marks it non-literal (the
/// interpolation discipline the version grammar pins). Returns the
/// shape, the cursor past the literal, and the carried line counter.
fn include_arg(
    bytes: &[u8],
    at: usize,
    quote: u8,
    line: usize,
) -> Result<(IncludeShape, usize, usize), GradleError> {
    if quote == b'\'' {
        let (value, span, end, end_line) = scan_single_quoted(bytes, at, line)?;
        Ok((IncludeShape::Path { path: value, span }, end, end_line))
    } else {
        let (value, span, end, end_line, templated) = scan_double_quoted(bytes, at, line)?;
        if templated {
            Ok((IncludeShape::NonLiteral, end, end_line))
        } else {
            Ok((IncludeShape::Path { path: value, span }, end, end_line))
        }
    }
}

/// Consumes a non-literal argument's bytes to its boundary (a `,`, the
/// closing `)` for the parenthesized form, or the end of the logical
/// line), so the call's remaining arguments still capture.
fn skip_to_arg_boundary(
    bytes: &[u8],
    mut at: usize,
    mut line: usize,
    parenthesized: bool,
) -> (usize, usize) {
    while let Some(byte) = bytes.get(at).copied() {
        match byte {
            b'\n' => {
                if parenthesized {
                    line += 1;
                    at += 1;
                } else {
                    break;
                }
            }
            b',' | b')' if parenthesized => break,
            b',' => break,
            _ => at += 1,
        }
    }
    (at, line)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_groovy_root_name_assignment_is_a_literal() {
        let text = "rootProject.name = 'polyglot-parent'\n";
        let parsed = parse_settings(text).expect("parses");
        assert_eq!(parsed.root_names.len(), 1);
        let Some(NameAssignment {
            rhs: NameRhs::Literal { value, .. },
        }) = parsed.root_names.first()
        else {
            panic!("the literal assignment captures");
        };
        assert_eq!(value, "polyglot-parent");
    }

    #[test]
    fn a_kotlin_root_name_assignment_is_a_literal() {
        let parsed = parse_settings("rootProject.name = \"kotlin-parent\"\n").expect("parses");
        assert_eq!(parsed.root_names.len(), 1);
    }

    #[test]
    fn a_templated_root_name_is_non_literal() {
        let parsed = parse_settings("rootProject.name = \"${rootName}\"\n").expect("parses");
        let Some(assignment) = parsed.root_names.first() else {
            panic!("the row is recognized");
        };
        assert_eq!(assignment.rhs, NameRhs::NonLiteral);
    }

    #[test]
    fn a_property_reference_root_name_is_non_literal() {
        let parsed =
            parse_settings("rootProject.name = findProperty('n') as String\n").expect("parses");
        let Some(assignment) = parsed.root_names.first() else {
            panic!("the row is recognized");
        };
        assert_eq!(assignment.rhs, NameRhs::NonLiteral);
    }

    #[test]
    fn the_last_root_name_assignment_is_recorded_last() {
        let parsed = parse_settings("rootProject.name = 'first'\nrootProject.name = 'second'\n")
            .expect("parses");
        assert_eq!(parsed.root_names.len(), 2);
        let Some(NameAssignment {
            rhs: NameRhs::Literal { value, .. },
        }) = parsed.root_names.last()
        else {
            panic!("the last assignment is a literal");
        };
        assert_eq!(value, "second");
    }

    #[test]
    fn a_root_project_read_without_an_assignment_is_not_captured() {
        let parsed = parse_settings("println rootProject.name\n").expect("parses");
        assert_eq!(parsed.root_names, Vec::new());
    }

    #[test]
    fn a_nested_property_access_is_not_root_project() {
        let parsed = parse_settings("my.rootProject.name = 'x'\n").expect("parses");
        assert_eq!(parsed.root_names, Vec::new());
    }

    #[test]
    fn a_groovy_unparenthesized_include_captures_its_paths() {
        let parsed =
            parse_settings("include 'library'\ninclude ':services:api'\n").expect("parses");
        assert_eq!(parsed.includes.len(), 2);
        let first = &parsed.includes.first().expect("the call").args;
        assert_eq!(first.len(), 1);
        let IncludeShape::Path { path, .. } = &first.first().expect("the argument").shape else {
            panic!("a literal argument is a path");
        };
        assert_eq!(path, "library");
    }

    #[test]
    fn a_multi_argument_include_captures_every_path() {
        let parsed = parse_settings("include 'a', 'b', ':c:d'\n").expect("parses");
        assert_eq!(parsed.includes.len(), 1);
        let args = &parsed.includes.first().expect("the call").args;
        assert_eq!(args.len(), 3);
        let paths: Vec<&str> = args
            .iter()
            .filter_map(|arg| match &arg.shape {
                IncludeShape::Path { path, .. } => Some(path.as_str()),
                IncludeShape::NonLiteral => None,
            })
            .collect();
        assert_eq!(paths, vec!["a", "b", ":c:d"]);
    }

    #[test]
    fn a_kotlin_parenthesized_include_captures_its_paths() {
        let parsed =
            parse_settings("include(\":library\")\ninclude(\":a\", \":b\")\n").expect("parses");
        assert_eq!(parsed.includes.len(), 2);
        let args: Vec<&str> = parsed
            .includes
            .get(1)
            .expect("the second call")
            .args
            .iter()
            .filter_map(|arg| match &arg.shape {
                IncludeShape::Path { path, .. } => Some(path.as_str()),
                IncludeShape::NonLiteral => None,
            })
            .collect();
        assert_eq!(args, vec![":a", ":b"]);
    }

    #[test]
    fn a_templated_include_argument_is_non_canonical() {
        let parsed = parse_settings("include \":services:$name\"\n").expect("parses");
        let args = &parsed.includes.first().expect("the call").args;
        assert_eq!(args.len(), 1);
        assert_eq!(
            args.first().expect("the argument").shape,
            IncludeShape::NonLiteral
        );
    }

    #[test]
    fn a_sibling_include_word_inside_a_string_is_not_captured() {
        let parsed = parse_settings("description = \"include ':ghost'\"\ninclude ':real'\n")
            .expect("parses");
        assert_eq!(parsed.includes.len(), 1);
        let IncludeShape::Path { path, .. } = &parsed
            .includes
            .first()
            .expect("the call")
            .args
            .first()
            .expect("the argument")
            .shape
        else {
            panic!("the live call captures");
        };
        assert_eq!(path, ":real");
    }

    #[test]
    fn a_commented_include_is_not_captured() {
        let parsed = parse_settings("// include ':ghost'\ninclude ':real'\n").expect("parses");
        assert_eq!(parsed.includes.len(), 1);
    }

    #[test]
    fn an_unclosed_block_comment_fails_closed() {
        let error = parse_settings("/* never closed\n").expect_err("refuses");
        assert!(error.message.contains("closing"));
    }

    #[test]
    fn an_unclosed_include_literal_fails_closed() {
        let error = parse_settings("include ':library\n").expect_err("refuses");
        assert!(error.message.contains("closing"));
    }

    #[test]
    fn a_settings_file_without_the_subset_is_empty() {
        let parsed = parse_settings("pluginManagement {\n    repositories { mavenCentral() }\n}\n")
            .expect("parses");
        assert_eq!(parsed.root_names, Vec::new());
        assert_eq!(parsed.includes, Vec::new());
    }
}

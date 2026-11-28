//! The Gradle build-file grammar (M8 ADR-0058 continued — the
//! Maven/Gradle cell's Gradle half), hand-rolled over the VERIFIED
//! reference (`docs/research/gradle-grammars.md`, from
//! `docs.gradle.org/current/dsl/org.gradle.api.Project.html` and
//! `docs.gradle.org/current/userguide/building_java_projects.html`):
//!
//! - the `version` property is a writable `Object` on every Project
//!   (*"The version of this project. Gradle always uses the `toString()`
//!   value of the version. The version defaults to `unspecified`."*);
//!   build scripts delegate to the Project setter, so `version = …` is
//!   the documented assignment;
//! - the canonical assignment is a **literal string** on the right of
//!   `=`: the Groovy single-quoted `version = '1.2.1'` and the Kotlin
//!   double-quoted `version = "1.2.1"` (the Building Java Projects
//!   guide's own pair);
//! - a double-quoted value carrying `$` is a Groovy `GString` / Kotlin
//!   string template — interpolation, not a literal — so the row is
//!   non-canonical (no byte span to edit);
//! - the last `version = …` assignment in document order wins (the
//!   Project setter's ordinary overwrite — no override *rule*, no
//!   superseded warning; a conditional `version = …` is normal control
//!   flow);
//! - every other right-hand side (a property reference, a
//!   `gradle.properties` lookup, a version-catalog accessor, a plugin
//!   convention, a closure) is recognized — a `version = …` row exists
//!   — but non-literal, so the row warns `non-canonical-version` (not
//!   `no-version`).
//!
//! The scanner is offset-preserving like the sibling hand-written
//! lexers (ADR-0001's JSON scanner, ADR-0057's go.mod lexer, ADR-0061's
//! gemspec lexer, ADR-0063's Dockerfile lexer): the literal's inner-byte
//! span points into the ORIGINAL bytes, so the edit seam swaps exactly
//! those bytes and nothing else moves (invariant I4). Escapes inside a
//! quoted literal ride verbatim (a value the plan contract's semver
//! cannot parse is never rewritten). Comments (`//` line, `/* */` block)
//! and string literals that are not the `version = …` right-hand side are
//! skipped in place so a `version = "1.0.0"` inside a comment or a
//! sibling string is never captured.

use halyard_core::package::Span;

/// Parse error with the offending 1-based physical line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GradleError {
    /// 1-based line the error was detected on.
    pub line: usize,
    /// What went wrong.
    pub message: String,
}

impl std::fmt::Display for GradleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "invalid Gradle build file at line {}: {}",
            self.line, self.message
        )
    }
}

impl std::error::Error for GradleError {}

/// The right-hand side of one `version = …` row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VersionRhs {
    /// A literal string assignment — the Groovy single-quoted
    /// `version = '1.0.0'` or the Kotlin double-quoted
    /// `version = "1.0.0"` (no `$` template). Carries the inner bytes
    /// and their absolute span in the original file.
    Literal {
        /// The literal's inner bytes (escapes ride verbatim).
        value: String,
        /// Absolute span of the inner bytes.
        span: Span,
    },
    /// A non-literal right-hand side (a property reference, a
    /// `gradle.properties` lookup, a version-catalog accessor, a plugin
    /// convention, a closure, or a double-quoted template carrying `$`).
    /// Recognized — a `version = …` row exists — but non-canonical: no
    /// byte span to edit, never rewritten.
    NonLiteral,
}

/// One `version = …` assignment in document order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VersionAssignment {
    /// The right-hand side shape.
    pub rhs: VersionRhs,
}

/// One parsed Gradle build file (the planning-relevant subset).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GradleBuild {
    /// Every `version = …` assignment in document order (literal and
    /// non-literal). The adapter applies Gradle's last-assignment-wins
    /// rule (the Project API's property-setter overwrite — the LAST
    /// assignment, whatever its form, is the row's version source).
    pub version_assignments: Vec<VersionAssignment>,
}

/// Parses the planning-relevant subset of `text` at the VERIFIED
/// grammar: `//` line and `/* */` block comments are skipped in place,
/// string literals that are not the `version = …` right-hand side are
/// skipped, and every `version = <literal-or-non-literal>` assignment is
/// captured with its right-hand-side shape (the literal's inner-byte
/// span preserved for the edit seam).
pub fn parse_gradle_build(text: &str) -> Result<GradleBuild, GradleError> {
    let bytes = text.as_bytes();
    let mut out = GradleBuild::default();
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
                at += 2;
                while bytes.get(at).is_some_and(|&b| b != b'\n') {
                    at += 1;
                }
            }
            b'/' if bytes.get(at + 1) == Some(&b'*') => {
                // A `/* … */` block comment (not nested — the Gradle/Groovy
                // subset halyard plans carries no nested block comments).
                let start_line = line;
                at += 2;
                let mut closed = false;
                while let Some(block) = bytes.get(at).copied() {
                    match block {
                        b'\n' => {
                            line += 1;
                            at += 1;
                        }
                        b'*' if bytes.get(at + 1) == Some(&b'/') => {
                            at += 2;
                            closed = true;
                            break;
                        }
                        _ => at += 1,
                    }
                }
                if !closed {
                    return Err(GradleError {
                        line: start_line,
                        message: "the block comment carries no closing `*/`".to_owned(),
                    });
                }
            }
            b'\'' | b'"' => {
                // A string literal that is not a `version = …`
                // right-hand side (the assignment arm consumes its own
                // literal). Skip it in place so a `version = "1.0.0"`
                // inside a sibling string is never captured.
                at = skip_string(bytes, at, byte, line)?;
            }
            b'v' if bytes.get(at..at + 7) == Some(b"version")
                && is_ident_boundary(bytes.get(at.wrapping_sub(1)).copied())
                && bytes.get(at.wrapping_sub(1)).is_none_or(|&b| b != b'.')
                && is_ident_boundary(bytes.get(at + 7).copied()) =>
            {
                // The `version` identifier at a word boundary, not
                // preceded by `.` (the `project.version` qualified form
                // is a recorded-open remainder — the documented form is
                // the bare `version = …`).
                let mut cursor = at + 7;
                skip_ws(bytes, &mut cursor, &mut line);
                if bytes.get(cursor) == Some(&b'=') {
                    cursor += 1;
                    skip_ws(bytes, &mut cursor, &mut line);
                    let (rhs, advanced) = parse_rhs(bytes, cursor, line)?;
                    line = advanced.1;
                    out.version_assignments.push(VersionAssignment { rhs });
                    at = advanced.0;
                } else {
                    // `version` not followed by `=` — the method-call
                    // form (`version '1.0.0'`) or a different property
                    // (`version.code`). Not the documented assignment;
                    // the scanner does not model it (a recorded-open
                    // remainder). Advance past `version` and continue.
                    at += 7;
                }
            }
            _ => at += 1,
        }
    }
    Ok(out)
}

/// The byte after a `version = ` sequence (whitespace skipped): the
/// right-hand side shape, plus the cursor advanced past it (and the line
/// counter carried through). A literal string captures its inner-byte
/// span; every other right-hand side is `NonLiteral` and the cursor
/// advances one byte (the main scan consumes the rest — a string inside
/// a non-literal RHS is skipped as a sibling literal, never captured).
fn parse_rhs(
    bytes: &[u8],
    at: usize,
    line: usize,
) -> Result<(VersionRhs, (usize, usize)), GradleError> {
    match bytes.get(at).copied() {
        Some(b'\'') => {
            let (value, span, end, end_line) = scan_single_quoted(bytes, at, line)?;
            Ok((VersionRhs::Literal { value, span }, (end, end_line)))
        }
        Some(b'"') => {
            let (value, span, end, end_line, templated) = scan_double_quoted(bytes, at, line)?;
            if templated {
                // A `$` inside a double-quoted value is Groovy GString /
                // Kotlin string-template interpolation — the value is
                // computed, not a literal.
                Ok((VersionRhs::NonLiteral, (end, end_line)))
            } else {
                Ok((VersionRhs::Literal { value, span }, (end, end_line)))
            }
        }
        Some(_) => {
            // A non-literal right-hand side (property reference, lookup,
            // catalog accessor, closure, …). Recognized but non-canonical;
            // advance one byte and let the main scan consume the rest.
            Ok((VersionRhs::NonLiteral, (at + 1, line)))
        }
        None => Ok((VersionRhs::NonLiteral, (at, line))),
    }
}

/// Scans a single-quoted literal (Groovy `java.lang.String`) starting at
/// the opening quote: the inner bytes, their absolute span, the cursor
/// past the closing quote, and the line counter carried through. A
/// backslash escapes the next byte (escapes ride verbatim in the value).
fn scan_single_quoted(
    bytes: &[u8],
    open: usize,
    line: usize,
) -> Result<(String, Span, usize, usize), GradleError> {
    let mut at = open + 1;
    let mut line = line;
    let inner_start = at;
    while let Some(byte) = bytes.get(at).copied() {
        match byte {
            b'\n' => {
                line += 1;
                at += 1;
            }
            b'\\' => at = at.saturating_add(2),
            b'\'' => {
                let value = String::from_utf8_lossy(bytes.get(inner_start..at).unwrap_or_default())
                    .into_owned();
                let span = span_of(inner_start, at, line)?;
                return Ok((value, span, at + 1, line));
            }
            _ => at += 1,
        }
    }
    Err(GradleError {
        line,
        message: "the single-quoted version literal carries no closing quote".to_owned(),
    })
}

/// Scans a double-quoted literal starting at the opening quote: the
/// inner bytes, their absolute span, the cursor past the closing quote,
/// the line counter carried through, and whether a `$` template was seen
/// (interpolation — the value is non-literal). A backslash escapes the
/// next byte; an unescaped `$` flags the template.
fn scan_double_quoted(
    bytes: &[u8],
    open: usize,
    line: usize,
) -> Result<(String, Span, usize, usize, bool), GradleError> {
    let mut at = open + 1;
    let mut line = line;
    let inner_start = at;
    let mut templated = false;
    while let Some(byte) = bytes.get(at).copied() {
        match byte {
            b'\n' => {
                line += 1;
                at += 1;
            }
            b'\\' => at = at.saturating_add(2),
            b'$' => {
                templated = true;
                at += 1;
            }
            b'"' => {
                let value = String::from_utf8_lossy(bytes.get(inner_start..at).unwrap_or_default())
                    .into_owned();
                let span = span_of(inner_start, at, line)?;
                return Ok((value, span, at + 1, line, templated));
            }
            _ => at += 1,
        }
    }
    Err(GradleError {
        line,
        message: "the double-quoted version literal carries no closing quote".to_owned(),
    })
}

/// Skips a string literal that is not a `version = …` right-hand side
/// (the opening quote is at `at`), returning the cursor past the closing
/// quote. The literal's bytes are consumed in place; a `$` inside is
/// irrelevant (the literal is not a version value).
fn skip_string(bytes: &[u8], open: usize, quote: u8, line: usize) -> Result<usize, GradleError> {
    let mut at = open + 1;
    let mut line = line;
    while let Some(byte) = bytes.get(at).copied() {
        match byte {
            b'\n' => {
                line += 1;
                at += 1;
            }
            b'\\' => at = at.saturating_add(2),
            b if b == quote => return Ok(at + 1),
            _ => at += 1,
        }
    }
    let _ = quote;
    Err(GradleError {
        line,
        message: "the string literal carries no closing quote".to_owned(),
    })
}

/// Advances `*cursor` (and `*line`) past ASCII whitespace and newlines.
fn skip_ws(bytes: &[u8], cursor: &mut usize, line: &mut usize) {
    while let Some(byte) = bytes.get(*cursor).copied() {
        match byte {
            b' ' | b'\t' | b'\r' => *cursor += 1,
            b'\n' => {
                *line += 1;
                *cursor += 1;
            }
            _ => break,
        }
    }
}

/// Whether `byte` is an identifier boundary for the `version` word
/// (`None` at the file edges is a boundary). Identifier bytes are
/// `[A-Za-z0-9_$]` in both Groovy and Kotlin.
fn is_ident_boundary(byte: Option<u8>) -> bool {
    match byte {
        None => true,
        Some(b) => !b.is_ascii_alphanumeric() && b != b'_' && b != b'$',
    }
}

/// Builds a span from absolute byte offsets, refusing offsets past the
/// plan contract's coordinate range.
fn span_of(start: usize, end: usize, line: usize) -> Result<Span, GradleError> {
    let to_u32 = |offset: usize| {
        u32::try_from(offset).map_err(|_| GradleError {
            line,
            message: "the version span exceeds the plan contract's coordinate range".to_owned(),
        })
    };
    Ok(Span {
        start: to_u32(start)?,
        end: to_u32(end)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The span of `needle`'s inner bytes inside `whole` (the bytes
    /// between the quotes the fixture carries).
    fn inner_span(whole: &str, needle: &str) -> Span {
        let i = whole.find(needle).expect("fixture carries the needle");
        Span {
            start: u32::try_from(i).expect("fits"),
            end: u32::try_from(i + needle.len()).expect("fits"),
        }
    }

    #[test]
    fn a_groovy_single_quoted_version_is_a_literal() {
        let text = "plugins { id 'java' }\nversion = '1.2.3'\n";
        let parsed = parse_gradle_build(text).expect("parses");
        assert_eq!(parsed.version_assignments.len(), 1);
        let Some(VersionAssignment {
            rhs: VersionRhs::Literal { value, span },
        }) = parsed.version_assignments.first()
        else {
            panic!("a single-quoted literal is a Literal");
        };
        assert_eq!(value, "1.2.3");
        assert_eq!(*span, inner_span(text, "1.2.3"));
    }

    #[test]
    fn a_kotlin_double_quoted_version_is_a_literal() {
        let text = "plugins { java() }\nversion = \"1.2.3\"\n";
        let parsed = parse_gradle_build(text).expect("parses");
        assert_eq!(parsed.version_assignments.len(), 1);
        let Some(VersionAssignment {
            rhs: VersionRhs::Literal { value, span },
        }) = parsed.version_assignments.first()
        else {
            panic!("a double-quoted literal is a Literal");
        };
        assert_eq!(value, "1.2.3");
        assert_eq!(*span, inner_span(text, "1.2.3"));
    }

    #[test]
    fn a_double_quoted_template_is_non_literal() {
        let text = "version = \"${major}.${minor}\"\n";
        let parsed = parse_gradle_build(text).expect("parses");
        assert_eq!(parsed.version_assignments.len(), 1);
        let Some(VersionAssignment { rhs }) = parsed.version_assignments.first() else {
            panic!("one assignment");
        };
        assert_eq!(
            rhs,
            &VersionRhs::NonLiteral,
            "a `$` template is interpolation, not a literal"
        );
    }

    #[test]
    fn a_property_reference_rhs_is_non_literal() {
        let text = "version = findProperty('appVersion')\n";
        let parsed = parse_gradle_build(text).expect("parses");
        assert_eq!(parsed.version_assignments.len(), 1);
        let Some(VersionAssignment { rhs }) = parsed.version_assignments.first() else {
            panic!("one assignment");
        };
        assert_eq!(
            rhs,
            &VersionRhs::NonLiteral,
            "a property-reference RHS is recognized but non-canonical"
        );
    }

    #[test]
    fn the_last_assignment_in_document_order_wins() {
        let text = "version = '1.0.0'\nversion = '2.0.0'\n";
        let parsed = parse_gradle_build(text).expect("parses");
        assert_eq!(
            parsed.version_assignments.len(),
            2,
            "both assignments are recorded"
        );
        let Some(VersionAssignment {
            rhs: VersionRhs::Literal { value, .. },
        }) = parsed.version_assignments.get(1)
        else {
            panic!("the last assignment is a literal");
        };
        assert_eq!(value, "2.0.0", "the last assignment is the row's source");
    }

    #[test]
    fn a_version_inside_a_comment_is_not_captured() {
        let text = "// version = '9.9.9'\nversion = '1.0.0'\n";
        let parsed = parse_gradle_build(text).expect("parses");
        assert_eq!(parsed.version_assignments.len(), 1);
        let Some(VersionAssignment {
            rhs: VersionRhs::Literal { value, .. },
        }) = parsed.version_assignments.first()
        else {
            panic!("the comment's row is skipped");
        };
        assert_eq!(value, "1.0.0");
    }

    #[test]
    fn a_version_inside_a_sibling_string_is_not_captured() {
        let text = "description = \"version = '9.9.9'\"\nversion = '1.0.0'\n";
        let parsed = parse_gradle_build(text).expect("parses");
        assert_eq!(parsed.version_assignments.len(), 1);
        let Some(VersionAssignment {
            rhs: VersionRhs::Literal { value, .. },
        }) = parsed.version_assignments.first()
        else {
            panic!("the sibling string's row is skipped");
        };
        assert_eq!(value, "1.0.0");
    }

    #[test]
    fn a_block_comment_is_skipped_in_place() {
        let text = "/* version = '9.9.9' */\nversion = '1.0.0'\n";
        let parsed = parse_gradle_build(text).expect("parses");
        assert_eq!(parsed.version_assignments.len(), 1);
        let Some(VersionAssignment {
            rhs: VersionRhs::Literal { value, .. },
        }) = parsed.version_assignments.first()
        else {
            panic!("the block comment's row is skipped");
        };
        assert_eq!(value, "1.0.0");
    }

    #[test]
    fn a_substring_identifier_is_not_version() {
        let text = "myversion = '1.0.0'\nversion = '2.0.0'\n";
        let parsed = parse_gradle_build(text).expect("parses");
        assert_eq!(
            parsed.version_assignments.len(),
            1,
            "`myversion` is not the `version` identifier"
        );
        let Some(VersionAssignment {
            rhs: VersionRhs::Literal { value, .. },
        }) = parsed.version_assignments.first()
        else {
            panic!("only the `version` row is captured");
        };
        assert_eq!(value, "2.0.0");
    }

    #[test]
    fn a_project_dot_version_qualified_form_is_not_captured() {
        // The documented form is the bare `version = …`; the
        // `project.version` qualified form is a recorded-open remainder.
        let text = "project.version = '1.0.0'\n";
        let parsed = parse_gradle_build(text).expect("parses");
        assert_eq!(parsed.version_assignments, Vec::new());
    }

    #[test]
    fn an_unclosed_block_comment_fails_closed() {
        let text = "/* never closed\nversion = '1.0.0'\n";
        let err = parse_gradle_build(text).expect_err("the unclosed block comment refuses");
        assert!(err.message.contains("closing"));
    }

    #[test]
    fn an_unclosed_string_literal_fails_closed() {
        let text = "version = '1.0.0\n";
        let err = parse_gradle_build(text).expect_err("the unclosed literal refuses");
        assert!(err.message.contains("closing"));
    }

    #[test]
    fn a_file_with_no_version_assignment_is_empty() {
        let text = "plugins { id 'java' }\nrepositories { mavenCentral() }\n";
        let parsed = parse_gradle_build(text).expect("parses");
        assert_eq!(parsed.version_assignments, Vec::new());
    }
}

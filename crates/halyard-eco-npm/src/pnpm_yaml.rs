//! Restricted `pnpm-workspace.yaml` parser (§6.4, §19.2 default: a
//! hand-written scanner over the subset pnpm actually defines there).
//!
//! Handled grammar (everything else is an error, never a guess):
//!
//! ```yaml
//! packages:
//!   - 'packages/*'
//!   - docs/**        # negations: '!**/test/**'
//! catalog:
//!   react: ^18.0.0
//! catalogs:
//!   a:
//!     react: ^19.0.0
//! ```
//!
//! Block lists and one-level block maps of scalars, single/double-quoted
//! strings, `#` comments, blank lines, CRLF. Tabs in indentation are
//! rejected (YAML forbids them). Total on arbitrary input: errors carry a
//! line number instead of panicking.

/// Catalogs: `""` = the default `catalog:`, otherwise the `catalogs:`
/// name. Entries keep the requirement text verbatim.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PnpmWorkspace {
    /// `packages:` glob patterns in document order (negations included).
    pub packages: Vec<String>,
    /// `catalog:` / `catalogs:` entries keyed by catalog name.
    pub catalogs: std::collections::BTreeMap<String, std::collections::BTreeMap<String, String>>,
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
        write!(f, "invalid pnpm-workspace.yaml at line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for YamlError {}

/// Parses the restricted subset. Total: returns [`YamlError`] instead of
/// panicking.
pub fn parse_pnpm_workspace(text: &str) -> Result<PnpmWorkspace, YamlError> {
    let mut out = PnpmWorkspace::default();
    let lines: Vec<&str> = text.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)).collect();
    let mut i = 0usize;
    while i < lines.len() {
        let raw = lines[i];
        let trimmed = strip_comment(raw).trim_end().to_owned();
        i += 1;
        if trimmed.trim().is_empty() {
            continue;
        }
        let (indent, content) = split_indent(&trimmed).ok_or(YamlError {
            line: i,
            message: "tab used for indentation (YAML forbids tabs)".to_owned(),
        })?;
        if indent != 0 {
            return Err(YamlError {
                line: i,
                message: "unexpectedly indented line at top level".to_owned(),
            });
        }
        let Some((key, _)) = split_key(&content) else {
            return Err(YamlError {
                line: i,
                message: format!("expected `key:` at top level, found {content:?}"),
            });
        };
        match key {
            "packages" => {
                let (items, next) = parse_block_list(&lines, i)?;
                out.packages = items;
                i = next;
            }
            "catalog" => {
                let (map, next) = parse_block_map(&lines, i)?;
                out.catalogs.insert(String::new(), map);
                i = next;
            }
            "catalogs" => {
                // One level of named catalogs.
                let (maps, next) = parse_named_maps(&lines, i)?;
                for (name, map) in maps {
                    out.catalogs.insert(name, map);
                }
                i = next;
            }
            other => {
                return Err(YamlError {
                    line: i,
                    message: format!(
                        "unsupported key {other:?} (supported: packages, catalog, catalogs)"
                    ),
                });
            }
        }
    }
    Ok(out)
}

/// Expands the package patterns: returns include patterns with negations
/// applied in document order (`micromatch` semantics — later negations
/// win, later includes re-include).
pub fn apply_negations(patterns: &[String]) -> Vec<String> {
    let mut included: Vec<String> = Vec::new();
    let mut excluded: Vec<String> = Vec::new();
    for pattern in patterns {
        if let Some(neg) = pattern.strip_prefix('!') {
            let neg = neg.trim();
            excluded.push(neg.to_owned());
            included.retain(|p| p != neg);
        } else {
            let pattern = pattern.trim().to_owned();
            excluded.retain(|p| p != &pattern);
            if !included.contains(&pattern) {
                included.push(pattern);
            }
        }
    }
    included
}

/// Strips a trailing `#` comment (only when preceded by whitespace or at
/// line start, so `#` inside quotes survives).
fn strip_comment(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut in_single = false;
    let mut in_double = false;
    for (idx, b) in bytes.iter().enumerate() {
        match b {
            b'\'' if !in_double => in_single = !in_single,
            b'"' if !in_single => in_double = !in_double,
            b'#' if !in_single && !in_double => {
                if idx == 0 || bytes[idx - 1] == b' ' || bytes[idx - 1] == b'\t' {
                    return &line[..idx];
                }
            }
            _ => {}
        }
    }
    line
}

/// Splits leading indentation; `None` when a tab is used.
fn split_indent(line: &str) -> Option<(usize, &str)> {
    let bytes = line.as_bytes();
    let mut count = 0usize;
    while bytes.get(count) == Some(&b' ') {
        count += 1;
    }
    if bytes.get(count) == Some(&b'\t') {
        return None;
    }
    Some((count, &line[count..]))
}

/// Splits `key:` off a line (`None` when the line has no key colon).
fn split_key(content: &str) -> Option<(&str, &str)> {
    let idx = content.find(':')?;
    let key = &content[..idx];
    if key.is_empty() || key.contains(' ') || key.contains('\t') {
        return None;
    }
    Some((key, content[idx + 1..].trim()))
}

/// Unquotes a scalar ('single' / "double"), pass-through otherwise.
fn unquote(scalar: &str) -> String {
    let trimmed = scalar.trim();
    if trimmed.len() >= 2
        && ((trimmed.starts_with('\'') && trimmed.ends_with('\''))
            || (trimmed.starts_with('"') && trimmed.ends_with('"')))
    {
        return trimmed[1..trimmed.len() - 1].to_owned();
    }
    trimmed.to_owned()
}

/// Parses a block list (`- item` lines) at any indent; returns the items
/// and the index of the first line after the list.
fn parse_block_list(lines: &[&str], mut i: usize) -> Result<(Vec<String>, usize), YamlError> {
    let mut items = Vec::new();
    let line_no = |i: usize| i + 1;
    while i < lines.len() {
        let trimmed_full = strip_comment(lines[i]).trim_end().to_owned();
        if trimmed_full.trim().is_empty() {
            i += 1;
            continue;
        }
        let (indent, content) = split_indent(&trimmed_full).ok_or(YamlError {
            line: line_no(i),
            message: "tab used for indentation".to_owned(),
        })?;
        if indent == 0 {
            break;
        }
        let rest = content.trim_start();
        if let Some(item) = rest.strip_prefix("- ") {
            items.push(unquote(item.trim()));
        } else if rest == "-" {
            return Err(YamlError {
                line: line_no(i),
                message: "multi-line list items are not supported".to_owned(),
            });
        } else {
            // A new key under the same parent ends the list.
            if split_key(content).is_some() {
                break;
            }
            return Err(YamlError {
                line: line_no(i),
                message: format!("expected a `- item` list entry, found {content:?}"),
            });
        }
        i += 1;
    }
    Ok((items, i))
}

/// Parses a block map of scalars (`name: value`).
fn parse_block_map(lines: &[&str], mut i: usize) -> Result<(std::collections::BTreeMap<String, String>, usize), YamlError> {
    let mut map = std::collections::BTreeMap::new();
    while i < lines.len() {
        let trimmed_full = strip_comment(lines[i]).trim_end().to_owned();
        if trimmed_full.trim().is_empty() {
            i += 1;
            continue;
        }
        let (indent, content) = split_indent(&trimmed_full).ok_or(YamlError {
            line: i + 1,
            message: "tab used for indentation".to_owned(),
        })?;
        if indent == 0 {
            break;
        }
        let Some((key, rest)) = split_key(content) else {
            if rest_is_list_marker(content) {
                break;
            }
            return Err(YamlError {
                line: i + 1,
                message: format!("expected `name: value`, found {content:?}"),
            });
        };
        if rest.is_empty() {
            return Err(YamlError {
                line: i + 1,
                message: format!("catalog entry {key:?} has no version"),
            });
        }
        map.insert(unquote(key), unquote(rest));
        i += 1;
    }
    Ok((map, i))
}

/// True when the line opens a nested block (`name:` with empty value).
fn rest_is_list_marker(content: &str) -> bool {
    split_key(content)
        .map(|(_, rest)| rest.is_empty())
        .unwrap_or(false)
}

/// Parses `catalogs:` children: `name:` lines each followed by an indented
/// `dep: version` map.
fn parse_named_maps(
    lines: &[&str],
    mut i: usize,
) -> Result<(Vec<(String, std::collections::BTreeMap<String, String>)>, usize), YamlError> {
    let mut maps: Vec<(String, std::collections::BTreeMap<String, String>)> = Vec::new();
    while i < lines.len() {
        let trimmed_full = strip_comment(lines[i]).trim_end().to_owned();
        if trimmed_full.trim().is_empty() {
            i += 1;
            continue;
        }
        let (indent, content) = split_indent(&trimmed_full).ok_or(YamlError {
            line: i + 1,
            message: "tab used for indentation".to_owned(),
        })?;
        if indent == 0 {
            break;
        }
        let Some((name, rest)) = split_key(content) else {
            return Err(YamlError {
                line: i + 1,
                message: format!("expected a catalog name, found {content:?}"),
            });
        };
        if !rest.is_empty() {
            return Err(YamlError {
                line: i + 1,
                message: "catalog names must open a nested block".to_owned(),
            });
        }
        let (map, next) = parse_block_map(lines, i + 1)?;
        maps.push((unquote(name), map));
        i = next;
    }
    Ok((maps, i))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packages_globs_and_negations() {
        let ws = parse_pnpm_workspace(
            "packages:\n  - 'packages/*'\n  - docs\n  - '!**/test/**'\n  - '!apps/experimental'\n",
        )
        .unwrap();
        assert_eq!(
            ws.packages,
            vec!["packages/*", "docs", "!**/test/**", "!apps/experimental"]
        );
        assert_eq!(apply_negations(&ws.packages), vec!["packages/*", "docs"]);
    }

    #[test]
    fn catalogs_and_named_catalogs() {
        let ws = parse_pnpm_workspace(
            "packages:\n  - 'apps/*'\ncatalog:\n  react: ^18.0.0\n  lodash: 4.17.21\ncatalogs:\n  next:\n    react: ^19.0.0\n",
        )
        .unwrap();
        assert_eq!(ws.packages, vec!["apps/*"]);
        let default = ws.catalogs.get("").unwrap();
        assert_eq!(default.get("react").map(String::as_str), Some("^18.0.0"));
        let next = ws.catalogs.get("next").unwrap();
        assert_eq!(next.get("react").map(String::as_str), Some("^19.0.0"));
    }

    #[test]
    fn comments_crlf_and_blank_lines() {
        let ws = parse_pnpm_workspace(
            "# header comment\r\npackages:\r\n  # inline comment\r\n  - 'pkgs/*' # trailing\r\n\r\ncatalog:\r\n  a: 1.0.0\r\n",
        )
        .unwrap();
        assert_eq!(ws.packages, vec!["pkgs/*"]);
        assert_eq!(ws.catalogs.get("").unwrap().get("a").map(String::as_str), Some("1.0.0"));
    }

    #[test]
    fn unsupported_keys_error() {
        let err = parse_pnpm_workspace("onlyBuiltDependencies:\n  - esbuild\n").unwrap_err();
        assert!(err.message.contains("unsupported key"));
    }

    #[test]
    fn tabs_are_rejected() {
        assert!(parse_pnpm_workspace("packages:\n\t- 'x'\n").is_err());
    }

    #[test]
    fn totality_on_arbitrary_text() {
        proptest::proptest!(|(text in ".*{0,40}")| {
            let _ = parse_pnpm_workspace(&text);
        });
    }
}

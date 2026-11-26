//! PEP 508 requirement-row parsing (M8 step 1, ADR-0056): the leading
//! distribution name of a `dependencies` row, validated per the
//! name-normalization spec and normalized for member matching
//! (`docs/research/pypi-uv-grammars.md` §5–§6).
//!
//! halyard's plan contract classifies every requirement row exactly once
//! (§5.3): the version text a dependency carries is *foreign grammar*
//! for the semver rewriter (ADR-0006), so the specifier part is carried
//! verbatim (`ReqSpec::Other`) and only the name — normalized through
//! the `PyPA` rule (`runs of -,_,.` → `-`, lowercased) — participates in
//! the workspace join. The full PEP 508 grammar (extras, `@` URL specs,
//! PEP 440 specifiers, environment markers) therefore rides uninterpreted
//! behind the name.

/// One parsed requirement row: the normalized distribution name and the
/// raw row text (carried verbatim into the plan's `raw` field).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedRequirement {
    /// The normalized distribution name (the `PyPA` identity both uv and
    /// `PyPI` match on).
    pub name: String,
    /// The raw requirement text.
    pub raw: String,
}

/// Validates and normalizes the leading distribution name of one PEP 508
/// requirement row (`None` when the row does not start with a valid
/// project name — the caller warns and skips, like npm's
/// non-string-requirement rows).
///
/// The name grammar is the name-normalization spec's regex — *"valid
/// name consists only of ASCII letters and numbers, period, underscore
/// and hyphen. It must start and end with a letter or number"* — run
/// case-insensitively.
#[must_use]
pub fn parse_requirement(row: &str) -> Option<ParsedRequirement> {
    let row = row.trim();
    let name_len = row
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == '_' || *c == '-')
        .count();
    if name_len == 0 {
        return None;
    }
    let name = &row[..name_len];
    // The spec's regex: starts and ends with a letter or number (the
    // chars inside may be `.`/`_`/`-`).
    let valid = name
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphanumeric())
        && name
            .chars()
            .last()
            .is_some_and(|c| c.is_ascii_alphanumeric());
    if !valid {
        return None;
    }
    Some(ParsedRequirement {
        name: crate::pep440::normalize_name(name),
        raw: row.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_parse_and_normalize() {
        let row = parse_requirement("tqdm>=4,<5").expect("row");
        assert_eq!(row.name, "tqdm");
        assert_eq!(row.raw, "tqdm>=4,<5");

        let row =
            parse_requirement("BIRD.feeder [extra1,extra2] >= 0.2 ; python_version < \"3.11\"")
                .expect("row");
        assert_eq!(row.name, "bird-feeder");
    }

    #[test]
    fn url_and_marker_rows_keep_the_name() {
        let row = parse_requirement("tqdm @ https://example.com/tqdm.whl").expect("row");
        assert_eq!(row.name, "tqdm");
        let row = parse_requirement("numpy>=1.26 ; sys_platform == 'win32'").expect("row");
        assert_eq!(row.name, "numpy");
    }

    #[test]
    fn invalid_names_fail_closed() {
        for row in ["", "-tqdm>=1", "tqdm->=1", "=1.2", "@file.tar.gz"] {
            assert!(parse_requirement(row).is_none(), "{row:?} must not parse");
        }
    }
}

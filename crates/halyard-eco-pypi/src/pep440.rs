//! The PEP 440 strategy (§17 M8 *"PEP 440 strategy"*, ADR-0056): the
//! lossless pairing between PEP 440 spellings and the workspace's
//! semver-typed plan contract (§5.3 `version`).
//!
//! PEP 440 defines up to five segments — *"Epoch segment: N!, Release
//! segment: N(.N)\*, Pre-release segment: {a|b|rc}N, Post-release
//! segment: .postN, Development release segment: .devN"* with *"All
//! numeric components MUST be non-negative integers represented as
//! sequences of ASCII digits"* (`docs/research/pypi-uv-grammars.md` §4).
//! The plan contract's version type is semver, so the adapter plans
//! exactly the spellings that round-trip losslessly: three-component
//! release segments plus the canonical `{a|b|rc}N` pre-release segment.
//! Everything else the parser *recognizes* (epochs, post/dev releases,
//! local segments, implicit release segments, implicit pre-release
//! numbers, leading zeros) fails closed at discovery — the
//! `non-canonical-version` warning and an unpublishable package, never
//! a guessed rewrite.
//!
//! The mapping (both directions, lossless):
//!
//! | PEP 440    | semver        |
//! | ---------- | ------------- |
//! | `1.2.3`    | `1.2.3`       |
//! | `1.2.3a2`  | `1.2.3-a.2`   |
//! | `1.2.3b10` | `1.2.3-b.10`  |
//! | `1.2.3rc1` | `1.2.3-rc.1`  |
//!
//! The write-back always renders the PEP 440 spelling (the semver
//! hyphen form is not valid PEP 440, so `pyproject.toml` never carries
//! it).

use semver::Prerelease;
use semver::Version;

/// Parses a **canonical** PEP 440 version into its semver pairing.
///
/// Recognized-and-rejected spellings (return `None`): an epoch (`1!…`),
/// a post-release (`.postN`), a dev-release (`.devN`), a local segment
/// (`+…`), an implicit release segment (`1.2`), leading zeros (`01.2.3`,
/// `1.2.3rc01`), an implicit pre-release number (`1.2.3rc`), a
/// preceding `v`, surrounding whitespace other than trimmed edges, and
/// anything that is not ASCII digits per the PEP's grammar.
///
/// Canonical pre-release letters are `a`, `b`, `rc` (the PEP's
/// `{a|b|rc}N` spelling; the aliases `alpha`, `beta`, `c`, `pre`,
/// `preview` normalize per the PEP but are not canonical spellings —
/// rejected here, never guessed).
#[must_use]
pub fn parse_pep440(text: &str) -> Option<Version> {
    let text = text.trim();
    // Canonical spelling: release `N.N.N`, optional pre-release `{a|b|rc}N`.
    // The release segment is `[0-9.]`; the first non-release byte starts
    // the pre-release tail (an epoch `!`, post/dev `.`, local `+` and
    // `v` all surface here and fail closed in `parse_pre`).
    let split = text.find(|c: char| !c.is_ascii_digit() && c != '.');
    let (release, tail) = match split {
        Some(i) => (&text[..i], &text[i..]),
        None => (text, ""),
    };
    let release = parse_release(release)?;
    let pre = if tail.is_empty() {
        Prerelease::EMPTY
    } else {
        parse_pre(tail)?
    };
    let text = if pre.is_empty() {
        release
    } else {
        format!("{release}-{pre}")
    };
    Version::parse(&text).ok()
}

/// Parses the release segment: exactly three dot-separated ASCII-digit
/// components without leading zeros.
fn parse_release(release: &str) -> Option<String> {
    let parts: Vec<&str> = release.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    let mut out = Vec::with_capacity(3);
    for part in parts {
        let number = parse_u32(part)?;
        out.push(number.to_string());
    }
    Some(out.join("."))
}

/// Parses one ASCII-digit component: non-empty, digits only, no leading
/// zero (except the single `0`), and `u32`-range (the semver pairing).
fn parse_u32(part: &str) -> Option<u32> {
    if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if part.len() > 1 && part.starts_with('0') {
        return None;
    }
    part.parse().ok()
}

/// Parses the canonical pre-release spelling: `aN`, `bN` or `rcN` with a
/// non-empty digit tail without leading zeros. Returns the semver
/// pre-release text (`a.2`), to be spliced after `-`.
fn parse_pre(pre: &str) -> Option<Prerelease> {
    let (letter, tail) = pre
        .strip_prefix("rc")
        .map(|tail| ("rc", tail))
        .or_else(|| pre.strip_prefix('a').map(|tail| ("a", tail)))
        .or_else(|| pre.strip_prefix('b').map(|tail| ("b", tail)))?;
    let number = parse_u32(tail)?;
    Prerelease::new(&format!("{letter}.{number}")).ok()
}

/// Renders the semver pairing back to the canonical PEP 440 spelling.
///
/// `None` for pre-release identifiers outside the `{a,b,rc}` + number
/// shape (semver admits arbitrary identifiers; the plan contract's bump
/// algebra only ever produces the three letters a parsed version
/// carried) — a target that cannot render is the edit path's
/// fail-closed `E-EDIT-UNREWRITABLE`, never a guessed spelling.
#[must_use]
pub fn render_pep440(version: &Version) -> Option<String> {
    let base = format!("{}.{}.{}", version.major, version.minor, version.patch);
    if version.pre.is_empty() {
        return Some(base);
    }
    let pre = version.pre.as_str();
    // Semver pre-release identifiers are dot-separated; the PEP 440
    // pairing collapses `{letter}.{number}` onto `{letter}{number}`.
    let mut parts = pre.split('.');
    let letter = parts.next()?;
    let number = match letter {
        "a" | "b" | "rc" => parts.next()?,
        _ => return None,
    };
    if parts.next().is_some() {
        return None;
    }
    let number = parse_u32(number)?;
    Some(format!("{base}{letter}{number}"))
}

/// The PEP 440 spelling of the requirement-name normalization rule
/// (`docs/research/pypi-uv-grammars.md` §5): lowercase, runs of `.`, `-`
/// or `_` collapsed to a single `-`.
#[must_use]
pub fn normalize_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut last_dash = false;
    for ch in name.chars() {
        if ch == '.' || ch == '-' || ch == '_' {
            if !last_dash {
                out.push('-');
            }
            last_dash = true;
        } else {
            last_dash = false;
            for lower in ch.to_lowercase() {
                out.push(lower);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_releases_pair_with_semver() {
        assert_eq!(
            parse_pep440("1.2.3").map(|v| v.to_string()),
            Some("1.2.3".to_owned())
        );
        assert_eq!(
            parse_pep440("0.0.0").map(|v| v.to_string()),
            Some("0.0.0".to_owned())
        );
        assert_eq!(
            parse_pep440("2021.4.19").map(|v| v.to_string()),
            Some("2021.4.19".to_owned())
        );
    }

    #[test]
    fn canonical_pre_releases_pair_with_semver() {
        assert_eq!(
            parse_pep440("1.2.3rc1").map(|v| v.to_string()),
            Some("1.2.3-rc.1".to_owned())
        );
        assert_eq!(
            parse_pep440("1.2.3a2").map(|v| v.to_string()),
            Some("1.2.3-a.2".to_owned())
        );
        assert_eq!(
            parse_pep440("1.2.3b10").map(|v| v.to_string()),
            Some("1.2.3-b.10".to_owned())
        );
    }

    #[test]
    fn recognized_non_canonical_spellings_fail_closed() {
        for spelling in [
            "1!1.2.3",     // epoch
            "1.2.3.post1", // post-release
            "1.2.3.dev4",  // dev-release
            "1.2.3+local", // local segment
            "1.2",         // implicit release segment
            "1.2.3.4",     // four release components
            "01.2.3",      // leading zero (release)
            "1.2.3rc01",   // leading zero (pre-release number)
            "1.2.3rc",     // implicit pre-release number
            "1.2.3alpha1", // non-canonical alias
            "1.2.3c1",     // non-canonical alias
            "v1.2.3",      // preceding v
            "1.2.3 ",      // trailing whitespace is trimmed above — this
                           // one is "1.2.3" and must PASS; listed for clarity
        ] {
            let parsed = parse_pep440(spelling);
            if spelling == "1.2.3 " {
                assert!(parsed.is_some(), "trimmed edges are canonical");
            } else {
                assert!(parsed.is_none(), "{spelling} must fail closed");
            }
        }
    }

    #[test]
    fn render_maps_back_to_the_pep440_spelling() {
        for (pep, semver_text) in [
            ("1.2.3", "1.2.3"),
            ("1.2.3rc1", "1.2.3-rc.1"),
            ("1.2.3a2", "1.2.3-a.2"),
            ("1.2.3b10", "1.2.3-b.10"),
        ] {
            let version = Version::parse(semver_text).expect("fixture");
            assert_eq!(render_pep440(&version).as_deref(), Some(pep));
            // Lossless pairing: render ∘ parse = identity.
            assert_eq!(parse_pep440(pep).as_ref(), Some(&version));
        }
    }

    #[test]
    fn foreign_pre_release_identifiers_do_not_render() {
        let version = Version::parse("1.2.3-sha.abc1").expect("fixture");
        assert!(render_pep440(&version).is_none());
    }

    #[test]
    fn names_normalize_per_the_pypa_rule() {
        assert_eq!(normalize_name("bird-feeder"), "bird-feeder");
        assert_eq!(normalize_name("Bird_Feeder"), "bird-feeder");
        assert_eq!(normalize_name("BIRD.feeder"), "bird-feeder");
        assert_eq!(normalize_name("a...b__c--d"), "a-b-c-d");
    }
}

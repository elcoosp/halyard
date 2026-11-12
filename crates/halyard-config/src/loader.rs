//! The `halyard.toml` loader (M1.8): strict unknown-key detection with
//! did-you-mean suggestions, then the typed v1 parse.
//!
//! serde's default is to *ignore* unknown keys, which silently disables
//! whole sections on typos — so the loader first walks the raw TOML
//! tables against the frozen v1 key table, and only then hands the text
//! to serde. Both steps are deterministic (invariant I3).

use std::path::Path;

use serde::Deserialize;
use toml::Table;

use crate::error::ConfigError;
use crate::model::HalyardConfig;

/// Loads `halyard.toml` from `path` (path displayed as given, never
/// canonicalized, so diagnostics stay reproducible).
pub fn load(path: &Path) -> Result<HalyardConfig, ConfigError> {
    let text = std::fs::read_to_string(path).map_err(|source| ConfigError::Read {
        path: path.display().to_string(),
        message: source.to_string(),
    })?;
    parse_and_check(&text, &path.display().to_string())
}

/// Loads a configuration from its raw text (diagnostics carry the
/// conventional `halyard.toml` label).
pub fn load_from_str(src: &str) -> Result<HalyardConfig, ConfigError> {
    parse_and_check(src, "halyard.toml")
}

/// Parses, validates unknown keys (labelled with `label`), then
/// deserializes the typed v1 model.
fn parse_and_check(src: &str, label: &str) -> Result<HalyardConfig, ConfigError> {
    let table = toml::from_str::<Table>(src).map_err(|message| ConfigError::Read {
        path: label.to_owned(),
        message: message.to_string(),
    })?;
    check_unknown_keys(&table, "", label)?;
    HalyardConfig::deserialize(table).map_err(|message| ConfigError::Read {
        path: label.to_owned(),
        message: message.to_string(),
    })
}

/// Top-level keys of the frozen v1 schema.
const ROOT_KEYS: &[&str] = &[
    "repo",
    "versioning",
    "commits",
    "changelog",
    "publish",
    "forge",
    "package",
    "link",
];

/// Valid keys under a dotted path (`""` = root); `None` for a path that is
/// itself unknown (reported before descending) or has no nested table.
fn section_keys(path: &str) -> Option<&'static [&'static str]> {
    match path {
        "" => Some(ROOT_KEYS),
        "repo" => Some(&["base_branch", "tag_style"]),
        "versioning" => Some(&["policy", "pre_major", "cascade"]),
        "versioning.pre_major" => Some(&["breaking", "feat"]),
        "commits" => Some(&["types", "merge_policy", "initial_release"]),
        "commits.initial_release" => Some(&["max_commits"]),
        "changelog" => Some(&["style", "hidden_types"]),
        "publish" => Some(&["enabled", "cargo", "npm"]),
        "publish.cargo" => Some(&["registry", "wait_for_index"]),
        "publish.npm" => Some(&["access", "provenance"]),
        "forge" => Some(&["kind", "commit_mode"]),
        "package.element" => Some(&["name", "release", "include", "exclude"]),
        "link.element" => Some(&["from", "to", "mode"]),
        // `package` / `link` are array-of-tables: their elements are
        // validated against the `<path>.element` keys instead.
        _ => None,
    }
}

/// Recursively validates every table against [`section_keys`], reporting
/// the FIRST unknown key in deterministic (sorted) order.
fn check_unknown_keys(table: &Table, path: &str, label: &str) -> Result<(), ConfigError> {
    let keys: Vec<&String> = table.keys().collect();
    let known = section_keys(path);
    if let Some(known) = known {
        for key in keys {
            if !known.contains(&key.as_str()) {
                let suggestion = suggest(key, known);
                return Err(ConfigError::UnknownKey {
                    path: if path.is_empty() {
                        format!("{label} (root)")
                    } else {
                        format!("{label} in `{path}`")
                    },
                    key: key.clone(),
                    suggestion: suggestion.map(str::to_owned),
                    valid: known.iter().map(|k| (*k).to_owned()).collect(),
                });
            }
        }
    }
    // Recurse into nested tables (sorted for determinism).
    for (key, value) in table {
        let child_path = if path.is_empty() {
            key.clone()
        } else {
            format!("{path}.{key}")
        };
        match value {
            toml::Value::Table(nested) => check_unknown_keys(nested, &child_path, label)?,
            toml::Value::Array(items) => {
                // Array-of-tables (`[[package]]`, `[[link]]`): the parent
                // path has no key table; each element is validated against
                // `<path>.element`.
                for (index, item) in items.iter().enumerate() {
                    if let toml::Value::Table(element) = item {
                        let element_path = format!("{child_path}[{index}]");
                        if section_keys(&child_path).is_none() {
                            check_element(element, &element_path, child_path.as_str(), label)?;
                        } else {
                            check_unknown_keys(element, &element_path, label)?;
                        }
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// Validates one array-of-tables element against its schema keys.
fn check_element(
    element: &Table,
    element_path: &str,
    parent_path: &str,
    label: &str,
) -> Result<(), ConfigError> {
    let known = if parent_path == "package" {
        section_keys("package.element")
    } else {
        section_keys("link.element")
    };
    let Some(known) = known else {
        return Ok(());
    };
    for key in element.keys() {
        if !known.contains(&key.as_str()) {
            let suggestion = suggest(key, known);
            return Err(ConfigError::UnknownKey {
                path: format!("{label} in `{element_path}`"),
                key: key.clone(),
                suggestion: suggestion.map(str::to_owned),
                valid: known.iter().map(|k| (*k).to_owned()).collect(),
            });
        }
    }
    Ok(())
}

/// Classic edit-distance (Levenshtein) over `char`s.
fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut current = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            let substitution = previous.get(j).copied().unwrap_or(0) + usize::from(ca != cb);
            let deletion = previous.get(j + 1).copied().unwrap_or(0).saturating_add(1);
            let insertion = current.last().copied().unwrap_or(0).saturating_add(1);
            current.push(substitution.min(deletion).min(insertion));
        }
        previous = current;
    }
    previous.last().copied().unwrap_or(usize::MAX)
}

/// The closest candidate within edit distance 2; ties resolve to the
/// lexicographically smallest candidate (deterministic diagnostics).
pub(crate) fn suggest<'a>(key: &str, candidates: &[&'a str]) -> Option<&'a str> {
    let mut best: Option<(usize, &str)> = None;
    for candidate in candidates {
        let distance = levenshtein(key, candidate);
        if distance > 2 {
            continue;
        }
        let replace = match best {
            None => true,
            Some((best_distance, best_candidate)) => {
                distance < best_distance
                    || (distance == best_distance && *candidate < best_candidate)
            }
        };
        if replace {
            best = Some((distance, candidate));
        }
    }
    best.map(|(_, candidate)| candidate)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levenshtein_basics() {
        assert_eq!(levenshtein("repo", "repo"), 0);
        assert_eq!(levenshtein("repoz", "repo"), 1);
        assert_eq!(levenshtein("base_branh", "base_branch"), 1);
        assert_eq!(levenshtein("zzzzzzzz", "repo"), 8);
    }

    #[test]
    fn suggestion_prefers_closest_then_lexicographic() {
        let candidates = ["style", "types", "enabled"];
        assert_eq!(suggest("type", &candidates), Some("types"));
        assert_eq!(suggest("zzzz", &candidates), None);
        let tied = ["alpha", "alpso"];
        assert_eq!(suggest("alpsa", &tied), Some("alpha"));
    }
}

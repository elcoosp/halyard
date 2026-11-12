//! Deterministic serialization and hashing helpers (§5.8, invariant I3).
//!
//! The [`Plan`] contract is hashable: `plan_hash` is blake3 over the
//! *canonical JSON* of the plan, so the same content always produces the
//! same bytes regardless of how the value was constructed. Canonical JSON
//! here means: object keys sorted lexicographically, no insignificant
//! whitespace, strings escaped per RFC 8785 (`\b`, `\f`, `\n`, `\r`, `\t`
//! short forms; other control characters as `\uXXXX` with lowercase hex).
//! UTF-8 non-ASCII characters are emitted as-is (they are already
//! deterministic).

use std::fmt::Write as _;

use serde_json::Value;

use crate::plan::Plan;

/// Serializes `value` as canonical JSON (see module docs for the exact
/// grammar). Arrays keep their order; object keys are sorted; nested values
/// recurse.
pub fn canonical_json(value: &Value) -> String {
    match value {
        Value::Null => "null".to_owned(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => escape_string(s),
        Value::Array(items) => {
            let parts: Vec<String> = items.iter().map(canonical_json).collect();
            format!("[{}]", parts.join(","))
        }
        Value::Object(map) => {
            // `serde_json` maps are `BTreeMap` unless `preserve_order` is
            // enabled by *some* dependency; sorting explicitly keeps the
            // output canonical regardless of that feature flag.
            let mut keys: Vec<&str> = map.keys().map(String::as_str).collect();
            keys.sort_unstable();
            let parts: Vec<String> = keys
                .iter()
                .filter_map(|k| {
                    map.get(*k)
                        .map(|v| format!("{}:{}", escape_string(k), canonical_json(v)))
                })
                .collect();
            format!("{{{}}}", parts.join(","))
        }
    }
}

/// Escapes a JSON string per RFC 8785: the short escape forms for `\b`,
/// `\f`, `\n`, `\r`, `\t`, `\uXXXX` (lowercase hex) for other control
/// characters, and literal UTF-8 for everything else.
fn escape_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// blake3 digest of `bytes`, lowercase hex.
pub fn blake3_hex(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

/// The `blake3:<64 hex>` hash of a plan (§5.8): blake3 over the canonical
/// JSON of the plan with the `plan_hash` field scrubbed to the empty
/// string, so the hash cannot cover itself.
///
/// Serialization of the frozen v1 wire types cannot fail (no floats, no
/// non-string map keys); should it ever fail, the hash of the empty string
/// is returned deterministically rather than panicking — the determinism
/// tests pin the real path.
pub fn plan_hash(plan: &Plan) -> String {
    let Ok(value) = serde_json::to_value(plan) else {
        return format!("blake3:{}", blake3_hex(&[]));
    };
    let mut value = value;
    if let Some(obj) = value.as_object_mut() {
        obj.insert("plan_hash".to_owned(), Value::String(String::new()));
    }
    format!("blake3:{}", blake3_hex(canonical_json(&value).as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_string_handles_rfc8785_cases() {
        assert_eq!(escape_string("a\"b"), "\"a\\\"b\"");
        assert_eq!(escape_string("a\\b"), "\"a\\\\b\"");
        assert_eq!(escape_string("a\u{8}b"), "\"a\\bb\"");
        assert_eq!(escape_string("a\u{c}b"), "\"a\\fb\"");
        assert_eq!(escape_string("a\u{1}b"), "\"a\\u0001b\"");
        assert_eq!(escape_string("mé"), "\"mé\"");
    }

    #[test]
    fn canonical_json_scalars() {
        assert_eq!(canonical_json(&Value::Null), "null");
        assert_eq!(canonical_json(&Value::Bool(true)), "true");
        assert_eq!(
            canonical_json(&serde_json::from_str::<Value>("42").unwrap_or_default()),
            "42"
        );
    }
}

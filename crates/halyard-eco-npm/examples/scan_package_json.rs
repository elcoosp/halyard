//! Example: scan a `package.json` with spans and edit one version
//! losslessly (M1.6, ADR-0001).
//!
//! Run: `cargo run -p halyard-eco-npm --example scan_package_json`
//!
//! The edit replaces only the inner span of the version string; every
//! untouched byte (indentation, key order, trailing newline) survives —
//! invariant I4.

use halyard_eco_npm::json_span::{JsonDoc, JsonValue};

const SRC: &str = r#"{
  "name": "@scope/engine",
  "version": "1.2.3",
  "dependencies": {
    "@scope/core": "workspace:^"
  }
}
"#;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut doc = JsonDoc::parse(SRC.as_bytes())?;
    // Show the spans of the top-level fields.
    if let JsonValue::Object(entries) = &doc.root().value {
        for entry in entries {
            println!(
                "entry {:?} at bytes {}..{}",
                entry.key, entry.value.span.start, entry.value.span.end
            );
        }
    }
    // Lossless edit: only the version's inner span changes.
    let span = doc.set_string(&["version"], "1.3.0")?;
    println!(
        "patched inner span {}..{} -> \"1.3.0\"",
        span.start, span.end
    );
    println!("--- rendered ---");
    print!("{}", String::from_utf8(doc.render())?);
    Ok(())
}

//! Snapshot tests for the npm span scanner output (M1.6, §15.1 golden
//! layer): the lossless render is pinned byte-for-byte.

use halyard_eco_npm::json_span::JsonDoc;

#[test]
fn render_is_byte_identical_without_edits() {
    let src = "{\n  \"name\": \"@scope/engine\",\n  \"version\": \"1.2.3\",\n  \"deps\": {\n    \"a\": \"workspace:^\"\n  }\n}\n";
    let doc = JsonDoc::parse(src.as_bytes()).expect("parses");
    insta::assert_snapshot!(String::from_utf8(doc.render()).unwrap(), @r#"
    {
      "name": "@scope/engine",
      "version": "1.2.3",
      "deps": {
        "a": "workspace:^"
      }
    }
    "#);
}

#[test]
fn single_field_edit_touches_only_that_span() {
    let src = "{\n  \"name\": \"@scope/engine\",\n  \"version\": \"1.2.3\"\n}\n";
    let mut doc = JsonDoc::parse(src.as_bytes()).expect("parses");
    doc.set_string(&["version"], "1.3.0").expect("edit");
    insta::assert_snapshot!(String::from_utf8(doc.render()).unwrap(), @r#"
    {
      "name": "@scope/engine",
      "version": "1.3.0"
    }
    "#);
}

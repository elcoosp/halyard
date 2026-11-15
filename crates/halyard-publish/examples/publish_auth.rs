//! The publish-auth plumbing (M5.3 step 1, ADR-0019) over an injected
//! table — the same [`EnvPublishAuth`] the shell captures from the
//! process environment, exercised offline so no token is ever needed
//! (and none is printed: the example prints provenance — variable
//! names — never values, the seam's own convention).
//!
//! Run: `cargo run -p halyard-publish --example publish_auth`

use std::collections::BTreeMap;

use halyard_publish::{
    AuthOutcome, EnvPublishAuth, PublishAuth, PublishTarget, scrub_secrets, variables_for,
};

fn main() {
    // A capture-shaped table (injected here; the shell's
    // `EnvPublishAuth::capture()` fills it from the process
    // environment — same data, same rules, no env needed offline).
    let table: BTreeMap<String, String> = [
        ("CARGO_REGISTRY_TOKEN", "czp+example-cargo-token"),
        ("NODE_AUTH_TOKEN", "example-node-token"),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_owned(), value.to_owned()))
    .collect();
    let auth = EnvPublishAuth::from_table(table);

    // The grammar is data: the documented candidates per target, in
    // resolution order.
    let cargo = PublishTarget {
        package: "halyard-demo".to_owned(),
        version: "0.1.0".to_owned(),
        ecosystem: "cargo".to_owned(),
        registry: "crates.io".to_owned(),
        manifest_dir: "crates/demo".to_owned(),
        manifest: "crates/demo/Cargo.toml".to_owned(),
    };
    let npm = PublishTarget {
        package: "demo-pkg".to_owned(),
        version: "1.0.0".to_owned(),
        ecosystem: "npm".to_owned(),
        registry: "npm".to_owned(),
        manifest_dir: "packages/demo".to_owned(),
        manifest: "packages/demo/package.json".to_owned(),
    };
    for target in [&cargo, &npm] {
        println!(
            "{} ({}): candidates {:?}",
            target.registry,
            target.ecosystem,
            variables_for(target)
        );
        match PublishAuth::credentials(&auth, target) {
            // Display is provenance-only: `env:VAR_NAME`.
            Ok(AuthOutcome::Token(credential)) => println!(
                "  → inject {credential} (the value is scrubbed from tool output)"
            ),
            Ok(AuthOutcome::Absent) => {
                println!("  → nothing injected (the tool's own credential store governs)");
            }
            Err(error) => println!("  → refused: {error}"),
        }
    }

    // The scrubbing contract: every resolved value is listed by
    // `secrets()` and replaced by the `[secret]` marker in surfaced
    // tool output — a token a misconfigured tool echoes never reaches
    // a report row.
    let secrets = PublishAuth::secrets(&auth);
    println!("scrub list carries {} resolved value(s)", secrets.len());
    let echoed_tool_error = format!(
        "error: upload rejected — token {} is not authorized for this scope",
        PublishAuth::credentials(&auth, &cargo)
            .ok()
            .and_then(|outcome| match outcome {
                AuthOutcome::Token(credential) => Some(credential.value),
                AuthOutcome::Absent => None,
            })
            .unwrap_or_default()
    );
    let refs: Vec<&str> = secrets.iter().map(String::as_str).collect();
    println!("tool said:   {echoed_tool_error}");
    println!("report sees: {}", scrub_secrets(&echoed_tool_error, &refs));
}

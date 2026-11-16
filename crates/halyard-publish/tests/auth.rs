//! The publish-auth contract (M5.3 step 1, ADR-0019): the
//! [`PublishAuth`] seam that resolves a publish target's registry
//! credentials into an injectable env pair, the [`EnvPublishAuth`]
//! production resolution over the documented environment-variable
//! grammar (VERIFIED: Cargo Book + pnpm npmrc docs), the fail-closed
//! `E-PUB-AUTH` for a present-but-empty token, and the
//! [`scrub_secrets`] helper that keeps resolved secrets out of surfaced
//! tool output. Written first (red on the unresolved `halyard_publish`
//! auth API), implemented second.
//!
//! No test mutates the process environment (edition-2024 `set_var` is
//! unsafe and parallel tests would race): [`EnvPublishAuth::
//! from_table`] injects the table, and the two `capture` tests only
//! pin snapshot determinism.

use std::collections::BTreeMap;

use halyard_publish::{
    AuthOutcome, CredentialEnv, EnvPublishAuth, PublishAuth, PublishError, PublishTarget,
    auth_summary, scrub_secrets, variables_for,
};

/// One cargo publish target on crates.io.
fn crates_io() -> PublishTarget {
    PublishTarget {
        package: "p1".to_owned(),
        version: "0.2.0".to_owned(),
        ecosystem: "cargo".to_owned(),
        registry: "crates.io".to_owned(),
        manifest_dir: "crates/p1".to_owned(),
        manifest: "crates/p1/Cargo.toml".to_owned(),
        access: None,
        provenance: None,
    }
}

/// One npm publish target (the pnpm spawn, §13.4).
fn npm() -> PublishTarget {
    PublishTarget {
        package: "pkg".to_owned(),
        version: "1.1.0".to_owned(),
        ecosystem: "npm".to_owned(),
        registry: "npm".to_owned(),
        manifest_dir: "packages/pkg".to_owned(),
        manifest: "packages/pkg/package.json".to_owned(),
        access: None,
        provenance: None,
    }
}

fn table(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
    entries
        .iter()
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect()
}

#[test]
fn crates_io_resolves_the_documented_variable() {
    let auth = EnvPublishAuth::from_table(table(&[("CARGO_REGISTRY_TOKEN", "czp+abc123")]));
    let outcome = auth
        .credentials(&crates_io())
        .unwrap_or_else(|e| panic!("resolves: {e}"));
    let AuthOutcome::Token(credential) = outcome else {
        panic!("crates.io with CARGO_REGISTRY_TOKEN set must resolve a token");
    };
    assert_eq!(credential.key, "CARGO_REGISTRY_TOKEN");
    assert_eq!(credential.value, "czp+abc123");
}

#[test]
fn custom_cargo_registry_maps_punctuation_to_underscores() {
    // The Cargo config rule: NAME is the registry name uppercased with
    // dots and dashes converted to underscores (VERIFIED, Cargo Book
    // config reference: "Keys are converted to uppercase, dots and
    // dashes are converted to underscores").
    let auth =
        EnvPublishAuth::from_table(table(&[("CARGO_REGISTRIES_MY_CORPS_INDEX_TOKEN", "tok")]));
    let target = PublishTarget {
        registry: "my-corps.index".to_owned(),
        ..crates_io()
    };
    let outcome = auth
        .credentials(&target)
        .unwrap_or_else(|e| panic!("resolves: {e}"));
    let AuthOutcome::Token(credential) = outcome else {
        panic!("the mapped variable resolves");
    };
    assert_eq!(credential.key, "CARGO_REGISTRIES_MY_CORPS_INDEX_TOKEN");
}

#[test]
fn crates_io_falls_back_to_the_registries_form() {
    // crates.io's canonical variable is singular CARGO_REGISTRY_TOKEN;
    // the general registries.<name> mapping (registries.crates-io.token
    // → CARGO_REGISTRIES_CRATES_IO_TOKEN) is the documented fallback.
    let auth = EnvPublishAuth::from_table(table(&[("CARGO_REGISTRIES_CRATES_IO_TOKEN", "tok")]));
    let outcome = auth
        .credentials(&crates_io())
        .unwrap_or_else(|e| panic!("resolves: {e}"));
    let AuthOutcome::Token(credential) = outcome else {
        panic!("the registries-form fallback resolves");
    };
    assert_eq!(credential.key, "CARGO_REGISTRIES_CRATES_IO_TOKEN");
}

#[test]
fn npm_resolves_the_documented_candidates_in_order() {
    // NPM_TOKEN is the canonical .npmrc `${NPM_TOKEN}` interpolation
    // candidate; NODE_AUTH_TOKEN is the actions/setup-node convention.
    // Documented order first: both present ⇒ NPM_TOKEN.
    let both = EnvPublishAuth::from_table(table(&[
        ("NPM_TOKEN", "npm-value"),
        ("NODE_AUTH_TOKEN", "node-value"),
    ]));
    let AuthOutcome::Token(credential) = both
        .credentials(&npm())
        .unwrap_or_else(|e| panic!("resolves: {e}"))
    else {
        panic!("npm with candidates set resolves");
    };
    assert_eq!(credential.key, "NPM_TOKEN");

    let only_node = EnvPublishAuth::from_table(table(&[("NODE_AUTH_TOKEN", "node-value")]));
    let AuthOutcome::Token(credential) = only_node
        .credentials(&npm())
        .unwrap_or_else(|e| panic!("resolves: {e}"))
    else {
        panic!("the setup-node candidate resolves");
    };
    assert_eq!(credential.key, "NODE_AUTH_TOKEN");
}

#[test]
fn absent_credentials_are_a_value_not_an_error() {
    // Absent is honest: the tool's own credential store (cargo login,
    // the user-level npmrc) governs — halyard cannot see it, so it
    // reports nothing injected instead of guessing.
    let auth = EnvPublishAuth::from_table(BTreeMap::new());
    let outcome = auth
        .credentials(&crates_io())
        .unwrap_or_else(|e| panic!("absent: {e}"));
    assert_eq!(outcome, AuthOutcome::Absent);
    let outcome = auth
        .credentials(&npm())
        .unwrap_or_else(|e| panic!("absent: {e}"));
    assert_eq!(outcome, AuthOutcome::Absent);
}

#[test]
fn present_but_empty_token_fails_closed_naming_the_variable() {
    // An empty token env var is a misconfiguration: with a `${VAR}`
    // auth-file interpolation it becomes an empty bearer token and a
    // confusing 401. The seam refuses before the spawn with E-PUB-AUTH
    // naming the variable.
    let auth = EnvPublishAuth::from_table(table(&[("CARGO_REGISTRY_TOKEN", "")]));
    let error = auth
        .credentials(&crates_io())
        .expect_err("an empty token must fail closed");
    assert_eq!(error.code(), "E-PUB-AUTH");
    assert!(error.to_string().contains("CARGO_REGISTRY_TOKEN"));
    assert!(!error.retryable(), "a misconfiguration is structural");
}

#[test]
fn unknown_ecosystem_is_an_honest_absent() {
    // No invented grammar: an ecosystem the documented table does not
    // cover resolves to Absent (the tool's own credentials govern).
    let auth = EnvPublishAuth::from_table(table(&[("PIP_PASSWORD", "unused")]));
    let target = PublishTarget {
        ecosystem: "python".to_owned(),
        registry: "pypi".to_owned(),
        ..crates_io()
    };
    let outcome = auth
        .credentials(&target)
        .unwrap_or_else(|e| panic!("absent: {e}"));
    assert_eq!(outcome, AuthOutcome::Absent);
}

#[test]
fn credential_debug_redacts_the_value() {
    let credential = CredentialEnv {
        key: "CARGO_REGISTRY_TOKEN".to_owned(),
        value: "super-secret-value".to_owned(),
    };
    let rendered = format!("{credential:?}");
    assert!(
        !rendered.contains("super-secret-value"),
        "Debug must redact the token value: {rendered}"
    );
    assert!(
        rendered.contains("CARGO_REGISTRY_TOKEN"),
        "the name shows: {rendered}"
    );

    let outcome = AuthOutcome::Token(credential);
    assert!(
        !format!("{outcome:?}").contains("super-secret-value"),
        "the outcome Debug redacts too"
    );

    let auth = EnvPublishAuth::from_table(table(&[("NPM_TOKEN", "npm-secret-value")]));
    assert!(
        !format!("{auth:?}").contains("npm-secret-value"),
        "the provider Debug redacts the resolved table"
    );
}

#[test]
fn provider_exposes_resolved_secrets_for_scrubbing() {
    // The scrubbing contract: every resolved token value is listed by
    // `secrets()` (sorted by variable name — deterministic), and
    // credentials only ever resolve values the list contains.
    let auth = EnvPublishAuth::from_table(table(&[
        ("NPM_TOKEN", "npm-value"),
        ("CARGO_REGISTRY_TOKEN", "cargo-value"),
    ]));
    assert_eq!(auth.secrets(), vec!["cargo-value", "npm-value"]);
}

#[test]
fn scrub_replaces_every_occurrence_of_every_secret() {
    let text = "error: token cargo-value rejected for cargo-value upload; npm-value ok";
    let scrubbed = scrub_secrets(text, &["cargo-value", "npm-value"]);
    assert!(
        !scrubbed.contains("cargo-value"),
        "every occurrence goes: {scrubbed}"
    );
    assert!(
        !scrubbed.contains("npm-value"),
        "every secret goes: {scrubbed}"
    );
    assert_eq!(
        scrubbed,
        "error: token [secret] rejected for [secret] upload; [secret] ok"
    );
}

#[test]
fn scrub_without_secrets_is_identity() {
    let text = "error: upstream returned 502";
    assert_eq!(scrub_secrets(text, &[]), text);
    // Empty strings never match anything, so they are inert.
    assert_eq!(scrub_secrets(text, &[""]), text);
}

#[test]
fn auth_error_carries_the_stable_code() {
    let error = PublishError::Auth {
        registry: "crates.io".to_owned(),
        message: "CARGO_REGISTRY_TOKEN is set but empty".to_owned(),
    };
    assert_eq!(error.code(), "E-PUB-AUTH");
    assert!(error.to_string().starts_with("[E-PUB-AUTH]"));
    assert!(
        !error.retryable(),
        "structural: a re-run cannot fix the env"
    );
    assert_eq!(
        error,
        error.clone(),
        "clone equality (the row-report shape)"
    );
}

#[test]
fn provider_is_object_safe_and_shareable_across_threads() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<EnvPublishAuth>();
    // The engine's parallel path holds `&dyn PublishAuth` in scoped
    // threads — the object-safety pin.
    let auth = EnvPublishAuth::from_table(table(&[("NPM_TOKEN", "v")]));
    let dynamic: &dyn PublishAuth = &auth;
    let outcome = dynamic
        .credentials(&npm())
        .unwrap_or_else(|e| panic!("resolves: {e}"));
    assert!(matches!(outcome, AuthOutcome::Token(_)));
}

#[test]
fn capture_snapshot_is_deterministic() {
    // The production capture reads the process env once into a table;
    // two captures in the same environment agree (I3) and the capture
    // never panics on an empty environment.
    let first = EnvPublishAuth::capture().unwrap_or_else(|e| panic!("capture: {e}"));
    let second = EnvPublishAuth::capture().unwrap_or_else(|e| panic!("capture: {e}"));
    assert_eq!(first, second);
}

#[test]
fn variables_for_pins_the_grammar_table() {
    // The doctor's publish-auth check consumes the same table — the
    // grammar is data, not scattered string literals.
    let cargo_custom = PublishTarget {
        registry: "my-corps".to_owned(),
        ..crates_io()
    };
    assert_eq!(
        variables_for(&crates_io()),
        vec!["CARGO_REGISTRY_TOKEN", "CARGO_REGISTRIES_CRATES_IO_TOKEN"]
    );
    assert_eq!(
        variables_for(&cargo_custom),
        vec!["CARGO_REGISTRIES_MY_CORPS_TOKEN"]
    );
    assert_eq!(variables_for(&npm()), vec!["NPM_TOKEN", "NODE_AUTH_TOKEN"]);
}

#[test]
fn capture_resolves_the_documented_table_shape() {
    // capture() over the test process env (no token vars set in CI —
    // and none may be asserted here): the resolved set agrees with
    // secrets(), and nothing captured is empty.
    let auth = EnvPublishAuth::capture().unwrap_or_else(|e| panic!("capture: {e}"));
    let secrets = auth.secrets();
    for (name, value) in auth.table() {
        assert!(
            secrets.contains(value),
            "every resolved value is scrub-listed: {name}"
        );
        assert!(
            !value.is_empty(),
            "capture never stores an empty token: {name}"
        );
    }
}

#[test]
fn auth_summary_maps_registry_to_variable_names_only() {
    // The renderers' input: registry → `env:VARIABLE` provenance (names
    // only) or `none`; the map is name-sorted (BTreeMap, I3), and no
    // value ever appears.
    let auth = EnvPublishAuth::from_table(table(&[
        ("CARGO_REGISTRY_TOKEN", "cargo-value"),
        ("NPM_TOKEN", "npm-value"),
    ]));
    let targets = vec![crates_io(), npm()];
    let summary = auth_summary(&auth, &targets).unwrap_or_else(|e| panic!("summary: {e}"));
    assert_eq!(
        summary.get("crates.io").map(String::as_str),
        Some("env:CARGO_REGISTRY_TOKEN")
    );
    assert_eq!(
        summary.get("npm").map(String::as_str),
        Some("env:NPM_TOKEN")
    );
    assert!(
        !format!("{summary:?}").contains("cargo-value"),
        "no value rides"
    );
    assert_eq!(summary.len(), 2);
}

#[test]
fn auth_summary_reports_none_when_nothing_resolves() {
    // Absence is honest provenance: `none` (the tool's own credential
    // store governs), never an error, never a guessed variable.
    let auth = EnvPublishAuth::from_table(BTreeMap::new());
    let summary = auth_summary(&auth, &[crates_io()]).unwrap_or_else(|e| panic!("summary: {e}"));
    assert_eq!(summary.get("crates.io").map(String::as_str), Some("none"));
}

#[test]
fn auth_summary_refuses_a_misconfiguration() {
    // A present-but-empty token is the seam's E-PUB-AUTH — a summary
    // never coexists with a misconfiguration (the run refuses before
    // any spawn).
    let auth = EnvPublishAuth::from_table(table(&[("NPM_TOKEN", "")]));
    let error = auth_summary(&auth, &[npm()]).expect_err("empty token refuses");
    assert_eq!(error.code(), "E-PUB-AUTH");
}

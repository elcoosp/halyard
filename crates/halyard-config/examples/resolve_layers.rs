//! Layered config resolution demo (M1.8): defaults < file < env < CLI,
//! with did-you-mean diagnostics and the deterministic `config_hash`.
//!
//! Run: `cargo run -p halyard-config --example resolve_layers`

use std::error::Error;

use halyard_config::{ConfigError, Layer, config_hash, env_layer_from, load_from_str, resolve};

fn main() -> Result<(), Box<dyn Error>> {
    let file = load_from_str(
        r#"
[repo]
base_branch = "develop"

[versioning]
cascade = "always"

[commits.types]
docs = "patch"
"#,
    )
    .map_err(|e| format!("file layer: {e}"))?;

    // The env layer: injectable lookup (the real path reads `HALYARD_*`).
    let env = env_layer_from(&|var| match var {
        "HALYARD_BASE_BRANCH" => Some("hotfix".to_owned()),
        _ => None,
    })
    .map_err(|e| format!("env layer: {e}"))?;

    let resolved = resolve(&[Layer::Defaults, Layer::File(file), Layer::Env(env.clone())])
        .map_err(|e| format!("resolve: {e}"))?;
    println!("base branch: {:?}", resolved.base_branch);
    println!("cascade:     {:?}", resolved.cascade);
    println!(
        "commit types: feat={:?} fix={:?} docs={:?}",
        resolved.commit_types.get("feat"),
        resolved.commit_types.get("fix"),
        resolved.commit_types.get("docs")
    );
    println!("config hash: {}", config_hash(&resolved));

    // Determinism: same inputs ⇒ same hash byte-for-byte (I3).
    let again = resolve(&[
        Layer::Defaults,
        Layer::File(
            load_from_str(
                "[repo]\nbase_branch = \"develop\"\n\n[versioning]\ncascade = \"always\"\n\n\
             [commits.types]\ndocs = \"patch\"\n",
            )
            .map_err(|e| format!("file layer: {e}"))?,
        ),
        Layer::Env(env),
    ])
    .map_err(|e| format!("resolve: {e}"))?;
    assert_eq!(config_hash(&resolved), config_hash(&again));
    println!("re-run hash: {} (identical)", config_hash(&again));

    // Diagnostics teach: an unknown key names the closest valid one.
    if let Some(err) = load_from_str("[repo]\nbase_branh = \"x\"\n").err() {
        println!("diagnostic:  {err}");
        let code = match err {
            ConfigError::UnknownKey { .. } => ConfigError::UNKNOWN_KEY_CODE,
            _ => err.code(),
        };
        println!("code:        {code}");
    }
    Ok(())
}

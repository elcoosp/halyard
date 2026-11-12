//! HALYARD configuration model (`halyard.toml`, §16.4 of
//! `docs/HALYARD_ARCHITECTURE.md`).
//!
//! Every key is optional; the types here carry serde defaults that mirror the
//! documented built-in defaults. Layered resolution (built-in defaults <
//! `halyard.toml` < `package.metadata.halyard` / `package.json#halyard` < env
//! `HALYARD_*` < CLI flags) and `did-you-mean` diagnostics for unknown keys
//! land with the config loader (`M1.8`); these types are the frozen v1
//! surface from which `schemas/halyard-config-v1.json` is generated.

pub mod model;

pub use model::HalyardConfig;

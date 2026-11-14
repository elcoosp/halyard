//! The GitHub App credentials (M4.1c, §11.5, ADR-0014): the
//! RS256 JWT mint and the credential vocabulary the adapter resolves to
//! a bearer. Wire shapes per the §7 verification section of
//! `docs/research/github-rest-forge.md`:
//!
//! * The JWT is signed **RS256**; the header is `{"alg":"RS256","typ":"JWT"}`;
//!   the claims ride `iat` **60 s in the past** (the documented clock-drift
//!   guard) and `exp` **10 minutes out** (the documented maximum window);
//!   `iss` is the App ID (the docs also accept the client ID — both are
//!   non-empty strings here and ride verbatim).
//! * The private key is the App's PEM (GitHub downloads PKCS#1
//!   `BEGIN RSA PRIVATE KEY`; PKCS#8 `BEGIN PRIVATE KEY` is accepted
//!   equivalently). PEM → DER is decoded here, PKCS#1 bodies are wrapped
//!   into the PKCS#8 `PrivateKeyInfo` shape `ring` consumes, and
//!   `ring::signature::RsaKeyPair` is the signer — already in the
//!   runtime graph through rustls, no build scripts, no new dependency
//!   beyond the direct edge (ADR-0014).
//! * The JWT authenticates exactly one exchange: the
//!   `POST /app/installations/{id}/access_tokens` mint, whose
//!   `{ "token": … }` body is the installation token the adapter rides
//!   until the conservative 45-minute bound
//!   ([`INSTALLATION_TOKEN_TTL_SECS`]) expires it (the token's own
//!   lifetime is one hour; our bound re-mints 15 minutes early and
//!   deliberately does not parse the wire's ISO-8601 `expires_at`).
//!
//! No wall clock lives here: [`AppCredentials::mint_jwt`] takes `now`
//! from the caller, and the adapter injects its clock ([`EpochClock`]) —
//! determinism (I3) is testable, and the adapter's auth state is a pure
//! function of the calls it made.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use ring::rand::SystemRandom;
use ring::signature::RsaKeyPair;

use halyard_forge::error::ForgeError;

/// The conservative re-mint bound on an installation token (§7
/// verification: the token's lifetime is one hour; 45 minutes keeps a
/// 15-minute margin without parsing the wire's `expires_at` timestamp).
pub const INSTALLATION_TOKEN_TTL_SECS: u64 = 2_700;

/// The JWT window: `iat` rides this many seconds in the past (the
/// documented clock-drift guard).
const JWT_IAT_DRIFT_SECS: u64 = 60;

/// The JWT window: `exp` rides this many seconds out — the documented
/// 10-minute maximum.
const JWT_WINDOW_SECS: u64 = 600;

/// The wall-clock seam for the adapter's token-expiry state: seconds
/// since the Unix epoch. The default reads the system clock; tests and
/// the `app_exchange` example inject fixed values (determinism, I3 —
/// the auth state is a pure function of the injected clock).
pub type EpochClock = Arc<dyn Fn() -> u64 + Send + Sync>;

/// The system clock as seconds since the epoch (the adapter's default
/// [`EpochClock`]).
#[must_use]
pub fn system_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

/// The validated GitHub App credential bundle: the App ID (the JWT's
/// `iss`), the installation to mint tokens for and the RSA signer
/// parsed from the App's private-key PEM. Constructed eagerly and
/// fail-closed ([`AppCredentials::from_pem`]): an unusable key is an
/// `E-FORGE-AUTH` before any call, mirroring the adapter's
/// anonymous-token contract.
pub struct AppCredentials {
    app_id: String,
    installation_id: u64,
    key: RsaKeyPair,
    /// The key's public `(n, e)` in the canonical big-endian
    /// no-leading-zeros encoding, captured at construction (`ring`
    /// keeps the signer's public side opaque in 0.17).
    public: (Vec<u8>, Vec<u8>),
}

impl AppCredentials {
    /// Parses the App's private-key PEM (`PKCS#1 "BEGIN RSA PRIVATE
    /// KEY"` or `PKCS#8 "BEGIN PRIVATE KEY"`), validates it as a usable
    /// RSA signer and returns the credential bundle.
    ///
    /// # Errors
    ///
    /// [`ForgeError::Auth`] (`E-FORGE-AUTH`) when the app id is empty,
    /// the installation id is zero, the PEM is not one of the two
    /// supported shapes, the DER does not parse as an RSA private key,
    /// or `ring` rejects the key — always before any network call.
    pub fn from_pem(app_id: &str, pem: &str, installation_id: u64) -> Result<Self, ForgeError> {
        let app_id = app_id.trim().to_owned();
        if app_id.is_empty() {
            return Err(ForgeError::auth(
                "the GitHub App id is empty: set HALYARD_APP_ID (or the client ID) — the \
                 JWT's `iss` claim must name the app",
            ));
        }
        if installation_id == 0 {
            return Err(ForgeError::auth(
                "the GitHub App installation id must be positive: set \
                 HALYARD_APP_INSTALLATION_ID to the id the app was installed under",
            ));
        }
        let (label, der) = pem_section(pem).ok_or_else(|| {
            ForgeError::auth(
                "the GitHub App private key is not a PEM block: expected a \
                 -----BEGIN RSA PRIVATE KEY----- (PKCS#1) or -----BEGIN PRIVATE KEY----- \
                 (PKCS#8) section",
            )
        })?;
        // Both shapes normalize to (pkcs8_for_ring, pkcs1_body): the
        // PKCS#1 body is validated in full (the nine-INTEGER
        // RSAPrivateKey structure) and yields the public components,
        // whatever the armor said.
        let (pkcs8, pkcs1) = match label.as_str() {
            "RSA PRIVATE KEY" => (pkcs1_to_pkcs8(&der)?, der),
            // `ring` consumes PKCS#8 verbatim; the inner OCTET STRING
            // carries the PKCS#1 body.
            "PRIVATE KEY" => {
                let pkcs1 = pkcs8_inner_key(&der)?;
                (der, pkcs1)
            }
            other => {
                return Err(ForgeError::auth(format!(
                    "the GitHub App private key PEM carries the label {other:?}; expected \
                     RSA PRIVATE KEY (PKCS#1) or PRIVATE KEY (PKCS#8)"
                )));
            }
        };
        let fields = pkcs1_components(&pkcs1)?;
        // fields[0] is the version (validated zero); n and e follow,
        // canonicalized to the big-endian no-leading-zeros encoding
        // the JWT verification consumes.
        let n = canonical_integer(fields.get(1).ok_or_else(unusable_key)?);
        let e = canonical_integer(fields.get(2).ok_or_else(unusable_key)?);
        let key = RsaKeyPair::from_pkcs8(&pkcs8).map_err(|_| {
            ForgeError::auth(
                "the GitHub App private key is not a usable RSA key (ring rejected the \
                 parsed DER): re-download the App's key or regenerate it",
            )
        })?;
        Ok(Self {
            app_id,
            installation_id,
            key,
            public: (n, e),
        })
    }

    /// The App ID — the JWT's `iss` claim (App ID or client ID; the
    /// documented verifier accepts both).
    #[must_use]
    pub fn app_id(&self) -> &str {
        &self.app_id
    }

    /// The installation the adapter mints tokens for.
    #[must_use]
    pub fn installation_id(&self) -> u64 {
        self.installation_id
    }

    /// The RSA public components `(n, e)` — big-endian, no leading
    /// zeros, captured at construction. The conformance-test accessor:
    /// verification of a minted JWT needs the public side, and the
    /// private key is the only credential this type keeps.
    #[must_use]
    pub fn public_components(&self) -> (Vec<u8>, Vec<u8>) {
        (self.public.0.clone(), self.public.1.clone())
    }

    /// Mints the App JWT at `now` (seconds since the epoch — the
    /// caller's clock): `RS256(header.payload)`, claims `iss` = the App
    /// ID, `iat` = `now - 60`, `exp` = `iat + 600` (the documented
    /// 10-minute maximum window). A fresh JWT per call — the
    /// RSASSA-PKCS1-v1_5 signature is blinded per signing, but the
    /// claims are a pure function of `now` (I3, pinned by tests).
    ///
    /// # Errors
    ///
    /// [`ForgeError::Transport`] when the claims cannot be serialized
    /// or `ring` cannot produce the signature (the signing surface
    /// produced nothing usable — never a caller bug, never a
    /// credentials problem at this point: the key was validated at
    /// construction).
    pub fn mint_jwt(&self, now: u64) -> Result<String, ForgeError> {
        let iat = now.saturating_sub(JWT_IAT_DRIFT_SECS);
        let exp = iat + JWT_WINDOW_SECS;
        let header = br#"{"alg":"RS256","typ":"JWT"}"#;
        let payload = serde_json::to_vec(&serde_json::json!({
            "exp": exp,
            "iat": iat,
            "iss": self.app_id,
        }))
        .map_err(|error| {
            ForgeError::transport(format!("the JWT claims could not be serialized: {error}"))
        })?;
        let encoded_header = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(header);
        let encoded_payload = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&payload);
        let signed_input = format!("{encoded_header}.{encoded_payload}");
        let mut signature = vec![0_u8; self.key.public().modulus_len()];
        self.key
            .sign(
                &ring::signature::RSA_PKCS1_SHA256,
                &SystemRandom::new(),
                signed_input.as_bytes(),
                &mut signature,
            )
            .map_err(|_| {
                ForgeError::transport(
                    "the JWT signature could not be produced (the signing surface failed)",
                )
            })?;
        let encoded_signature = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&signature);
        Ok(format!("{signed_input}.{encoded_signature}"))
    }
}

impl std::fmt::Debug for AppCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never the key material: the debug shape names the identity
        // only (the MemoryForge discipline — reasons carry no secrets).
        f.debug_struct("AppCredentials")
            .field("app_id", &self.app_id)
            .field("installation_id", &self.installation_id)
            .field("key", &"<rsa signer>")
            .field("public", &"<captured components>")
            .finish()
    }
}

/// The credential the adapter resolves to a bearer: the static token
/// (PAT / fine-grained / Actions `GITHUB_TOKEN`) or the GitHub App
/// (JWT → installation token, minted by
/// [`crate::forge::GitHubRestForge`]).
#[derive(Debug)]
pub enum ForgeCredentials {
    /// The static bearer (`Bearer <token>` on every call).
    Token(String),
    /// The App identity: mint once per TTL, ride the installation token
    /// (boxed — the RSA signer is far larger than the token string).
    App(Box<AppCredentials>),
}

impl ForgeCredentials {
    /// Builds the static-token credential (the M4.1b shape — the
    /// legacy constructor's behavior is unchanged).
    #[must_use]
    pub fn from_token(token: &str) -> Self {
        Self::Token(token.to_owned())
    }

    /// The credential mode — the reports' observability surface
    /// (`"token"` or `"app"`).
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Token(_) => "token",
            Self::App(_) => "app",
        }
    }
}

fn unusable_key() -> ForgeError {
    ForgeError::auth(
        "the GitHub App private key is not an RSA private key: the DER body is not the \
         nine-INTEGER PKCS#1 structure (RSAPrivateKey)",
    )
}

/// The PKCS#1 INTEGER components of the RSA private key (version, n, e,
/// d, p, q, dp, dq, qinv) as raw DER bytes — the structural validation
/// the wrapper and the public-component extraction consume.
fn pkcs1_components(der: &[u8]) -> Result<Vec<Vec<u8>>, ForgeError> {
    let mut cursor = der_sequence(der).ok_or_else(unusable_key)?;
    let mut fields = Vec::new();
    while !cursor.is_empty() {
        let integer = der_integer(&mut cursor).ok_or_else(unusable_key)?;
        fields.push(integer);
    }
    // `RSAPrivateKey` = version, n, e, d, p, q, dp, dq, qinv (RFC 8017
    // A.1.1) — nine INTEGERs, version zero, every component non-empty.
    if fields.len() != 9 {
        return Err(unusable_key());
    }
    let version_zero = fields
        .first()
        .is_some_and(|version| version.len() == 1 && version.first() == Some(&0));
    let nonempty = fields.iter().skip(1).all(|field| !field.is_empty());
    if !version_zero || !nonempty {
        return Err(unusable_key());
    }
    Ok(fields)
}

/// The canonical big-endian encoding of a non-negative INTEGER: leading
/// zero bytes stripped (the shape `ring`'s public-key verification
/// consumes).
fn canonical_integer(raw: &[u8]) -> Vec<u8> {
    raw.iter().skip_while(|byte| **byte == 0).copied().collect()
}

/// Extracts the PKCS#1 body (the inner OCTET STRING) from a PKCS#8
/// `PrivateKeyInfo`: `SEQUENCE`(`version`, `AlgorithmIdentifier`, `key`).
fn pkcs8_inner_key(der: &[u8]) -> Result<Vec<u8>, ForgeError> {
    let not_pkcs8 = || {
        ForgeError::auth(
            "the GitHub App private key is not a PKCS#8 PrivateKeyInfo: expected \
             SEQUENCE(version, AlgorithmIdentifier, key)",
        )
    };
    let (_, content, _) = der_element(der).ok_or_else(not_pkcs8)?;
    // version INTEGER, AlgorithmIdentifier SEQUENCE, then the key.
    let (tag, _, rest) = der_element(content).ok_or_else(not_pkcs8)?;
    if tag != 0x02 {
        return Err(not_pkcs8());
    }
    let (tag, _, rest) = der_element(rest).ok_or_else(not_pkcs8)?;
    if tag != 0x30 {
        return Err(not_pkcs8());
    }
    let (tag, content, _rest) = der_element(rest).ok_or_else(not_pkcs8)?;
    if tag != 0x04 {
        return Err(not_pkcs8());
    }
    Ok(content.to_vec())
}

/// Extracts the DER body of `der` as a SEQUENCE, returning the content
/// after the header.
fn der_sequence(der: &[u8]) -> Option<&[u8]> {
    let (tag, content, _rest) = der_element(der)?;
    if tag != 0x30 {
        return None;
    }
    Some(content)
}

/// Reads one DER INTEGER at `cursor`'s head, advancing it; the value is
/// returned as the raw DER bytes (leading zeros included — DER INTEGERs
/// may carry a sign byte; canonicalization happens at the consumer).
fn der_integer(cursor: &mut &[u8]) -> Option<Vec<u8>> {
    let (tag, content, rest) = der_element(cursor)?;
    if tag != 0x02 {
        return None;
    }
    *cursor = rest;
    Some(content.to_vec())
}

/// Reads one DER tag-length-value element at `der`'s head; returns
/// `(tag, content, rest_after_element)`. Lengths follow ITU X.690
/// 8.1.3: the short form up to 127, the long form 0x81/0x82 (the
/// shapes PEM bodies actually carry; the indefinite form is not DER
/// and is rejected).
fn der_element(der: &[u8]) -> Option<(u8, &[u8], &[u8])> {
    let (&tag, tail) = der.split_first()?;
    let (length, rest) = match tail.first()? {
        // Definite short form; the indefinite form is not allowed in
        // DER and is rejected here.
        length if *length & 0x80 == 0 => match tail.split_first() {
            Some((_, rest)) => (u64::from(*length), rest),
            None => return None,
        },
        // The long form's first byte is the 0x81/0x82 tag itself; skip
        // it before reading the length octets.
        0x81 => {
            let (_, tail) = tail.split_first()?;
            let (low, tail) = tail.split_first()?;
            (u64::from(*low), tail)
        }
        0x82 => {
            let (_, tail) = tail.split_first()?;
            let (high, tail) = tail.split_first()?;
            let (low, tail) = tail.split_first()?;
            ((u64::from(*high) << 8) | u64::from(*low), tail)
        }
        _ => return None,
    };
    let length = usize::try_from(length).ok()?;
    if rest.len() < length {
        return None;
    }
    let (content, after) = rest.split_at(length);
    Some((tag, content, after))
}

/// Extracts the first PEM section: `(label, der_body)`. Whitespace
/// inside the armored body is ignored (PEM line wrapping is
/// insignificant per RFC 7468).
fn pem_section(pem: &str) -> Option<(String, Vec<u8>)> {
    let begin = "-----BEGIN ";
    let end = "-----END ";
    let begin_start = pem.find(begin)?;
    let label_start = begin_start + begin.len();
    let label_end = pem[label_start..].find("-----")? + label_start;
    let label = pem[label_start..label_end].to_owned();
    let body_start = label_end + "-----".len();
    let end_marker = format!("{end}{label}-----");
    let body_end = pem.get(body_start..)?.find(&end_marker)? + body_start;
    let armored: String = pem[body_start..body_end]
        .chars()
        .filter(|character| !character.is_ascii_whitespace())
        .collect();
    let der = base64::engine::general_purpose::STANDARD
        .decode(armored)
        .ok()?;
    Some((label, der))
}

/// Wraps a PKCS#1 `RSAPrivateKey` DER body into the PKCS#8
/// `PrivateKeyInfo` shape (RFC 5958): the version INTEGER, the
/// `rsaEncryption` `AlgorithmIdentifier`, and the key as an `OCTET STRING`.
/// The wrapper is byte-for-byte what `openssl pkcs8 -topk8 -nocrypt`
/// emits — the shape `ring` parses.
fn pkcs1_to_pkcs8(pkcs1: &[u8]) -> Result<Vec<u8>, ForgeError> {
    // Validate the body first: a non-RSA DER wrapped into PKCS#8 would
    // otherwise surface as an opaque `ring` rejection. The wrapper's
    // lengths encode in two bytes; a DER body that large cannot come
    // from a PEM key anyway.
    pkcs1_components(pkcs1)?;
    let body_len = u16::try_from(pkcs1.len()).map_err(|_| unusable_key())?;
    // OCTET STRING header over the PKCS#1 body.
    let mut octet = vec![0x04];
    der_length(body_len, &mut octet);
    octet.extend_from_slice(pkcs1);
    // PrivateKeyInfo content: version 0 + rsaEncryption + the key.
    let mut content = vec![0x02, 0x01, 0x00, 0x30, 0x0d];
    content.extend_from_slice(&[
        0x06, 0x09, 0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x01, 0x05, 0x00,
    ]);
    content.extend_from_slice(&octet);
    let content_len = u16::try_from(content.len()).map_err(|_| unusable_key())?;
    let mut pkcs8 = vec![0x30];
    der_length(content_len, &mut pkcs8);
    pkcs8.extend_from_slice(&content);
    Ok(pkcs8)
}

/// Appends the DER length encoding for `length` to `out` (short form
/// under 128, long form 0x81/0x82 beyond — key bodies live far below
/// the 0x83/0x84 forms).
fn der_length(length: u16, out: &mut Vec<u8>) {
    if length < 0x80 {
        out.push(u8::try_from(length).unwrap_or(0x7F));
    } else if length <= 0xFF {
        out.push(0x81);
        out.extend_from_slice(&length.to_be_bytes());
    } else {
        out.push(0x82);
        out.extend_from_slice(&length.to_be_bytes());
    }
}

//! The forge data vocabulary (§11.4): the request/response types the
//! [`Forge`](crate::Forge) trait passes across the seam. Plain data —
//! deterministic, `Eq`, no handles, no URLs that pretend to be real (the
//! reference adapter's URLs are `mem://`-scheme, documented as synthetic).

use crate::error::ForgeError;

/// A git ref name a Release PR lives on (§11.2 `halyard/release/<base>`).
/// Validated on construction: fail-closed against empty or
/// whitespace-bearing names so a misconfigured base branch never reaches
/// a real forge call (`E-FORGE-REQUEST`).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BranchName(String);

impl BranchName {
    /// Validates and constructs a branch name (`E-FORGE-REQUEST` when
    /// empty or whitespace-bearing).
    pub fn new(name: &str) -> Result<Self, ForgeError> {
        if name.is_empty() || name.bytes().any(|b| b.is_ascii_whitespace()) {
            return Err(ForgeError::request(format!(
                "branch name {name:?} is empty or contains whitespace"
            )));
        }
        Ok(Self(name.to_owned()))
    }

    /// The validated name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for BranchName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// The input of [`Forge::upsert_pr`](crate::Forge::upsert_pr): the
/// Release PR the state machine wants on `branch` (§11.1 step 3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrSpec {
    /// The release branch the PR lives on (found by branch name, §11.4).
    pub branch: BranchName,
    /// PR title (Conventional-Commit style; the squash-merge bump input,
    /// §11.6).
    pub title: String,
    /// PR body: the §11.3 renderer's stable, parseable markdown with the
    /// embedded plan.
    pub body: String,
}

/// A Release PR as the forge reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pr {
    /// The forge's PR number (1-based, per-forge scope).
    pub number: u64,
    /// The branch the PR is opened against (echoed verbatim).
    pub branch: String,
    /// Current title.
    pub title: String,
    /// Current body.
    pub body: String,
    /// Forge URL of the PR (the reference adapter mints synthetic
    /// `mem://` URLs — a mock mints no real web resources).
    pub url: String,
}

/// The input of [`Forge::create_release`](crate::Forge::create_release):
/// one package release's tag plus its notes (§11.1 step 2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReleaseSpec {
    /// The tag to release (`{name}-v{version}`-style, §5.3
    /// `tag_template`).
    pub tag: String,
    /// Release notes (the changelog renderer's section, §9.4).
    pub notes: String,
}

/// A release as the forge reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    /// The released tag.
    pub tag: String,
    /// Forge URL of the release (synthetic `mem://` in the reference
    /// adapter).
    pub url: String,
    /// `true` when this call created the release; `false` when the
    /// GET-by-tag-first idempotence path returned the existing one (I1).
    pub created: bool,
}

/// One file payload of an [`ApiCommit`] (§11.5 `commit_mode = "api"`:
/// tree/commit via the forge's Git Data API). Bytes, no mode bits — the
/// M4.2 state machine pins the full tree vocabulary if it needs more.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApiFile {
    /// Repo-relative path, forward slashes.
    pub path: String,
    /// Full file content (the API commit replaces the blob).
    pub content: Vec<u8>,
}

/// The input of [`Forge::commit_via_api`](crate::Forge::commit_via_api):
/// a commit created through the forge's API so the forge signs it
/// ("Verified", §11.5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApiCommit {
    /// Branch to commit onto.
    pub branch: BranchName,
    /// Commit message (Conventional-Commit style; we dogfood it, §0.7).
    pub message: String,
    /// File payloads to write in the commit.
    pub files: Vec<ApiFile>,
}

/// The forge's API budget snapshot (§11.4 `rate_limit`; feeds the
/// end-of-run API usage summary, §11.5). No clock crosses the seam — a
/// real adapter fills the fields from response headers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RateInfo {
    /// Requests the current window allows in total.
    pub limit: u32,
    /// Requests still available in the current window.
    pub remaining: u32,
}

impl RateInfo {
    /// The reference adapter's modeled budget: mirrors the GitHub
    /// authenticated REST quota (5 000 requests/hour; `VERIFY` per §0.4 —
    /// real adapters must fill from headers, never from this constant).
    pub const MODEL_DEFAULT: Self = Self {
        limit: 5_000,
        remaining: 5_000,
    };
}

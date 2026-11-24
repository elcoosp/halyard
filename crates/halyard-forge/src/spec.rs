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

/// The caller-applied `EditSet`, ready for the prepare phase's content
/// commit (§11.1 step 3): the caller applied the plan's edits **in
/// memory**, built the content tree and probed the remote release branch
/// (`ls-remote`, git-side — 0 REST). The machine owns the rest of the
/// step: it compares the probe with the applied tree, commits through
/// [`Forge::commit_via_api`](crate::Forge::commit_via_api) when they
/// differ and skips the commit when they match (I1 — the branch already
/// carries the content).
///
/// The machine never edits: the payloads are the full file contents the
/// applied `EditSet` produced, and the tree id is the git tree hash the
/// caller computed over them (deterministic — the idempotence key).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContentCommit {
    /// Commit message (Conventional-Commit style; we dogfood it, §0.7).
    pub message: String,
    /// The edited files' full contents (the applied `EditSet`, §11.1
    /// "apply `EditSet` in memory").
    pub files: Vec<ApiFile>,
    /// The git tree OID of the applied content (the caller hashed the
    /// built tree; §11.1 "build tree"). The compare key against
    /// `remote_tree_oid`.
    pub tree_oid: halyard_core::reason::Oid,
    /// The remote release branch tip's tree OID the caller probed (§11.1
    /// "compare tree OID with remote release branch tip"). `None` = the
    /// branch does not exist remotely yet (the first prepare pass).
    pub remote_tree_oid: Option<halyard_core::reason::Oid>,
}

impl ContentCommit {
    /// Fail-closed spec validation, shared by every consumer: a
    /// non-empty message and a non-empty payload list. `E-FORGE-REQUEST`
    /// names the violated rule — an invalid commit never reaches a forge
    /// call (an empty payload list would silently commit a tree that
    /// carries nothing).
    ///
    /// # Errors
    ///
    /// [`ForgeError::Request`] for each rule listed above.
    pub fn validate(&self) -> Result<(), ForgeError> {
        if self.message.is_empty() {
            return Err(ForgeError::request(
                "content commit message is empty — render the plan's Conventional-Commit \
                 release title before handing the commit to the machine",
            ));
        }
        if self.files.is_empty() {
            return Err(ForgeError::request(
                "content commit carries no files — a plan with releases edits at least one \
                 file; the caller passes no ContentCommit for an empty plan, never an empty \
                 payload list",
            ));
        }
        Ok(())
    }
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

/// The input of [`Forge::upsert_pr_comment`](crate::Forge::upsert_pr_comment):
/// the §11.6 check comment halyard keeps on one PR, found by its marker.
/// The marker is the idempotence key (I1): the upsert scans the PR's
/// comments for it and rewrites the hit in place, or creates when the
/// scan comes back empty — the same find-first convention `upsert_pr`
/// (find-by-branch) and `create_release` (GET-by-tag-first) follow.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommentSpec {
    /// The PR the comment belongs to (the forge's PR number, 1-based).
    pub pr_number: u64,
    /// The idempotence marker — the stable HTML-comment line the upsert
    /// scans for (`<!-- halyard:check v1 -->`). It must appear in `body`
    /// (it is the find key; a marker the body does not carry makes the
    /// comment unfindable, which the validation below refuses).
    pub marker: String,
    /// The full comment body (markdown, marker included).
    pub body: String,
}

impl CommentSpec {
    /// Fail-closed spec validation, shared by every adapter: `1`-based
    /// PR number, a non-empty marker, a non-empty body that carries the
    /// marker. `E-FORGE-REQUEST` names the violated rule — an invalid
    /// spec never reaches a transport or a store.
    ///
    /// # Errors
    ///
    /// [`ForgeError::Request`] for each rule listed above.
    pub fn validate(&self) -> Result<(), ForgeError> {
        if self.pr_number == 0 {
            return Err(ForgeError::request(
                "comment PR number is 0 — forge PR numbers are 1-based; the caller resolved \
                 no PR (a missing target is a value the caller reports, never a spec to guess)",
            ));
        }
        if self.marker.is_empty() {
            return Err(ForgeError::request(
                "comment marker is empty — the marker is the find key the idempotent upsert \
                 scans for; use the renderer's stable `<!-- halyard:check v1 -->` line",
            ));
        }
        if self.body.is_empty() {
            return Err(ForgeError::request(
                "comment body is empty — there is nothing to post",
            ));
        }
        if !self.body.contains(&self.marker) {
            return Err(ForgeError::request(
                "comment body does not contain its own marker — the upsert would never find \
                 the comment again (I1); embed the marker line in the body",
            ));
        }
        Ok(())
    }
}

/// A PR comment as the forge reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrComment {
    /// Web URL of the comment (synthetic `mem://` in the reference
    /// adapter, `preview://` in the dry-run adapter).
    pub url: String,
    /// `true` when this call created the comment; `false` when the
    /// marker scan found the existing one and rewrote it in place (I1).
    pub created: bool,
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

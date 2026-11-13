//! The requirement rewriter (appendix A.2, §10.4) — the canonical
//! implementation (ADR-0008): updates a dependent's requirement so it
//! admits a bumped dependency's new version, preserving the operator style
//! and the numeric precision of the original text.
//!
//! Supported shapes (single comparator): bare versions (`0.4`), `=`, `^`,
//! `~`, `>=`, wildcards (`1.*`, `1.2.*`), open upper bounds (`<2` under
//! the widen policy) and the two-comparator window `>=X, <Y` (widen
//! shifts the window to the new major). Closed upper bounds (`<=`),
//! strict lower bounds (`>`) and mixed multi-comparator sets are
//! `E-EDIT-UNREWRITABLE` — the caller falls back to a native refresh.
//!
//! Input is the **requirement text as written** (`halyard-discover`
//! records it next to the edit site, ADR-0006): `0.4` parses as an
//! implicit caret but must be rewritten bare, which `VersionReq`'s
//! `Display` cannot reproduce.

use std::fmt::Write as _;

use semver::{Comparator, Op, Version, VersionReq};

/// Policy for the `>=1.0, <2` + `2.0.0` case (appendix A.2): widen the
/// window (`>=2.0, <3`) or error.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UpperBoundPolicy {
    /// Shift the window to the new major (appendix A.2 example outcome).
    #[default]
    Widen,
    /// Never rewrite crossed upper bounds; return `E-EDIT-UNREWRITABLE`.
    Error,
}

/// Why a rewrite was not needed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnchangedReason {
    /// The original requirement already admits the new version.
    Satisfied,
    /// The requirement is a workspace/catalog/link/file protocol that is
    /// resolved at publish time.
    Protocol,
    /// The requirement admits everything (`*`).
    Any,
}

/// Why a requirement could not be rewritten.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnrewritableReason {
    /// The upper bound was crossed and the policy forbids widening.
    UpperBoundCrossed,
    /// The requirement text does not parse as a semver requirement.
    Unparseable,
}

/// Result of [`rewrite_requirement`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RewriteOutcome {
    /// No text change needed.
    Unchanged {
        /// Why no change was needed.
        because: UnchangedReason,
    },
    /// The requirement text to write back.
    Rewritten(String),
}

/// `E-EDIT-UNREWRITABLE`: the requirement cannot be rewritten so it
/// admits the new version (see `docs/errors/E-EDIT-UNREWRITABLE.md`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VersionError {
    req: String,
    new_version: String,
    reason: UnrewritableReason,
}

impl VersionError {
    /// Stable error code.
    pub const CODE: &'static str = "E-EDIT-UNREWRITABLE";

    pub(crate) fn new(req: &str, new_version: &Version, reason: UnrewritableReason) -> Self {
        Self {
            req: req.to_owned(),
            new_version: new_version.to_string(),
            reason,
        }
    }

    /// Public constructor for the adapter edit paths (M3.1/M3.3): the
    /// requirement text at a patch span failed to parse as a semver
    /// requirement, so no rewrite can admit `new_version`. The rewriter
    /// itself reports the same reason (`E-EDIT-UNREWRITABLE`).
    pub fn unparseable(req: &str, new_version: &Version) -> Self {
        Self::new(req, new_version, UnrewritableReason::Unparseable)
    }

    /// Stable error code, equal to [`VersionError::CODE`].
    pub fn code(&self) -> &'static str {
        Self::CODE
    }

    /// The original requirement text.
    pub fn req(&self) -> &str {
        &self.req
    }

    /// The new version that could not be accommodated.
    pub fn new_version(&self) -> &str {
        &self.new_version
    }

    /// Why the rewrite failed.
    pub fn reason(&self) -> UnrewritableReason {
        self.reason
    }
}

impl std::fmt::Display for VersionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let why = match self.reason {
            UnrewritableReason::UpperBoundCrossed => {
                "the upper bound is crossed and the policy forbids widening"
            }
            UnrewritableReason::Unparseable => "the requirement text is not a semver requirement",
        };
        write!(
            f,
            "[{}] cannot rewrite requirement {:?} for version {}: {} (rely on a native lockfile \
             refresh or adjust the requirement)",
            Self::CODE,
            self.req,
            self.new_version,
            why
        )
    }
}

impl std::error::Error for VersionError {}

/// Rewrites `req` so it admits `new` (appendix A.2). Protocols
/// (`workspace:`, `catalog:`, `link:`, `file:`) and `*` are returned
/// unchanged; satisfied requirements are returned unchanged.
pub fn rewrite_requirement(
    req: &str,
    new: &Version,
    policy: UpperBoundPolicy,
) -> Result<RewriteOutcome, VersionError> {
    let trimmed = req.trim();
    for protocol in ["workspace:", "catalog:", "link:", "file:"] {
        if trimmed.starts_with(protocol) {
            return Ok(RewriteOutcome::Unchanged {
                because: UnchangedReason::Protocol,
            });
        }
    }
    if trimmed == "*" {
        return Ok(RewriteOutcome::Unchanged {
            because: UnchangedReason::Any,
        });
    }
    let parsed = VersionReq::parse(trimmed)
        .map_err(|_| VersionError::new(trimmed, new, UnrewritableReason::Unparseable))?;
    if parsed.matches(new) {
        return Ok(RewriteOutcome::Unchanged {
            because: UnchangedReason::Satisfied,
        });
    }
    let raw_parts: Vec<&str> = trimmed.split(',').map(str::trim).collect();
    let comparators: &[Comparator] = &parsed.comparators;
    match (comparators.len(), raw_parts.len()) {
        (1, 1) => rewrite_single(comparators.first(), raw_parts.first().copied(), new, policy),
        (2, 2) => rewrite_window(comparators, &raw_parts, new, policy),
        _ => Err(VersionError::new(
            trimmed,
            new,
            UnrewritableReason::UpperBoundCrossed,
        )),
    }
}

/// Rewrites one comparator, preserving the operator and precision.
fn rewrite_single(
    comparator: Option<&Comparator>,
    raw: Option<&str>,
    new: &Version,
    policy: UpperBoundPolicy,
) -> Result<RewriteOutcome, VersionError> {
    let Some(comparator) = comparator else {
        return Err(VersionError::new(
            new.to_string().as_str(),
            new,
            UnrewritableReason::Unparseable,
        ));
    };
    let Some(raw) = raw else {
        return Err(VersionError::new(
            new.to_string().as_str(),
            new,
            UnrewritableReason::Unparseable,
        ));
    };
    let op = comparator.op;
    match op {
        Op::Exact | Op::Caret | Op::Tilde | Op::Wildcard | Op::GreaterEq => {
            Ok(RewriteOutcome::Rewritten(format_comparator(op, raw, new)))
        }
        Op::Less => {
            // `<2` crossed by 2.0.0 → widen to the next major boundary.
            if policy == UpperBoundPolicy::Widen {
                let next_major = new.major.saturating_add(1);
                if !new.pre.is_empty() {
                    // Cargo prerelease matching: a prerelease version only
                    // matches a comparator carrying the same triple with a
                    // prerelease, so a widened upper bound must pair with
                    // an explicit floor.
                    return Ok(RewriteOutcome::Rewritten(format!(
                        ">={}, <{next_major}",
                        full_with_pre(new)
                    )));
                }
                let precision = numeric_precision(strip_operator(raw));
                let body = match precision {
                    1 => format!("{next_major}"),
                    2 => format!("{next_major}.0"),
                    _ => format!("{next_major}.0.0"),
                };
                Ok(RewriteOutcome::Rewritten(format!("<{body}")))
            } else {
                Err(VersionError::new(
                    raw,
                    new,
                    UnrewritableReason::UpperBoundCrossed,
                ))
            }
        }
        // Closed upper bounds, strict lower bounds and any future
        // `#[non_exhaustive]` op cannot admit the new version while
        // preserving their meaning.
        Op::LessEq | Op::Greater | _ => Err(VersionError::new(
            raw,
            new,
            UnrewritableReason::UpperBoundCrossed,
        )),
    }
}

/// Rewrites the `>=X, <Y` window (appendix A.2): a crossed upper bound
/// widens the window to `>=<new>, <<new.major+1>`; a crossed lower bound
/// (rare: new below the floor) raises only the floor.
fn rewrite_window(
    comparators: &[Comparator],
    raw_parts: &[&str],
    new: &Version,
    policy: UpperBoundPolicy,
) -> Result<RewriteOutcome, VersionError> {
    let lower = comparators.first();
    let upper = comparators.get(1);
    let (lower_raw, upper_raw) = (raw_parts.first().copied(), raw_parts.get(1).copied());
    let (Some(lower), Some(upper)) = (lower, upper) else {
        return Err(VersionError::new(
            new.to_string().as_str(),
            new,
            UnrewritableReason::Unparseable,
        ));
    };
    let (Some(lower_raw), Some(upper_raw)) = (lower_raw, upper_raw) else {
        return Err(VersionError::new(
            new.to_string().as_str(),
            new,
            UnrewritableReason::Unparseable,
        ));
    };
    let lower_admits = lower.matches(new);
    let upper_admits = upper.matches(new);
    if !upper_admits {
        if policy == UpperBoundPolicy::Widen && lower.op == Op::GreaterEq && upper.op == Op::Less {
            let next_major = new.major.saturating_add(1);
            if !new.pre.is_empty() {
                // Cargo prerelease matching (see `rewrite_single`): pair
                // the widened window with an explicit prerelease floor.
                return Ok(RewriteOutcome::Rewritten(format!(
                    ">={}, <{next_major}",
                    full_with_pre(new)
                )));
            }
            return Ok(RewriteOutcome::Rewritten(format!(
                ">={}.{}, <{next_major}",
                new.major, new.minor
            )));
        }
        return Err(VersionError::new(
            upper_raw,
            new,
            UnrewritableReason::UpperBoundCrossed,
        ));
    }
    if !lower_admits {
        // Raise the floor only; the upper bound still admits the new version.
        if lower.op == Op::GreaterEq || lower.op == Op::Caret || lower.op == Op::Tilde {
            return Ok(RewriteOutcome::Rewritten(format!(
                ">={}.{}, {upper_raw}",
                new.major, new.minor
            )));
        }
        return Err(VersionError::new(
            lower_raw,
            new,
            UnrewritableReason::UpperBoundCrossed,
        ));
    }
    // Both admit the new version: the whole-req mismatch came from
    // normalization; treat as satisfied.
    Ok(RewriteOutcome::Unchanged {
        because: UnchangedReason::Satisfied,
    })
}

/// Formats `new` under `op`, mirroring the numeric precision of the raw
/// comparator text (appendix A.2: `0.4`→`0.5` two-part, `^0.4.1`→`^0.5.0`
/// three-part).
fn format_comparator(op: Op, raw: &str, new: &Version) -> String {
    let numeric = strip_operator(raw);
    let precision = numeric_precision(numeric);
    let mut out = String::new();
    // The operator prefix comes from the RAW text, not the parsed op:
    // bare requirements (`0.4`) parse as implicit carets but must be
    // rewritten bare (`0.5`), per appendix A.2.
    let prefix = raw
        .chars()
        .take_while(|c| !c.is_ascii_digit() && *c != '*')
        .collect::<String>();
    out.push_str(&prefix);
    if op == Op::Wildcard {
        // `1.*` → `2.*`, `1.2.*` → `1.3.*`
        let has_minor = numeric_precision(numeric) >= 2;
        if has_minor {
            let _ = write!(out, "{}.{}.*", new.major, new.minor);
        } else {
            let _ = write!(out, "{}.*", new.major);
        }
    } else {
        let body = if new.pre.is_empty() {
            match precision {
                1 => format!("{}", new.major),
                2 => format!("{}.{}", new.major, new.minor),
                _ => format!("{}.{}.{}", new.major, new.minor, new.patch),
            }
        } else {
            // Cargo prerelease matching: the comparator must carry the
            // exact triple with the prerelease, so precision-2 raws
            // grow a `.0` (e.g. `^0.0` + `0.0.0-rc.0` → `^0.0.0-rc.0`).
            format!("{}.{}.{}-{}", new.major, new.minor, new.patch, new.pre)
        };
        out.push_str(&body);
    }
    out
}

/// `major.minor.patch-prerelease` (build metadata dropped).
fn full_with_pre(version: &Version) -> String {
    if version.pre.is_empty() {
        version.to_string()
    } else {
        format!(
            "{}.{}.{}-{}",
            version.major, version.minor, version.patch, version.pre
        )
    }
}

/// Strips the operator prefix (`^`, `~`, `=`, `>=`, `>`, `<`, `<=`, `*`).
fn strip_operator(raw: &str) -> &str {
    raw.trim_start_matches(['^', '~', '=', '>', '<', '*', ' '])
}

/// Number of dot-separated numeric segments in the raw comparator body
/// (before any prerelease part); `1`–`3`, wildcards count their numeric
/// segments.
fn numeric_precision(raw: &str) -> usize {
    let end = raw.find(['-', '+', '*']).unwrap_or(raw.len());
    let numeric = raw.get(..end).unwrap_or(raw).trim();
    if numeric.is_empty() {
        return 1;
    }
    numeric
        .split('.')
        .filter(|part| !part.is_empty())
        .count()
        .clamp(1, 3)
}

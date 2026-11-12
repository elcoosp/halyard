//! Member-glob matching (§6.2): one deterministic matcher shared by the
//! Cargo and npm adapters so both ecosystems expand member patterns the
//! same way (invariant I3).
//!
//! Supported syntax (superset of what Cargo `members` / pnpm `packages`
//! globs use in practice):
//!
//! * `*` — any run of characters **not** containing `/`;
//! * `**` — a whole path segment matching any number of segments,
//!   including zero (`a/**/b` matches `a/b`);
//! * `?` — exactly one character, not `/`;
//! * `[abc]`, `[a-z]`, `[!a-z]` — character classes (negated with `!`);
//! * a trailing `/` means "everything under this prefix" (`prefix/**`).
//!
//! Patterns are matched against **directories** derived from the committed
//! file list (a workspace member is a directory containing a manifest);
//! `match_segments` is the single-segment `fnmatch`-style matcher and
//! [`matches`] the whole-path entry point. Matching is pure, allocation
//! light and total: no input panics.

/// True when `path` (forward-slash separated) matches `pattern`.
pub fn matches(pattern: &str, path: &str) -> bool {
    // A trailing `/` in the pattern targets everything under the prefix.
    let (pattern, path) = if pattern.ends_with('/') {
        let stripped = pattern.strip_suffix('/').unwrap_or(pattern);
        let prefixed = format!("{stripped}/**");
        let path = path.strip_suffix('/').unwrap_or(path);
        (prefixed, path.to_owned())
    } else {
        (pattern.to_owned(), path.to_owned())
    };
    let pat_segs: Vec<&str> = pattern.split('/').collect();
    let path_segs: Vec<&str> = path.split('/').collect();
    match_segments(&pat_segs, &path_segs)
}

/// Glob matcher over one path segment (`fnmatch` semantics, UTF-8 aware).
pub fn match_segment(pattern: &str, segment: &str) -> bool {
    let pat: Vec<char> = pattern.chars().collect();
    let seg: Vec<char> = segment.chars().collect();
    seg_match(&pat, 0, &seg, 0)
}

/// Backtracking matcher over pattern/path segment char slices.
fn seg_match(pat: &[char], mut pi: usize, seg: &[char], mut si: usize) -> bool {
    // Backtracking star: remember the last star and retry with one more
    // consumed character (classic greedy-with-fallback, bounded by input
    // length so matching is total).
    let mut star: Option<(usize, usize)> = None;
    loop {
        match (pat.get(pi), seg.get(si)) {
            (Some('*'), _) => {
                star = Some((pi, si));
                pi += 1;
            }
            (Some('?'), Some(_)) => {
                pi += 1;
                si += 1;
            }
            (Some('['), Some(_)) => {
                let next = match class_end(pat, pi) {
                    Some(end) => end,
                    None => {
                        // Unterminated class: treat `[` literally.
                        if *seg.get(si).unwrap_or(&'\0') == '[' {
                            pi += 1;
                            si += 1;
                            continue;
                        }
                        return false;
                    }
                };
                if class_matches(&pat[pi + 1..next], seg[si]) {
                    pi = next + 1;
                    si += 1;
                } else {
                    match star {
                        Some((sp, ss)) => {
                            pi = sp + 1;
                            si = ss + 1;
                            star = Some((sp, ss + 1));
                            continue;
                        }
                        None => return false,
                    }
                }
            }
            (Some(&pc), Some(&sc)) if pc == sc => {
                pi += 1;
                si += 1;
            }
            _ => match star {
                Some((sp, ss)) => {
                    pi = sp + 1;
                    si = ss + 1;
                    star = Some((sp, ss + 1));
                    continue;
                }
                None => return seg.get(si).is_none() && pat.get(pi).is_none(),
            },
        }
        if pat.get(pi).is_none() && seg.get(si).is_none() {
            return true;
        }
        if pat.get(pi).is_none() || seg.get(si).is_none() {
            match star {
                Some((sp, ss)) => {
                    pi = sp + 1;
                    si = ss + 1;
                    star = Some((sp, ss + 1));
                    continue;
                }
                None => return false,
            }
        }
    }
}

/// Finds the `]` closing a class opened at `open` (index of `[`); returns
/// the index of `]`. Handles `[!`/`[^` prefixes and single `]` literals
/// right after the opening (`[]]`).
fn class_end(pat: &[char], open: usize) -> Option<usize> {
    let mut i = open + 1;
    if matches!(pat.get(i), Some('!') | Some('^')) {
        i += 1;
    }
    if pat.get(i) == Some(&']') {
        i += 1; // first `]` is a literal member
    }
    while pat.get(i).is_some() && pat.get(i) != Some(&']') {
        i += 1;
    }
    if pat.get(i) == Some(&']') {
        Some(i)
    } else {
        None
    }
}

/// Evaluates the class body (`a-z`, `!abc`, …) against one char.
fn class_matches(body: &[char], c: char) -> bool {
    let (negated, body) = match body.first() {
        Some('!') | Some('^') => (true, &body[1..]),
        _ => (false, body),
    };
    let mut hit = false;
    let mut i = 0usize;
    while i < body.len() {
        if body.len() >= i + 3 && body[i + 1] == '-' {
            if (body[i]..=body[i + 2]).contains(&c) {
                hit = true;
            }
            i += 3;
        } else {
            if body[i] == c {
                hit = true;
            }
            i += 1;
        }
    }
    hit != negated
}

/// Whole-pattern matcher over segment lists supporting `**`.
fn match_segments(pat_segs: &[&str], path_segs: &[&str]) -> bool {
    match pat_segs.split_first() {
        None => path_segs.is_empty(),
        Some((&"**", rest)) => {
            // `**` consumes zero or more path segments.
            (0..=path_segs.len()).any(|take| match_segments(rest, &path_segs[take..]))
        }
        Some((first, rest)) => {
            let (head, tail) = match path_segs.split_first() {
                Some(h) => h,
                None => return false,
            };
            match_segment(first, head) && match_segments(rest, tail)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_and_single_star() {
        assert!(matches("crates/core", "crates/core"));
        assert!(!matches("crates/core", "crates/other"));
        assert!(matches("crates/*", "crates/core"));
        assert!(matches("crates/*", "crates/a-b"));
        // `*` does not cross separators.
        assert!(!matches("crates/*", "crates/a/b"));
        assert!(!matches("crates/*", "crates"));
    }

    #[test]
    fn double_star_semantics() {
        assert!(matches("crates/**", "crates/a/b/c"));
        assert!(matches("crates/**", "crates/core"));
        // `**` may match zero segments.
        assert!(matches("a/**/b", "a/b"));
        assert!(matches("a/**/b", "a/x/y/b"));
        assert!(!matches("a/**/b", "a/x/b/c"));
        assert!(matches("**/core", "a/b/core"));
        assert!(matches("**/core", "core"));
    }

    #[test]
    fn question_and_classes() {
        assert!(matches("crates/???", "crates/abc"));
        assert!(!matches("crates/???", "crates/ab"));
        assert!(!matches("crates/?", "crates/ab"));
        assert!(matches("[abc]*", "apple"));
        assert!(!matches("[abc]*", "date"));
        assert!(matches("[a-f][0-9]", "b7"));
        assert!(!matches("[!a-f]x", "ax"));
        assert!(matches("[!a-f]x", "gx"));
        assert!(matches("arr[]]ow", "arr]ow"));
        assert!(matches("a.b", "a.b"));
        // `.` is literal, not regex meta.
        assert!(!matches("a.b", "axb"));
    }

    #[test]
    fn trailing_slash_prefix() {
        assert!(matches("packages/", "packages/a/b"));
        assert!(matches("packages/", "packages/a"));
        assert!(!matches("packages/", "other/a"));
    }

    #[test]
    fn total_on_arbitrary_input() {
        proptest::proptest!(|(pat in ".*{0,12}", path in "ab/.*{0,12}")| {
            let _ = matches(&pat, &path);
        });
    }

    #[test]
    fn star_is_greedy_but_backtracks() {
        assert!(matches("a*a*a", "aXXaXXa"));
        assert!(matches("*a*b*a*", "bXaYbZaW"));
        assert!(!matches("a*a*a", "aXXaXXb"));
        assert!(matches("***", "anything/at/all"));
        assert!(matches("?", "?"));
    }
}

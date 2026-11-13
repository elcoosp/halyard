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
    let pattern_parts: Vec<&str> = pattern.split('/').collect();
    let path_parts: Vec<&str> = path.split('/').collect();
    match_segments(&pattern_parts, &path_parts)
}

/// Glob matcher over one path segment (`fnmatch` semantics, UTF-8 aware).
pub fn match_segment(pattern: &str, segment: &str) -> bool {
    let pat: Vec<char> = pattern.chars().collect();
    let seg: Vec<char> = segment.chars().collect();
    seg_match(&pat, &seg)
}

/// Backtracking matcher over pattern/path segment char slices (classic
/// greedy-with-last-star-fallback). Every star retry consumes exactly one
/// more segment character, so the loop is bounded by `pat.len() ×
/// seg.len()` steps — matching is total on every input (the previous
/// version retried past the segment end and never terminated on
/// non-matching star patterns).
fn seg_match(pat: &[char], seg: &[char]) -> bool {
    let mut pi = 0usize;
    let mut si = 0usize;
    // Last star seen: its pattern index and the segment index its next
    // retry starts from (the star has consumed `star_next` characters).
    let mut star_pi: Option<usize> = None;
    let mut star_next = 0usize;
    loop {
        let p = pat.get(pi).copied();
        let s = seg.get(si).copied();
        match (p, s) {
            // Both exhausted: the segment matched exactly.
            (None, None) => return true,
            (Some('*'), _) => {
                star_pi = Some(pi);
                star_next = si;
                pi += 1;
            }
            (Some('?'), Some(_)) => {
                pi += 1;
                si += 1;
            }
            (Some('['), Some(_)) => {
                let Some(end) = class_end(pat, pi) else {
                    // Unterminated class: treat `[` literally.
                    if s == Some('[') {
                        pi += 1;
                        si += 1;
                    } else if !backtrack(&mut pi, &mut si, &mut star_pi, &mut star_next, seg.len())
                    {
                        return false;
                    }
                    continue;
                };
                let body = pat.get(pi + 1..end).unwrap_or(&[]);
                if s.is_some_and(|c| class_matches(body, c)) {
                    pi = end + 1;
                    si += 1;
                } else if !backtrack(&mut pi, &mut si, &mut star_pi, &mut star_next, seg.len()) {
                    return false;
                }
            }
            (Some(pc), Some(sc)) if pc == sc => {
                pi += 1;
                si += 1;
            }
            // Mismatch or one side exhausted: retry from after the last
            // star with the star consuming one more character; a star at
            // the segment end cannot consume more ⇒ no match.
            _ => {
                if !backtrack(&mut pi, &mut si, &mut star_pi, &mut star_next, seg.len()) {
                    return false;
                }
            }
        }
    }
}

/// The single failure path of [`seg_match`]: retry from after the last
/// star with the star consuming one more character; returns `false` when
/// no retryable star exists (none seen, or already consuming to the
/// segment end) — the caller translates that into "no match".
fn backtrack(
    pi: &mut usize,
    si: &mut usize,
    star_pi: &mut Option<usize>,
    star_next: &mut usize,
    seg_len: usize,
) -> bool {
    match *star_pi {
        Some(sp) if *star_next < seg_len => {
            *star_next += 1;
            *si = *star_next;
            *pi = sp + 1;
            true
        }
        _ => false,
    }
}

/// Finds the `]` closing a class opened at `open` (index of `[`); returns
/// the index of `]`. Handles `[!`/`[^` prefixes and single `]` literals
/// right after the opening (`[]]`).
fn class_end(pat: &[char], open: usize) -> Option<usize> {
    let mut i = open + 1;
    if matches!(pat.get(i), Some('!' | '^')) {
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
        Some('!' | '^') => (true, body.get(1..).unwrap_or(&[])),
        _ => (false, body),
    };
    let mut hit = false;
    let mut i = 0usize;
    while i < body.len() {
        let first = body.get(i).copied();
        let dash = body.get(i + 1).copied();
        let last = body.get(i + 2).copied();
        if dash == Some('-') && last.is_some() {
            if let (Some(a), Some(b)) = (first, last)
                && (a..=b).contains(&c)
            {
                hit = true;
            }
            i += 3;
        } else {
            if first == Some(c) {
                hit = true;
            }
            i += 1;
        }
    }
    hit != negated
}

/// Whole-pattern matcher over segment lists supporting `**`. Bottom-up
/// DP over `(pattern segment, path segment)` indices: polynomial time and
/// total on every input (nested `**` cannot blow up — the naive per-take
/// recursion was exponential there).
fn match_segments(pattern_parts: &[&str], path_parts: &[&str]) -> bool {
    let pattern_len = pattern_parts.len();
    let path_len = path_parts.len();
    // `table[i * (path_len + 1) + j]` =
    // pattern_parts[i..] matches path_parts[j..].
    let mut table = vec![false; (pattern_len + 1) * (path_len + 1)];
    // Both exhausted: match.
    if let Some(cell) = table.get_mut(pattern_len * (path_len + 1) + path_len) {
        *cell = true;
    }
    for i in (0..pattern_len).rev() {
        for j in (0..=path_len).rev() {
            let pattern = pattern_parts.get(i).unwrap_or(&"");
            // Two or more consecutive stars in one segment act as `**`
            // (fnmatch/gitignore convention: `***` ≡ `**`); a single `*`
            // stays within its segment.
            let multi_star = pattern.len() >= 2 && pattern.chars().all(|c| c == '*');
            let ok = if multi_star {
                // `**` consumes zero path segments…
                let consume_zero = table
                    .get((i + 1) * (path_len + 1) + j)
                    .copied()
                    .unwrap_or(false);
                // …or one (including the last, so it can also swallow a
                // trailing empty segment produced by a trailing `/`).
                let consume_one = j < path_len
                    && table
                        .get(i * (path_len + 1) + (j + 1))
                        .copied()
                        .unwrap_or(false);
                consume_zero || consume_one
            } else {
                let segment = path_parts.get(j);
                match segment {
                    Some(seg) => {
                        match_segment(pattern, seg)
                            && table
                                .get((i + 1) * (path_len + 1) + (j + 1))
                                .copied()
                                .unwrap_or(false)
                    }
                    None => false,
                }
            };
            if let Some(cell) = table.get_mut(i * (path_len + 1) + j) {
                *cell = ok;
            }
        }
    }
    table.first().copied().unwrap_or(false)
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

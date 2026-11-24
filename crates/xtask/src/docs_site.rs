//! `check-docs-site`: the M4.8 docs-site gate (link-checked in CI).
//!
//! The M4.8 acceptance row pins "Docs site v1 (mdBook/Starlight):
//! quickstart, concepts, reference, errors | link-checked in CI". This
//! gate is that link check, as a workspace gate so it runs identically
//! locally and in CI (the §15.6 convention — `xtask` hosts the gates,
//! the CI job calls the same command). It validates the mdBook tree at
//! `docs/site`:
//!
//! 1. `book.toml` exists, parses, and declares a `book.title` (the
//!    renderer's minimum configuration).
//! 2. `src/SUMMARY.md` parses (the mdBook `- [title](path)` grammar),
//!    every entry's file exists, and no path is listed twice.
//! 3. No orphan pages: every `.md` under the book's `src` directory is
//!    reachable from `SUMMARY.md` (mdBook simply drops unreferenced
//!    files — a dropped page would silently rot).
//! 4. Every relative link in every book page resolves — within the
//!    book, or (when the target escapes the book) to an existing
//!    repository file; `file#anchor` targets are checked against the
//!    target's GitHub-style heading slugs. External (`http(s)://`,
//!    `mailto:`) targets are counted but not fetched: the gate is
//!    offline and deterministic (I5).
//! 5. `{{#include PATH}}` directives resolve to existing files.
//! 6. The `reference/errors.md` code table stays in set-equality with
//!    the `docs/errors/README.md` index — the site's curated copy
//!    cannot silently drift from the per-code pages it mirrors.
//!
//! The render itself is `mdbook build docs/site` (ADR-0048); the gate
//! is the structural half that a plain render does not prove.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// The book directory, relative to the workspace root.
const SITE_DIR: &str = "docs/site";
/// The site's errors-reference page, relative to the book directory.
const ERRORS_PAGE: &str = "src/reference/errors.md";
/// The error-page index the site's table must stay in sync with.
const ERRORS_INDEX: &str = "docs/errors/README.md";

/// One parsed `SUMMARY.md` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SummaryEntry {
    /// Entry title inside the brackets.
    title: String,
    /// Book-`src`-relative page path with `/` separators.
    path: String,
}

/// Run the gate against the workspace's real `docs/site` book.
pub fn check_docs_site() -> Result<(), String> {
    let root = crate::schema::workspace_root()?;
    check_docs_site_at(&root)
}

/// The gate body, parameterized over the workspace root (the test
/// fixture path runs the identical logic against a temporary tree).
fn check_docs_site_at(root: &Path) -> Result<(), String> {
    let site = root.join(SITE_DIR);
    let mut failures: Vec<String> = Vec::new();
    let src_dir = check_book_toml(&site, &mut failures);
    let entries = check_summary(&site, &mut failures);
    if let (Some(src), Some(entries)) = (src_dir.as_deref(), entries.as_deref()) {
        check_orphans(src, entries, &mut failures);
    }
    let external_links = src_dir.as_deref().map_or(0, |src| {
        check_links_and_includes(root, &site, src, &mut failures)
    });
    check_errors_mirror(root, &site, &mut failures);
    if failures.is_empty() {
        println!(
            "docs-site: book.toml + SUMMARY + links + includes + errors index checked — ok \
             ({external_links} external links counted, not fetched: offline gate, I5)"
        );
        Ok(())
    } else {
        Err(failures.join("\n"))
    }
}

/// Step 1: `book.toml` — present, parseable, titled; returns the book
/// `src` directory when the configuration is usable.
fn check_book_toml(site: &Path, failures: &mut Vec<String>) -> Option<PathBuf> {
    let book_toml = site.join("book.toml");
    let Ok(text) = std::fs::read_to_string(&book_toml) else {
        failures.push(format!(
            "docs/site/book.toml is missing ({}): the M4.8 site is not scaffolded",
            book_toml.display()
        ));
        return None;
    };
    match toml::from_str::<toml::Table>(&text) {
        Ok(table) => {
            let title = table
                .get("book")
                .and_then(|book| book.get("title"))
                .and_then(toml::Value::as_str);
            let src = table
                .get("book")
                .and_then(|book| book.get("src"))
                .and_then(toml::Value::as_str)
                .unwrap_or("src");
            match title {
                Some(t) if !t.trim().is_empty() => Some(site.join(src)),
                Some(_) => {
                    failures.push("book.toml: [book].title is present but empty".to_owned());
                    None
                }
                None => {
                    failures.push("book.toml: missing [book].title".to_owned());
                    None
                }
            }
        }
        Err(e) => {
            failures.push(format!("book.toml does not parse: {e}"));
            None
        }
    }
}

/// Step 2: `SUMMARY.md` — the grammar parses, every entry's file
/// exists, and no path is listed twice.
fn check_summary(site: &Path, failures: &mut Vec<String>) -> Option<Vec<SummaryEntry>> {
    let summary_path = site.join("src").join("SUMMARY.md");
    let Ok(text) = std::fs::read_to_string(&summary_path) else {
        failures.push(
            "docs/site/src/SUMMARY.md is missing: a book without a summary renders empty"
                .to_owned(),
        );
        return None;
    };
    match parse_summary(&text) {
        Ok(entries) => {
            let mut seen: BTreeSet<String> = BTreeSet::new();
            for entry in &entries {
                let path = &entry.path;
                if !seen.insert(entry.path.clone()) {
                    failures.push(format!("SUMMARY.md: path {path:?} is listed twice"));
                }
                let page = site.join("src").join(path);
                if !page.is_file() {
                    failures.push(format!(
                        "SUMMARY.md: entry {path:?} has no file at {}",
                        page.display()
                    ));
                }
            }
            Some(entries)
        }
        Err(e) => {
            failures.push(format!("SUMMARY.md: {e}"));
            None
        }
    }
}

/// Step 3: no orphan pages — every `.md` under the book `src` is
/// reachable from `SUMMARY.md` (the summary file itself is the index,
/// never a page).
fn check_orphans(src_dir: &Path, entries: &[SummaryEntry], failures: &mut Vec<String>) {
    let mut on_disk: BTreeSet<String> = BTreeSet::new();
    collect_md_files(src_dir, &mut on_disk);
    on_disk.remove("SUMMARY.md");
    let listed: BTreeSet<String> = entries.iter().map(|e| e.path.clone()).collect();
    for path in &on_disk - &listed {
        failures.push(format!(
            "orphan page src/{path}: present on disk but not listed in SUMMARY.md \
             (mdBook would silently drop it)"
        ));
    }
}

/// Steps 4/5: every page's relative links resolve and every include
/// directive points at an existing file; returns the external-link
/// count (counted, never fetched — the gate is offline, I5).
fn check_links_and_includes(
    root: &Path,
    site: &Path,
    src_dir: &Path,
    failures: &mut Vec<String>,
) -> usize {
    let mut pages: Vec<PathBuf> = Vec::new();
    collect_md_paths(src_dir, &mut pages);
    pages.sort();
    let mut external_links = 0usize;
    for page in &pages {
        let Ok(text) = std::fs::read_to_string(page) else {
            continue; // already reported by the SUMMARY/orphan checks
        };
        let page_dir = page.parent().unwrap_or(src_dir);
        let rel = page
            .strip_prefix(src_dir)
            .unwrap_or(page)
            .to_string_lossy()
            .replace('\\', "/");
        for target in link_targets(&text) {
            if is_external(&target) {
                external_links += 1;
                continue;
            }
            if let Err(e) = validate_link_target(root, site, page_dir, &text, &target) {
                failures.push(format!("src/{rel}: broken link {target:?}: {e}"));
            }
        }
        for include in include_paths(&text) {
            let resolved = page_dir.join(&include);
            if !resolved.is_file() {
                failures.push(format!(
                    "src/{rel}: include {include:?} does not resolve ({})",
                    resolved.display()
                ));
            }
        }
    }
    external_links
}

/// Step 6: the errors table cannot drift from `docs/errors/README.md`
/// (set-equality in both directions).
fn check_errors_mirror(root: &Path, site: &Path, failures: &mut Vec<String>) {
    let Ok(site_page) = std::fs::read_to_string(site.join(ERRORS_PAGE)) else {
        failures.push(format!(
            "docs/site/{ERRORS_PAGE} is missing: M4.8's `errors` reference area is absent"
        ));
        return;
    };
    let Ok(index) = std::fs::read_to_string(root.join(ERRORS_INDEX)) else {
        failures.push("docs/errors/README.md unreadable".to_owned());
        return;
    };
    let site_codes = table_codes(&site_page);
    let index_codes = table_codes(&index);
    for missing in &index_codes - &site_codes {
        failures.push(format!(
            "src/reference/errors.md: code {missing} is missing from the site table \
             (docs/errors/README.md lists it)"
        ));
    }
    for extra in &site_codes - &index_codes {
        failures.push(format!(
            "src/reference/errors.md: code {extra} has no page in docs/errors/README.md \
             (a site row must mirror a real page)"
        ));
    }
}

/// Parses the mdBook summary grammar: `- [title](path)` entries; blank
/// lines, `#`-prefixed part headers and `---` separators are skipped;
/// anything else is a parse error (a typo'd entry must fail loudly,
/// not silently drop out of the book).
fn parse_summary(summary: &str) -> Result<Vec<SummaryEntry>, String> {
    let mut entries = Vec::new();
    for (n, raw) in summary.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with("---") {
            continue;
        }
        let err = |msg: String| format!("line {}: {msg} ({line:?})", n + 1);
        let Some(rest) = line.strip_prefix("- [") else {
            return Err(err("expected `- [title](path)`".to_owned()));
        };
        let Some(split) = rest.find("](") else {
            return Err(err("entry has no `](path)` tail".to_owned()));
        };
        let Some(title) = rest.get(..split) else {
            return Err(err("unsplitable title".to_owned()));
        };
        let Some(path) = rest.get(split + 2..).and_then(|p| p.strip_suffix(')')) else {
            return Err(err("entry path is not closed with `)`".to_owned()));
        };
        if path.starts_with('#') {
            return Err(err(
                "entry points at a pure anchor — mdBook parts are `#` heading lines, \
                 entries must name a page"
                    .to_owned(),
            ));
        }
        if path.trim().is_empty() || title.trim().is_empty() {
            return Err(err("entry title and path must both be non-empty".to_owned()));
        }
        entries.push(SummaryEntry {
            title: title.to_owned(),
            path: path.to_owned(),
        });
    }
    if entries.is_empty() {
        return Err("no entries found — the book would render empty".to_owned());
    }
    Ok(entries)
}

/// Collects every `.md` file under `dir` as book-relative paths with
/// `/` separators (recursion is bounded by the site tree, never user
/// input).
fn collect_md_files(dir: &Path, out: &mut BTreeSet<String>) {
    let mut paths: Vec<PathBuf> = Vec::new();
    collect_md_paths(dir, &mut paths);
    for path in paths {
        let rel = path
            .strip_prefix(dir)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        out.insert(rel);
    }
}

/// Collects every `.md` file under `dir` (recursive, deterministic
/// order is the caller's concern).
fn collect_md_paths(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in read.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_md_paths(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "md") {
            out.push(path);
        }
    }
}

/// Extracts link targets from markdown link/image syntax. Only the
/// `](`…`)` targets matter to the gate; targets inside fenced code
/// blocks are not distinguished (the site's pages carry no such
/// examples, and the gate over-approximates on purpose).
fn link_targets(page: &str) -> Vec<String> {
    let mut targets = Vec::new();
    let mut search = 0usize;
    while let Some(open) = page.get(search..).and_then(|tail| tail.find("](")) {
        let abs_open = search + open;
        let rest = &page[abs_open + 2..];
        let Some(close) = rest.find(')') else {
            break;
        };
        if let Some(target) = rest.get(..close) {
            targets.push(target.to_owned());
        }
        search = abs_open + 2 + close + 1;
    }
    targets
}

/// Extracts `{{#include PATH}}` paths (mdBook's include directive).
fn include_paths(page: &str) -> Vec<String> {
    let mut includes = Vec::new();
    let mut search = 0usize;
    let needle = "{{#include ";
    while let Some(open) = page.get(search..).and_then(|tail| tail.find(needle)) {
        let abs_open = search + open + needle.len();
        let Some(rest) = page.get(abs_open..) else {
            break;
        };
        let Some(close) = rest.find("}}") else {
            break;
        };
        if let Some(path) = rest.get(..close) {
            includes.push(path.trim().to_owned());
        }
        search = abs_open + close + 2;
    }
    includes
}

/// External targets are counted, never fetched: the gate stays offline
/// and deterministic (I5).
fn is_external(target: &str) -> bool {
    target.starts_with("http://")
        || target.starts_with("https://")
        || target.starts_with("mailto:")
        || target.starts_with("ftp://")
}

/// Validates one link target: a pure `#anchor` against the linking
/// page's own headings, `file`/`file#anchor` against the book tree
/// (falling back to the repository root for targets that escape the
/// book, which the site uses to point at canonical repo files).
fn validate_link_target(
    root: &Path,
    site: &Path,
    page_dir: &Path,
    page_text: &str,
    target: &str,
) -> Result<(), String> {
    let (file_part, anchor) = match target.split_once('#') {
        Some((f, a)) => (f, Some(a)),
        None => (target, None),
    };
    if file_part.is_empty() {
        // Pure same-page anchor.
        let Some(a) = anchor else {
            return Err("empty link target".to_owned());
        };
        if a.trim().is_empty() {
            return Err("# — empty anchor fragment".to_owned());
        }
        if !anchor_exists(page_text, a) {
            return Err(format!("no heading with GitHub slug #{a} on this page"));
        }
        return Ok(());
    }
    // Link paths are relative to the page's own directory (mdBook's
    // rule); a miss falls back to the book root and then the repo root
    // (targets that escape the book point at canonical repo files).
    let in_book = page_dir.join(file_part);
    let resolved_in_book = if in_book.is_file() {
        Some(in_book.clone())
    } else {
        let from_root = site.join(file_part);
        if from_root.is_file() {
            Some(from_root)
        } else {
            None
        }
    };
    let mut escaped_book = false;
    let resolved;
    if let Some(path) = resolved_in_book {
        resolved = path;
    } else {
        let from_root = site.join(file_part);
        let in_repo = root.join(file_part);
        if in_repo.is_file() {
            escaped_book = true;
            resolved = in_repo;
        } else {
            return Err(format!(
                "no file at {} (page-relative), {} (book) or {} (repo)",
                in_book.display(),
                from_root.display(),
                in_repo.display()
            ));
        }
    }
    if let Some(a) = anchor
        && a.trim().is_empty()
    {
        return Err(format!("{file_part}# — empty anchor fragment"));
    }
    let Some(a) = anchor else {
        return Ok(());
    };
    if escaped_book {
        // Repo files outside the book are canonical sources, not
        // rendered pages; their anchors are accepted verbatim.
        return Ok(());
    }
    let Ok(text) = std::fs::read_to_string(&resolved) else {
        return Err(format!("{} unreadable", resolved.display()));
    };
    if !anchor_exists(&text, a) {
        return Err(format!("no heading with GitHub slug #{a}"));
    }
    Ok(())
}

/// GitHub-style heading slug: lowercase ASCII alphanumerics kept,
/// spaces and underscores become hyphens, hyphens kept, everything
/// else dropped. The v1 checker's scope is the site's own headings
/// (plain ASCII prose); the honest over-approximation is documented in
/// the module docs.
fn github_slug(text: &str) -> String {
    let mut slug = String::new();
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
        } else if matches!(ch, ' ' | '_' | '-') {
            slug.push('-');
        }
    }
    slug
}

/// Whether `anchor` matches one of `page`'s heading slugs.
fn anchor_exists(page: &str, anchor: &str) -> bool {
    page.lines().any(|line| {
        let trimmed = line.trim_start();
        let Some(after) = trimmed.strip_prefix('#') else {
            return false;
        };
        let hashes = after.chars().take_while(|c| *c == '#').count();
        let level = 1 + hashes;
        let heading = trimmed.get(level..).unwrap_or_default().trim();
        !heading.is_empty() && github_slug(heading) == anchor
    })
}

/// Backticked exact error codes (`E-…`) found in markdown table rows.
/// Prose outside tables never matches, so the §15.4 family wildcards
/// (`E-CFG-*`) stay out of the compared sets.
fn table_codes(page: &str) -> BTreeSet<String> {
    let mut codes = BTreeSet::new();
    for line in page.lines().filter(|l| l.trim_start().starts_with('|')) {
        let mut search = 0usize;
        while let Some(open) = line.get(search..).and_then(|tail| tail.find('`')) {
            let abs = search + open;
            let Some(rest) = line.get(abs + 1..) else {
                break;
            };
            let Some(close) = rest.find('`') else {
                break;
            };
            if let Some(token) = rest.get(..close)
                && token.starts_with("E-")
                && token
                    .chars()
                    .all(|c| c.is_ascii_uppercase() || c == '-' || c.is_ascii_digit())
            {
                codes.insert(token.to_owned());
            }
            search = abs + 1 + close + 1;
        }
    }
    codes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_parser_accepts_parts_and_rejects_malformed_entries() {
        let ok = parse_summary(
            "# Summary\n\n- [One](one.md)\n\n---\n\n# Part two\n\n  - [Two](sub/two.md)\n",
        )
        .expect("parses");
        assert_eq!(ok.len(), 2);
        let first = ok.first().expect("two entries");
        let second = ok.get(1).expect("two entries");
        assert_eq!(first.path, "one.md");
        assert_eq!(second.path, "sub/two.md");

        let bad = parse_summary("- One without brackets\n- [Two](two.md)");
        assert!(bad.is_err(), "non-entry line must fail loudly");
        let unclosed = parse_summary("- [Two](two.md");
        assert!(unclosed.is_err(), "unclosed path must fail loudly");
        let anchor_entry = parse_summary("- [Part](#part)");
        assert!(
            anchor_entry.is_err(),
            "a pure-anchor entry is not a page (parts are `#` heading lines)"
        );
        let empty = parse_summary("# Summary\n\n---\n");
        assert!(empty.is_err(), "entry-less summary renders an empty book");
    }

    #[test]
    fn link_extraction_keeps_relative_and_counts_external() {
        let page = "[book](concepts/index.md) [ext](https://github.com/elcoosp/halyard) \
                    [tag](v1.md#the-tag) [same](#same-page)";
        let targets = link_targets(page);
        assert_eq!(
            targets,
            vec![
                "concepts/index.md".to_owned(),
                "https://github.com/elcoosp/halyard".to_owned(),
                "v1.md#the-tag".to_owned(),
                "#same-page".to_owned(),
            ]
        );
        assert!(is_external("https://github.com/elcoosp/halyard"));
        assert!(!is_external("concepts/index.md"));
    }

    #[test]
    fn anchors_validate_against_github_slugs() {
        let page = "# Title\n\n## The plan contract\n\n### Exit codes\n";
        assert!(anchor_exists(page, "the-plan-contract"));
        assert!(anchor_exists(page, "exit-codes"));
        assert!(!anchor_exists(page, "plan-contract"), "partial slug fails");
        assert_eq!(github_slug("The plan contract"), "the-plan-contract");
        assert_eq!(github_slug("Exit codes"), "exit-codes");
    }

    #[test]
    fn same_page_anchors_validate_against_the_linking_page() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path();
        let site = root.join("docs/site");
        let src = site.join("src");
        std::fs::create_dir_all(&src).expect("mkdir");
        let page_text = "# Errors\n\n## Exit codes\n\n| `E-A-X` |\n";
        let good = validate_link_target(root, &site, &src, page_text, "#exit-codes");
        assert!(
            good.is_ok(),
            "same-page anchor over a real heading: {good:?}"
        );
        let bad = validate_link_target(root, &site, &src, page_text, "#no-such-heading");
        assert!(
            bad.is_err(),
            "same-page anchor over a missing heading must fail"
        );
        let empty = validate_link_target(root, &site, &src, page_text, "#");
        assert!(empty.is_err(), "bare `#` must fail");
    }

    #[test]
    fn checker_flags_broken_links_missing_files_and_orphans_in_one_run() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path();
        let src = root.join("docs/site/src");
        std::fs::create_dir_all(src.join("sub")).expect("mkdir");
        std::fs::write(root.join("docs/site/book.toml"), "[book]\ntitle = \"t\"\n")
            .expect("book.toml");
        std::fs::write(
            src.join("SUMMARY.md"),
            "- [One](one.md)\n- [Sub](sub/two.md)\n",
        )
        .expect("summary");
        std::fs::write(src.join("one.md"), "[good](sub/two.md) [bad](missing.md)").expect("one");
        std::fs::write(src.join("sub").join("two.md"), "ok").expect("two");
        std::fs::write(src.join("orphan.md"), "nobody lists me").expect("orphan");

        let failures = check_docs_site_at(root).expect_err("three failures");
        assert!(
            failures.contains("broken link \"missing.md\""),
            "{failures}"
        );
        assert!(failures.contains("orphan page src/orphan.md"), "{failures}");

        // Fix the two breaks; the tree passes.
        std::fs::write(src.join("one.md"), "[good](sub/two.md)").expect("one");
        std::fs::write(
            src.join("SUMMARY.md"),
            "- [One](one.md)\n- [Sub](sub/two.md)\n- [Orphan](orphan.md)\n",
        )
        .expect("summary");
        check_docs_site_at(root).expect("green after fixes");
    }

    #[test]
    fn errors_table_must_mirror_the_index_exactly() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path();
        let src = root.join("docs/site/src");
        std::fs::create_dir_all(src.join("reference")).expect("mkdir");
        std::fs::create_dir_all(root.join("docs/errors")).expect("mkdir index dir");
        std::fs::write(root.join("docs/site/book.toml"), "[book]\ntitle = \"t\"\n")
            .expect("book.toml");
        std::fs::write(src.join("SUMMARY.md"), "- [Errors](reference/errors.md)\n").expect("sum");
        std::fs::write(
            src.join("reference").join("errors.md"),
            "# Errors\n\n| Code | Area |\n|---|---|\n| `E-A-X` | a |\n",
        )
        .expect("page");
        std::fs::write(
            root.join("docs/errors/README.md"),
            "| Code | Page |\n|---|---|\n| `E-A-X` | a.md |\n| `E-B-Y` | b.md |\n",
        )
        .expect("index");

        let failures = check_docs_site_at(root).expect_err("E-B-Y missing");
        assert!(
            failures.contains("E-B-Y is missing from the site table"),
            "{failures}"
        );

        std::fs::write(
            src.join("reference").join("errors.md"),
            "# Errors\n\n| Code | Area |\n|---|---|\n| `E-A-X` | a |\n| `E-B-Y` | b |\n| `E-C-Z` | c |\n",
        )
        .expect("page");
        let failures = check_docs_site_at(root).expect_err("E-C-Z has no page");
        assert!(failures.contains("E-C-Z has no page"), "{failures}");
    }

    #[test]
    fn include_directives_must_resolve() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path();
        let src = root.join("docs/site/src");
        std::fs::create_dir_all(&src).expect("mkdir");
        std::fs::write(root.join("docs/site/book.toml"), "[book]\ntitle = \"t\"\n").expect("toml");
        std::fs::write(src.join("SUMMARY.md"), "- [One](one.md)\n").expect("sum");
        std::fs::write(src.join("one.md"), "{{#include ./gone.md}}").expect("page");

        let failures = check_docs_site_at(root).expect_err("include missing");
        assert!(failures.contains("include \"./gone.md\""), "{failures}");
    }

    /// Pins the M4.8 acceptance row verbatim (the §17 milestone table):
    ///
    /// > M4.8 | Docs site v1 (mdBook/Starlight): quickstart, concepts,
    /// > reference, errors | link-checked in CI
    ///
    /// The site ships all four content areas (`quickstart`, `concepts`,
    /// `reference`, `errors`) and the whole tree passes the link gate —
    /// the gate is the acceptance's "link-checked" half; the CI wiring
    /// is pinned separately by [`tests::ci_runs_the_docs_site_gate`].
    #[test]
    fn docs_site_v1_carries_the_plan_content_areas_and_passes_the_gate() {
        let root = crate::schema::workspace_root().expect("workspace root");
        let summary = std::fs::read_to_string(root.join("docs/site/src/SUMMARY.md"))
            .expect("docs/site/src/SUMMARY.md must exist (M4.8 site is scaffolded)");
        let entries = parse_summary(&summary).expect("summary parses");
        let paths: Vec<&str> = entries.iter().map(|e| e.path.as_str()).collect();
        for area in [
            "quickstart.md",
            "concepts/architecture.md",
            "concepts/plan.md",
            "concepts/dry-run.md",
            "concepts/channels.md",
            "reference/cli.md",
            "reference/config.md",
            "reference/errors.md",
        ] {
            assert!(paths.contains(&area), "M4.8 area missing: {area}");
        }
        check_docs_site_at(&root).expect("the whole book passes the link gate");
    }

    /// Pins M4.8's "link-checked in CI" half: the ci.yml battery runs
    /// this gate as its own job (the §15.6 xtask-gate convention).
    #[test]
    fn ci_runs_the_docs_site_gate() {
        let root = crate::schema::workspace_root().expect("workspace root");
        let ci =
            std::fs::read_to_string(root.join(".github/workflows/ci.yml")).expect("ci.yml is read");
        assert!(
            ci.contains("docs-site:") && ci.contains("check-docs-site"),
            "ci.yml must carry a docs-site job running the check-docs-site gate"
        );
    }
}

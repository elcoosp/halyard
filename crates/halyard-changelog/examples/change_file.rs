//! The §9.4 change-file demo (ADR-0028): the optional
//! `.halyard/changes/*.md` input — front-matter naming per-package
//! bumps, a markdown note merged into the release notes — parsed
//! through the strict grammar, merged into one `ChangeFiles` planning
//! input and rendered the way the planner merges it into a release's
//! changelog preview.
//!
//! Run: `cargo run -p halyard-changelog --example change_file`

use halyard_changelog::change_files::load_change_files;

/// The two files a polyglot repo might carry: one bump-only note and
/// one with an upgrade guide (the squash-merge escape hatch — the note
/// says what the commits cannot).
const FILES: [(&str, &str); 2] = [
    (
        ".halyard/changes/bump-only.md",
        "---\npackages = { halyard-eco-npm = \"patch\" }\n---\n",
    ),
    (
        ".halyard/changes/next.md",
        "---\npackages = { halyard-core = \"minor\" }\n---\n\nUpgrade notes:\n\n- run `halyard migrate` first\n- flip the `strict-config` flag\n",
    ),
];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Parse + merge: path order, strongest bump wins, notes verbatim.
    let files = FILES
        .iter()
        .map(|(path, body)| ((*path).to_owned(), (*body).as_bytes().to_vec()));
    let changes = load_change_files(files)?;
    println!("loaded {} change files\n", FILES.len());

    // What the planner reads: the explicit bump per package…
    for name in ["halyard-core", "halyard-eco-npm"] {
        let bump = changes.bump_of(name).ok_or("the sample names it")?;
        println!("{name}: {:?} (from {})", bump.bump, bump.file);
    }

    // …and the notes it merges into the changelog preview, in the
    // merge format (`\n{note}\n` after the commit sections).
    println!("\nmerged preview (halyard-core):");
    let mut entry = String::from("## 1.3.0\n\n### Features\n\n- grow surface\n");
    for note in changes.notes_for("halyard-core") {
        entry.push('\n');
        entry.push_str(&note.body);
        entry.push('\n');
    }
    print!("{entry}");
    Ok(())
}

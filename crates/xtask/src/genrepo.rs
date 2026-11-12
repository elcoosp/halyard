//! `gen-repo` (M0.4): deterministic synthetic repository generator.
//!
//! Same flags + seed ⇒ byte-identical repository (identical OIDs). Shapes:
//! `linear`, `squash` (subjects carry `(#N)`), `merge` (`--no-ff` two-branch
//! merges), `octopus` (three-branch merges).

use std::fmt::Write as _;
use std::path::PathBuf;

use halyard_test_support::{CommitSpec, GitFixture};

/// Repository shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    /// All commits on `main`.
    Linear,
    /// All commits on `main`, subjects suffixed `(#N)` (squash-merge style).
    Squash,
    /// Cycles of: one main commit, two feature-branch commits, `--no-ff`
    /// merge.
    Merge,
    /// Cycles of: two main commits, three one-commit branches, octopus
    /// merge.
    Octopus,
}

impl Shape {
    fn parse(s: &str) -> Result<Self, String> {
        match s {
            "linear" => Ok(Self::Linear),
            "squash" => Ok(Self::Squash),
            "merge" => Ok(Self::Merge),
            "octopus" => Ok(Self::Octopus),
            other => Err(format!(
                "unknown shape {other:?} (linear|squash|merge|octopus)"
            )),
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Linear => "linear",
            Self::Squash => "squash",
            Self::Merge => "merge",
            Self::Octopus => "octopus",
        }
    }
}

/// Fully-specified generator invocation.
pub struct GenRepoOpts {
    /// Number of workspace packages.
    pub packages: usize,
    /// Number of logical changes to commit.
    pub commits: usize,
    /// History shape.
    pub shape: Shape,
    /// Seed for the deterministic PRNG.
    pub seed: u64,
    /// Output directory (created if missing; must be empty).
    pub out: PathBuf,
}

/// splitmix64 PRNG — deterministic across platforms, no external crate.
struct SplitMix(u64);

impl SplitMix {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: usize) -> usize {
        let n = u64::try_from(n.max(1)).unwrap_or(u64::MAX);
        usize::try_from(self.next() % n).unwrap_or(0)
    }
}

const TYPES: [&str; 6] = ["feat", "fix", "perf", "refactor", "docs", "chore"];
const DESCS: [&str; 5] = [
    "tune core loop",
    "adjust inputs",
    "rework cache",
    "trim output",
    "stabilize api",
];

/// Parse command-line flags into [`GenRepoOpts`].
pub fn parse(args: &[String]) -> Result<GenRepoOpts, String> {
    let mut packages = 4usize;
    let mut commits = 50usize;
    let mut shape = Shape::Linear;
    let mut seed = 1u64;
    let mut out: Option<PathBuf> = None;
    let mut iter = args.iter();
    while let Some(a) = iter.next() {
        match a.as_str() {
            "--packages" => {
                packages = iter
                    .next()
                    .cloned()
                    .ok_or("--packages needs a value")?
                    .parse()
                    .map_err(|e| format!("--packages: {e}"))?;
            }
            "--commits" => {
                commits = iter
                    .next()
                    .cloned()
                    .ok_or("--commits needs a value")?
                    .parse()
                    .map_err(|e| format!("--commits: {e}"))?;
            }
            "--shape" => {
                shape = Shape::parse(&iter.next().cloned().ok_or("--shape needs a value")?)?;
            }
            "--seed" => {
                seed = iter
                    .next()
                    .cloned()
                    .ok_or("--seed needs a value")?
                    .parse()
                    .map_err(|e| format!("--seed: {e}"))?;
            }
            "--out" => {
                out = Some(PathBuf::from(
                    iter.next().cloned().ok_or("--out needs a value")?,
                ));
            }
            other => return Err(format!("unknown flag {other:?}")),
        }
    }
    let out = out.ok_or("--out is required")?;
    if packages == 0 || packages > 10_000 {
        return Err("--packages must be in 1..=10000".to_owned());
    }
    if commits == 0 {
        return Err("--commits must be >= 1".to_owned());
    }
    Ok(GenRepoOpts {
        packages,
        commits,
        shape,
        seed,
        out,
    })
}

/// Entry point for the CLI subcommand.
pub fn gen_repo(args: &[String]) -> Result<(), String> {
    let opts = parse(args)?;
    let head = build(&opts)?;
    println!(
        "gen-repo: shape={} packages={} commits={} seed={}",
        opts.shape.name(),
        opts.packages,
        opts.commits,
        opts.seed
    );
    println!("head: {head}");
    Ok(())
}

/// Build the repository; returns the final HEAD OID. Deterministic:
/// identical opts ⇒ identical OID.
pub fn build(opts: &GenRepoOpts) -> Result<String, String> {
    let out = opts.out.clone();
    if out.exists() {
        let is_empty = std::fs::read_dir(&out)
            .map_err(|e| format!("read {}: {e}", out.display()))?
            .next()
            .is_none();
        if !is_empty {
            return Err(format!("--out {} exists and is not empty", out.display()));
        }
    } else {
        std::fs::create_dir_all(&out).map_err(|e| format!("mkdir {}: {e}", out.display()))?;
    }
    let mut fx = GitFixture::init_in(&out).map_err(|e| e.to_string())?;
    let mut rng = SplitMix(opts.seed);

    // Scaffold: root virtual workspace + N member crates.
    let members: Vec<String> = (0..opts.packages)
        .map(|i| format!("crates/p{:03}", i + 1))
        .collect();
    let member_refs: Vec<&str> = members.iter().map(String::as_str).collect();
    let mut scaffold: Vec<(String, String)> = Vec::new();
    scaffold.push(("Cargo.toml".to_owned(), workspace_manifest(&member_refs)));
    for m in &members {
        let name = pkg_name(m);
        scaffold.push((format!("{m}/Cargo.toml"), package_manifest(&name)));
        scaffold.push((format!("{m}/src/lib.rs"), lib_rs(1)));
    }
    fx.commit_with(&CommitSpec::files(
        scaffold,
        "feat: scaffold generated workspace",
    ))
    .map_err(|e| e.to_string())?;

    // One logical change per index, arranged by shape. Merge shapes consume
    // whole cycles (Merge: 4 changes = 1 main + 2 branch + merge; Octopus:
    // 5 changes = 2 main + 3 branch + merge) and emit the remainder as
    // linear commits on main, so `--commits N` is respected exactly.
    let mut idx = 0usize;
    let mut branch_no = 0usize;
    let (cycles, tail): (usize, usize) = match opts.shape {
        Shape::Linear | Shape::Squash => (0, opts.commits),
        Shape::Merge => (opts.commits / 4, opts.commits % 4),
        Shape::Octopus => (opts.commits / 5, opts.commits % 5),
    };
    for _ in 0..cycles {
        if opts.shape == Shape::Merge {
            one_change(&mut rng, &mut fx, &members, false, idx)?;
            idx += 1;
            branch_no += 1;
            let branch = format!("feat-{branch_no}");
            fx.checkout_new(&branch, None).map_err(|e| e.to_string())?;
            for j in 0..2 {
                let subject = subject_for(idx + j, Some(branch_no));
                fx.commit_with(&CommitSpec::files(
                    change(&mut rng, &members, idx + j),
                    subject,
                ))
                .map_err(|e| e.to_string())?;
            }
            fx.checkout("main").map_err(|e| e.to_string())?;
            fx.merge_no_ff(&branch, &format!("Merge branch '{branch}'"))
                .map_err(|e| e.to_string())?;
            idx += 2;
        } else {
            for _ in 0..2 {
                one_change(&mut rng, &mut fx, &members, false, idx)?;
                idx += 1;
            }
            branch_no += 1;
            let mut heads = Vec::new();
            for b in 0..3 {
                let branch = format!("oct-{branch_no}-{b}");
                fx.checkout_new(&branch, None).map_err(|e| e.to_string())?;
                let subject = subject_for(idx + b, Some(branch_no));
                fx.commit_with(&CommitSpec::files(
                    change(&mut rng, &members, idx + b),
                    subject,
                ))
                .map_err(|e| e.to_string())?;
                heads.push(branch);
            }
            fx.checkout("main").map_err(|e| e.to_string())?;
            let head_refs: Vec<&str> = heads.iter().map(String::as_str).collect();
            fx.merge_octopus(&head_refs, &format!("Merge branches {}", heads.join(", ")))
                .map_err(|e| e.to_string())?;
            idx += 3;
        }
    }
    for _ in 0..tail {
        one_change(
            &mut rng,
            &mut fx,
            &members,
            opts.shape == Shape::Squash,
            idx,
        )?;
        idx += 1;
    }
    fx.head_oid()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "generated repo has no commits".to_owned())
}

fn pkg_name(member: &str) -> String {
    member.rsplit('/').next().unwrap_or(member).to_owned()
}

fn subject_for(idx: usize, branch: Option<usize>) -> String {
    let kind = TYPES.get(idx % TYPES.len()).copied().unwrap_or("feat");
    let desc = DESCS
        .get((idx / TYPES.len()) % DESCS.len())
        .copied()
        .unwrap_or("adjust inputs");
    match branch {
        Some(b) => format!("{kind}: {desc} [b{b}]"),
        None => format!("{kind}: {desc}"),
    }
}

/// Commit one logical change on the current branch.
fn one_change(
    rng: &mut SplitMix,
    fx: &mut GitFixture,
    members: &[String],
    squash: bool,
    idx: usize,
) -> Result<(), String> {
    let subject = subject_for(idx, None);
    let subject = if squash {
        format!("{subject} (#{})", idx + 1)
    } else {
        subject
    };
    fx.commit_with(&CommitSpec::files(change(rng, members, idx), subject))
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Files for one logical change: append deterministic content to one
/// package's lib.rs (and grow its manifest changelog section).
fn change(rng: &mut SplitMix, members: &[String], idx: usize) -> Vec<(String, String)> {
    let member = members
        .get(rng.below(members.len()))
        .cloned()
        .unwrap_or_default();
    let lines = idx * 2 + 4;
    vec![(format!("{member}/src/lib.rs"), lib_rs(lines))]
}

fn workspace_manifest(members: &[&str]) -> String {
    let mut list = String::new();
    for m in members {
        let _ = writeln!(list, "    \"{m}\",");
    }
    format!("[workspace]\nresolver = \"2\"\nmembers = [\n{list}\n]\n")
}

fn package_manifest(name: &str) -> String {
    format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n")
}

fn lib_rs(lines: usize) -> String {
    let mut src = String::new();
    for i in 0..lines {
        let _ = writeln!(src, "pub const C{i}: usize = {i};");
    }
    src
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    fn tmpdir(label: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "halyard-genrepo-test-{label}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("mkdir");
        d
    }

    #[test]
    fn same_seed_same_head() {
        let a_dir = tmpdir("same-a");
        let b_dir = tmpdir("same-b");
        let opts = |dir: &Path| GenRepoOpts {
            packages: 3,
            commits: 12,
            shape: Shape::Merge,
            seed: 42,
            out: dir.to_path_buf(),
        };
        let head_a = build(&opts(&a_dir)).expect("build a");
        let head_b = build(&opts(&b_dir)).expect("build b");
        assert_eq!(head_a, head_b, "same seed must yield identical OIDs");
        let _ = std::fs::remove_dir_all(&a_dir);
        let _ = std::fs::remove_dir_all(&b_dir);
    }

    #[test]
    fn different_seed_different_head() {
        let a_dir = tmpdir("diff-a");
        let b_dir = tmpdir("diff-b");
        let mut opts_a = GenRepoOpts {
            packages: 3,
            commits: 10,
            shape: Shape::Linear,
            seed: 1,
            out: a_dir.clone(),
        };
        let head_a = build(&opts_a).expect("build a");
        opts_a.seed = 2;
        opts_a.out = b_dir.clone();
        let head_b = build(&opts_a).expect("build b");
        assert_ne!(head_a, head_b);
        let _ = std::fs::remove_dir_all(&a_dir);
        let _ = std::fs::remove_dir_all(&b_dir);
    }

    #[test]
    fn merge_shape_produces_merges() {
        let dir = tmpdir("merge-shape");
        let opts = GenRepoOpts {
            packages: 2,
            commits: 8,
            shape: Shape::Merge,
            seed: 7,
            out: dir.clone(),
        };
        build(&opts).expect("build");
        let fx = GitFixture::init_in(&dir).expect("reopen");
        let merges = fx.rev_list(&["--merges", "HEAD"]).expect("rev-list");
        assert_eq!(merges.len(), 2, "8 logical changes → 2 merge cycles");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

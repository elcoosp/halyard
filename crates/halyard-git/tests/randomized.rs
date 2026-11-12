//! Randomized differential (M2.2 acceptance): pseudo-random DAG histories
//! painted by the engine and compared against `git rev-list` ground truth,
//! per package. The in-repo sample count keeps `cargo test` fast; the
//! 10 000-sample campaign runs through `xtask git-diff` (results recorded
//! in the PR body).

use std::collections::{BTreeSet, HashSet};

use halyard_core::package::{
    Ecosystem, GroupId, Package, PathFilter, RelPath, TagTemplate, VersionSource, Workspace,
};
use halyard_core::{HistoryProvider, PkgId};
use halyard_git::GitHistory;
use halyard_test_support::fixture::{CommitSpec, GitFixture};

/// A tiny deterministic LCG (splitmix-flavored) so every sample is
/// reproducible from its seed.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }

    fn below(&mut self, bound: u64) -> u64 {
        self.next() % bound.max(1)
    }
}

const SUBJECTS: [&str; 8] = [
    "feat: grow surface",
    "fix: mend edge",
    "perf: tighten loop",
    "feat(api): extend api",
    "fix(core): correct offset",
    "docs: note behavior",
    "chore: housekeeping",
    "feat!: reshape contract",
];

/// Builds one pseudo-random repo: commits on main, side branches, no-ff
/// merges, and `count` release tags on random commits.
fn build_random_repo(label: &str, seed: u64, commits: usize, tags: usize) -> Result<GitFixture, halyard_test_support::FixtureError> {
    let mut rng = Lcg(seed);
    let mut fx = GitFixture::new(label)?;
    let mut oids: Vec<String> = Vec::new();
    for i in 0..commits {
        let subject = SUBJECTS[(rng.below(SUBJECTS.len() as u64)) as usize];
        let spec = CommitSpec::files(
            vec![(
                format!("src/file{}.rs", rng.below(4)),
                format!("pub const V{i}: u32 = {};\n", rng.below(1000)),
            )],
            subject,
        );
        oids.push(fx.commit_with(&spec)?);
        // Occasionally branch + merge for non-linear history.
        if i > 0 && rng.below(5) == 0 {
            let branch = format!("side-{i}");
            fx.checkout_new(&branch, None)?;
            let subject = SUBJECTS[(rng.below(SUBJECTS.len() as u64)) as usize];
            let spec = CommitSpec::files(
                vec![(
                    format!("src/side{i}.rs",),
                    format!("pub const S: u32 = {};\n", rng.below(1000)),
                )],
                subject,
            );
            fx.commit_with(&spec)?;
            fx.checkout("main")?;
            fx.merge_no_ff(&branch, &format!("Merge branch '{branch}'"))?;
            let head = fx.head_oid()?.unwrap_or_default();
            oids.push(head);
        }
    }
    let mut seen: HashSet<String> = HashSet::new();
    let mut placed = 0usize;
    let mut tag_no = 0usize;
    while placed < tags && tag_no < tags * 4 {
        tag_no += 1;
        let oid = &oids[(rng.below(oids.len() as u64)) as usize];
        if seen.insert(oid.clone()) {
            fx.checkout(oid)?;
            fx.tag_light(&format!("v0.{tag_no}.0"))?;
            placed += 1;
        }
    }
    fx.checkout("main")?;
    Ok(fx)
}

fn single_pkg_workspace() -> Workspace {
    let mut ws = Workspace::new();
    let pkg = Package {
        id: PkgId::FIRST,
        name: ws.intern("app"),
        ecosystem: Ecosystem::Cargo,
        root: RelPath::new("").unwrap(),
        manifest: RelPath::new("Cargo.toml").unwrap(),
        version: Some(semver::Version::new(0, 1, 0)),
        version_source: VersionSource::Local,
        publishable: true,
        deps: Vec::new(),
        tag_template: TagTemplate::parse("v{version}").unwrap(),
        changelog: None,
        version_site: None,
        include: PathFilter::new(Vec::new(), Vec::new()),
        group: GroupId::new(0),
        facets: halyard_core::Facets::default(),
    };
    ws.push_package(pkg);
    ws
}

#[test]
fn randomized_histories_match_rev_list() {
    const SAMPLES: usize = 60;
    let mut checked = 0usize;
    for seed in 0..SAMPLES as u64 {
        let fx = build_random_repo("rand", seed, 8 + (seed % 5) as usize, 1 + (seed % 3) as usize)
            .unwrap_or_else(|e| panic!("seed {seed}: {e}"));
        let ws = single_pkg_workspace();
        let history = GitHistory::build(fx.root(), &ws, 10_000).unwrap();
        let digest = history.digest();
        // Ground truth per package (single package here): rev-list HEAD
        // ^base, or the full history for an initial release.
        let mut expected: BTreeSet<String> = BTreeSet::new();
        match digest.base_of(PkgId::new(0)) {
            Some(base) => {
                let tag = base.tag.to_string();
                let peeled = fx
                    .resolve(&format!("{tag}^{{commit}}"))
                    .unwrap_or_else(|_| fx.resolve(&tag).unwrap_or_default());
                expected.extend(fx.rev_list(&["HEAD", &format!("^{peeled}")]).unwrap_or_default());
            }
            None => {
                expected.extend(fx.rev_list(&["HEAD"]).unwrap_or_default());
            }
        }
        let expected_conventional: BTreeSet<String> = expected
            .into_iter()
            .filter(|oid| {
                let show = fx.run_git(&["show", "--format=%B", "-s", oid]).unwrap_or_default();
                halyard_conventional::parse(show.as_bytes()).is_some()
            })
            .collect();
        let got: BTreeSet<String> = digest.commits.iter().map(|c| c.oid.to_string()).collect();
        assert_eq!(got, expected_conventional, "seed {seed}");
        // Digest commit oids are unique (no double-emission).
        let unique: HashSet<String> = digest.commits.iter().map(|c| c.oid.to_string()).collect();
        assert_eq!(unique.len(), digest.commits.len(), "seed {seed}: duplicates");
        checked += 1;
    }
    assert_eq!(checked, SAMPLES);
}

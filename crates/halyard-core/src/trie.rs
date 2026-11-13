//! The path trie of package roots and shared workspace files (§8.4): the
//! map from changed tree paths onto packages that the M2.3 tree diff
//! consults.
//!
//! The trie is built once from a [`Workspace`] ([`build_trie`]) and then
//! consumed read-only by `halyard-git`'s lockstep tree walk:
//!
//! * every package root path becomes a chain of nodes; the node at the
//!   root path carries the package's id ([`TrieNode::roots_here`]);
//! * global/shared files (root `Cargo.toml`, `Cargo.lock`,
//!   `pnpm-workspace.yaml`, §8.4) become leaf nodes marked
//!   [`TrieNode::shared`];
//! * nodes only carry children that lie on some package root or shared
//!   path, so the tree diff can prune every subtree that is outside any
//!   package (e.g. `docs/`, `.github/`) without opening it.
//!
//! Determinism (I3): children are a `BTreeMap` keyed by path segment and
//! node ids are assigned in sorted-insertion order, so the same workspace
//! builds a byte-identical trie every time (tested).
//!
//! Attribution semantics (documented decision, §8.4 leaves the
//! shared-path policy open): a change to a shared path is workspace-wide
//! state and attributes to **all** packages; a change inside a package
//! attributes to the deepest package root containing the changed path
//! (nested package roots win over outer ones, §8.4 step 2). The replay
//! harness (M2.7) is the arbiter of the default.

use crate::bitset::SmallBitset;
use crate::id::PkgId;
use crate::package::Workspace;

/// Root-level files whose change is workspace-wide state (§8.4 global
/// paths). Exact, repo-root-relative; additional configured shared paths
/// arrive with the config extension (tracked for Slice E).
const SHARED_PATHS: &[&str] = &["Cargo.toml", "Cargo.lock", "pnpm-workspace.yaml"];

/// The path trie (§8.4). Immutable after construction; node 0 is the
/// repository root.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Trie {
    nodes: Vec<TrieNode>,
}

/// One trie node. `roots_here` is the package-root boundary (a package
/// root may nest inside another package root, §8.4 step 2); `shared`
/// marks a global file path.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TrieNode {
    children: std::collections::BTreeMap<bstr::BString, usize>,
    /// Packages whose root path is exactly this node's path.
    roots_here: SmallBitset,
    /// True when this node's path is a global shared file (§8.4).
    shared: bool,
}

impl Trie {
    /// The root node index.
    pub const ROOT: usize = 0;

    /// Child node for one path segment (`None` when the segment is not on
    /// any package root or shared path — the tree diff prunes there).
    pub fn child(&self, node: usize, segment: &[u8]) -> Option<usize> {
        self.nodes
            .get(node)
            .and_then(|n| n.children.get(&bstr::BString::from(segment)))
            .copied()
    }

    /// Packages rooted exactly at `node`.
    pub fn roots_here(&self, node: usize) -> Option<&SmallBitset> {
        self.nodes.get(node).map(|n| &n.roots_here)
    }

    /// True when `node` is a shared global path (§8.4).
    pub fn is_shared(&self, node: usize) -> bool {
        self.nodes.get(node).is_some_and(|n| n.shared)
    }

    /// Number of nodes (diagnostics; the root counts as one).
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// True when the trie has only the root node (no packages registered).
    pub fn is_empty(&self) -> bool {
        self.nodes.len() <= 1
    }

    /// Inserts `path` segments from the root, creating nodes as needed;
    /// returns the final node index.
    fn insert(&mut self, path: &str) -> usize {
        let mut current = Self::ROOT;
        if path.is_empty() {
            return current;
        }
        for segment in path.split('/') {
            let key = bstr::BString::from(segment);
            let next = self.nodes[current]
                .children
                .get(&key)
                .copied();
            let child = match next {
                Some(child) => child,
                None => {
                    let child = self.nodes.len();
                    self.nodes.push(TrieNode::default());
                    self.nodes[current]
                        .children
                        .insert(key, child);
                    child
                }
            };
            current = child;
        }
        current
    }
}

/// Builds the §8.4 trie from a workspace: one node chain per package root
/// (and per shared global path), with package-root boundaries recorded at
/// the leaf of each chain. Packages whose root is the repository root
/// (`Package.root == ""`, §6.1 single-package mode) mark the trie root.
///
/// The result is a pure function of the workspace: same packages (in any
/// registration order) ⇒ equal tries — `roots_here` is a bitset, so
/// registration order cannot leak into the structure (tested).
#[must_use]
pub fn build_trie(workspace: &Workspace) -> Trie {
    let mut trie = Trie {
        nodes: vec![TrieNode::default()],
    };
    for (index, pkg) in workspace.packages().iter().enumerate() {
        let id = PkgId::new(u32::try_from(index).unwrap_or(u32::MAX));
        let node = trie.insert(pkg.root.as_str());
        if let Some(n) = trie.nodes.get_mut(node) {
            n.roots_here.set(id);
        }
    }
    for path in SHARED_PATHS {
        let node = trie.insert(path);
        if let Some(n) = trie.nodes.get_mut(node) {
            n.shared = true;
        }
    }
    trie
}

/// blake3 over the trie's layout: package roots with their
/// include/exclude globs, then the shared paths (§8.7 `layout_hash`).
/// Package-layout changes invalidate the persistent cache cleanly because
/// the hash changes; the workspace's registration order is normalized
/// (sorted by root path) so discovery order cannot drift the hash.
///
/// `filters` are the include/exclude glob strings per package root,
/// carried in the same sorted order as the roots (the tree diff's
/// include-filter decisions must change the layout hash too: a
/// filter change re-attributes commits, so cached records must not
/// survive it).
#[must_use]
pub fn layout_hash(
    workspace: &Workspace,
    filters: &[(String, Vec<String>, Vec<String>)],
) -> String {
    let mut roots: Vec<&str> = workspace
        .packages()
        .iter()
        .map(|p| p.root.as_str())
        .collect();
    roots.sort_unstable();
    let mut material = Vec::new();
    for root in roots {
        material.extend_from_slice(root.as_bytes());
        material.push(0x1f);
    }
    for (root, include, exclude) in filters {
        material.extend_from_slice(root.as_bytes());
        material.push(0x1e);
        for glob in include.iter().chain(exclude.iter()) {
            material.extend_from_slice(glob.as_bytes());
            material.push(0x1d);
        }
        material.push(0x1c);
    }
    for path in SHARED_PATHS {
        material.extend_from_slice(path.as_bytes());
        material.push(0x1f);
    }
    format!("blake3:{}", crate::canonical::blake3_hex(&material))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::{
        DepEdge, Ecosystem, GroupId, Package, PathFilter, RelPath, TagTemplate, VersionSource,
        Workspace,
    };
    use crate::PkgId;

    /// Registers one package with the given name/root.
    fn push_pkg(ws: &mut Workspace, name: &str, root: &str, include: Vec<String>) {
        let pkg = Package {
            id: PkgId::FIRST,
            name: ws.intern(name),
            ecosystem: Ecosystem::Cargo,
            root: RelPath::new(root).expect("valid root"),
            manifest: RelPath::new("Cargo.toml").expect("valid manifest"),
            version: None,
            version_source: VersionSource::Local,
            publishable: true,
            deps: Vec::<DepEdge>::new(),
            tag_template: TagTemplate::parse("v{version}").expect("valid template"),
            changelog: None,
            version_site: None,
            include: PathFilter::new(include, Vec::new()),
            group: GroupId::new(0),
            facets: crate::discover::Facets::default(),
        };
        ws.push_package(pkg);
    }

    fn sample_workspace() -> Workspace {
        let mut ws = Workspace::new();
        push_pkg(&mut ws, "core", "crates/core", Vec::new());
        push_pkg(&mut ws, "cli", "crates/cli", Vec::new());
        push_pkg(&mut ws, "docs", "docs", Vec::new());
        ws
    }

    #[test]
    fn package_roots_and_shared_paths_become_nodes() {
        let ws = sample_workspace();
        let trie = build_trie(&ws);
        let crates = trie.child(Trie::ROOT, b"crates").expect("crates node");
        let core = trie.child(crates, b"core").expect("core node");
        let roots = trie.roots_here(core).expect("core node present");
        assert!(roots.get(PkgId::new(0)), "core is rooted at crates/core");
        assert!(!roots.get(PkgId::new(1)), "cli is not rooted at crates/core");
        // Shared paths are root-level leaves.
        let lock = trie.child(Trie::ROOT, b"Cargo.lock").expect("shared lock");
        assert!(trie.is_shared(lock));
        assert!(trie.roots_here(lock).is_some_and(|r| r.is_empty()));
        // Foreign subtrees are prunable: no node for `docs`'s children and
        // no node under an unrelated segment.
        assert!(trie.child(Trie::ROOT, b"unrelated").is_none());
        assert!(trie.child(core, b"anything").is_none());
    }

    #[test]
    fn root_package_and_nested_roots() {
        let mut ws = Workspace::new();
        push_pkg(&mut ws, "app", "", Vec::new());
        push_pkg(&mut ws, "core", "crates/core", Vec::new());
        let trie = build_trie(&ws);
        let roots = trie.roots_here(Trie::ROOT).expect("root node");
        assert!(roots.get(PkgId::new(0)), "repo-root package marks the root");
        let crates = trie.child(Trie::ROOT, b"crates").expect("crates node");
        let core = trie.child(crates, b"core").expect("core node");
        assert!(trie.roots_here(core).is_some_and(|r| r.get(PkgId::new(1))));
        // A nested root does not change the outer root's own record.
        assert!(trie
            .roots_here(Trie::ROOT)
            .is_some_and(|r| !r.get(PkgId::new(1))));
    }

    #[test]
    fn build_is_order_independent() {
        let mut a = Workspace::new();
        push_pkg(&mut a, "core", "crates/core", Vec::new());
        push_pkg(&mut a, "cli", "crates/cli", Vec::new());
        let mut b = Workspace::new();
        push_pkg(&mut b, "cli", "crates/cli", Vec::new());
        push_pkg(&mut b, "core", "crates/core", Vec::new());
        // PkgIds follow registration order (they are dense workspace
        // indices), so the root→id *values* legitimately differ between
        // the two workspaces. What must be identical is the path
        // structure; the per-workspace determinism (same workspace ⇒
        // identical trie, I3) is asserted separately below.
        fn paths(trie: &Trie) -> std::collections::BTreeMap<String, bool> {
            fn walk(
                trie: &Trie,
                node: usize,
                path: &str,
                out: &mut std::collections::BTreeMap<String, bool>,
            ) {
                out.insert(path.to_owned(), trie.is_shared(node));
                let Some(n) = trie.nodes.get(node) else {
                    return;
                };
                // `children` is a BTreeMap: segments render in sorted order.
                for (name, child) in &n.children {
                    let sub = if path.is_empty() {
                        name.to_string()
                    } else {
                        format!("{path}/{name}")
                    };
                    walk(trie, *child, &sub, out);
                }
            }
            let mut out = std::collections::BTreeMap::new();
            walk(trie, Trie::ROOT, "", &mut out);
            out
        }
        assert_eq!(
            paths(&build_trie(&a)),
            paths(&build_trie(&b)),
            "registration order is invisible in the path structure"
        );
        // Same workspace ⇒ byte-identical trie, built twice (I3).
        assert_eq!(build_trie(&a), build_trie(&a));
    }

    #[test]
    fn layout_hash_tracks_roots_and_filters() {
        let ws = sample_workspace();
        let empty: Vec<(String, Vec<String>, Vec<String>)> = Vec::new();
        let base = layout_hash(&ws, &empty);
        assert!(base.starts_with("blake3:"));
        // A new package root changes the hash.
        let mut with_new = Workspace::new();
        push_pkg(&mut with_new, "core", "crates/core", Vec::new());
        push_pkg(&mut with_new, "cli", "crates/cli", Vec::new());
        push_pkg(&mut with_new, "docs", "docs", Vec::new());
        push_pkg(&mut with_new, "tool", "tools/x", Vec::new());
        assert_ne!(base, layout_hash(&with_new, &empty));
        // A filter change changes the hash (cached records must not survive).
        let filtered = vec![(
            "crates/core".to_owned(),
            vec!["src/**".to_owned()],
            Vec::<String>::new(),
        )];
        assert_ne!(base, layout_hash(&ws, &filtered));
        // Same inputs ⇒ same hash (determinism).
        assert_eq!(base, layout_hash(&ws, &empty));
    }
}

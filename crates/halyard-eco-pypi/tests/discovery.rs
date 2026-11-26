//! M8 step 1 discovery contract tests (ADR-0056): the `halyard-eco-pypi`
//! adapter's `EcosystemAdapter::discover` over PEP 621 manifests and uv
//! workspaces (§6.1's third adapter row).
//!
//! The plan line these tests pin — `docs/HALYARD_ARCHITECTURE.md` §17,
//! **M8** scope cell: *"`PyPI` (`pyproject`, `uv`, PEP 440 strategy,
//! trusted publishing)"* with the acceptance *"each: discover/edit/
//! publish/resume tests + replay on 2 real repos"*. This file is the
//! **discover** half; `edit.rs` is the edit half; the publish/resume
//! half is M8 step 2 (the same ADR).

use halyard_core::EcosystemAdapter;
use halyard_core::discover::{DiscoverCtx, RawVersionSource};
use halyard_core::package::{DepKind, Ecosystem, ReqSpec};
use halyard_eco_pypi::PyPiAdapter;

type BoxError = Box<dyn std::error::Error>;

fn ctx_with(files: &[(&str, &str)]) -> DiscoverCtx {
    let mut ctx = DiscoverCtx::new();
    for (path, body) in files {
        assert!(ctx.insert_file(path, body.as_bytes().to_vec()), "{path}");
    }
    ctx
}

/// The M8 `PyPI` cell's red→green pin: a uv workspace (root + one globed
/// member + one excluded member) discovers into `Ecosystem::PyPi` rows —
/// the root (a workspace member per the uv grammar) plus the globed
/// member, with the excluded directory out, the workspace-internal
/// dependency resolved through `tool.uv.sources`, and the version site
/// pointed at the exact `version` string bytes.
#[test]
fn pypi_uv_workspace_discovery_plan_pin() -> Result<(), BoxError> {
    let ctx = ctx_with(&[
        (
            "pyproject.toml",
            "[project]\nname = \"albatross\"\nversion = \"0.1.0\"\nrequires-python = \">=3.12\"\ndependencies = [\"bird-feeder>=0.2\", \"tqdm>=4,<5\"]\n\n[tool.uv.sources]\nbird-feeder = { workspace = true }\n\n[tool.uv.workspace]\nmembers = [\"packages/*\"]\nexclude = [\"packages/seeds\"]\n",
        ),
        (
            "packages/bird-feeder/pyproject.toml",
            "[project]\nname = \"bird-feeder\"\nversion = \"0.2.0\"\ndependencies = [\"tqdm>=4,<5\"]\n",
        ),
        (
            "packages/seeds/pyproject.toml",
            "[project]\nname = \"seeds\"\nversion = \"0.0.1\"\n",
        ),
        (
            "packages/stray/pyproject.toml",
            "[project]\nname = \"stray\"\nversion = \"0.0.9\"\n",
        ),
    ]);
    let rows = PyPiAdapter.discover(&ctx)?;
    let mut names = rows.iter().map(|r| r.name.as_str()).collect::<Vec<_>>();
    names.sort_unstable();
    // The root is a member (uv: "Every workspace needs a root, which is
    // also a workspace member"); the glob "packages/*" matches
    // bird-feeder, seeds and stray; the exclude removes only seeds.
    assert_eq!(names, vec!["albatross", "bird-feeder", "stray"]);

    for row in &rows {
        assert_eq!(row.ecosystem, Ecosystem::PyPi);
    }

    let root = rows.iter().find(|r| r.name == "albatross").expect("root");
    assert_eq!(
        root.version.as_ref().map(std::string::ToString::to_string),
        Some("0.1.0".into())
    );
    assert_eq!(root.root, "");
    assert!(root.publishable);
    assert_eq!(root.version_source, RawVersionSource::Local);
    // The version site carries the exact value bytes of `version = "0.1.0"`.
    let site = root.version_site.as_ref().expect("version site");
    assert_eq!(site.file.to_string(), "pyproject.toml");
    let span = site.spans.first().expect("one span");
    let text = std::str::from_utf8(ctx.file("pyproject.toml").expect("root bytes"))?;
    assert_eq!(&text[span.start as usize..span.end as usize], "0.1.0");

    // The workspace-internal row rides tool.uv.sources (workspace = true);
    // the external row is registry-resolved.
    let internal: Vec<_> = root
        .deps
        .iter()
        .filter(|d| d.name == "bird-feeder")
        .collect();
    assert_eq!(internal.len(), 1);
    assert!(internal.first().expect("one row").internal);
    assert_eq!(internal.first().expect("one row").kind, DepKind::Normal);
    // PEP 508 specifiers are foreign grammar (ADR-0006): carried verbatim.
    assert!(matches!(
        internal.first().expect("one row").req,
        ReqSpec::Other { .. }
    ));
    let external = root
        .deps
        .iter()
        .find(|d| d.name == "tqdm")
        .expect("external row");
    assert!(!external.internal);

    let member = rows
        .iter()
        .find(|r| r.name == "bird-feeder")
        .expect("member");
    assert_eq!(member.root, "packages/bird-feeder");
    assert_eq!(member.manifest, "packages/bird-feeder/pyproject.toml");
    Ok(())
}

/// PEP 440 strategy (the M8 cell's named strategy): canonical release
/// versions plan; the recognized-but-non-canonical spellings (epoch,
/// post, dev, local, implicit `1.2`, leading zeros) fail closed with the
/// discovery warning and drop the version site — never guessed.
#[test]
fn pep440_strategy_fails_closed_on_non_canonical_spellings() -> Result<(), BoxError> {
    let mk = |version: &str| format!("[project]\nname = \"p\"\nversion = \"{version}\"\n");
    let adapter = PyPiAdapter;
    for (version, expected) in [
        ("1.2.3", true),
        ("0.0.0", true),
        ("1.2.3rc1", true),     // PEP 440 pre-release → semver -rc.1
        ("1.2.3a2", true),      // → -a.2
        ("1.2.3b10", true),     // → -b.10
        ("1!1.2.3", false),     // epoch
        ("1.2.3.post1", false), // post-release
        ("1.2.3.dev4", false),  // dev-release
        ("1.2.3+local", false), // local segment
        ("1.2", false),         // implicit release segment
        ("01.2.3", false),      // leading zero
        ("1.2.3rc", false),     // implicit pre-release number (not canonical)
        ("2021.4.19", true),    // calendar release — still three segments
    ] {
        let ctx = ctx_with(&[("pyproject.toml", &mk(version))]);
        let rows = adapter.discover(&ctx)?;
        assert_eq!(rows.len(), 1, "{version}");
        let row = rows.first().expect("one row");
        let ok = row.version.is_some() && row.version_site.is_some();
        assert_eq!(ok, expected, "version {version:?}");
        if expected {
            assert!(row.publishable, "{version}");
        } else {
            assert!(!row.publishable, "{version} must be unpublishable");
            assert!(
                row.warnings
                    .iter()
                    .any(|w| w.code == "non-canonical-version"),
                "{version} must warn non-canonical-version"
            );
        }
    }
    Ok(())
}

/// A `[project]` manifest with `dynamic = ["version"]` carries no static
/// version (PEP 621): warned, unpublishable, no version site — the npm
/// no-version posture at PEP 621's grammar.
#[test]
fn dynamic_version_is_warned_and_unpublishable() -> Result<(), BoxError> {
    let ctx = ctx_with(&[(
        "pyproject.toml",
        "[project]\nname = \"dyn\"\ndynamic = [\"version\"]\n",
    )]);
    let rows = PyPiAdapter.discover(&ctx)?;
    assert_eq!(rows.len(), 1);
    let row = rows.first().expect("one row");
    assert!(row.version.is_none());
    assert_eq!(row.version_source, RawVersionSource::Dynamic);
    assert!(row.version_site.is_none());
    assert!(!row.publishable);
    assert!(row.warnings.iter().any(|w| w.code == "dynamic-version"));
    Ok(())
}

/// `tool.uv.package = false` marks an application member (uv: "will force
/// a project package not to be built and installed") — halyard plans no
/// release publish for it (the npm `private: true` posture).
#[test]
fn uv_package_false_is_unpublishable() -> Result<(), BoxError> {
    let ctx = ctx_with(&[(
        "pyproject.toml",
        "[project]\nname = \"cli\"\nversion = \"1.0.0\"\n\n[tool.uv]\npackage = false\n",
    )]);
    let rows = PyPiAdapter.discover(&ctx)?;
    assert_eq!(rows.len(), 1);
    let row = rows.first().expect("one row");
    assert!(row.version.is_some(), "the version still exists");
    assert!(!row.publishable);
    Ok(())
}

/// Names match members through the `PyPA` normalization (case-insensitive,
/// runs of `-_.` → `-`): `Bird_Feeder` in tool.uv.sources resolves the
/// `bird-feeder` member, and normalized duplicate member names are the
/// orchestrator's E-DISC-DUPLICATE.
#[test]
fn member_names_match_through_normalization() -> Result<(), BoxError> {
    let ctx = ctx_with(&[
        (
            "pyproject.toml",
            "[project]\nname = \"root\"\nversion = \"0.1.0\"\ndependencies = [\"BIRD.feeder>=0.2\"]\n\n[tool.uv.sources]\n\"BIRD.feeder\" = { workspace = true }\n\n[tool.uv.workspace]\nmembers = [\"pkgs/*\"]\n",
        ),
        (
            "pkgs/bird-feeder/pyproject.toml",
            "[project]\nname = \"bird-feeder\"\nversion = \"0.2.0\"\n",
        ),
    ]);
    let rows = PyPiAdapter.discover(&ctx)?;
    let root = rows.iter().find(|r| r.name == "root").expect("root");
    let dep = root
        .deps
        .iter()
        .find(|d| d.name == "bird-feeder")
        .expect("dep");
    assert!(
        dep.internal,
        "normalized source name must resolve the member"
    );
    Ok(())
}

/// A PEP 621 manifest outside any uv workspace is still one package
/// (§6.1 single-package mode); a uv workspace member manifest missing
/// `version` is warned like any unnamed/empty version.
#[test]
fn orphan_manifest_is_a_single_package() -> Result<(), BoxError> {
    let ctx = ctx_with(&[(
        "pyproject.toml",
        "[project]\nname = \"solo\"\nversion = \"2.3.4\"\n",
    )]);
    let rows = PyPiAdapter.discover(&ctx)?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows.first().expect("one row").name, "solo");
    assert!(rows.first().expect("one row").publishable);
    Ok(())
}

/// A pyproject.toml with no `[project]` table (a tool-only file, e.g. a
/// black config at the repo root) is not a package — silently skipped,
/// like npm skips anonymous package.json workspace hosts... except here
/// there is nothing to host: no `[project]`, no package.
#[test]
fn tool_only_pyproject_is_skipped() -> Result<(), BoxError> {
    let ctx = ctx_with(&[("pyproject.toml", "[tool.black]\nline-length = 88\n")]);
    let rows = PyPiAdapter.discover(&ctx)?;
    assert_eq!(rows.len(), 0);
    Ok(())
}

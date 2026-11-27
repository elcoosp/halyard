//! Bench: the Maven pom edit seam (M2 of the Maven/Gradle cell, ADR-0058).
//! Measures the semantic in-place patch — parse, slot match, byte swap,
//! read-back verify — over the representative root pom (comments,
//! attributes, a modules block around the version element).

use criterion::Criterion;
use halyard_core::EcosystemAdapter;
use halyard_core::package::RelPath;
use halyard_core::plan::PlanPatch;
use halyard_eco_maven::MavenAdapter;

const ROOT_POM: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!-- the aggregator; pom packaging is required for multi-module builds -->
<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.example</groupId>
  <artifactId>bench-demo</artifactId>
  <version>1.4.0</version>
  <packaging>pom</packaging>
  <modules>
    <module>api</module>
    <module>web</module>
  </modules>
</project>
"#;

fn version_span(pom: &str) -> (u32, u32) {
    let marker = "<version>";
    let close = "</version>";
    // The fixture is a constant that must carry the version row; a miss
    // exits loudly (bench fns are not `#[test]`, so the test-lint
    // allowance does not reach them — the csproj_edit bench posture).
    let fail = |what: &str| -> ! {
        eprintln!("the bench fixture must carry {what}");
        std::process::exit(101);
    };
    let Some(open_idx) = pom.find(marker) else {
        fail(marker);
    };
    let value_start = open_idx + marker.len();
    let Some(close_idx) = pom
        .match_indices(close)
        .map(|(idx, _)| idx)
        .find(|idx| *idx >= value_start)
    else {
        fail(close);
    };
    let to_u32 = |value: usize, what: &str| u32::try_from(value).unwrap_or_else(|_| fail(what));
    (to_u32(value_start, marker), to_u32(close_idx, close))
}

fn edit() {
    // The fixture is a constant that must edit; a failure exits loudly
    // (bench fns are not `#[test]`, so the test-lint allowance does not
    // reach them — the csproj_edit bench posture).
    let fail = |what: &str| -> ! {
        eprintln!("the bench fixture must {what}");
        std::process::exit(101);
    };
    let Some(file) = RelPath::new("pom.xml") else {
        fail("carry a normalized fixture path");
    };
    let (start, end) = version_span(ROOT_POM);
    let patches = [PlanPatch {
        start,
        end,
        text: "1.5.0".to_owned(),
    }];
    match MavenAdapter::new().edit_versions(&file, ROOT_POM.as_bytes(), &patches) {
        Ok(out) if out.applied == 1 => {}
        _ => {
            fail("edit cleanly");
        }
    }
}

fn bench(c: &mut Criterion) {
    c.bench_function("pom_edit_in_place", |b| b.iter(edit));
}

/// The bench entry point (the criterion macros generate undocumented
/// items — the workspace's missing-docs gate forbids them; the sibling
/// benches call the group directly).
fn main() {
    bench(&mut Criterion::default().configure_from_args());
}

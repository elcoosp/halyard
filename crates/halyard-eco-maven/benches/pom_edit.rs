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
    let start = pom.find(marker).expect("fixture carries the version") + marker.len();
    let end = pom[start..].find("</version>").expect("fixture closes") + start;
    (start as u32, end as u32)
}

fn edit() {
    let file = RelPath::new("pom.xml").expect("fixture path");
    let (start, end) = version_span(ROOT_POM);
    let patches = [PlanPatch {
        start,
        end,
        text: "1.5.0".to_owned(),
    }];
    let out = MavenAdapter::new()
        .edit_versions(&file, ROOT_POM.as_bytes(), &patches)
        .expect("bench fixture edits");
    assert_eq!(out.applied, 1);
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

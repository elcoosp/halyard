//! Bench: Maven pom scan (§6.4's hot path at the pom.xml grammar, M8
//! Maven/Gradle cell, ADR-0058). Measures the hand-rolled scanner + row
//! assembly over a representative multi-module workspace (root
//! aggregator + three member poms with parent blocks, dependency rows,
//! comments, and one inherited version).

use criterion::Criterion;
use halyard_core::EcosystemAdapter;
use halyard_core::discover::DiscoverCtx;
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
    <module>libs/metrics</module>
  </modules>
</project>
"#;

const API_POM: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <parent>
    <groupId>com.example</groupId>
    <artifactId>bench-demo</artifactId>
    <version>1.4.0</version>
  </parent>
  <artifactId>bench-demo-api</artifactId>
  <dependencies>
    <dependency>
      <groupId>com.example</groupId>
      <artifactId>bench-demo-web</artifactId>
      <version>1.4.0</version>
    </dependency>
    <dependency>
      <groupId>com.example</groupId>
      <artifactId>bench-demo-metrics</artifactId>
      <version>1.4.0</version>
    </dependency>
    <!-- an external coordinate stays registry-resolved -->
    <dependency>
      <groupId>org.apache.commons</groupId>
      <artifactId>commons-lang3</artifactId>
      <version>3.14.0</version>
    </dependency>
  </dependencies>
</project>
"#;

const WEB_POM: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <parent>
    <groupId>com.example</groupId>
    <artifactId>bench-demo</artifactId>
    <version>1.4.0</version>
  </parent>
  <artifactId>bench-demo-web</artifactId>
  <version>1.4.0</version>
</project>
"#;

const METRICS_POM: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <parent>
    <groupId>com.example</groupId>
    <artifactId>bench-demo</artifactId>
    <version>1.4.0</version>
  </parent>
  <artifactId>bench-demo-metrics</artifactId>
  <!-- inherited from the parent (the documented inheritance list) -->
</project>
"#;

fn ctx() -> DiscoverCtx {
    let mut ctx = DiscoverCtx::new();
    ctx.insert_file("pom.xml", ROOT_POM.as_bytes().to_vec());
    ctx.insert_file("api/pom.xml", API_POM.as_bytes().to_vec());
    ctx.insert_file("web/pom.xml", WEB_POM.as_bytes().to_vec());
    ctx.insert_file("libs/metrics/pom.xml", METRICS_POM.as_bytes().to_vec());
    ctx
}

fn scan() {
    let ctx = ctx();
    // The fixture is a constant that must parse; a parse failure exits
    // the bench loudly (bench fns are not `#[test]`, so the test-lint
    // allowance does not reach them — the sibling pypi bench posture).
    let rows = match MavenAdapter::new().discover(&ctx) {
        Ok(rows) => rows,
        Err(error) => {
            eprintln!("the bench fixture must parse: {error}");
            std::process::exit(101);
        }
    };
    assert_eq!(rows.len(), 4);
}

fn bench(c: &mut Criterion) {
    c.bench_function("pom_scan_3_modules", |b| b.iter(scan));
}

/// The bench entry point (the criterion macros generate undocumented
/// items — the workspace's missing-docs gate forbids them; the sibling
/// benches call the group directly).
fn main() {
    bench(&mut Criterion::default().configure_from_args());
}

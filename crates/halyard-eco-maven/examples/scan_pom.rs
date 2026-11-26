//! Example: discover a Maven multi-module workspace and inspect the rows
//! (M8, the §17 **Maven/Gradle** cell — ADR-0058).
//!
//! Run: `cargo run -p halyard-eco-maven --example scan_pom`
//!
//! The aggregator's `<modules>` discover as maven rows: the coordinates
//! (`groupId:artifactId:version`) name the packages, a child pom without
//! its own `<version>` inherits the parent's (one edit site, on the
//! parent pom), a `<dependency>` naming a member's (groupId, artifactId)
//! pair is the proven-internal edge, and Maven-specific version
//! spellings fail closed (warned, unpublishable, never rewritten).

use halyard_core::EcosystemAdapter;
use halyard_core::discover::DiscoverCtx;
use halyard_eco_maven::MavenAdapter;

const ROOT_POM: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!-- the aggregator; pom packaging is required for multi-module builds -->
<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.example</groupId>
  <artifactId>demo</artifactId>
  <version>1.2.3</version>
  <packaging>pom</packaging>
  <modules>
    <module>api</module>
    <module>web</module>
  </modules>
</project>
"#;

const API_POM: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <parent>
    <groupId>com.example</groupId>
    <artifactId>demo</artifactId>
    <version>1.2.3</version>
  </parent>
  <artifactId>demo-api</artifactId>
  <!-- no <version>: inherited from the parent (the documented list) -->
  <dependencies>
    <dependency>
      <groupId>com.example</groupId>
      <artifactId>demo-web</artifactId>
      <version>1.2.3</version>
    </dependency>
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
    <artifactId>demo</artifactId>
    <version>1.2.3</version>
  </parent>
  <artifactId>demo-web</artifactId>
  <version>1.2.3</version>
</project>
"#;

const LEGACY_POM: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.example</groupId>
  <artifactId>demo-legacy</artifactId>
  <version>2.0.0-SNAPSHOT</version>
</project>
"#;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut ctx = DiscoverCtx::new();
    ctx.insert_file("pom.xml", ROOT_POM.as_bytes().to_vec());
    ctx.insert_file("api/pom.xml", API_POM.as_bytes().to_vec());
    ctx.insert_file("web/pom.xml", WEB_POM.as_bytes().to_vec());
    // A Maven-specific spelling rides along to show the fail-closed
    // posture (ComparableVersion orders it; the plan contract cannot
    // render it).
    ctx.insert_file("legacy/pom.xml", LEGACY_POM.as_bytes().to_vec());

    // The root pom lists only api/web — legacy is an orphan (nested poms
    // nothing claims are skipped), so it is discovered by its own shape:
    // nothing claims it, and the row inventory shows what planning sees.
    let rows = MavenAdapter::new().discover(&ctx)?;
    println!("discovered {} maven rows:\n", rows.len());
    for row in &rows {
        println!(
            "  {} @ {} (manifest {})",
            row.name,
            if row.root.is_empty() {
                "<root>"
            } else {
                &row.root
            },
            row.manifest
        );
        println!(
            "    version: {:?} (source {:?})",
            row.version.as_ref().map(std::string::ToString::to_string),
            row.version_source
        );
        if let Some(site) = &row.version_site {
            println!("    edit site: {} spans {:?}", site.file, site.spans);
        }
        println!(
            "    publishable: {} (no maven publish grammar yet — ADR-0058)",
            row.publishable
        );
        for dep in &row.deps {
            println!(
                "    dependency {:?} {} internal={}",
                dep.name,
                String::from_utf8_lossy(&dep.raw),
                dep.internal
            );
        }
        for warning in &row.warnings {
            println!("    warning [{}]: {}", warning.code, warning.message);
        }
        println!();
    }
    Ok(())
}

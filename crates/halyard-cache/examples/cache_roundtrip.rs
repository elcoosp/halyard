//! The persistent per-commit cache's write/read contract (§8.7, M2.5):
//! a workspace's attributions cached under `$GIT_DIR/halyard/cache/v1/`,
//! flushed, reopened from disk and read back — the exact shape the
//! planning pipeline rides (`CacheKey::compute` over the commit OID +
//! layout/config hashes, `CacheRecord`'s touched bitset + header
//! offsets). Offline, over a scratch git directory.
//!
//! Run: `cargo run -p halyard-cache --example cache_roundtrip`

use std::path::PathBuf;

use halyard_cache::{Cache, CacheKey, CacheRecord};
use halyard_core::{PkgId, SmallBitset};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // The scratch `$GIT_DIR` — `Cache::open` creates the
    // `halyard/cache/v1/` layout under it and writes the header on a
    // fresh file.
    let dir = std::env::temp_dir().join("halyard-cache-roundtrip-example");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    let git_dir = PathBuf::from(&dir);

    // The first walk: two commits' attributions cached.
    let mut cache = Cache::open(&git_dir)?;
    let key_a = CacheKey::compute(b"9f2ca67", "layout-1", "config-1");
    let key_b = CacheKey::compute(b"ab12cd3", "layout-1", "config-1");
    let mut touched = SmallBitset::new();
    touched.set(PkgId::FIRST);
    cache.insert(
        key_a.clone(),
        CacheRecord {
            touched: touched.clone(),
            subject_end: 24,
            body_start: 25,
        },
    )?;
    cache.insert(
        key_b.clone(),
        CacheRecord {
            touched: SmallBitset::new(),
            subject_end: 18,
            body_start: 0,
        },
    )?;
    cache.flush()?;
    println!(
        "written: {} records, {} bytes on disk",
        cache.len(),
        cache.size_bytes()
    );

    // The re-opened walk (the process boundary): the §8.7 read path —
    // identical keys over the same layout/config hashes hit the cache.
    let reopened = Cache::open(&git_dir)?;
    let hit = reopened
        .get(&key_a)
        .ok_or("the flushed record must read back")?;
    println!(
        "hit: {} touched package(s), subject ends at byte {}, body starts at {}",
        hit.touched.count(),
        hit.subject_end,
        hit.body_start
    );
    let miss = reopened.get(&CacheKey::compute(b"0000000", "layout-1", "config-1"));
    println!("miss on a foreign key: {miss:?} (the honest None — recompute)");
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

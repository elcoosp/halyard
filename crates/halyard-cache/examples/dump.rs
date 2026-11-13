//! `dump` (M2.5 example): opens the persistent per-commit cache at
//! `$GIT_DIR/halyard/cache/v1/`, prints the header summary (format
//! version, record count, valid size, hit-rate) and the first 10 keys.
//!
//! Run:
//!
//! ```sh
//! cargo run -p halyard-cache --example dump -- /path/to/repo/.git
//! ```

use std::path::PathBuf;

use halyard_cache::Cache;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let Some(git_dir) = args.next().map(PathBuf::from) else {
        return Err("usage: dump <git-dir> (the cache lives at \
                    <git-dir>/halyard/cache/v1/)"
            .into());
    };
    let cache = Cache::open(&git_dir).map_err(|e| e.to_string())?;
    let (hits, misses) = cache.hit_rate();
    let total = hits + misses;
    println!("dir        : {}", Cache::cache_dir(&git_dir).display());
    println!("records    : {}", cache.len());
    println!("valid bytes: {}", cache.size_bytes());
    println!(
        "hit-rate   : {:.1}% ({} hits / {} misses since last flush)",
        if total == 0 {
            0.0
        } else {
            f64::from(u32::try_from(hits).unwrap_or(u32::MAX)) * 100.0
                / f64::from(u32::try_from(total).unwrap_or(u32::MAX))
        },
        hits,
        misses,
    );
    println!("empty      : {}", cache.is_empty());
    Ok(())
}

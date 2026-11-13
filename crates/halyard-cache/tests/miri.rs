//! Miri suite for the cache crate (M2.5, §15.1 soundness layer).
//!
//! Run: `cargo +nightly miri test -p halyard-cache --features miri`
//!
//! The real `mmap(2)` syscall is outside Miri's isolated execution model
//! (no real file mappings in the interpreter), so under `cfg(miri)` /
//! `--features miri` the `Mmap::open` shim returns `None` and every cache
//! operation runs through the `read()`-backed shadow buffer. That still
//! exercises, under Miri's full checking:
//!
//! * the slice-projection contract shared by both read paths
//!   (`bytes()` → `as_slice` semantics),
//! * the record encode/decode byte handling (no out-of-bounds slicing on
//!   hostile payloads),
//! * the full open/insert/flush/reopen lifecycle incl. torn-tail skip.
//!
//! The genuine `mmap`/`munmap` pair on real files is covered by the
//! regular (non-Miri) suite (`tests/cache.rs` + `crate::mmap` module
//! tests) on every CI run.

#![cfg(any(miri, feature = "miri"))]

use halyard_cache::{Cache, CacheKey, CacheRecord};
use halyard_core::{PkgId, SmallBitset};

fn record(ids: &[u32]) -> CacheRecord {
    let mut touched = SmallBitset::new();
    for id in ids {
        touched.set(PkgId::new(*id));
    }
    CacheRecord {
        touched,
        subject_end: 33,
        body_start: 35,
    }
}

#[test]
fn miri_full_lifecycle_roundtrip() {
    let dir = tempfile::tempdir().expect("tempdir");
    let git = dir.path().join("git");
    std::fs::create_dir_all(&git).expect("mkdir");
    let key = CacheKey::compute(
        b"bbbb2222bbbb2222bbbb2222bbbb2222bbbb2222",
        "blake3:layout",
        "blake3:cfg",
    );
    let mut cache = Cache::open(&git).expect("open");
    cache
        .insert(key.clone(), record(&[0, 64, 129]))
        .expect("insert");
    cache.flush().expect("flush");
    drop(cache);
    let reopened = Cache::open(&git).expect("reopen");
    let got = reopened.get(&key).expect("hit");
    assert_eq!(got.touched.count(), 3);
    assert_eq!(got.subject_end, 33);
}

#[test]
fn miri_torn_tail_skip() {
    let dir = tempfile::tempdir().expect("tempdir");
    let git = dir.path().join("git");
    std::fs::create_dir_all(&git).expect("mkdir");
    let keys: Vec<CacheKey> = (0..3u8)
        .map(|n| CacheKey::compute(&[n; 40], "blake3:layout", "blake3:cfg"))
        .collect();
    let mut cache = Cache::open(&git).expect("open");
    for (n, key) in keys.iter().enumerate() {
        let torn = n == 2;
        let pkg = u32::try_from(n).unwrap_or(u32::MAX);
        cache
            .insert_with_failpoint(key.clone(), record(&[pkg]), &|_| torn)
            .expect("insert");
        if torn {
            break;
        }
    }
    drop(cache);
    let reopened = Cache::open(&git).expect("reopen skips torn tail");
    assert_eq!(reopened.len(), 2);
    assert!(reopened.get(keys.first().expect("three keys")).is_some());
    assert!(reopened.get(keys.get(2).expect("three keys")).is_none());
}

#[test]
fn miri_record_bytes_stay_in_bounds() {
    // Hostile payloads through the decode path: no out-of-bounds slicing,
    // no panics (the corpus for the corruption-fuzz target mirrors this).
    let rec = record(&[0, 7, 200]);
    let encoded = rec.encode();
    for cut in 0..encoded.len() {
        let _ = CacheRecord::decode(encoded.get(..cut).unwrap_or(&[]));
    }
    let mut overflow = encoded.clone();
    overflow.push(0xFF);
    overflow.push(0xFF);
    overflow.push(0xFF);
    overflow.push(0xFF);
    overflow.push(0xFF);
    let _ = CacheRecord::decode(&overflow);
}

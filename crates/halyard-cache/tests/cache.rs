//! Cache lifecycle tests (M2.5, §8.7): open/get/insert/flush roundtrip,
//! determinism (run twice, byte-compare), torn-tail recovery via the
//! injectable failpoint, and a **real** kill-during-write test that
//! re-executes this test binary as a child process, has it abort()
//! mid-record, and asserts the reopened cache skips the truncated tail
//! and stays re-runnable (I2).

use std::path::Path;

use halyard_cache::{Cache, CacheError, CacheKey, CacheRecord};
use halyard_core::PkgId;
use halyard_core::SmallBitset;

fn bitset(ids: &[u32]) -> SmallBitset {
    let mut set = SmallBitset::new();
    for id in ids {
        set.set(PkgId::new(*id));
    }
    set
}

fn record(ids: &[u32]) -> CacheRecord {
    CacheRecord {
        touched: bitset(ids),
        subject_end: 17,
        body_start: 19,
    }
}

fn temp_git_dir() -> std::path::PathBuf {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("git");
    std::fs::create_dir_all(&path).expect("mkdir");
    // Leak the tempdir guard on purpose: the fixtures below need the
    // directory for the whole test, and the test process cleans up /tmp.
    std::mem::forget(dir);
    path
}

#[test]
fn roundtrip_and_counters() {
    let git = temp_git_dir();
    let key = CacheKey::compute(b"aaaa1111aaaa1111aaaa1111aaaa1111aaaa1111", "blake3:layout", "blake3:cfg");
    let mut cache = Cache::open(&git).expect("open");
    assert!(cache.is_empty());
    cache.insert(key.clone(), record(&[0, 1, 130])).expect("insert");
    assert_eq!(cache.len(), 1);
    cache.flush().expect("flush");

    // Re-open: the record survives the process boundary.
    let reopened = Cache::open(&git).expect("reopen");
    let got = reopened.get(&key).expect("hit after reopen");
    assert_eq!(got.touched, bitset(&[0, 1, 130]));
    assert_eq!((got.subject_end, got.body_start), (17, 19));
    let (hits, misses) = reopened.hit_rate();
    assert_eq!((hits, misses), (1, 0), "counters survive the header roundtrip");

    // A miss counts too.
    let absent = CacheKey::compute(b"aaaa1111aaaa1111aaaa1111aaaa1111aaaa1112", "blake3:layout", "blake3:cfg");
    assert!(reopened.get(&absent).is_none());
    assert_eq!(reopened.hit_rate(), (1, 1));
}

#[test]
fn determinism_run_twice_byte_compares() {
    // Two independent caches fed the same records must be byte-identical
    // after flush (I3: the record encoding is deterministic, no clocks).
    let a = temp_git_dir();
    let b = temp_git_dir();
    let mut first = Cache::open(&a).expect("open a");
    let mut second = Cache::open(&b).expect("open b");
    for n in 0..7u8 {
        let key = CacheKey::compute(&[n; 40], "blake3:layout", "blake3:cfg");
        first.insert(key.clone(), record(&[u32::from(n), 200])).expect("insert");
        second.insert(key, record(&[u32::from(n), 200])).expect("insert");
    }
    first.flush().expect("flush");
    second.flush().expect("flush");
    let bytes_a = std::fs::read(Cache::cache_dir(&a).join("cache.bin")).expect("read a");
    let bytes_b = std::fs::read(Cache::cache_dir(&b).join("cache.bin")).expect("read b");
    assert_eq!(bytes_a, bytes_b, "same inputs ⇒ byte-identical cache file");
}

#[test]
fn failpoint_torn_tail_is_skipped_and_recoverable() {
    let git = temp_git_dir();
    let keys: Vec<CacheKey> = (0..5u8)
        .map(|n| CacheKey::compute(&[n; 40], "blake3:layout", "blake3:cfg"))
        .collect();
    // Write 2 records, then the 3rd "dies" mid-record — and the writing
    // session ends there (a real crash cannot keep appending).
    let mut cache = Cache::open(&git).expect("open");
    for (n, key) in keys.iter().enumerate() {
        let torn = n == 2;
        cache
            .insert_with_failpoint(key.clone(), record(&[n as u32]), &|_| torn)
            .expect("insert");
        if torn {
            break;
        }
    }
    // No flush: the process died with the torn tail on disk.
    drop(cache);

    // Reopen: records 0, 1 are readable, the torn 2 and the never-written
    // 3/4 are absent — and inserting again works.
    let mut reopened = Cache::open(&git).expect("reopen skips torn tail");
    assert_eq!(reopened.len(), 2, "records after the torn one are skipped");
    assert!(reopened.get(&keys[0]).is_some());
    assert!(reopened.get(&keys[1]).is_some());
    for key in keys.iter().skip(2) {
        assert!(reopened.get(key).is_none());
    }
    reopened.insert(keys[4].clone(), record(&[4])).expect("re-runnable");
    reopened.flush().expect("flush again");
    let reopened_final = Cache::open(&git).expect("final open");
    assert_eq!(reopened_final.len(), 3);
}

#[test]
fn kill_during_write_leaves_a_recoverable_cache() {
    let helper_env = "HALYARD_CACHE_KILL_HELPER";
    let git_env = "HALYARD_CACHE_KILL_GITDIR";
    if std::env::var_os(helper_env).is_some() {
        // Child mode: write 4 records with a torn 3rd, then abort() —
        // the exact on-disk state a SIGKILL mid-write produces.
        let git = std::path::PathBuf::from(std::env::var(git_env).expect("gitdir env"));
        let mut cache = Cache::open(&git).expect("child open");
        for n in 0..4u8 {
            let key = CacheKey::compute(&[n; 40], "blake3:layout", "blake3:cfg");
            let torn = n == 2;
            cache
                .insert_with_failpoint(key, record(&[u32::from(n)]), &|_| torn)
                .expect("child insert");
            if torn {
                // A real kill cannot keep appending: stop the session.
                break;
            }
        }
        // Deliberately no flush: the process dies with the torn tail on
        // disk (I2 failure-injection matrix, §15.2).
        std::process::abort();
    }
    let git = temp_git_dir();
    let exe = std::env::current_exe().expect("current exe");
    let status = std::process::Command::new(exe)
        .env(helper_env, "1")
        .env(git_env, &git)
        // Run ONLY this test in the child: the libtest harness otherwise
        // executes the whole suite (and this test again), recursing.
        .args([
            "--exact",
            "kill_during_write_leaves_a_recoverable_cache",
            "--test-threads=1",
        ])
        .status()
        .expect("spawn kill-helper");
    assert!(
        !status.success(),
        "the helper must die by signal (abort), not exit cleanly"
    );
    // The reopened cache skips the torn tail and is re-runnable.
    let mut reopened = Cache::open(&git).expect("reopen after real kill");
    assert_eq!(reopened.len(), 2, "records before the torn one survived");
    let key = CacheKey::compute(&[9; 40], "blake3:layout", "blake3:cfg");
    reopened.insert(key.clone(), record(&[9])).expect("re-run insert");
    reopened.flush().expect("re-run flush");
    let reopened_final = Cache::open(&git).expect("final open");
    assert!(reopened_final.get(&key).is_some());
}

#[test]
fn bad_header_is_an_actionable_error() {
    let git = temp_git_dir();
    let path = Cache::cache_dir(&git).join("cache.bin");
    std::fs::create_dir_all(Cache::cache_dir(&git)).expect("mkdir");
    std::fs::write(&path, b"NOTACACHEFILE!!!!!!!!!!!!!!!!!!!!!!!!!!").expect("write junk");
    let err = Cache::open(&git).unwrap_err();
    assert_eq!(err.code(), "E-CACHE-HEADER");
    assert!(err.to_string().contains("safe to delete"));
}

#[test]
fn zero_byte_file_is_initialized() {
    let git = temp_git_dir();
    std::fs::create_dir_all(Cache::cache_dir(&git)).expect("mkdir");
    std::fs::write(Cache::cache_dir(&git).join("cache.bin"), b"").expect("touch empty");
    let cache = Cache::open(&git).expect("empty file initializes");
    assert!(cache.is_empty());
}

#[test]
fn corrupt_records_never_kill_the_scan() {
    eprintln!("T1 open");
    let git = temp_git_dir();
    {
        eprintln!("T2 first open");
        let mut cache = Cache::open(&git).expect("open");
        eprintln!("T3 inserting");
        for n in 0..4u8 {
            cache
                .insert(CacheKey::compute(&[n; 40], "blake3:layout", "blake3:cfg"), record(&[u32::from(n)]))
                .expect("insert");
        }
        eprintln!("T4 flushing");
        cache.flush().expect("flush");
        eprintln!("T5 dropping");
    }
    eprintln!("T6 flipped reopen");
    // Flip one byte inside a record payload (after the header): the scan
    // skips the corrupted record (and anything after it — the chain is
    // length-delimited), the rest reads fine.
    let path = Cache::cache_dir(&git).join("cache.bin");
    let mut bytes = std::fs::read(&path).expect("read");
    let at = 28 + 40; // header + first record length/crc fields
    let before = bytes.get(at).copied().expect("byte exists");
    bytes[at] = before ^ 0xFF;
    std::fs::write(&path, &bytes).expect("write back");
    let cache = Cache::open(&git).expect("open skips corruption");
    assert!(
        cache.len() < 4,
        "the corrupted chain segment is skipped, not fatal"
    );
    // And the file stays usable.
    let _ = Cache::open(&git).expect("re-openable");
}

#[test]
fn error_codes_are_stable() {
    let open_err = CacheError::Open {
        path: "/tmp/x".to_owned(),
        message: "nope".to_owned(),
    };
    assert_eq!(open_err.code(), "E-CACHE-OPEN");
    let lock_err = CacheError::Lock {
        path: "/tmp/x".to_owned(),
        message: "busy".to_owned(),
    };
    assert_eq!(lock_err.code(), "E-CACHE-LOCK");
}

#[test]
fn cache_dir_layout_matches_spec() {
    let git = Path::new("/repo/.git");
    assert_eq!(
        Cache::cache_dir(git),
        Path::new("/repo/.git/halyard/cache/v1")
    );
}

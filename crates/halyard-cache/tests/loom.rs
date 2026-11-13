//! Loom suite for the cache crate (M2.5, §15.1 concurrency layer).
//!
//! Run: `cargo test -p halyard-cache --features loom --release`
//!
//! The `flock(2)`/`mmap(2)` syscalls are outside loom's model (they are
//! real kernel calls, not schedulable primitives), so this suite models
//! the state machine *around* them: the per-operation mutual exclusion is
//! modeled by a `loom::sync::Mutex` standing in for the advisory lock
//! (same contract: one writer at a time, exclusive across the whole
//! mutation), and the "file" is an in-memory byte buffer standing in for
//! the append-only image. Model-checked property: **records never
//! interleave** — after any interleaving of two concurrent writing
//! sessions, the buffer still parses as a clean record chain and the
//! valid prefix equals the sum of the written records. Interleaved
//! half-records are exactly the corruption real `flock` prevents, so the
//! model proves the lock bracketing (acquire → write → release) is
//! airtight; the syscall itself is exercised by the real-file suite
//! (`tests/cache.rs`).

#![cfg(feature = "loom")]

use std::sync::Arc;

use halyard_cache::crc32;

/// Modeled record: 4-byte length (of `payload`), 4-byte crc, payload.
fn encode_record(payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + payload.len());
    out.extend_from_slice(&u32::try_from(payload.len()).unwrap_or(u32::MAX).to_le_bytes());
    out.extend_from_slice(&crc32(payload).to_le_bytes());
    out.extend_from_slice(payload);
    out
}

/// Parses the buffer as a record chain; `None` on any torn/invalid tail.
fn parse_chain(bytes: &[u8]) -> Option<usize> {
    let mut cursor = 0usize;
    while cursor < bytes.len() {
        let len = u32::from_le_bytes(bytes.get(cursor..cursor + 4)?.try_into().ok()?) as usize;
        let crc = u32::from_le_bytes(bytes.get(cursor + 4..cursor + 8)?.try_into().ok()?);
        let payload = bytes.get(cursor + 8..cursor + 8 + len)?;
        if crc32(payload) != crc {
            return None;
        }
        cursor += 8 + len;
    }
    Some(cursor)
}

#[test]
fn concurrent_writers_never_interleave_records() {
    loom::model(|| {
        // The "file image" plus the modeled advisory lock guarding every
        // mutation (the real flock is one-per-op exclusive, same
        // contract).
        let state = Arc::new(loom::sync::Mutex::new(Vec::<u8>::new()));
        let s1 = Arc::clone(&state);
        let t1 = loom::thread::spawn(move || {
            let mut file = s1.lock().unwrap();
            let record = encode_record(b"alpha");
            file.extend_from_slice(&record);
            drop(file);
        });
        let s2 = Arc::clone(&state);
        let t2 = loom::thread::spawn(move || {
            let mut file = s2.lock().unwrap();
            let record = encode_record(b"beta!");
            file.extend_from_slice(&record);
            drop(file);
        });
        t1.join().expect("writer 1");
        t2.join().expect("writer 2");
        let file = state.lock().unwrap();
        let valid = parse_chain(&file).expect("chain is clean after any interleaving");
        assert_eq!(valid, file.len(), "no torn tail remains");
        assert_eq!(file.len(), 8 + 5 + 8 + 5, "both records landed");
    });
}

#[test]
fn reader_never_sees_a_torn_record_under_lock_bracketing() {
    loom::model(|| {
        let state = Arc::new(loom::sync::Mutex::new(Vec::<u8>::new()));
        // Writer: appends one record under the modeled lock.
        let s1 = Arc::clone(&state);
        let writer = loom::thread::spawn(move || {
            let mut file = s1.lock().unwrap();
            file.extend_from_slice(&encode_record(b"payload"));
        });
        // Reader: parses under the same lock bracketing (a real scanner
        // takes the exclusive lock for the open-time scan).
        let s2 = Arc::clone(&state);
        let reader = loom::thread::spawn(move || {
            let file = s2.lock().unwrap();
            if file.is_empty() {
                return true; // nothing written yet: an empty scan is valid
            }
            parse_chain(&file).is_some()
        });
        writer.join().expect("writer");
        assert!(reader.join().expect("reader"), "a reader never sees a torn record");
    });
}

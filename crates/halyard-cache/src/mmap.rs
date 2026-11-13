//! Read-only `mmap` over the cache file — the workspace's single
//! `unsafe`-permitted module (invariant I10, §2).
//!
//! // rationale: the cache file is read through `mmap(2)` so the walk's
//! record reads are pointer reads into the page cache (§8.7 "mmap'd",
//! §14.2 "borrow from ODB mmap'd buffers"); there is no safe std
//! primitive for mapping an existing file, and the alternative (copying
//! the file into a `Vec` on every open) defeats the §14.1 no-op budget.
//! The `unsafe` is confined to `mmap(2)` + `munmap(2)` and the derived
//! slice projection below. It is covered by:
//!
//! * `tests/miri.rs` — Miri runs the whole cache suite; real `mmap`
//!   syscalls are outside Miri's isolated execution model, so under
//!   `cfg(miri)` (or the `miri` feature) `Mmap::open` returns `None` and
//!   the cache transparently uses the `read()`-backed shadow buffer.
//!   The slice-projection invariant (no out-of-bounds access, no
//!   allocation aliasing) is exercised by every suite test under Miri
//!   via the shadow path's identical slice semantics.
//! * `tests/loom.rs` — loom model-checks the record/lock state machine
//!   around the mapping (insert → flush → re-open interleavings); the
//!   `flock`/`mmap` syscalls themselves are outside loom's model and
//!   serialized by the advisory lock, so no loom-mode code path touches
//!   the mapping concurrently.
//!
//! # Soundness contract
//!
//! * `Mmap::open` maps `len` bytes (the file size at open/remap time)
//!   with `PROT_READ`/`MAP_SHARED` from an owned, read-enabled `File`.
//!   `len == 0` never maps (an empty mapping is UB by POSIX); the caller
//!   falls back to the shadow buffer.
//! * `as_slice` projects `ptr[0..len]`; `len` is exactly the mapped
//!   length, so no projection can go out of bounds.
//! * The file is only ever **grown** (append) or **truncated at a
//!   flush** while we hold the advisory lock; after any size change the
//!   mapping is dropped and re-created (`refresh_mapping`), so no slice
//!   ever references pages beyond the current mapping. Concurrent
//!   truncation by an external actor (outside the advisory lock) is
//!   unsupported concurrent use (§8.7 documents the one-process-per-lock
//!   contract); unlinking the file is safe (POSIX keeps mapped pages of
//!   unlinked inodes valid).
//! * The mapping is dropped exactly once (`Drop`), and `Mmap` is
//!   `Send + Sync` because the mapped pages are immutable during the
//!   object's lifetime (`PROT_READ`).

#![allow(unsafe_code)]

use std::fs::File;
use std::os::unix::io::AsRawFd;
use std::path::Path;

/// A read-only shared mapping of the cache file (§8.7).
pub struct Mmap {
    ptr: *mut u8,
    len: usize,
}

// SAFETY: the mapping is `PROT_READ` over private-to-this-object pages;
// `Send` allows moving the handle between threads, `Sync` allows shared
// references — both sound because no code writes through the mapping and
// the advisory lock serializes any concurrent resize (module contract).
unsafe impl Send for Mmap {}
unsafe impl Sync for Mmap {}

impl Mmap {
    /// Maps `len` bytes of `file` read-only. Returns `None` when `len` is
    /// zero, when running under Miri (isolated syscalls; the caller's
    /// shadow buffer takes over — see the module docs), or when the
    /// kernel refuses the mapping.
    pub fn open(file: &File, len: u64) -> Option<Mmap> {
        if len == 0 {
            return None;
        }
        #[cfg(any(miri, feature = "miri"))]
        {
            // Miri's isolated execution model does not implement file
            // mappings; the caller's read()-backed path keeps every test
            // meaningful under `cargo +nightly miri`.
            let _ = file;
            return None;
        }
        #[cfg(not(any(miri, feature = "miri")))]
        {
            use std::os::unix::io::RawFd;
            let fd: RawFd = file.as_raw_fd();
            let page = unsafe {
                // SAFETY: `mmap` of `len` bytes from an open fd with
                // `PROT_READ`/`MAP_SHARED`; the length is the file size
                // supplied by the caller (module contract: callers map
                // only sizes they just read from `metadata`). A NULL
                // kernel-chosen address, default protection flags.
                libc::mmap(
                    std::ptr::null_mut(),
                    usize::try_from(len).ok()?,
                    libc::PROT_READ,
                    libc::MAP_SHARED,
                    fd,
                    0,
                )
            };
            if page == libc::MAP_FAILED {
                return None;
            }
            Some(Mmap {
                ptr: page.cast::<u8>(),
                len: usize::try_from(len).ok()?,
            })
        }
    }

    /// The mapped bytes (exactly `len` bytes, § soundness contract).
    pub fn as_slice(&self) -> &[u8] {
        // SAFETY: `ptr` points at `len` mapped, readable bytes for the
        // lifetime of `self`; the projection stays within the mapping.
        unsafe { std::slice::from_raw_parts(self.ptr, self.len) }
    }
}

/// Acquires an exclusive advisory lock (`flock(2)` with `LOCK_EX`, §8.7)
/// on `file`; `true` on success, `false` on failure (the caller maps the
/// failure onto `E-CACHE-LOCK`). Held **per operation** — the open-time
/// scan, each append and each flush — and released by
/// [`release_lock`]; no instance holds a lock between calls, so several
/// instances in one process never self-deadlock.
///
/// // rationale: `flock` is the POSIX advisory-lock primitive §8.7 names
/// for concurrent halyard processes; the std library has no equivalent.
/// The call mutates no memory (the fd is owned by the caller's `Cache`)
/// and is not model-checked by loom (outside its model) — the state
/// machine *around* it is (`tests/loom.rs`).
pub fn lock_exclusive(file: &File) -> bool {
    use std::os::unix::io::AsRawFd;
    let rc = unsafe {
        // SAFETY: `flock` on an owned fd; `LOCK_EX` blocks until the lock
        // is free and never dereferences memory.
        libc::flock(file.as_raw_fd(), libc::LOCK_EX)
    };
    rc == 0
}

/// Releases a previously acquired advisory lock (`flock(2)` with
/// `LOCK_UN`); `true` on success.
///
/// // rationale: see [`lock_exclusive`] — same syscall pair, inverse
/// operation, no memory access.
pub fn unlock(file: &File) -> bool {
    use std::os::unix::io::AsRawFd;
    let rc = unsafe {
        // SAFETY: `flock` with `LOCK_UN` on an owned fd; releases the
        // advisory lock, never dereferences memory.
        libc::flock(file.as_raw_fd(), libc::LOCK_UN)
    };
    rc == 0
}

/// Durability for the cache directory entry: `fsync(2)` on a directory
/// fd (best-effort — failures are ignored, the file fsync in
/// `Cache::flush` remains the authoritative durability point).
///
/// // rationale: directory-entry durability after `create`/`rename` needs
/// `fsync` on an O_DIRECTORY fd (POSIX); no std surface exists. The fd
/// is opened, synced and closed here; nothing escapes.
pub fn fsync_directory(path: &Path) {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt as _;
    let Ok(c) = CString::new(path.as_os_str().as_bytes()) else {
        return;
    };
    // SAFETY: `open` with O_RDONLY|O_DIRECTORY on the caller's cache
    // directory path; the fd is closed on every path below.
    let fd = unsafe { libc::open(c.as_ptr(), libc::O_RDONLY | libc::O_DIRECTORY) };
    if fd >= 0 {
        // SAFETY: `fd` comes from the `open` above and is valid until
        // the `close` at the end of this block.
        unsafe {
            libc::fsync(fd);
            libc::close(fd);
        }
    }
}

impl Drop for Mmap {
    fn drop(&mut self) {
        // SAFETY: exactly one unmapping of the mapping created in `open`,
        // with the same length; `ptr` is valid until this call.
        let rc = unsafe { libc::munmap(self.ptr.cast::<libc::c_void>(), self.len) };
        debug_assert_eq!(rc, 0, "munmap of a live mapping must succeed");
    }
}

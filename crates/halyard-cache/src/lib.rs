//! The persistent per-commit cache (M2.5, §8.7).
//!
//! Layout: `$GIT_DIR/halyard/cache/v1/cache.bin` — a single append-only
//! file with a versioned header and checksummed records. Each record
//! caches the §8.4 attribution of one commit: the touched-package bitset
//! plus the parsed-commit header offsets, keyed by
//! `blake3(commit_oid ‖ layout_hash ‖ config_hash)` so package-layout or
//! configuration changes invalidate cleanly.
//!
//! Crash model (I2): a process that dies mid-write leaves at most one
//! torn record at the tail. On [`Cache::open`] the file is scanned record
//! by record and every record is checksum-verified; the scan stops at the
//! first invalid record and everything from there on is skipped (never
//! fatal, §8.7). Inserts append after the valid prefix; [`Cache::flush`]
//! truncates the stale tail, fsyncs and rewrites the header (hit/miss
//! counters live in the header, so the `doctor` hit-rate survives
//! restarts and resets exactly at a flush).
//!
//! Concurrency: an exclusive advisory lock (`flock`, §8.7) guards each
//! mutating operation — the open-time scan, each append, each flush — and
//! is released before the call returns (per-operation locking). Reads
//! through the mapping are immutable bytes within the scanned valid
//! prefix; the only `unsafe` in the workspace (invariant I10, §2) is the
//! `mmap`/`munmap`/`flock` trio in [`mmap`], which carries its
//! rationale and is covered by `tests/miri.rs` (Miri runs the cache suite
//! through the `read()`-backed fallback — real `mmap` syscalls are
//! outside Miri's isolated execution model) and `tests/loom.rs` (the
//! record/lock state machine is model-checked; the `flock` syscall
//! itself is outside loom's model and documented there).
//!
//! Layer note: this crate depends on `halyard-core` only (the record
//! payload is a [`SmallBitset`]); it is consumed by `halyard-git` *before*
//! the [`Snapshot`](halyard_core::planner::Snapshot) is constructed and
//! must never leak into `halyard-core` (planner purity, §4/I6).
//!
//! The crate-level `unsafe_code` permission lives in `Cargo.toml`
//! (`[lints.rust] unsafe_code = "allow"`, invariant I10) — the source
//! carries no allow attributes; the `unsafe` surface is confined to the
//! [`mmap`] module with its own SAFETY rationale.

use std::cell::Cell;
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read as _, Seek as _, SeekFrom, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use halyard_core::SmallBitset;
use tracing::trace;

mod mmap;

use mmap::Mmap;

/// Magic bytes of the v1 header (`HLYC`).
const MAGIC: &[u8; 4] = b"HLYC";
/// On-disk format version (§8.7: versioned header).
const FORMAT_VERSION: u32 = 1;
/// Header size: magic(4) + version(4) + hits(8) + misses(8) + crc32(4).
const HEADER_LEN: usize = 28;
/// Key size of the blake3 content hash.
const KEY_LEN: usize = 32;

/// Content-addressed cache key (§8.7):
/// `blake3(commit_oid ‖ layout_hash ‖ config_hash)`.
///
/// `layout_hash` is the trie hash ([`halyard_core::layout_hash`]),
/// `config_hash` comes from `halyard-config` (never re-derived here).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CacheKey([u8; KEY_LEN]);

impl CacheKey {
    /// Derives the key from the three content inputs (§8.7). The inputs
    /// are hashed as `commit_oid ‖ layout_hash ‖ config_hash` in that
    /// order, with no separators beyond their own self-delimiting shapes
    /// (fixed-length OID hex, `blake3:<hex>` text).
    #[must_use]
    pub fn compute(commit_oid: &[u8], layout_hash: &str, config_hash: &str) -> Self {
        let mut hasher = blake3::Hasher::new();
        hasher.update(commit_oid);
        hasher.update(layout_hash.as_bytes());
        hasher.update(config_hash.as_bytes());
        let mut key = [0u8; KEY_LEN];
        key.copy_from_slice(hasher.finalize().as_bytes());
        Self(key)
    }

    /// The raw 32-byte key.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Lowercase hex rendering (diagnostics, `examples/dump.rs`).
    #[must_use]
    pub fn to_hex(&self) -> String {
        use std::fmt::Write as _;
        let mut out = String::with_capacity(KEY_LEN * 2);
        for b in &self.0 {
            // `write!` to a `String` cannot fail; the `let _` keeps the
            // house no-`expect` rule.
            let _ = write!(out, "{b:02x}");
        }
        out
    }
}

/// The cached attribution of one commit (§8.7 value shape):
/// the touched-package bitset plus the parsed-commit header offsets the
/// digest assembly needs without re-reading the message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CacheRecord {
    /// Packages the commit touches (§8.4 tree-diff result, before the
    /// per-package include/exclude refinement — the refinement is a pure
    /// function of the filters that feed `layout_hash`, so a cached
    /// raw bitset re-filters identically).
    pub touched: SmallBitset,
    /// Byte offset where the commit's subject ends inside the raw
    /// message (§5.7 `subject` slicing on a cache hit).
    pub subject_end: u32,
    /// Byte offset where the body starts (`0..0` shape when the message
    /// has no body; §5.7 `body_range` reconstruction on a cache hit).
    pub body_start: u32,
}

impl CacheRecord {
    /// Deterministic little-endian serialization of the record payload
    /// (I3: bytes are part of the roundtrip determinism test). Public so
    /// the `dump` example and the corruption/miri suites exercise the
    /// exact on-disk payload bytes; the framing (length + CRC + key)
    /// around the payload stays private.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let words: Vec<u64> = if self.touched.is_empty() {
            Vec::new()
        } else {
            self.touched.iter().fold(Vec::new(), |mut acc, pkg| {
                let idx = pkg.to_u32() as usize / 64;
                if acc.len() <= idx {
                    acc.resize(idx + 1, 0);
                }
                if let Some(word) = acc.get_mut(idx) {
                    *word |= 1u64 << (pkg.to_u32() % 64);
                }
                acc
            })
        };
        let mut out = Vec::with_capacity(4 + words.len() * 8 + 8);
        out.extend_from_slice(&u32::try_from(words.len()).unwrap_or(u32::MAX).to_le_bytes());
        for word in &words {
            out.extend_from_slice(&word.to_le_bytes());
        }
        out.extend_from_slice(&self.subject_end.to_le_bytes());
        out.extend_from_slice(&self.body_start.to_le_bytes());
        out
    }

    /// Decodes a payload; `None` when the bytes are malformed (the scan
    /// treats this like a checksum failure and skips the record). Public
    /// for the same hostile-input suites as [`CacheRecord::encode`].
    #[must_use]
    pub fn decode(payload: &[u8]) -> Option<Self> {
        let words_len = u32::from_le_bytes(*payload.first_chunk::<4>()?);
        // `words_len` is hostile input: the capacity arithmetic runs in
        // `u64` so no target can overflow before the bounds check
        // rejects an over-long claim.
        let words_bytes = u64::from(words_len) * 8;
        if u64::try_from(payload.len()).unwrap_or(u64::MAX) < 4u64 + words_bytes + 8 {
            return None;
        }
        // Guarded above: the words and trailing offsets fit `usize`.
        let words_len = usize::try_from(words_len).unwrap_or(usize::MAX);
        let mut touched = SmallBitset::new();
        for i in 0..words_len {
            let start = 4 + i * 8;
            let word = u64::from_le_bytes(payload.get(start..start + 8)?.try_into().ok()?);
            for bit in 0..64u32 {
                if word & (1u64 << bit) != 0 {
                    // Saturating instead of `as`: a payload large enough
                    // to overflow the PkgId arithmetic is malformed by
                    // any sane measure; `None` skips the record.
                    let pkg = u32::try_from(i).ok()?.checked_mul(64)?.checked_add(bit)?;
                    touched.set(halyard_core::PkgId::new(pkg));
                }
            }
        }
        let tail = payload.get(4 + words_len * 8..)?;
        let subject_end = u32::from_le_bytes(*tail.first_chunk::<4>()?);
        let body_start = u32::from_le_bytes(tail.get(4..8)?.try_into().ok()?);
        Some(Self {
            touched,
            subject_end,
            body_start,
        })
    }
}

/// Cache failures (§15.4 `E-CACHE-*`; each variant has a page under
/// `docs/errors/`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CacheError {
    /// The cache directory or file cannot be created/opened
    /// (`E-CACHE-OPEN`).
    Open {
        /// The path that was opened.
        path: String,
        /// Underlying error text.
        message: String,
    },
    /// The existing file's header is not a valid v1 header
    /// (`E-CACHE-HEADER`): wrong magic, unknown version or bad header
    /// checksum. Remediation: delete the file (it is safe to delete
    /// anywhere, §8.7) or `halyard cache prune`.
    Header {
        /// The path of the rejected file.
        path: String,
        /// What is wrong with the header.
        reason: String,
    },
    /// A read/write/fsync syscall failed (`E-CACHE-IO`).
    Io {
        /// What operation failed.
        operation: String,
        /// Underlying error text.
        message: String,
    },
    /// The advisory lock could not be acquired (`E-CACHE-LOCK`).
    Lock {
        /// The locked path.
        path: String,
        /// Underlying error text.
        message: String,
    },
}

impl CacheError {
    /// Stable §15.4 code of this error.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Open { .. } => "E-CACHE-OPEN",
            Self::Header { .. } => "E-CACHE-HEADER",
            Self::Io { .. } => "E-CACHE-IO",
            Self::Lock { .. } => "E-CACHE-LOCK",
        }
    }
}

impl std::fmt::Display for CacheError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Open { path, message } => {
                write!(f, "[E-CACHE-OPEN] cannot open cache at {path}: {message}")
            }
            Self::Header { path, reason } => write!(
                f,
                "[E-CACHE-HEADER] {path}: {reason} — the cache is safe to delete \
                 (`halyard cache prune`) and will be rebuilt"
            ),
            Self::Io { operation, message } => {
                write!(f, "[E-CACHE-IO] cache {operation} failed: {message}")
            }
            Self::Lock { path, message } => {
                write!(f, "[E-CACHE-LOCK] cannot lock cache {path}: {message}")
            }
        }
    }
}

impl std::error::Error for CacheError {}

/// Process-global hit/miss counters (`tracing`-adjacent; read by the
/// profiler and `doctor`). These are the *current run's* counters; the
/// cross-run hit-rate lives in the file header (reset at flush).
static HITS: AtomicU64 = AtomicU64::new(0);
static MISSES: AtomicU64 = AtomicU64::new(0);

/// The current process's cache hit/miss counters (`(hits, misses)`).
#[must_use]
pub fn global_stats() -> (u64, u64) {
    (HITS.load(Ordering::Relaxed), MISSES.load(Ordering::Relaxed))
}

/// Resets the process-global counters (test/doctor hook).
pub fn reset_stats() {
    HITS.store(0, Ordering::Relaxed);
    MISSES.store(0, Ordering::Relaxed);
}

/// An open cache (§8.7). The in-memory index maps keys onto
/// `(offset, length)` inside the valid prefix; iteration order of the
/// index never reaches any output (lookups only, I3).
pub struct Cache {
    file: File,
    /// Mapping bytes at `open` time (lazily remapped after growth).
    map: Option<Mmap>,
    /// `read()`-backed bytes when mmap is unavailable (len-0 file, Miri).
    shadow: Vec<u8>,
    dir: PathBuf,
    /// End offset of the valid record prefix (inserts append here).
    valid_len: u64,
    index: HashMap<CacheKey, (u64, u32)>,
    /// Cumulative hit/miss counters including the ones read from the
    /// header (`Cell`: `get` is a shared borrow; counters are plain
    /// diagnostics, not synchronization — concurrency is serialized by
    /// the advisory lock).
    hits: Cell<u64>,
    misses: Cell<u64>,
}

impl Cache {
    /// Directory of the cache file (`$GIT_DIR/halyard/cache/v1/`, §8.7).
    #[must_use]
    pub fn cache_dir(git_dir: &Path) -> PathBuf {
        git_dir.join("halyard").join("cache").join("v1")
    }

    /// Opens (and creates, when missing) the cache under
    /// `$GIT_DIR/halyard/cache/v1/`, validates the header and scans the
    /// record chain under a **brief exclusive** advisory lock (§8.7).
    /// Locks are held per operation — the open-time scan, each
    /// [`Cache::insert`] and [`Cache::flush`] — and released before the
    /// call returns, so a process may hold several instances (e.g.
    /// `doctor` after `plan`) without self-deadlocking: `flock` conflicts
    /// are per open-file description, and no instance holds a lock
    /// outside its own mutating call.
    ///
    /// # Errors
    ///
    /// [`CacheError::Open`] for filesystem failures, [`CacheError::Header`]
    /// for a rejected existing header, [`CacheError::Io`] for read
    /// failures, [`CacheError::Lock`] when the advisory lock cannot be
    /// taken.
    pub fn open(git_dir: &Path) -> Result<Self, CacheError> {
        let dir = Self::cache_dir(git_dir);
        std::fs::create_dir_all(&dir).map_err(|e| CacheError::Open {
            path: dir.display().to_string(),
            message: e.to_string(),
        })?;
        let path = dir.join("cache.bin");
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(|e| CacheError::Open {
                path: path.display().to_string(),
                message: e.to_string(),
            })?;
        // Exclusive for the brief open-time scan (the fresh-file branch
        // writes the header).
        acquire_lock(&file, &path)?;
        let mut cache = Self {
            file,
            map: None,
            shadow: Vec::new(),
            dir,
            valid_len: HEADER_LEN as u64,
            index: HashMap::new(),
            hits: Cell::new(0),
            misses: Cell::new(0),
        };
        let outcome = cache.load_locked(&path);
        release_lock(&cache.file, &path);
        outcome?;
        Ok(cache)
    }

    /// The open-time body (header validation + record scan) under the
    /// already-acquired exclusive lock.
    fn load_locked(&mut self, path: &Path) -> Result<(), CacheError> {
        let _ = path;
        let size = self
            .file
            .metadata()
            .map_err(|e| CacheError::Io {
                operation: "metadata".to_owned(),
                message: e.to_string(),
            })?
            .len();
        if size == 0 {
            self.write_header(0, 0)?;
        } else {
            self.load_existing(size)?;
            self.scan_records()?;
        }
        self.refresh_mapping();
        Ok(())
    }

    /// Walks the record chain after the header, checksum-verifying every
    /// record and indexing the valid prefix (§8.7: "corrupted records are
    /// skipped, never fatal"). The chain is length-delimited, so a torn
    /// or corrupt record ends the scan — everything from there on is the
    /// skipped tail that the next [`Cache::flush`] truncates.
    fn scan_records(&mut self) -> Result<(), CacheError> {
        let size = self
            .file
            .metadata()
            .map_err(|e| CacheError::Io {
                operation: "metadata".to_owned(),
                message: e.to_string(),
            })?
            .len();
        let mut cursor = HEADER_LEN as u64;
        let mut header = [0u8; 8];
        let mut key = [0u8; KEY_LEN];
        while cursor + 8 <= size {
            self.file
                .seek(SeekFrom::Start(cursor))
                .and_then(|_| self.file.read_exact(&mut header))
                .map_err(|e| CacheError::Io {
                    operation: "scan record header".to_owned(),
                    message: e.to_string(),
                })?;
            let payload_len =
                u32::from_le_bytes(header.first_chunk::<4>().map_or([0u8; 4], |s| *s));
            let stored_crc = u32::from_le_bytes(
                header
                    .get(4..8)
                    .and_then(|s| <[u8; 4]>::try_from(s).ok())
                    .unwrap_or([0; 4]),
            );
            let record_len = 8 + KEY_LEN + payload_len as usize;
            if cursor + record_len as u64 > size {
                // Torn tail: the record claims more bytes than exist.
                break;
            }
            self.file
                .seek(SeekFrom::Start(cursor + 8))
                .and_then(|_| self.file.read_exact(&mut key))
                .map_err(|e| CacheError::Io {
                    operation: "scan record key".to_owned(),
                    message: e.to_string(),
                })?;
            let mut payload = vec![0u8; payload_len as usize];
            self.file
                .seek(SeekFrom::Start(cursor + 8 + KEY_LEN as u64))
                .and_then(|_| self.file.read_exact(&mut payload))
                .map_err(|e| CacheError::Io {
                    operation: "scan record payload".to_owned(),
                    message: e.to_string(),
                })?;
            let mut crc_input = Vec::with_capacity(KEY_LEN + payload_len as usize);
            crc_input.extend_from_slice(&key);
            crc_input.extend_from_slice(&payload);
            if crc32(&crc_input) != stored_crc || CacheRecord::decode(&payload).is_none() {
                // Corrupt or malformed: the scan stops here (§8.7 skip,
                // never fatal). A later flush truncates the skipped tail.
                break;
            }
            let cache_key = CacheKey(key);
            // Duplicate keys keep the FIRST (oldest) record so a re-run's
            // outcome is stable (I3); the later record is skipped from the
            // index but the chain still advances.
            self.index
                .entry(cache_key)
                .or_insert((cursor, u32::try_from(record_len).unwrap_or(u32::MAX)));
            cursor += record_len as u64;
        }
        self.valid_len = cursor.max(HEADER_LEN as u64);
        Ok(())
    }

    /// Validates the v1 header of an existing file and reads the persisted
    /// hit/miss counters.
    fn load_existing(&mut self, size: u64) -> Result<(), CacheError> {
        let path = self.path();
        if size < HEADER_LEN as u64 {
            return Err(CacheError::Header {
                path: path.display().to_string(),
                reason: format!("file is {size} bytes, shorter than the {HEADER_LEN}-byte header"),
            });
        }
        let mut header = [0u8; HEADER_LEN];
        self.file
            .seek(SeekFrom::Start(0))
            .and_then(|_| self.file.read_exact(&mut header))
            .map_err(|e| CacheError::Io {
                operation: "read header".to_owned(),
                message: e.to_string(),
            })?;
        if !header.starts_with(MAGIC) {
            return Err(CacheError::Header {
                path: path.display().to_string(),
                reason: "bad magic (not a halyard cache file)".to_owned(),
            });
        }
        let version = u32::from_le_bytes(
            header
                .get(4..8)
                .and_then(|s| <[u8; 4]>::try_from(s).ok())
                .ok_or_else(|| CacheError::Header {
                    path: path.display().to_string(),
                    reason: "truncated version field".to_owned(),
                })?,
        );
        if version != FORMAT_VERSION {
            return Err(CacheError::Header {
                path: path.display().to_string(),
                reason: format!(
                    "unknown format version {version} (this build reads {FORMAT_VERSION})"
                ),
            });
        }
        let crc = u32::from_le_bytes(
            header
                .get(HEADER_LEN - 4..)
                .and_then(|s| <[u8; 4]>::try_from(s).ok())
                .ok_or_else(|| CacheError::Header {
                    path: path.display().to_string(),
                    reason: "truncated header checksum".to_owned(),
                })?,
        );
        if crc32(&header[..HEADER_LEN - 4]) != crc {
            return Err(CacheError::Header {
                path: path.display().to_string(),
                reason: "header checksum mismatch".to_owned(),
            });
        }
        let read_u64 = |at: usize| -> u64 {
            header
                .get(at..at + 8)
                .and_then(|s| <[u8; 8]>::try_from(s).ok())
                .map_or(0, u64::from_le_bytes)
        };
        self.hits.set(read_u64(8));
        self.misses.set(read_u64(16));
        Ok(())
    }

    /// Writes a fresh header (new file or post-flush rewrite). The header
    /// rewrite happens in place (28 bytes, atomic enough at the fsync in
    /// [`flush`]; a torn header fails [`Cache::open`] with
    /// [`CacheError::Header`], which the user remediates by deleting —
    /// the cache is always safe to delete, §8.7).
    fn write_header(&mut self, hits: u64, misses: u64) -> Result<(), CacheError> {
        let mut header = Vec::with_capacity(HEADER_LEN);
        header.extend_from_slice(MAGIC);
        header.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
        header.extend_from_slice(&hits.to_le_bytes());
        header.extend_from_slice(&misses.to_le_bytes());
        let crc = crc32(&header);
        header.extend_from_slice(&crc.to_le_bytes());
        self.file
            .seek(SeekFrom::Start(0))
            .and_then(|_| self.file.write_all(&header))
            .map_err(|e| CacheError::Io {
                operation: "write header".to_owned(),
                message: e.to_string(),
            })
    }

    /// The cache file path (diagnostics and tests).
    fn path(&self) -> PathBuf {
        self.dir.join("cache.bin")
    }

    /// (Re)maps the file for reads. Falls back to a plain `read()`-backed
    /// buffer when mmap is unavailable (empty file, Miri isolation).
    fn refresh_mapping(&mut self) {
        let size = self.file.metadata().map_or(0, |m| m.len());
        if size == 0 {
            self.map = None;
            self.shadow.clear();
            return;
        }
        if let Some(map) = Mmap::open(&self.file, size) {
            self.map = Some(map);
            self.shadow.clear();
        } else {
            self.map = None;
            // `unwrap_or(0)`: a size that does not fit `usize` (32-bit
            // target, huge file) simply skips the pre-allocation; the
            // read below still reports what it can.
            let mut shadow = Vec::with_capacity(usize::try_from(size).unwrap_or(0));
            let mut file = self.file.try_clone().ok();
            if let Some(f) = file.as_mut() {
                let _ = f.seek(SeekFrom::Start(0));
                let _ = f.read_to_end(&mut shadow);
            }
            self.shadow = shadow;
        }
    }

    /// The mapped (or shadow) bytes of the whole file.
    fn bytes(&self) -> &[u8] {
        if let Some(map) = self.map.as_ref() {
            map.as_slice()
        } else {
            &self.shadow
        }
    }

    /// Looks up one record (§8.7 `get`). A missing key, a skipped-tail
    /// entry or a checksum mismatch all read as `None` (corruption is
    /// never fatal); every lookup counts towards the hit-rate.
    pub fn get(&self, key: &CacheKey) -> Option<CacheRecord> {
        let found = match self.index.get(key) {
            Some((offset, record_len)) => {
                // `usize::MAX` sentinel: the `checked_add`/bounds check
                // below rejects it as a miss (never a truncation).
                let start = usize::try_from(*offset).unwrap_or(usize::MAX);
                let end = start.checked_add(usize::try_from(*record_len).unwrap_or(usize::MAX))?;
                let bytes = self.bytes();
                if end > bytes.len() {
                    return None;
                }
                let record = bytes.get(start..end)?;
                let crc = u32::from_le_bytes(record.get(4..8)?.try_into().ok()?);
                let payload = record.get(8 + KEY_LEN..)?;
                if crc32(record.get(8..).unwrap_or(&[])) != crc {
                    trace!("cache record checksum mismatch (skipped)");
                    return None;
                }
                CacheRecord::decode(payload)
            }
            None => None,
        };
        if found.is_some() {
            HITS.fetch_add(1, Ordering::Relaxed);
            self.hits.set(self.hits.get() + 1);
            trace!(hit = true, "cache get");
        } else {
            MISSES.fetch_add(1, Ordering::Relaxed);
            self.misses.set(self.misses.get() + 1);
            trace!(hit = false, "cache get");
        }
        found
    }

    /// Appends one record (§8.7 `insert`). The bytes land after the valid
    /// prefix; they become durable/visible to reopeners at [`Cache::flush`]
    /// (fsync + tail truncation) — a crash between insert and flush loses
    /// the record but never corrupts the file (I2).
    ///
    /// # Errors
    ///
    /// [`CacheError::Io`] when the append fails.
    pub fn insert(&mut self, key: CacheKey, rec: CacheRecord) -> Result<(), CacheError> {
        self.insert_with_failpoint(key, rec, &|_| false)
    }

    /// [`insert`] with an injectable failpoint (`failpoint(nth_insert) ->
    /// bool`): when it returns `true` after the `n`th insert, the record's
    /// bytes are written only partially (torn tail, the exact state a
    /// SIGKILL mid-write produces) and the write reports success. **The
    /// caller must stop the writing session after a firing failpoint** —
    /// a real crash cannot keep appending; the kill-during-write test
    /// re-opens and asserts the tail skip (I2).
    ///
    /// # Errors
    ///
    /// [`CacheError::Io`] when the append fails.
    pub fn insert_with_failpoint(
        &mut self,
        key: CacheKey,
        rec: CacheRecord,
        failpoint: &dyn Fn(usize) -> bool,
    ) -> Result<(), CacheError> {
        let nth = self.index.len();
        let payload = rec.encode();
        // The owned record's bytes are fully materialized below; release
        // the in-memory copy before entering the lock section (its
        // `SmallBitset` may own a heap allocation).
        drop(rec);
        let mut record = Vec::with_capacity(8 + KEY_LEN + payload.len());
        record.extend_from_slice(
            &u32::try_from(payload.len())
                .unwrap_or(u32::MAX)
                .to_le_bytes(),
        );
        record.extend_from_slice(&[0u8; 4]); // crc placeholder
        record.extend_from_slice(&key.0);
        record.extend_from_slice(&payload);
        let crc = crc32(record.get(8..).unwrap_or(&[]));
        if let Some(slot) = record.get_mut(4..8) {
            slot.copy_from_slice(&crc.to_le_bytes());
        }
        let torn = failpoint(nth);
        let cut = if torn { record.len() / 2 } else { record.len() };
        let offset = self.valid_len;
        // Exclusive lock for the append; released before returning (per-op
        // locking, see [`Cache::open`]).
        acquire_lock(&self.file, &self.path())?;
        let outcome = self.write_record(
            offset,
            record.get(..cut).unwrap_or(&[]),
            record.len(),
            key,
            torn,
        );
        release_lock(&self.file, &self.path());
        outcome?;
        self.refresh_mapping();
        Ok(())
    }

    /// Writes the record bytes and updates the index/valid prefix. Split
    /// out of [`insert_with_failpoint`] so the advisory lock brackets only
    /// the syscall.
    fn write_record(
        &mut self,
        offset: u64,
        bytes: &[u8],
        full_len: usize,
        key: CacheKey,
        torn: bool,
    ) -> Result<(), CacheError> {
        self.file
            .seek(SeekFrom::Start(offset))
            .and_then(|_| self.file.write_all(bytes))
            .map_err(|e| CacheError::Io {
                operation: "insert".to_owned(),
                message: e.to_string(),
            })?;
        if torn {
            // The process "died" here: the record is half-written, the
            // valid prefix does NOT cover the torn bytes (the next flush
            // truncates them) and no index entry exists. A reopen must
            // skip the tail.
            return Ok(());
        }
        self.valid_len = offset + full_len as u64;
        self.index
            .insert(key, (offset, u32::try_from(full_len).unwrap_or(u32::MAX)));
        Ok(())
    }

    /// Durability point: fsync the file, truncate the stale tail, rewrite
    /// the header with the accumulated hit/miss counters, fsync the
    /// directory (§8.7) — all under the brief exclusive advisory lock.
    ///
    /// # Errors
    ///
    /// [`CacheError::Io`] for any write/fsync failure.
    pub fn flush(&mut self) -> Result<(), CacheError> {
        acquire_lock(&self.file, &self.path())?;
        let outcome = self.flush_locked();
        release_lock(&self.file, &self.path());
        outcome?;
        self.refresh_mapping();
        Ok(())
    }

    /// The flush body under the held exclusive lock.
    fn flush_locked(&mut self) -> Result<(), CacheError> {
        self.file
            .set_len(self.valid_len)
            .map_err(|e| CacheError::Io {
                operation: "truncate tail".to_owned(),
                message: e.to_string(),
            })?;
        self.write_header(self.hits.get(), self.misses.get())?;
        self.file
            .seek(SeekFrom::Start(self.valid_len))
            .map_err(|e| CacheError::Io {
                operation: "seek".to_owned(),
                message: e.to_string(),
            })?;
        self.file.sync_all().map_err(|e| CacheError::Io {
            operation: "fsync".to_owned(),
            message: e.to_string(),
        })?;
        if let Some(parent) = self.dir.parent() {
            sync_dir(parent);
        }
        Ok(())
    }

    /// Cached-record count in the valid prefix.
    #[must_use]
    pub fn len(&self) -> usize {
        self.index.len()
    }

    /// True when the cache holds no records.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    /// Cache file size in bytes (valid prefix — the on-disk size may be
    /// larger by a torn tail until the next flush).
    #[must_use]
    pub fn size_bytes(&self) -> u64 {
        self.valid_len
    }

    /// Cumulative `(hits, misses)` including the counters read from the
    /// header at open (the `doctor` hit-rate "since last flush", §8.7).
    #[must_use]
    pub fn hit_rate(&self) -> (u64, u64) {
        (self.hits.get(), self.misses.get())
    }
}

impl std::fmt::Debug for Cache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Cache")
            .field("dir", &self.dir)
            .field("file", &self.file)
            .field("map", &self.map)
            // The shadow buffer is rendered as its byte length, never as
            // bytes (it mirrors the whole file).
            .field("shadow", &self.shadow.len())
            .field("valid_len", &self.valid_len)
            .field("records", &self.index.len())
            .field("hits", &self.hits.get())
            .field("misses", &self.misses.get())
            .finish()
    }
}

impl Drop for Cache {
    fn drop(&mut self) {
        // The advisory lock is already released (per-op locking); this
        // final sync is best-effort durability for unflushed appends.
        let _ = self.file.sync_all();
    }
}

/// Acquires the exclusive advisory lock (`flock(LOCK_EX)`, §8.7) for one
/// operation. The syscall pair lives in [`mmap`] (the workspace's
/// confined `unsafe` module); loom models the surrounding state machine
/// (tests/loom.rs) — the syscall itself is outside loom's model.
fn acquire_lock(file: &File, path: &Path) -> Result<(), CacheError> {
    if mmap::lock_exclusive(file) {
        Ok(())
    } else {
        Err(CacheError::Lock {
            path: path.display().to_string(),
            message: std::io::Error::last_os_error().to_string(),
        })
    }
}

/// Releases the per-operation advisory lock (`flock(LOCK_UN)`).
fn release_lock(file: &File, path: &Path) {
    if !mmap::unlock(file) {
        // An unlock failure leaves the lock until the fd closes; the
        // next operation on this instance re-locks anyway.
        trace!("cache unlock failed on {}", path.display());
    }
}

/// fsync of the cache's parent directory (POSIX durability for the
/// append-only file's directory entry); best-effort.
fn sync_dir(path: &Path) {
    mmap::fsync_directory(path);
}

/// CRC-32 (IEEE 802.3 polynomial, reflected) — the record checksum.
/// Hand-rolled table-less implementation: 8 bits per step, no lookup
/// table allocation, deterministic across platforms.
#[must_use]
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            let mask = u32::from(crc & 1 != 0).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(pkgs: &[u32]) -> CacheRecord {
        let mut touched = SmallBitset::new();
        for p in pkgs {
            touched.set(halyard_core::PkgId::new(*p));
        }
        CacheRecord {
            touched,
            subject_end: 42,
            body_start: 44,
        }
    }

    #[test]
    fn crc32_matches_known_vectors() {
        assert_eq!(crc32(b""), 0x0000_0000);
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        // The header/record checksums are deterministic across runs.
        assert_eq!(crc32(b"halyard cache v1"), crc32(b"halyard cache v1"));
    }

    #[test]
    fn record_roundtrips_words_and_offsets() {
        for pkgs in [vec![], vec![0, 1, 63, 64, 127, 128, 200], vec![500]] {
            let rec = record(&pkgs);
            let decoded = CacheRecord::decode(&rec.encode()).expect("decodes");
            assert_eq!(decoded.touched, rec.touched);
            assert_eq!(decoded.subject_end, rec.subject_end);
            assert_eq!(decoded.body_start, rec.body_start);
        }
    }

    #[test]
    fn decode_rejects_malformed_payloads() {
        assert!(CacheRecord::decode(&[]).is_none());
        assert!(CacheRecord::decode(&[3, 0, 0, 0, 1, 2, 3]).is_none());
    }

    #[test]
    fn key_is_stable_across_orderings() {
        let a = CacheKey::compute(
            b"9f32036f7215bb0fb590f120442648feca5a6104",
            "blake3:aa",
            "blake3:bb",
        );
        let b = CacheKey::compute(
            b"9f32036f7215bb0fb590f120442648feca5a6104",
            "blake3:aa",
            "blake3:bb",
        );
        assert_eq!(a, b);
        assert_ne!(
            a,
            CacheKey::compute(
                b"9f32036f7215bb0fb590f120442648feca5a6104",
                "blake3:aa",
                "blake3:bc"
            )
        );
        assert_eq!(a.as_bytes().len(), 32);
    }
}

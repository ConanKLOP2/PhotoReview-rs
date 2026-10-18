//! Red tests for `pr_app::misc::file_hash` on the real file system (contract W8-04e, section "tests/file_hash.rs").
//! Port of `tests/PhotoReview.App.Tests/Services/FileHashServiceTests.cs` (C# baseline d92d6aae).
//!
//! Real files live under a [`TempRoot`] and are removed with it. The 24 MiB case is a real file, as in C#: the whole-file
//! byte cache must refuse it, and the streamed read must produce the same SHA-256 as a whole-file hash.
//!
//! Every test calls the stub, so each one fails through `todo!()` until the porter fills the bodies.

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime};

use pr_app::misc::file_hash::{FileHashError, FileHashService, FileHasher};
use pr_core::sync::CancelToken;
use pr_imaging::cache::source_bytes::SourceBytesCache;
use pr_test_support::TempRoot;

/// 64 GiB of physical memory for the cache clamp: 1 MiB is then never clamped to 20 % of RAM (contract trap 20).
const PHYSICAL_MEMORY_BYTES: i64 = 64 * 1024 * 1024 * 1024;

/// C# `Convert.ToHexString(SHA256.HashData(content))`: upper-case hex of the one-shot digest.
fn sha_hex(content: &[u8]) -> String {
    pr_core::session::store::sha256(content)
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect()
}

/// Deterministic pseudo-random bytes (C# `new Random(seed).NextBytes`), so the expectations are reproducible.
fn random_bytes(seed: u64, len: usize) -> Vec<u8> {
    let mut state = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    (0..len)
        .map(|_| {
            state ^= state >> 12;
            state ^= state << 25;
            state ^= state >> 27;
            (state.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 56) as u8
        })
        .collect()
}

fn write_file(root: &TempRoot, name: &str, content: &[u8]) -> PathBuf {
    let path = root.join(name);
    fs::write(&path, content).unwrap();
    path
}

fn byte_cache(capacity_bytes: i64) -> Arc<SourceBytesCache> {
    Arc::new(SourceBytesCache::new(capacity_bytes, PHYSICAL_MEMORY_BYTES, None).unwrap())
}

fn get(service: &FileHashService, path: &Path) -> Result<String, FileHashError> {
    service.get(path, &CancelToken::none())
}

// C#: FileHashServiceTests.FileHashServiceCachesAndInvalidates
#[test]
fn get_caches_and_invalidates_by_file_fingerprint() {
    let root = TempRoot::new("hash-cache");
    let fixture = write_file(&root, "hash-fixture.bin", &[1, 2, 3]);
    let service = FileHashService::new(None).unwrap();

    let first = get(&service, &fixture).unwrap();
    let cached = get(&service, &fixture).unwrap();
    fs::write(&fixture, [1, 2, 4]).unwrap();
    // C# forces a detectable mtime change instead of relying on wall-clock granularity; so does this test (no sleep).
    File::options()
        .write(true)
        .open(&fixture)
        .unwrap()
        .set_modified(SystemTime::now() + Duration::from_secs(1))
        .unwrap();
    let changed = get(&service, &fixture).unwrap();

    assert!(first == cached && first != changed);
}

// new: the hash is 64 upper-case hex characters (contract semantic C.14 and trap 25).
#[test]
fn get_returns_upper_case_hex_of_64_characters() {
    let root = TempRoot::new("hash-format");
    let content = random_bytes(3, 4096);
    let path = write_file(&root, "format.bin", &content);
    let service = FileHashService::new(None).unwrap();

    let hash = get(&service, &path).unwrap();

    assert_eq!(hash.len(), 64);
    assert!(
        hash.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_lowercase()),
        "hash {hash}"
    );
    assert_eq!(hash, sha_hex(&content));
}

// new: a missing file is an Io error (contract step 17b), not a path error and not a cached hash.
#[test]
fn get_missing_file_is_io_error() {
    let root = TempRoot::new("hash-missing");
    let service = FileHashService::new(None).unwrap();

    let result = get(&service, &root.join("does-not-exist.bin"));

    assert!(matches!(result, Err(FileHashError::Io(_))), "got {result:?}");
}

// new: a path with a NUL character is a Path error, kept as such (contract trap 24).
#[test]
fn get_path_containing_nul_is_path_error() {
    let service = FileHashService::new(None).unwrap();

    let result = get(&service, Path::new("bad\0name.bin"));

    assert!(matches!(result, Err(FileHashError::Path(_))), "got {result:?}");
}

// C#: FileHashServiceTests.FileHashServiceStreamsFileTooLargeForCache
#[test]
fn get_streams_file_larger_than_source_bytes_cache_without_whole_read() {
    let root = TempRoot::new("hash-big");
    const SIZE: usize = 24 * 1024 * 1024;
    let content = random_bytes(7, SIZE);
    let expected = sha_hex(&content);
    let fixture = write_file(&root, "big.bin", &content);
    drop(content);
    let cache = byte_cache(1024 * 1024);
    assert!(!cache.can_cache(i64::try_from(SIZE).unwrap()));
    let service = FileHashService::new(Some(Arc::clone(&cache))).unwrap();

    let actual = get(&service, &fixture).unwrap();

    assert_eq!(expected, actual);
    assert!(
        cache.last_read_thread_id_for_tests().is_none(),
        "the cache never read the file whole"
    );
}

// C#: FileHashServiceTests.FileHashServiceStreamsRawFilesInsteadOfUsingTheWholeFileByteCache (row `shot.cr3`)
#[test]
fn get_streams_raw_cr3_not_whole_file_cache() {
    assert_raw_file_is_streamed("shot.cr3");
}

// C#: FileHashServiceTests.FileHashServiceStreamsRawFilesInsteadOfUsingTheWholeFileByteCache (row `shot.NEF`; proves the
// RAW check is case-insensitive)
#[test]
fn get_streams_raw_nef_not_whole_file_cache() {
    assert_raw_file_is_streamed("shot.NEF");
}

fn assert_raw_file_is_streamed(name: &str) {
    let root = TempRoot::new("hash-raw");
    let content = random_bytes(11, 64 * 1024);
    let expected = sha_hex(&content);
    let fixture = write_file(&root, name, &content);
    let cache = byte_cache(1024 * 1024);
    assert!(
        cache.can_cache(i64::try_from(content.len()).unwrap()),
        "it would fit: only the RAW rule keeps it out"
    );
    let service = FileHashService::new(Some(Arc::clone(&cache))).unwrap();

    let actual = get(&service, &fixture).unwrap();

    assert_eq!(expected, actual);
    assert!(cache.last_read_thread_id_for_tests().is_none());
    assert_eq!(cache.count(), 0);
}

// C#: FileHashServiceTests.FileHashServiceUsesTheByteCacheForSmallNonRawFiles
#[test]
fn get_uses_byte_cache_for_small_non_raw_files() {
    let root = TempRoot::new("hash-small");
    let content = random_bytes(5, 4096);
    let fixture = write_file(&root, "small.jpg", &content);
    let cache = byte_cache(1024 * 1024);
    let service = FileHashService::new(Some(Arc::clone(&cache))).unwrap();

    let actual = get(&service, &fixture).unwrap();

    assert_eq!(sha_hex(&content), actual);
    assert_eq!(cache.count(), 1);
}

// C#: FileHashServiceTests.FileHashServiceDeduplicatesConcurrentReads (16 concurrent callers, all hashes equal)
#[test]
fn get_deduplicates_concurrent_reads() {
    let root = TempRoot::new("hash-dedup");
    let fixture = write_file(&root, "hash-fixture.bin", &[1, 2, 4]);
    let service = Arc::new(FileHashService::new(None).unwrap());
    let expected = get(&service, &fixture).unwrap();

    let handles: Vec<_> = (0..16)
        .map(|_| {
            let service = Arc::clone(&service);
            let fixture = fixture.clone();
            thread::spawn(move || get(&service, &fixture))
        })
        .collect();
    let results: Vec<String> = handles.into_iter().map(|h| h.join().unwrap().unwrap()).collect();

    assert!(results.iter().all(|hash| *hash == expected));
}

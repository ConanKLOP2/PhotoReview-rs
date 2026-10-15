//! Port of the C# `ImageCacheKey` tests (`tests/PhotoReview.Imaging.Tests/ImageCacheKeyTests.cs` and
//! `Properties/ImageCacheKeyPropertyTests.cs`, baseline d92d6aae), plus golden checks against
//! `tests/golden/image-cache-key.json` (oracle command `image-cache-key`, 91 cases).
//!
//! Port of `PhotoReview.Imaging/ImageCacheKey.cs` (C# baseline d92d6aae).
//!
//! Red phase (W4-01a): every body in `crates/pr-imaging/src/cache_key.rs` is `todo!()`, so these tests compile against
//! the stub and FAIL until the porter implements the unit. Native twins are `#[ignore = "native: real file system"]`
//! and run over `PhysicalFileSystem` and a `TempRoot` (removed on drop).
//!
//! Documented skips and substitutions:
//! - `PropertyRunner.Check` has no Rust twin: [`Rng`] is a deterministic xorshift64 (no `rand` crate), the iteration
//!   counts are kept (8000 and 3000).
//! - The golden `file` case `file-directory-is-not-a-source` and `file-unicode-name` run over the real file system only
//!   (the native twin); the fake twin skips them, as the contract requires.

use std::collections::HashMap;
use std::fs::OpenOptions;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::Path;
use std::time::{Duration, SystemTime};

use pr_core::abstractions::FileSystem;
use pr_core::catalog::types::CatalogEntry;
use pr_core::io::physical::PhysicalFileSystem;
use pr_core::model::DecoderBackend;
use pr_core::text::dotnet_path::PathError;
use pr_core::time::DateTimeUtc;
use pr_imaging::cache_key::{CacheKeyError, ImageCacheKey, KeyOptions};
use pr_imaging::decode_types::DecodeBox;
use pr_test_support::golden::{self, bool_of, i64_of, str_of};
use pr_test_support::{FakeFileSystem, TempRoot};
use serde_json::Value;

/// `638000000000000000` ticks: a fixed UTC write time used by every fixture.
const T0: i64 = 638_000_000_000_000_000;
/// One second in .NET ticks.
const ONE_SECOND: i64 = 10_000_000;
/// .NET ticks of 1970-01-01T00:00:00Z.
const UNIX_EPOCH_TICKS: i64 = 621_355_968_000_000_000;
const CACHE_IDENTITY: &str = r"C:\Pics\cache-identity.jpg";
const FAKE_ROOT: &str = r"C:\Golden";
const ROOT_TOKEN: &str = "<ROOT>";
const GOLDEN: &str = "image-cache-key";

fn dt(ticks: i64) -> DateTimeUtc {
    DateTimeUtc::from_ticks(ticks).unwrap_or_else(|| panic!("ticks {ticks} are outside the .NET DateTime range"))
}

fn opts() -> KeyOptions {
    KeyOptions::default()
}

fn i32_of(value: &Value, key: &str) -> i32 {
    i32::try_from(i64_of(value, key)).unwrap_or_else(|_| panic!("'{key}' does not fit an i32: {value}"))
}

fn fake_with_identity() -> FakeFileSystem {
    let fs = FakeFileSystem::new();
    fs.add_file_at(CACHE_IDENTITY, &[1u8, 2, 3], dt(T0));
    fs
}

fn width_key(fs: &dyn FileSystem, path: &str, original: bool, width: i32) -> ImageCacheKey {
    ImageCacheKey::create_width_only(fs, path, original, width, opts()).expect("width-only key")
}

fn box_key(fs: &dyn FileSystem, path: &str, original: bool, width: i32, height: i32) -> ImageCacheKey {
    ImageCacheKey::create(fs, path, original, DecodeBox::new(width, height), opts()).expect("box key")
}

fn hash_of(key: &ImageCacheKey) -> u64 {
    let mut hasher = DefaultHasher::new();
    key.hash(&mut hasher);
    hasher.finish()
}

// C#: ImageCacheKeyTests.CacheKeyReusesUnchangedSourceModeWidth
#[test]
fn cache_key_reuses_unchanged_source_mode_width() {
    let fs = fake_with_identity();
    let preview = width_key(&fs, CACHE_IDENTITY, false, 2400);
    let same = width_key(&fs, CACHE_IDENTITY, false, 2400);
    assert!(preview == same);
}

// C#: ImageCacheKeyTests.CacheKeySeparatesDecodeWidthAndOriginalMode
#[test]
fn cache_key_separates_decode_width_and_original_mode() {
    let fs = fake_with_identity();
    let preview = width_key(&fs, CACHE_IDENTITY, false, 2400);
    let resized = width_key(&fs, CACHE_IDENTITY, false, 3200);
    let original = width_key(&fs, CACHE_IDENTITY, true, 0);
    assert!(preview != resized && preview != original);
    assert!(preview.matches_current_source(&fs));
}

// C#: ImageCacheKeyTests.CacheKeyRejectsReplacementAtSamePath
#[test]
fn cache_key_rejects_replacement_at_same_path() {
    let fs = fake_with_identity();
    let preview = width_key(&fs, CACHE_IDENTITY, false, 2400);
    // A replacement may change the length, the write time or both: this one changes both.
    fs.add_file_at(CACHE_IDENTITY, &[4u8, 5, 6, 7], dt(T0 + ONE_SECOND));
    let replacement = width_key(&fs, CACHE_IDENTITY, false, 2400);
    assert!(preview != replacement && !preview.matches_current_source(&fs));
}

// C#: ImageCacheKeyTests.CacheKeyRejectsReplacementAtSamePath (native twin over the real file system)
#[test]
#[ignore = "native: real file system"]
fn cache_key_rejects_replacement_at_same_path_native() {
    let temp = TempRoot::new("cache-key");
    let path = temp.join("cache-identity.jpg");
    let path_text = path.to_str().expect("UTF-8 temp path");
    std::fs::write(&path, [1u8, 2, 3]).expect("write the source");
    let disk = PhysicalFileSystem;
    let preview = width_key(&disk, path_text, false, 2400);
    std::fs::write(&path, [4u8, 5, 6, 7]).expect("replace the source");
    let replacement = width_key(&disk, path_text, false, 2400);
    assert!(preview != replacement && !preview.matches_current_source(&disk));
}

// C#: ImageCacheKeyTests.CacheKeyRejectsRemovedSource
#[test]
fn cache_key_rejects_removed_source() {
    let fs = fake_with_identity();
    fs.add_file_at(CACHE_IDENTITY, &[4u8, 5, 6, 7], dt(T0 + ONE_SECOND));
    let replacement = width_key(&fs, CACHE_IDENTITY, false, 2400);
    fs.delete(Path::new(CACHE_IDENTITY)).expect("delete the source");
    assert!(!replacement.matches_current_source(&fs));
}

// C#: ImageCacheKeyTests.CacheKey_BoxHeight_IsPartOfIdentity
#[test]
fn cache_key_box_height_is_part_of_identity() {
    let fs = fake_with_identity();
    let boxed = box_key(&fs, CACHE_IDENTITY, false, 2304, 1280);
    let same = box_key(&fs, CACHE_IDENTITY, false, 2304, 1280);
    let taller = box_key(&fs, CACHE_IDENTITY, false, 2304, 1408);
    let width_only = width_key(&fs, CACHE_IDENTITY, false, 2304);

    assert_eq!(boxed.target_box(), DecodeBox::new(2304, 1280));
    assert_eq!(boxed.target_height(), 1280);
    assert!(boxed == same);
    assert!(boxed != taller);
    assert!(boxed != width_only);
    assert_eq!(width_only.target_height(), 0);
}

// C#: ImageCacheKeyTests.CacheKey_Original_IgnoresBox
#[test]
fn cache_key_original_ignores_box() {
    let fs = fake_with_identity();
    let original = box_key(&fs, CACHE_IDENTITY, true, 2304, 1280);

    assert!(original.target_box().is_unbounded());
    assert!(original == width_key(&fs, CACHE_IDENTITY, true, 0));
    let preview = box_key(&fs, CACHE_IDENTITY, false, 2304, 1280);
    assert!(ImageCacheKey::create_original(&preview) == original);
}

/// Deterministic xorshift64 (no `rand` crate; the rule of W2-05). Stands in for C# `PropertyRunner`.
struct Rng(u64);

impl Rng {
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    /// Uniform-enough index in `0..bound` (`bound > 0`).
    fn below(&mut self, bound: usize) -> usize {
        let bound = u64::try_from(bound).expect("small bound");
        usize::try_from(self.next_u64() % bound).expect("index fits usize")
    }
}

/// Spellings of one file (case, `/`, `.` and `..` segments) and of a second file: normalisation must map every spelling
/// of a file to one key path.
const SPELLINGS: [&[&str]; 2] = [
    &[
        r"C:\Pics\A.jpg",
        r"c:\pics\a.JPG",
        r"C:\Pics\.\A.jpg",
        "C:/Pics/A.jpg",
        r"C:\Pics\Sub\..\A.jpg",
    ],
    &[r"C:\Pics\B.jpg", r"c:\PICS\b.jpg"],
];
const WIDTHS: [i32; 3] = [0, 1600, 2400];
const HEIGHTS: [i32; 2] = [0, 900];
const BACKENDS: [DecoderBackend; 4] = [
    DecoderBackend::Wpf,
    DecoderBackend::WicDirect,
    DecoderBackend::TurboJpeg,
    DecoderBackend::LibRaw,
];

/// One randomly drawn key description (C# record `Spec`).
#[derive(Debug, Clone, Copy)]
struct Spec {
    file: usize,
    spelling: usize,
    length: i64,
    ticks: i64,
    original: bool,
    w: i32,
    h: i32,
    orient: bool,
    backend: DecoderBackend,
    kind: u8,
}

impl Spec {
    fn build(&self) -> ImageCacheKey {
        let mut entry = CatalogEntry::new(SPELLINGS[self.file][self.spelling]).expect("spelling is not blank");
        entry.length = Some(self.length);
        entry.last_write_utc = Some(dt(self.ticks));
        let options = KeyOptions {
            orientation_applied: self.orient,
            backend: self.backend,
            source_kind: self.kind,
        };
        ImageCacheKey::create_for_entry(
            &FakeFileSystem::new(),
            &entry,
            self.original,
            DecodeBox::new(self.w, self.h),
            options,
        )
        .expect("an entry with length and write time builds a key")
    }

    /// Identity fields as the C# `SameIdentity`: the spelling is not one of them, the box is ignored for Original.
    fn same_identity(&self, other: &Spec) -> bool {
        self.file == other.file
            && self.length == other.length
            && self.ticks == other.ticks
            && self.original == other.original
            && (self.original || (self.w == other.w && self.h == other.h))
            && self.orient == other.orient
            && self.backend == other.backend
            && self.kind == other.kind
    }
}

fn random_spec(rng: &mut Rng) -> Spec {
    let file = rng.below(SPELLINGS.len());
    Spec {
        file,
        spelling: rng.below(SPELLINGS[file].len()),
        length: 1 + i64::try_from(rng.below(2)).expect("small"),
        ticks: T0 + i64::try_from(rng.below(2)).expect("small"),
        original: rng.below(3) == 0,
        w: WIDTHS[rng.below(WIDTHS.len())],
        h: HEIGHTS[rng.below(HEIGHTS.len())],
        orient: rng.below(2) == 0,
        backend: BACKENDS[rng.below(BACKENDS.len())],
        kind: u8::try_from(rng.below(3)).expect("small"),
    }
}

// C#: ImageCacheKeyPropertyTests.KeyEquality_MatchesIdentityFields
#[test]
fn key_equality_matches_identity_fields() {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    for _ in 0..8000 {
        let a = random_spec(&mut rng);
        let b = if rng.below(3) == 0 {
            Spec {
                spelling: rng.below(SPELLINGS[a.file].len()),
                ..a
            }
        } else {
            random_spec(&mut rng)
        };
        let key_a = a.build();
        let key_b = b.build();
        let expected = a.same_identity(&b);
        let context = format!("{a:?} vs {b:?}");

        assert_eq!(expected, key_a == key_b, "== for {context}");
        assert_eq!(expected, key_a.eq(&key_b), "eq for {context}");
        assert_eq!(!expected, key_a != key_b, "!= for {context}");
        assert_eq!(key_a == key_b, key_b == key_a, "symmetry for {context}");
        if expected {
            assert_eq!(hash_of(&key_a), hash_of(&key_b), "hash for {context}");
            assert_eq!(key_a.path(), key_b.path(), "path for {context}");
        }
        let mut dictionary = HashMap::new();
        dictionary.insert(key_a.clone(), 1);
        assert_eq!(
            expected,
            dictionary.contains_key(&key_b),
            "HashMap lookup for {context}"
        );
    }
}

// C#: ImageCacheKeyPropertyTests.OriginalKeys_IgnoreTheBox
#[test]
fn original_keys_ignore_the_box() {
    let mut rng = Rng(0xD1B5_4A32_D192_ED03);
    for _ in 0..3000 {
        let spec = Spec {
            original: true,
            ..random_spec(&mut rng)
        };
        let key = spec.build();
        assert_eq!(key.target_width(), 0);
        assert_eq!(key.target_height(), 0);

        let other_box = Spec {
            w: i32::try_from(rng.below(5000)).expect("small"),
            h: i32::try_from(rng.below(5000)).expect("small"),
            ..spec
        };
        assert_eq!(key, other_box.build());

        let preview = Spec {
            original: false,
            w: 2400,
            h: 900,
            ..spec
        }
        .build();
        let from_preview = ImageCacheKey::create_original(&preview);
        assert!(from_preview.is_original());
        assert_eq!(preview.path(), from_preview.path());
        assert_eq!(preview.length(), from_preview.length());
        assert_eq!(preview.last_write_utc_ticks(), from_preview.last_write_utc_ticks());
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Golden checks (tests/golden/image-cache-key.json)
// ---------------------------------------------------------------------------------------------------------------

/// The `<ROOT>` token of the golden file stands for the fixture root: upper-cased inside a key path, as typed in a
/// file name (`file_name`).
struct Root {
    raw: String,
    upper: String,
}

impl Root {
    fn new(raw: &str) -> Self {
        Self {
            raw: raw.to_owned(),
            upper: raw.to_ascii_uppercase(),
        }
    }
}

fn parse_backend(name: &str) -> DecoderBackend {
    match name {
        "Wpf" => DecoderBackend::Wpf,
        "WicDirect" => DecoderBackend::WicDirect,
        "TurboJpeg" => DecoderBackend::TurboJpeg,
        "LibRaw" => DecoderBackend::LibRaw,
        other => panic!("unknown decoder backend '{other}' in the golden file"),
    }
}

/// Options of an `entry` input (`orient`, `backend`, `source_kind`, each optional) or of a snapshot object
/// (`orientation_applied`, `backend`, `source_kind`).
fn options_of(value: &Value) -> KeyOptions {
    KeyOptions {
        orientation_applied: value
            .get("orient")
            .or_else(|| value.get("orientation_applied"))
            .and_then(Value::as_bool)
            .unwrap_or(true),
        backend: parse_backend(value.get("backend").and_then(Value::as_str).unwrap_or("Wpf")),
        source_kind: u8::try_from(value.get("source_kind").and_then(Value::as_i64).unwrap_or(0))
            .expect("source kind fits a u8"),
    }
}

/// Builds a key from a snapshot object (`path`, `length`, `ticks`, `is_original`, `width`, `height`, options).
fn key_from_snapshot(snapshot: &Value) -> ImageCacheKey {
    let mut entry = CatalogEntry::new(str_of(snapshot, "path")).expect("snapshot path is not blank");
    entry.length = Some(i64_of(snapshot, "length"));
    entry.last_write_utc = Some(dt(i64_of(snapshot, "ticks")));
    ImageCacheKey::create_for_entry(
        &FakeFileSystem::new(),
        &entry,
        bool_of(snapshot, "is_original"),
        DecodeBox::new(i32_of(snapshot, "width"), i32_of(snapshot, "height")),
        options_of(snapshot),
    )
    .expect("snapshot key builds")
}

/// C# exception type name of a [`CacheKeyError`], as the golden file names it.
fn error_type_name(err: &CacheKeyError) -> &'static str {
    match err {
        CacheKeyError::SourceNoLongerExists { .. } => "FileNotFoundException",
        CacheKeyError::InvalidPath(PathError::Argument) => "ArgumentException",
        CacheKeyError::InvalidPath(PathError::TooLong) => "PathTooLongException",
    }
}

/// Compares a key with a golden `result` (or `original`, `original_kind`) object.
fn assert_key_fields(name: &str, key: &ImageCacheKey, expected: &Value, root: &Root) {
    let path = str_of(expected, "path").replace(ROOT_TOKEN, &root.upper);
    assert_eq!(key.path(), path, "{name}: path");
    assert_eq!(key.length(), i64_of(expected, "length"), "{name}: length");
    assert_eq!(key.last_write_utc_ticks(), i64_of(expected, "ticks"), "{name}: ticks");
    assert_eq!(
        key.is_original(),
        bool_of(expected, "is_original"),
        "{name}: is_original"
    );
    assert_eq!(key.target_width(), i32_of(expected, "width"), "{name}: width");
    assert_eq!(key.target_height(), i32_of(expected, "height"), "{name}: height");
    assert_eq!(
        key.target_box().is_unbounded(),
        bool_of(expected, "box_unbounded"),
        "{name}: box_unbounded"
    );
    assert_eq!(
        key.orientation_applied(),
        bool_of(expected, "orientation_applied"),
        "{name}: orientation_applied"
    );
    assert_eq!(
        format!("{:?}", key.backend()),
        str_of(expected, "backend"),
        "{name}: backend"
    );
    assert_eq!(
        i64::from(key.source_kind()),
        i64_of(expected, "source_kind"),
        "{name}: source_kind"
    );
}

/// Compares an outcome (a key or an error) with a golden `result` object.
fn check_outcome(name: &str, result: Result<ImageCacheKey, CacheKeyError>, expected: &Value, root: &Root) {
    if let Some(throws) = expected.get("throws").and_then(Value::as_str) {
        let err = match result {
            Err(err) => err,
            Ok(key) => panic!("{name}: expected {throws}, got the key {key:?}"),
        };
        assert_eq!(error_type_name(&err), throws, "{name}: exception type");
        if let CacheKeyError::SourceNoLongerExists { full_name } = &err {
            let file_name = str_of(expected, "file_name").replace(ROOT_TOKEN, &root.raw);
            assert_eq!(full_name, &file_name, "{name}: file_name");
        }
        if throws == "FileNotFoundException" {
            assert_eq!(err.to_string(), str_of(expected, "message"), "{name}: message");
        }
    } else {
        let key = result.unwrap_or_else(|err| panic!("{name}: unexpected error {err}"));
        assert_key_fields(name, &key, expected, root);
    }
}

fn no_root() -> Root {
    Root::new("")
}

fn cases_of<'a>(golden: &'a golden::Golden, kind: &str) -> Vec<&'a Value> {
    golden
        .cases
        .iter()
        .filter(|case| str_of(case, "kind") == kind)
        .collect()
}

// C#: golden `image-cache-key` kind `entry` (CreateFromEntry, CatalogEntry fast path with no file system access)
#[test]
fn golden_entry_cases_match_csharp() {
    let golden = golden::load(GOLDEN);
    let entries = cases_of(&golden, "entry");
    assert_eq!(entries.len(), 49, "entry cases in the golden file");
    for case in entries {
        let name = str_of(case, "name");
        let input = &case["input"];
        let expected = &case["result"];
        let mut entry = match CatalogEntry::new(str_of(input, "path")) {
            Ok(entry) => entry,
            Err(_) => {
                assert_eq!(
                    str_of(expected, "throws"),
                    "ArgumentException",
                    "{name}: blank path rejected"
                );
                continue;
            }
        };
        entry.length = Some(i64_of(input, "length"));
        entry.last_write_utc = Some(dt(i64_of(input, "ticks")));
        let result = ImageCacheKey::create_for_entry(
            &FakeFileSystem::new(),
            &entry,
            bool_of(input, "original"),
            DecodeBox::new(i32_of(input, "w"), i32_of(input, "h")),
            options_of(input),
        );
        check_outcome(name, result, expected, &no_root());
    }
}

// C#: golden `image-cache-key` kind `pair` (Equals, ==, !=, GetHashCode and Path of two keys)
#[test]
fn golden_pair_cases_match_csharp() {
    let golden = golden::load(GOLDEN);
    let pairs = cases_of(&golden, "pair");
    assert_eq!(pairs.len(), 21, "pair cases in the golden file");
    for case in pairs {
        let name = str_of(case, "name");
        let a = key_from_snapshot(&case["a"]);
        let b = key_from_snapshot(&case["b"]);
        assert_eq!(a == b, bool_of(case, "equal"), "{name}: ==");
        assert_eq!(a != b, bool_of(case, "not_equal"), "{name}: !=");
        assert_eq!(a.eq(&b), bool_of(case, "equals_method"), "{name}: Equals");
        assert_eq!(hash_of(&a) == hash_of(&b), bool_of(case, "hash_equal"), "{name}: hash");
        assert_eq!(a.path() == b.path(), bool_of(case, "path_equal"), "{name}: path");
    }
}

// C#: golden `image-cache-key` kind `original_of` (CreateOriginal, both overloads)
#[test]
fn golden_original_of_cases_match_csharp() {
    let golden = golden::load(GOLDEN);
    let originals = cases_of(&golden, "original_of");
    assert_eq!(originals.len(), 4, "original_of cases in the golden file");
    for case in originals {
        let name = str_of(case, "name");
        let source = key_from_snapshot(&case["source"]);
        assert_key_fields(
            name,
            &ImageCacheKey::create_original(&source),
            &case["original"],
            &no_root(),
        );
        let kind = u8::try_from(i64_of(case, "kind_arg")).expect("kind_arg fits a u8");
        assert_key_fields(
            name,
            &ImageCacheKey::create_original_with_kind(&source, kind),
            &case["original_kind"],
            &no_root(),
        );
    }
}

/// The fixture side of a golden `file` case: a real or fake file system with one fixture root.
trait Disk {
    fn fs(&self) -> &dyn FileSystem;
    fn root(&self) -> &Root;

    /// `<root>\<name>` as the overloads receive it (already a full path).
    fn path_of(&self, name: &str) -> String {
        format!(r"{}\{name}", self.root().raw)
    }
    /// Creates or replaces a file with the given bytes and write time (ticks).
    fn put_file(&self, name: &str, bytes: &[u8], ticks: i64);
    fn put_dir(&self, name: &str);
    fn set_write_time(&self, name: &str, ticks: i64);
    fn remove(&self, name: &str);
}

struct FakeDisk {
    fs: FakeFileSystem,
    root: Root,
}

impl FakeDisk {
    fn new() -> Self {
        Self {
            fs: FakeFileSystem::new(),
            root: Root::new(FAKE_ROOT),
        }
    }
}

impl Disk for FakeDisk {
    fn fs(&self) -> &dyn FileSystem {
        &self.fs
    }

    fn root(&self) -> &Root {
        &self.root
    }

    fn put_file(&self, name: &str, bytes: &[u8], ticks: i64) {
        self.fs.add_file_at(self.path_of(name), bytes, dt(ticks));
    }

    fn put_dir(&self, name: &str) {
        self.fs.add_dir(self.path_of(name));
    }

    fn set_write_time(&self, name: &str, ticks: i64) {
        self.fs.set_last_write(self.path_of(name), dt(ticks));
    }

    fn remove(&self, name: &str) {
        self.fs
            .delete(Path::new(&self.path_of(name)))
            .expect("delete the fake source");
    }
}

/// .NET ticks to a [`SystemTime`] (100 ns resolution, the NTFS write time resolution).
fn system_time_of(ticks: i64) -> SystemTime {
    let nanos = u64::try_from(ticks - UNIX_EPOCH_TICKS).expect("after 1970") * 100;
    SystemTime::UNIX_EPOCH + Duration::from_nanos(nanos)
}

struct NativeDisk {
    temp: TempRoot,
    root: Root,
    fs: PhysicalFileSystem,
}

impl NativeDisk {
    fn new(label: &str) -> Self {
        let temp = TempRoot::new(label);
        let root = Root::new(temp.path().to_str().expect("UTF-8 temp path"));
        Self {
            temp,
            root,
            fs: PhysicalFileSystem,
        }
    }

    fn file_path(&self, name: &str) -> std::path::PathBuf {
        self.temp.join(name)
    }
}

impl Disk for NativeDisk {
    fn fs(&self) -> &dyn FileSystem {
        &self.fs
    }

    fn root(&self) -> &Root {
        &self.root
    }

    fn put_file(&self, name: &str, bytes: &[u8], ticks: i64) {
        let path = self.file_path(name);
        std::fs::write(&path, bytes).expect("write the fixture");
        self.set_write_time(name, ticks);
    }

    fn put_dir(&self, name: &str) {
        std::fs::create_dir_all(self.file_path(name)).expect("create the fixture folder");
    }

    fn set_write_time(&self, name: &str, ticks: i64) {
        let file = OpenOptions::new()
            .write(true)
            .open(self.file_path(name))
            .expect("open the fixture");
        file.set_modified(system_time_of(ticks)).expect("set the write time");
    }

    fn remove(&self, name: &str) {
        std::fs::remove_file(self.file_path(name)).expect("delete the fixture");
    }
}

/// The overload a golden `file` case names, applied to `path` (a full path).
fn create_by_overload(
    disk: &dyn Disk,
    overload: &str,
    path: &str,
    original: bool,
    width: i32,
    height: i32,
) -> Result<ImageCacheKey, CacheKeyError> {
    let fs = disk.fs();
    match overload {
        "path" => ImageCacheKey::create(fs, path, original, DecodeBox::new(width, height), opts()),
        "width" => ImageCacheKey::create_width_only(fs, path, original, width, opts()),
        "fileinfo" => {
            let stat = fs.get_file_stat(Path::new(path));
            ImageCacheKey::create_from_stat(path, stat, original, DecodeBox::new(width, height), opts())
        }
        "entry-no-stat" => {
            let entry = CatalogEntry::new(path).expect("fixture path is not blank");
            ImageCacheKey::create_for_entry(fs, &entry, original, DecodeBox::new(width, height), opts())
        }
        other => panic!("unknown overload '{other}' in the golden file"),
    }
}

/// Runs one golden `file` case on `disk`: builds the fixture, creates the key, applies the `then` change, and checks
/// the match flags and the recreated key.
fn run_file_case(disk: &dyn Disk, case: &Value) {
    let name = str_of(case, "name");
    let input = &case["input"];
    let file = &input["file"];
    let file_name = str_of(file, "name");
    let write_ticks = i64_of(file, "write_ticks");
    let length = usize::try_from(i64_of(file, "length")).expect("length is not negative");
    if file.get("directory").and_then(Value::as_bool).unwrap_or(false) {
        disk.put_dir(file_name);
    } else if file.get("create").and_then(Value::as_bool).unwrap_or(true) {
        disk.put_file(file_name, &vec![b'a'; length], write_ticks);
    }

    let overload = str_of(input, "overload");
    let original = bool_of(input, "original");
    let width = i32_of(input, "w");
    let height = i32_of(input, "h");
    let path = disk.path_of(file_name);
    let result = create_by_overload(disk, overload, &path, original, width, height);
    let expected = &case["result"];
    if expected.get("throws").is_some() {
        check_outcome(name, result, expected, disk.root());
        return;
    }

    let key = result.unwrap_or_else(|err| panic!("{name}: unexpected error {err}"));
    check_outcome(name, Ok(key.clone()), expected, disk.root());
    assert_eq!(
        key.matches_current_source(disk.fs()),
        bool_of(case, "matches_before"),
        "{name}: matches_before"
    );

    match input.get("then").and_then(Value::as_str) {
        None => {}
        Some("rewrite") => {
            let new_length = usize::try_from(i64_of(input, "then_length")).expect("length is not negative");
            disk.put_file(file_name, &vec![b'b'; new_length], write_ticks + ONE_SECOND);
        }
        Some("touch") => disk.set_write_time(file_name, i64_of(input, "then_ticks")),
        Some("grow") => disk.put_file(file_name, &vec![b'a'; length + 1], write_ticks),
        Some("delete") => disk.remove(file_name),
        Some(other) => panic!("{name}: unknown 'then' action '{other}'"),
    }

    assert_eq!(
        key.matches_current_source(disk.fs()),
        bool_of(case, "matches_after"),
        "{name}: matches_after"
    );
    if let Some(recreated_equals) = case.get("recreated_equals").and_then(Value::as_bool) {
        let recreated = create_by_overload(disk, overload, &path, original, width, height)
            .unwrap_or_else(|err| panic!("{name}: recreate failed: {err}"));
        assert_eq!(recreated == key, recreated_equals, "{name}: recreated_equals");
    }
}

// C#: golden `image-cache-key` kind `file`, on the fake file system. The two cases that need the real disk
// (directory, Windows Unicode file name) run only in the native twin below.
#[test]
fn golden_file_cases_fake_file_system() {
    let golden = golden::load(GOLDEN);
    let files = cases_of(&golden, "file");
    assert_eq!(files.len(), 17, "file cases in the golden file");
    let mut ran = 0;
    for case in files {
        if matches!(
            str_of(case, "name"),
            "file-directory-is-not-a-source" | "file-unicode-name"
        ) {
            continue;
        }
        run_file_case(&FakeDisk::new(), case);
        ran += 1;
    }
    assert_eq!(ran, 15, "fake twins that ran");
}

// C#: golden `image-cache-key` kind `file`, on the real file system (every case, one temp root each).
#[test]
#[ignore = "native: real file system"]
fn golden_file_cases_physical_file_system() {
    let golden = golden::load(GOLDEN);
    for case in cases_of(&golden, "file") {
        run_file_case(&NativeDisk::new("cache-key-golden"), case);
    }
}

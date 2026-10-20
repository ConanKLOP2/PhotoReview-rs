//! Tests for `pr_app::composition::factories` (contract W8-09a). Port of the `ServiceFactoriesTests` slice of the C# baseline
//! d92d6aae (`tests/PhotoReview.App.Tests/Composition/ServiceFactoriesTests.cs`); `AppDecoderFactory_EveryBackendGoesThroughTheWebpHeicSwitch`
//! belongs to W8-09b (it resolves the whole service graph).
//!
//! The recovery tests run on `FakeFileSystem` and `FakeRecycleBin` (never the real Recycle Bin, AGENTS.md). The prefetch tests use
//! real temp files (the cache stats the file) and a counting `SourceReader`.

use std::io::{self, Cursor};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use pr_app::composition::decoders::{DecoderDeps, ProbeFn, ProbeResult};
use pr_app::composition::factories::{
    create_recovery_retry_service, create_source_prefetch, create_webp_heic_decoder, raw_decoder_options,
};
use pr_core::abstractions::{
    Clock, FileSystem, PhysicalSourceReader, ReadSeek, RecycleBin, SourceReadPriority, SourceReader,
};
use pr_core::app_paths::AppPaths;
use pr_core::file_actions::errors::SOURCE_STILL_EXISTS_AFTER_RECOVERY;
use pr_core::file_actions::journal::OperationJournal;
use pr_core::file_actions::journal_line::{JournalDateTime, JournalEntry, JournalGroupMember};
use pr_core::file_actions::recovery_retry::EnglishRecoveryRetryTexts;
use pr_core::model::{FileOperationType, JournalState};
use pr_core::settings::app_settings::{AppSettings, EnglishSettingsTexts};
use pr_core::settings::store::SettingsStore;
use pr_core::sync::CancelToken;
use pr_core::time::DateTimeUtc;
use pr_imaging::cache::source_bytes::SourceBytesCache;
use pr_imaging::decode_types::{DecodeRequest, DecodedImageView, ImageInfo};
use pr_imaging::decoder_traits::{DecodeError, ImageDecoder};
use pr_imaging::memory_headroom::MemoryInfo;
use pr_imaging::pixel_memory::PixelBufferImageCodec;
use pr_imaging::webp_heic::WicCodecSupport;
use pr_imaging_raw::RawPreviewFallback;
use pr_libraw::decoder::{LibRawApi, LibRawSession};
use pr_test_support::{FakeClock, FakeFileSystem, FakeRecycleBin, RecordingLog, TempRoot};
use pr_turbojpeg::decoder::{EnglishTurboJpegTexts, FineScaleError};

// ---------------------------------------------------------------------------------------------------------------
// Recovery retry: the live permanent-delete setting (Q-R8)
// ---------------------------------------------------------------------------------------------------------------

const STAMP_TICKS: i64 = 639_253_296_000_000_000; // 2026-09-18T12:00:00Z
const JPG: &str = r"C:\photos\a.jpg";
const RAW: &str = r"C:\photos\a.cr2";

fn stamp() -> DateTimeUtc {
    DateTimeUtc::from_ticks(STAMP_TICKS).expect("fixture ticks are in range")
}

/// C# `NoBinForRawRecycleBin`: the RAW file lives on a drive without a Recycle Bin, so only a permanent delete may touch it.
struct NoBinForRawRecycleBin {
    inner: FakeRecycleBin,
    raw: PathBuf,
}

impl RecycleBin for NoBinForRawRecycleBin {
    fn send_to_recycle_bin(&self, path: &Path) -> io::Result<()> {
        self.inner.send_to_recycle_bin(path)
    }

    fn can_recycle(&self, path: &Path) -> bool {
        !path.as_os_str().eq_ignore_ascii_case(self.raw.as_os_str())
    }

    fn delete_permanently(&self, path: &Path) -> io::Result<()> {
        self.inner.delete_permanently(path)
    }

    fn try_restore(
        &self,
        original_path: &Path,
        expected_size: u64,
        expected_last_write_utc: DateTimeUtc,
    ) -> io::Result<bool> {
        self.inner
            .try_restore(original_path, expected_size, expected_last_write_utc)
    }
}

struct RecoveryRig {
    fs: Arc<FakeFileSystem>,
    bin: Arc<NoBinForRawRecycleBin>,
    settings: Arc<SettingsStore>,
    failed: JournalEntry,
    service: pr_core::file_actions::recovery_retry::RecoveryRetryService,
}

impl RecoveryRig {
    fn new() -> Self {
        let fs = Arc::new(FakeFileSystem::new());
        fs.add_file_at(JPG, &[1, 2, 3, 4], stamp());
        fs.add_file_at(RAW, &[1, 2, 3, 4, 5, 6, 7, 8], stamp());
        let bin = Arc::new(NoBinForRawRecycleBin {
            inner: FakeRecycleBin::new(Arc::clone(&fs)),
            raw: PathBuf::from(RAW),
        });
        let paths = AppPaths::new(Path::new(r"C:\data"), None, false);
        let fs_dyn: Arc<dyn FileSystem> = Arc::clone(&fs) as Arc<dyn FileSystem>;
        let journal_clock: Arc<dyn Clock> = Arc::new(FakeClock::at(stamp()));
        let service_clock: Arc<dyn Clock> = Arc::new(FakeClock::at(stamp()));
        let settings = Arc::new(SettingsStore::new(
            paths.clone(),
            Arc::clone(&fs_dyn),
            Arc::new(RecordingLog::new()),
            Arc::clone(&journal_clock),
            Arc::new(EnglishSettingsTexts),
        ));
        let journal = Arc::new(OperationJournal::new(&paths, Arc::clone(&fs_dyn), journal_clock));
        let member = |source: &str, size: i64, permanent: bool| JournalGroupMember {
            source: source.to_owned(),
            destination: None,
            size,
            last_write_utc: JournalDateTime::utc(stamp()),
            permanent,
        };
        let failed = JournalEntry {
            id: "delete-group".to_owned(),
            op_type: FileOperationType::Recycle,
            state: JournalState::Failed,
            source: JPG.to_owned(),
            destination: None,
            size: 4,
            last_write_utc: JournalDateTime::utc(stamp()),
            timestamp_utc: JournalDateTime::utc(stamp()),
            error: Some("x".to_owned()),
            error_code: Some(SOURCE_STILL_EXISTS_AFTER_RECOVERY.to_owned()),
            permanent: Some(true),
            undo: None,
            group_id: Some("capture-a".to_owned()),
            group_members: Some(vec![member(JPG, 4, false), member(RAW, 8, true)]),
        };
        journal.append(&failed).expect("the fake journal accepts the line");
        let service = create_recovery_retry_service(
            journal,
            fs_dyn,
            service_clock,
            Arc::clone(&bin) as Arc<dyn RecycleBin>,
            Arc::clone(&settings),
            Arc::new(EnglishRecoveryRetryTexts),
            None,
        );
        Self {
            fs,
            bin,
            settings,
            failed,
            service,
        }
    }

    fn set_allow_permanent(&self, value: bool) {
        // A NEW AppSettings instance each time: a closure that copied the old value would not see it.
        let mut settings = AppSettings::new(&EnglishSettingsTexts);
        settings.allow_permanent_delete_without_recycle_bin = value;
        self.settings
            .save(settings)
            .expect("the fake disk accepts the settings");
    }

    fn retry(&self) -> pr_core::file_actions::recovery_retry::RecoveryRetryResult {
        self.service
            .retry_move_or_copy(&self.failed, false, &CancelToken::none())
            .expect("the token is never cancelled")
    }
}

// C#: ServiceFactoriesTests.RecoveryRetry_PermanentMemberIsRefusedWhileTheSettingIsOff
#[test]
fn recovery_retry_permanent_member_is_refused_while_the_setting_is_off() {
    let rig = RecoveryRig::new();
    rig.set_allow_permanent(false);

    let result = rig.retry();

    assert!(!result.succeeded, "{}", result.message);
    assert!(rig.bin.inner.binned_paths().is_empty());
    assert!(rig.bin.inner.permanently_deleted().is_empty());
    assert!(rig.fs.file_exists(Path::new(JPG)));
    assert!(rig.fs.file_exists(Path::new(RAW)));
}

// C#: ServiceFactoriesTests.RecoveryRetry_HonoursTheSettingChangedAfterConstruction
#[test]
fn recovery_retry_honours_the_setting_changed_after_construction() {
    let rig = RecoveryRig::new();
    rig.set_allow_permanent(false);
    assert!(!rig.retry().succeeded);

    rig.set_allow_permanent(true); // a NEW AppSettings instance: the closure must read store.current() at call time

    let result = rig.retry();

    assert!(result.succeeded, "{}", result.message);
    assert_eq!(rig.bin.inner.binned_paths(), [PathBuf::from(JPG)]);
    assert_eq!(rig.bin.inner.permanently_deleted(), [PathBuf::from(RAW)]);
}

// ---------------------------------------------------------------------------------------------------------------
// RAW wiring
// ---------------------------------------------------------------------------------------------------------------

struct StubDecoder;

impl ImageDecoder for StubDecoder {
    fn decode(&self, _request: &DecodeRequest) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
        Err(DecodeError::NotSupported {
            message: "stub".to_owned(),
            user_text: None,
            inner_hresult: None,
        })
    }

    fn read_info(&self, _path: &str) -> Result<ImageInfo, DecodeError> {
        Err(DecodeError::NotSupported {
            message: "stub".to_owned(),
            user_text: None,
            inner_hresult: None,
        })
    }
}

struct StubFallback;

impl RawPreviewFallback for StubFallback {
    fn read_jpeg_thumbnail(&self, _path: &str, _format: pr_imaging_raw::RawFormat) -> Result<Vec<u8>, DecodeError> {
        Ok(Vec::new())
    }
}

fn probe_counting(result: ProbeResult, calls: &Arc<AtomicUsize>) -> ProbeFn {
    let calls = Arc::clone(calls);
    Arc::new(move || {
        calls.fetch_add(1, Ordering::SeqCst);
        result.clone()
    })
}

fn same_object<T: ?Sized>(a: &Arc<T>, b: &Arc<T>) -> bool {
    std::ptr::addr_eq(Arc::as_ptr(a), Arc::as_ptr(b))
}

// C#: ServiceFactoriesTests.CreateRawDecoder_LibRawAvailable_WiresBothThePreviewFallbackAndTheNoPreviewDecoder
#[test]
fn create_raw_decoder_lib_raw_available_wires_both_the_preview_fallback_and_the_no_preview_decoder() {
    let no_preview: Arc<dyn ImageDecoder> = Arc::new(StubDecoder);
    let fallback: Arc<dyn RawPreviewFallback> = Arc::new(StubFallback);
    let probes = Arc::new(AtomicUsize::new(0));
    let created = Arc::new(AtomicUsize::new(0));
    let (fb, np, count) = (Arc::clone(&fallback), Arc::clone(&no_preview), Arc::clone(&created));

    let options = raw_decoder_options(
        Arc::new(PhysicalSourceReader),
        None,
        &probe_counting(ProbeResult::ok(), &probes),
        &{
            let count = Arc::clone(&count);
            move || {
                count.fetch_add(1, Ordering::SeqCst);
                Arc::clone(&fb)
            }
        },
        &move || {
            count.fetch_add(1, Ordering::SeqCst);
            Arc::clone(&np)
        },
    );

    let wired_fallback = options.preview_fallback.as_ref().expect("the fallback is wired");
    let wired_decoder = options
        .no_preview_decoder
        .as_ref()
        .expect("the no-preview decoder is wired");
    assert!(same_object(wired_fallback, &fallback));
    assert!(same_object(wired_decoder, &no_preview));
    assert_eq!(probes.load(Ordering::SeqCst), 1, "the probe runs exactly once");
    assert_eq!(created.load(Ordering::SeqCst), 2, "each creator runs once");
    assert!(options.source_reader.is_some());
}

// C#: ServiceFactoriesTests.CreateRawDecoder_LibRawMissing_WiresNeitherAndNeverCreatesThem
#[test]
fn create_raw_decoder_lib_raw_missing_wires_neither_and_never_creates_them() {
    let probes = Arc::new(AtomicUsize::new(0));
    let created = Arc::new(AtomicUsize::new(0));
    let (a, b) = (Arc::clone(&created), Arc::clone(&created));

    let options = raw_decoder_options(
        Arc::new(PhysicalSourceReader),
        None,
        &probe_counting(ProbeResult::unavailable("libraw.dll not found"), &probes),
        &move || {
            a.fetch_add(1, Ordering::SeqCst);
            Arc::new(StubFallback) as Arc<dyn RawPreviewFallback>
        },
        &move || {
            b.fetch_add(1, Ordering::SeqCst);
            Arc::new(StubDecoder) as Arc<dyn ImageDecoder>
        },
    );

    assert!(options.preview_fallback.is_none());
    assert!(options.no_preview_decoder.is_none());
    assert_eq!(created.load(Ordering::SeqCst), 0);
    assert_eq!(probes.load(Ordering::SeqCst), 1);
}

// ---------------------------------------------------------------------------------------------------------------
// Source prefetch (Q-FMT-WEBP-HEIC)
// ---------------------------------------------------------------------------------------------------------------

/// C# `CountingSourceReader`: serves four bytes and records every opened path.
#[derive(Default)]
struct CountingSourceReader {
    opened: Mutex<Vec<PathBuf>>,
}

impl CountingSourceReader {
    fn opened(&self) -> Vec<PathBuf> {
        self.opened.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }
}

impl SourceReader for CountingSourceReader {
    fn open_source(
        &self,
        path: &Path,
        _priority: SourceReadPriority,
        _buffer_size: usize,
    ) -> io::Result<Box<dyn ReadSeek>> {
        self.opened
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(path.to_path_buf());
        Ok(Box::new(Cursor::new(vec![1u8, 2, 3, 4])))
    }
}

fn codecs(heif: bool) -> WicCodecSupport {
    WicCodecSupport {
        webp: true,
        heif_container: heif,
        hevc_decoder: heif,
        detail: "t".to_owned(),
    }
}

fn prefetch_for(
    reader: &Arc<CountingSourceReader>,
    enabled: bool,
    heif: bool,
) -> pr_imaging::preload::scheduler::PrefetchSourceBytesFn {
    let reader_dyn: Arc<dyn SourceReader> = Arc::clone(reader) as Arc<dyn SourceReader>;
    let cache = Arc::new(SourceBytesCache::new(1 << 20, 0, Some(reader_dyn)).expect("a positive capacity"));
    create_source_prefetch(cache, Arc::new(move || enabled), Arc::new(move || codecs(heif)))
}

fn temp_file(root: &TempRoot, name: &str) -> PathBuf {
    let path = root.join(name);
    std::fs::write(&path, [1u8, 2, 3, 4]).expect("the temp folder is writable");
    path
}

// C#: ServiceFactoriesTests.SourcePrefetch_SkipsTheReadOnlyForFilesTheRouterWouldRefuse
#[test]
fn source_prefetch_skips_the_read_only_for_files_the_router_would_refuse() {
    let rows = [
        ("a.jpg", true, false, true),
        ("a.heic", true, false, false),  // HEIF codec missing
        ("a.webp", true, false, true),   // WebP codec present
        ("a.webp", false, false, false), // switch off
        ("a.heic", true, true, true),
    ];
    for (name, enabled, heif, expect_read) in rows {
        let root = TempRoot::new("webp-heic-prefetch");
        let path = temp_file(&root, name);
        let reader = Arc::new(CountingSourceReader::default());
        let prefetch = prefetch_for(&reader, enabled, heif);

        let result = prefetch(path.to_str().expect("utf-8 temp path"), &CancelToken::none());

        assert!(result.is_ok(), "{name}: {result:?}");
        let expected: Vec<PathBuf> = if expect_read { vec![path] } else { Vec::new() };
        assert_eq!(reader.opened(), expected, "{name} enabled={enabled} heif={heif}");
    }
}

// New test: T7, the refusal is decided before the cancel check; a cancelled token never reads.
#[test]
fn source_prefetch_cancelled_token_gives_canceled_and_reads_nothing() {
    let root = TempRoot::new("webp-heic-prefetch-cancel");
    let jpg = temp_file(&root, "a.jpg");
    let webp = temp_file(&root, "a.webp");
    let reader = Arc::new(CountingSourceReader::default());
    let cancelled = CancelToken::new();
    cancelled.cancel();

    // Readable file + cancelled token => Canceled, no read.
    let enabled = prefetch_for(&reader, true, true);
    let result = enabled(jpg.to_str().expect("utf-8 temp path"), &cancelled);
    assert!(matches!(result, Err(DecodeError::Canceled)), "{result:?}");

    // Refused file + cancelled token => Ok (the conditional of the C# returns the completed task first).
    let switched_off = prefetch_for(&reader, false, true);
    let refused = switched_off(webp.to_str().expect("utf-8 temp path"), &cancelled);
    assert!(refused.is_ok(), "{refused:?}");

    assert!(reader.opened().is_empty());
}

// New test: the switch is read per call, not captured as a value.
#[test]
fn source_prefetch_reads_the_switch_at_call_time() {
    let root = TempRoot::new("webp-heic-prefetch-live");
    let webp = temp_file(&root, "a.webp");
    let reader = Arc::new(CountingSourceReader::default());
    let reader_dyn: Arc<dyn SourceReader> = Arc::clone(&reader) as Arc<dyn SourceReader>;
    let cache = Arc::new(SourceBytesCache::new(1 << 20, 0, Some(reader_dyn)).expect("a positive capacity"));
    let switch = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let live = Arc::clone(&switch);
    let prefetch = create_source_prefetch(
        cache,
        Arc::new(move || live.load(Ordering::SeqCst)),
        Arc::new(|| codecs(true)),
    );
    let path = webp.to_str().expect("utf-8 temp path");

    prefetch(path, &CancelToken::none()).expect("refused is Ok");
    assert!(reader.opened().is_empty());

    switch.store(true, Ordering::SeqCst);
    prefetch(path, &CancelToken::none()).expect("read is Ok");
    assert_eq!(reader.opened(), [webp]);
}

// New test: T8, a failing read comes back as a DecodeError (a missing file is not a panic and not a success).
#[test]
fn source_prefetch_maps_a_missing_file_to_a_decode_error() {
    let root = TempRoot::new("webp-heic-prefetch-missing");
    let reader = Arc::new(CountingSourceReader::default());
    let prefetch = prefetch_for(&reader, true, true);

    let result = prefetch(
        root.join("absent.jpg").to_str().expect("utf-8 temp path"),
        &CancelToken::none(),
    );

    assert!(
        matches!(result, Err(DecodeError::FileNotFound { .. }) | Err(DecodeError::Io(_))),
        "{result:?}"
    );
    assert!(reader.opened().is_empty());
}

// ---------------------------------------------------------------------------------------------------------------
// WebP/HEIC decoder chain
// ---------------------------------------------------------------------------------------------------------------

struct UnreachableLibRaw;

impl LibRawApi for UnreachableLibRaw {
    fn init(&self) -> Result<Box<dyn LibRawSession>, DecodeError> {
        panic!("the LibRaw library must not be opened by this test");
    }

    fn format_error(&self, error_code: i32) -> String {
        format!("LibRaw error {error_code}")
    }
}

/// Serves bytes that no image codec accepts and records the opened paths: the decode fails inside WIC.
#[derive(Default)]
struct GarbageSourceReader {
    opened: Mutex<Vec<PathBuf>>,
}

impl SourceReader for GarbageSourceReader {
    fn open_source(
        &self,
        path: &Path,
        _priority: SourceReadPriority,
        _buffer_size: usize,
    ) -> io::Result<Box<dyn ReadSeek>> {
        self.opened
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(path.to_path_buf());
        Ok(Box::new(Cursor::new(b"this is not an image".to_vec())))
    }
}

/// A Wpf stand-in that counts its decode calls and fails with a recognisable error.
#[derive(Default)]
struct CountingWpf {
    decodes: AtomicUsize,
}

impl ImageDecoder for CountingWpf {
    fn decode(&self, _request: &DecodeRequest) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
        self.decodes.fetch_add(1, Ordering::SeqCst);
        Err(DecodeError::Other {
            message: "wpf stand-in".to_owned(),
        })
    }

    fn read_info(&self, _path: &str) -> Result<ImageInfo, DecodeError> {
        Err(DecodeError::Other {
            message: "wpf stand-in".to_owned(),
        })
    }
}

fn webp_deps(reader: Arc<dyn SourceReader>) -> DecoderDeps {
    DecoderDeps {
        source_reader: reader,
        codec: PixelBufferImageCodec::shared(),
        memory_info: Arc::new(|| MemoryInfo {
            total_available: 0,
            load: 0,
        }),
        libraw_api: Arc::new(UnreachableLibRaw),
        fine_scaler: Arc::new(|_: &_, _, _| Err(FineScaleError::OutOfMemory)),
        turbojpeg_texts: Arc::new(EnglishTurboJpegTexts),
    }
}

// New test: Win32 host (NE-8): WicDirect stands alone, over the shared reader, and no fallback decoder exists to catch its failure.
#[test]
fn webp_heic_decoder_without_wpf_fallback_is_wic_alone() {
    let reader = Arc::new(GarbageSourceReader::default());
    let decoder = create_webp_heic_decoder(
        &webp_deps(Arc::clone(&reader) as Arc<dyn SourceReader>),
        None,
        None,
        None,
    );

    let error = decoder
        .decode(&DecodeRequest::new(r"C:\x\a.webp", 0))
        .err()
        .expect("garbage bytes cannot be decoded");

    assert!(
        !matches!(error, DecodeError::Other { .. }),
        "WIC's own error, not a fallback's: {error:?}"
    );
    assert_eq!(
        reader.opened.lock().unwrap_or_else(PoisonError::into_inner).len(),
        1,
        "the shared reader is used"
    );
}

// New test: with a Wpf decoder the chain is WicDirect wrapped by the INV-12 fallback.
#[test]
fn webp_heic_decoder_with_wpf_fallback_is_wrapped() {
    let reader = Arc::new(GarbageSourceReader::default());
    let wpf = Arc::new(CountingWpf::default());

    let decoder = create_webp_heic_decoder(
        &webp_deps(Arc::clone(&reader) as Arc<dyn SourceReader>),
        Some(Arc::clone(&wpf) as Arc<dyn ImageDecoder>),
        None,
        None,
    );
    let error = decoder
        .decode(&DecodeRequest::new(r"C:\x\a.webp", 0))
        .err()
        .expect("both decoders fail");

    assert_eq!(wpf.decodes.load(Ordering::SeqCst), 1, "WIC's failure fell back to Wpf");
    assert!(
        matches!(error, DecodeError::Other { .. }),
        "the fallback's error comes out: {error:?}"
    );
    assert_eq!(
        reader.opened.lock().unwrap_or_else(PoisonError::into_inner).len(),
        1,
        "WicDirect read the file once"
    );
}

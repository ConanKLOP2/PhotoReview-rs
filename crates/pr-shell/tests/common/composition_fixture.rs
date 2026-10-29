use std::{io, path::Path, sync::Arc};

use pr_app::localization::service::CultureLookup;
use pr_core::{
    abstractions::{
        DisplayClock, DisplayTiming, ExplorerOrderProvider, InProcessLiveOperationRegistry, ReadSeek, ShellIntegration,
        SourceReadPriority, SourceReader,
    },
    app_paths::AppPaths,
    catalog::{
        explorer_types::{ExplorerGroupState, ExplorerOrderStatus, ExplorerQueryProgress, ExplorerViewSnapshot},
        natural_cmp::ManagedNaturalComparer,
    },
    time::DateTimeUtc,
};
use pr_imaging::{
    decode_types::{DecodeRequest, DecodedImageView, ImageInfo},
    decoder_traits::{DecodeError, ImageDecoder},
    memory_headroom::MemoryInfo,
    pixels::PixelBuffer,
};
use pr_l10n::loader::UiCulture;
use pr_libraw::decoder::{LibRawApi, LibRawSession};
use pr_shell::composition::{
    contracts::SharedServiceOptions,
    platform::{PlatformServices, UnavailableEmbeddedThumbnails},
    registry::{SharedServiceInputs, SharedServices},
};
use pr_test_support::{FakeFileSystem, FakeMemoryProbe, FakeRecycleBin, RecordingLog, TempRoot, golden::Golden};
use pr_turbojpeg::decoder::FineScaleError;

pub fn temp_root() -> TempRoot {
    TempRoot::new("composition-registry")
}
pub fn registrations_golden() -> Golden {
    pr_test_support::golden::load("shell-composition-registrations")
}

pub struct FixtureExplorer;
impl ExplorerOrderProvider for FixtureExplorer {
    fn try_get_snapshot_progressive(
        &self,
        folder: &Path,
        _timeout: std::time::Duration,
        _progress: Option<&(dyn Fn(ExplorerQueryProgress) + Sync)>,
        _batch_size: usize,
        _cancel: &pr_core::sync::CancelToken,
    ) -> ExplorerViewSnapshot {
        ExplorerViewSnapshot {
            folder: folder.to_string_lossy().into_owned(),
            ordered_paths: Vec::new(),
            sort_columns: Vec::new(),
            group_state: ExplorerGroupState::Unknown,
            status: ExplorerOrderStatus::NoMatchingWindow,
            reason: None,
            captured_utc: DateTimeUtc::now(),
        }
    }
}
struct FixtureDisplayClock;
impl DisplayClock for FixtureDisplayClock {
    fn get_timing(&self, _window: isize) -> Option<DisplayTiming> {
        None
    }
}
struct FixtureShell;
impl ShellIntegration for FixtureShell {
    fn get_state(&self, _executable_path: &str) -> pr_core::abstractions::ShellMenuState {
        pr_core::abstractions::ShellMenuState::NotRegistered
    }
    fn register(&self, _executable_path: &str) -> io::Result<()> {
        Ok(())
    }
    fn unregister(&self) -> io::Result<()> {
        Ok(())
    }
}
struct FixtureCulture;
impl CultureLookup for FixtureCulture {
    fn lookup(&self, _name: &str) -> Option<UiCulture> {
        None
    }
}
struct FixtureSourceReader;
impl SourceReader for FixtureSourceReader {
    fn open_source(
        &self,
        _path: &Path,
        _priority: SourceReadPriority,
        _buffer_size: usize,
    ) -> io::Result<Box<dyn ReadSeek>> {
        Ok(Box::new(io::Cursor::new(Vec::<u8>::new())))
    }
}
struct FixturePhysicalMemory;
impl pr_imaging::ram_budget::PhysicalMemorySource for FixturePhysicalMemory {
    fn total_available_memory_bytes(&self) -> i64 {
        16_i64 << 30
    }
}
struct FixtureLibRaw;
impl LibRawApi for FixtureLibRaw {
    fn init(&self) -> Result<Box<dyn LibRawSession>, DecodeError> {
        Err(DecodeError::InvalidData {
            message: "unused fixture decoder".to_owned(),
        })
    }
    fn format_error(&self, error_code: i32) -> String {
        format!("fixture error {error_code}")
    }
}
pub struct FixtureDecoder;
impl ImageDecoder for FixtureDecoder {
    fn decode(&self, _request: &DecodeRequest) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
        Err(DecodeError::InvalidData {
            message: "unused fixture decoder".to_owned(),
        })
    }
    fn read_info(&self, _path: &str) -> Result<ImageInfo, DecodeError> {
        Err(DecodeError::InvalidData {
            message: "unused fixture decoder".to_owned(),
        })
    }
}

pub fn shared_services() -> (SharedServices, TempRoot, Arc<RecordingLog>) {
    shared_services_with(None, None, None)
}

pub fn fixture_decoder() -> FixtureDecoder {
    FixtureDecoder
}

#[derive(Default)]
pub struct CountingProbe {
    pub calls: std::sync::atomic::AtomicUsize,
}

#[derive(Default)]
pub struct CountingSourceReader {
    pub reads: std::sync::atomic::AtomicUsize,
}

#[derive(Clone)]
pub struct ReaderOverride(pub Arc<CountingSourceReader>);

impl SourceReader for ReaderOverride {
    fn open_source(
        &self,
        path: &Path,
        priority: SourceReadPriority,
        buffer_size: usize,
    ) -> io::Result<Box<dyn ReadSeek>> {
        self.0.open_source(path, priority, buffer_size)
    }
}

impl SourceReader for CountingSourceReader {
    fn open_source(
        &self,
        _path: &Path,
        _priority: SourceReadPriority,
        _buffer_size: usize,
    ) -> io::Result<Box<dyn ReadSeek>> {
        self.reads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(Box::new(io::Cursor::new(vec![1, 2, 3, 4])))
    }
}

impl pr_core::abstractions::MemoryProbe for CountingProbe {
    fn has_headroom(&self, _maximum_load: f64, _reserve_bytes: i64) -> bool {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        false
    }

    fn get_snapshot(&self) -> Option<pr_core::abstractions::MemorySnapshot> {
        Some(pr_core::abstractions::MemorySnapshot {
            load_percent: 99,
            available_bytes: 0,
        })
    }
}

pub fn shared_services_with(
    wpf_fallback_decoder: Option<pr_imaging::factory::DecoderProvider>,
    decoder_wrapper: Option<pr_shell::composition::registry::DecoderFactoryWrapper>,
    data_root_override: Option<String>,
) -> (SharedServices, TempRoot, Arc<RecordingLog>) {
    shared_services_with_probe_and_options(
        Arc::new(FakeMemoryProbe::new()),
        wpf_fallback_decoder,
        decoder_wrapper,
        data_root_override,
    )
}

pub fn shared_services_with_probe(
    probe: Arc<dyn pr_core::abstractions::MemoryProbe>,
) -> (SharedServices, TempRoot, Arc<RecordingLog>) {
    shared_services_with_probe_and_options(probe, None, None, None)
}

pub fn shared_services_with_move_override_and_reader(
    move_override: Arc<dyn pr_app::coordinators::traits::IMoveOverride>,
    source_reader: Arc<dyn SourceReader>,
) -> (SharedServices, TempRoot, Arc<RecordingLog>) {
    shared_services_with_parts(Some(move_override), source_reader, None)
}

pub fn shared_services_with_move_override_and_fs(
    move_override: Option<Arc<dyn pr_app::coordinators::traits::IMoveOverride>>,
    fs: Arc<FakeFileSystem>,
) -> (SharedServices, TempRoot, Arc<RecordingLog>) {
    shared_services_with_parts(move_override, Arc::new(FixtureSourceReader), Some(fs))
}

fn shared_services_with_probe_and_options(
    probe: Arc<dyn pr_core::abstractions::MemoryProbe>,
    wpf_fallback_decoder: Option<pr_imaging::factory::DecoderProvider>,
    decoder_wrapper: Option<pr_shell::composition::registry::DecoderFactoryWrapper>,
    data_root_override: Option<String>,
) -> (SharedServices, TempRoot, Arc<RecordingLog>) {
    shared_services_with_parts_and_platform(
        probe,
        wpf_fallback_decoder,
        decoder_wrapper,
        data_root_override,
        None,
        None,
        None,
    )
}

fn shared_services_with_parts(
    move_override: Option<Arc<dyn pr_app::coordinators::traits::IMoveOverride>>,
    source_reader: Arc<dyn SourceReader>,
    fake_fs: Option<Arc<FakeFileSystem>>,
) -> (SharedServices, TempRoot, Arc<RecordingLog>) {
    shared_services_with_parts_and_platform(
        Arc::new(FakeMemoryProbe::new()),
        None,
        None,
        None,
        move_override,
        Some(source_reader),
        fake_fs,
    )
}

fn shared_services_with_parts_and_platform(
    probe: Arc<dyn pr_core::abstractions::MemoryProbe>,
    wpf_fallback_decoder: Option<pr_imaging::factory::DecoderProvider>,
    decoder_wrapper: Option<pr_shell::composition::registry::DecoderFactoryWrapper>,
    data_root_override: Option<String>,
    move_override: Option<Arc<dyn pr_app::coordinators::traits::IMoveOverride>>,
    source_reader: Option<Arc<dyn SourceReader>>,
    fake_fs: Option<Arc<FakeFileSystem>>,
) -> (SharedServices, TempRoot, Arc<RecordingLog>) {
    let root = temp_root();
    let log = Arc::new(RecordingLog::new());
    let fs = fake_fs.unwrap_or_else(|| Arc::new(FakeFileSystem::new()));
    let codec = pr_imaging::pixel_memory::PixelBufferImageCodec::shared();
    let platform = PlatformServices {
        recycle_bin: Arc::new(FakeRecycleBin::new(fs)),
        live_operations: Arc::new(InProcessLiveOperationRegistry::new()),
        memory_probe: probe,
        natural_comparer: Arc::new(ManagedNaturalComparer),
        display_clock: Arc::new(FixtureDisplayClock),
        explorer_order: Arc::new(FixtureExplorer),
        physical_memory: Arc::new(FixturePhysicalMemory),
        memory_info: Arc::new(|| MemoryInfo {
            total_available: 16_i64 << 30,
            load: 0,
        }),
        shell_integration: Arc::new(FixtureShell),
        payload_codec: Arc::new(pr_platform_win::wic::cache_adapters::WicPreviewPayloadCodec),
        cache_image_reader: Arc::new(pr_platform_win::wic::cache_adapters::WicCacheImageReader),
        embedded_thumbnails: Arc::new(UnavailableEmbeddedThumbnails),
        fine_scaler: Arc::new(|_source: &PixelBuffer, _width, _height| Err(FineScaleError::OutOfMemory)),
        libraw_api: Arc::new(FixtureLibRaw),
        cultures: Arc::new(FixtureCulture),
        installed_ui_culture: UiCulture::invariant(),
        codec: Arc::clone(&codec),
    };
    let inputs = SharedServiceInputs {
        options: SharedServiceOptions {
            codec,
            wpf_fallback_decoder,
            thumbnail_decoder: Arc::new(|| Arc::new(FixtureDecoder)),
        },
        paths: AppPaths::new(root.path(), data_root_override.as_deref(), false),
        log: log.clone(),
        clock: Arc::new(pr_test_support::FakeClock::new()),
        source_reader: source_reader.unwrap_or_else(|| Arc::new(FixtureSourceReader)),
        platform,
        decoder_wrapper,
        move_override,
    };
    (SharedServices::build(inputs), root, log)
}

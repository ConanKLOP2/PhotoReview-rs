//! The Windows [`EngineHost`] of the benchmark CLI (ST-BENCH-HOST; follows ST-BENCH-BACKEND, PR #466).
//!
//! C# counterpart: the composition done by `tools/PhotoReview.Benchmark.Cli/Program.cs` (WIC Direct decoder over
//! `WpfBitmapSourceCodec`, `WindowsMemoryProbe`, `WindowsRecycleBin.Instance` behind `BenchmarkRecycleBin`, `FileLog.Default`).
//!
//! `pr-bench` cannot name these types (`pr-platform-win -> pr-app -> pr-bench`, D-W8B1-1), so the host lives in this leaf crate,
//! which nothing depends on. The real Recycle Bin is opt-in exactly as in C# (`PHOTOREVIEW_RS_BENCH_REAL_RECYCLE_BIN=1`): the
//! factory below is only invoked then. No `unsafe` here.

use std::error::Error;
use std::sync::Arc;

use pr_bench::bench_recycle_bin;
use pr_bench::cli::{EngineHost, ExecutorParts, ToolError};
use pr_bench::image_executor::BenchmarkExecutorOptions;
use pr_core::abstractions::{Log, RecycleBin};
use pr_core::diagnostics::file_log::FileLog;
use pr_imaging::pixel_memory::PixelBufferImageCodec;
use pr_imaging::ram_budget::get_physical_memory_bytes;
use pr_platform_win::memory_probe::WindowsMemoryProbe;
use pr_platform_win::physical_memory::WindowsPhysicalMemory;
use pr_platform_win::recycle_bin::WindowsRecycleBin;
use pr_platform_win::recycle_restore::WindowsRecycleBinRestoreBackend;
use pr_platform_win::wic::cache_adapters::WicPreviewPayloadCodec;
use pr_platform_win::wic::decoder::WicDirectDecoder;

/// `Log` over the process-wide `FileLog::default_instance()` (C# `FileLog.Default`), resolved on every call.
#[derive(Debug, Default, Clone, Copy)]
pub struct DefaultFileLog;

impl Log for DefaultFileLog {
    fn enabled(&self) -> bool {
        FileLog::default_instance().enabled()
    }
    fn info(&self, message: &str) {
        FileLog::default_instance().info(message);
    }
    fn warn(&self, message: &str) {
        FileLog::default_instance().warn(message);
    }
    fn error(&self, message: &str, error: Option<&(dyn Error + 'static)>) {
        FileLog::default_instance().error(message, error);
    }
}

/// Builds the real Recycle Bin. Only called when the opt-in switch is set.
type RealBinFactory = Arc<dyn Fn() -> Arc<dyn RecycleBin> + Send + Sync>;

/// The real Windows host. Constructing it touches nothing: every OS object is created inside the trait methods.
pub struct WindowsEngineHost {
    log: Arc<dyn Log>,
    use_real_bin: bool,
    real_bin: RealBinFactory,
}

impl std::fmt::Debug for WindowsEngineHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WindowsEngineHost")
            .field("use_real_bin", &self.use_real_bin)
            .finish_non_exhaustive()
    }
}

impl WindowsEngineHost {
    /// Production host: log = `FileLog.Default`, real bin only if `PHOTOREVIEW_RS_BENCH_REAL_RECYCLE_BIN=1`.
    pub fn new() -> Self {
        let log: Arc<dyn Log> = Arc::new(DefaultFileLog);
        let bin_log = Arc::clone(&log);
        Self::with_parts(
            log,
            bench_recycle_bin::use_real_bin(),
            Arc::new(move || {
                Arc::new(WindowsRecycleBin::with_log(
                    Arc::new(WindowsRecycleBinRestoreBackend::new(Arc::clone(&bin_log))),
                    Arc::clone(&bin_log),
                )) as Arc<dyn RecycleBin>
            }),
        )
    }

    /// Seam for tests: explicit log, explicit opt-in and a fake "real" bin.
    pub fn with_parts(log: Arc<dyn Log>, use_real_bin: bool, real_bin: RealBinFactory) -> Self {
        Self {
            log,
            use_real_bin,
            real_bin,
        }
    }
}

impl Default for WindowsEngineHost {
    fn default() -> Self {
        Self::new()
    }
}

impl EngineHost for WindowsEngineHost {
    fn executor_parts(&self) -> Result<ExecutorParts, ToolError> {
        let codec = PixelBufferImageCodec::shared();
        let memory = Arc::new(WindowsPhysicalMemory);
        let decoder = Arc::new(WicDirectDecoder::new(codec.clone(), None, memory.clone()));
        Ok(ExecutorParts {
            platform_codec: codec,
            payload_codec: Arc::new(WicPreviewPayloadCodec),
            options: BenchmarkExecutorOptions {
                memory_probe: Some(Arc::new(WindowsMemoryProbe::new(Arc::clone(&self.log)))),
                decoder: Some(decoder),
                log: Some(Arc::clone(&self.log)),
                physical_memory_bytes: get_physical_memory_bytes(&*memory),
                ..BenchmarkExecutorOptions::default()
            },
        })
    }

    fn recycle_bin(&self) -> Arc<dyn RecycleBin> {
        let factory = Arc::clone(&self.real_bin);
        bench_recycle_bin::create_with(self.use_real_bin, move || factory())
    }

    fn log(&self) -> Arc<dyn Log> {
        Arc::clone(&self.log)
    }

    fn logging_enabled(&self) -> bool {
        FileLog::default_instance().enabled()
    }

    fn set_logging_enabled(&self, enabled: bool) {
        FileLog::default_instance().set_enabled(enabled);
    }

    fn machine_name(&self) -> Option<String> {
        std::env::var("COMPUTERNAME").ok()
    }
}

//! The engine-backed [`BenchmarkBackend`]: the profile catalog, `RunProfileAsync` through the real executor and workload
//! runner, and the report/summary JSON writers (ST-GAPMAP2 section 6, item 1; replaces `UnwiredBackend` in the CLI).
//!
//! Everything platform specific (the WIC decoder, the payload codec, the memory probe, the real Recycle Bin, the app log)
//! comes from an [`EngineHost`]: `pr-platform-win` depends on `pr-app`, which depends on this crate (D-W8B1-1), so this
//! crate cannot name those types. [`UnwiredEngineHost`] is the host of the `pr-bench` binary until a crate that can name
//! them supplies a real one: the catalog and `--benchmark-list-profiles` work, every profile run fails with the
//! "not wired" message. No timing is produced without a real host.

use std::sync::Arc;
use std::sync::mpsc;

use pr_core::abstractions::{Log, NullLog, RecycleBin};
use pr_core::diagnostics::file_log::FileLog;
use pr_core::io::physical::new_guid_n;
use pr_core::model::serde_ext::LenientEnum;
use pr_core::sync::CancelToken;
use pr_imaging::cache::preview_file::PreviewPayloadCodec;
use pr_imaging::decoder_traits::PlatformImageCodec;

use super::backend::{
    BenchmarkBackend, DatasetManifest, PhaseStatus, PhaseSummary, ProfileCatalog, ProfileInfo, ProfileRun,
    ProgressLine, ToolError,
};
use crate::engine::{BenchmarkError, BenchmarkProgress, RunPreparedOptions};
use crate::image_executor::{BenchmarkExecutorOptions, BenchmarkImageExecutor, BenchmarkProfileScope, ExecutorError};
use crate::models::{
    BenchmarkBatchSummary, BenchmarkDatasetManifest, BenchmarkPhaseResult, BenchmarkProfile, BenchmarkReport,
    BenchmarkResultStatus, BenchmarkTimestamp, BenchmarkWorkload,
};
use crate::{bench_recycle_bin, profiles, workload_runner};

/// Message of the error [`UnwiredEngineHost`] returns.
pub const UNWIRED_HOST_MESSAGE: &str = "the benchmark engine is not wired into this build";

/// What one executor needs from the platform (C# `WpfBitmapSourceCodec.Instance` + `WpfBitmapImageDecoder`).
pub struct ExecutorParts {
    pub platform_codec: Arc<dyn PlatformImageCodec>,
    pub payload_codec: Arc<dyn PreviewPayloadCodec>,
    /// Carries the decoder (`decoder: None` is an error), the memory probe and the physical RAM.
    pub options: BenchmarkExecutorOptions,
}

/// The platform side of the engine backend.
pub trait EngineHost {
    /// Codecs and options for the executor of one profile run. An error is a per-profile failure.
    fn executor_parts(&self) -> Result<ExecutorParts, ToolError>;
    /// `BenchmarkRecycleBin.Create(() => WindowsRecycleBin.Instance)`; called once per profile run.
    fn recycle_bin(&self) -> Arc<dyn RecycleBin>;
    /// The log handed to the engine (C# `FileLog.Default`).
    fn log(&self) -> Arc<dyn Log>;
    /// `AppLog.Enabled` getter, used by `BenchmarkProfileScope.ApplyLogging`.
    fn logging_enabled(&self) -> bool;
    /// `AppLog.Enabled` setter.
    fn set_logging_enabled(&self, enabled: bool);
    /// `Environment.MachineName`; `None` = the engine reads `COMPUTERNAME`.
    fn machine_name(&self) -> Option<String> {
        None
    }
}

/// Host of the `pr-bench` binary today: no codecs, so a profile run fails (honestly) before anything is measured.
#[derive(Debug, Default, Clone, Copy)]
pub struct UnwiredEngineHost;

impl EngineHost for UnwiredEngineHost {
    fn executor_parts(&self) -> Result<ExecutorParts, ToolError> {
        Err(ToolError::invalid_operation(UNWIRED_HOST_MESSAGE))
    }
    fn recycle_bin(&self) -> Arc<dyn RecycleBin> {
        // The real bin is opt-in and this host never opts in: the factory is not called.
        bench_recycle_bin::create_with(false, || unreachable!("the unwired host never asks for the real bin"))
    }
    fn log(&self) -> Arc<dyn Log> {
        Arc::new(NullLog)
    }
    fn logging_enabled(&self) -> bool {
        FileLog::default_instance().enabled()
    }
    fn set_logging_enabled(&self, enabled: bool) {
        FileLog::default_instance().set_enabled(enabled);
    }
}

impl std::fmt::Debug for ExecutorParts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExecutorParts").finish_non_exhaustive()
    }
}

impl<H: EngineHost> std::fmt::Debug for EngineBackend<H> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EngineBackend")
            .field("reports", &self.reports.len())
            .finish_non_exhaustive()
    }
}

/// Engine-backed backend. Stateful like the C# lists: `reports` and `outcomes` grow in call order.
pub struct EngineBackend<H: EngineHost> {
    host: H,
    reports: Vec<BenchmarkReport>,
    outcomes: Vec<BenchmarkPhaseResult>,
}

impl<H: EngineHost> EngineBackend<H> {
    pub fn new(host: H) -> Self {
        Self {
            host,
            reports: Vec::new(),
            outcomes: Vec::new(),
        }
    }

    /// Reports stored so far, in call order.
    pub fn reports(&self) -> &[BenchmarkReport] {
        &self.reports
    }

    fn store(&mut self, report: BenchmarkReport) -> Result<ProfileRun, ToolError> {
        let phase = report
            .phases
            .first()
            .cloned()
            .ok_or_else(|| ToolError::new("ArgumentOutOfRangeException", "The benchmark report has no phase"))?;
        let run = ProfileRun {
            run_id: report.run_id.clone(),
            phase: summary_of(&phase),
        };
        self.reports.push(report);
        self.outcomes.push(phase);
        Ok(run)
    }
}

fn status_of(status: BenchmarkResultStatus) -> PhaseStatus {
    match status {
        BenchmarkResultStatus::Pass => PhaseStatus::Pass,
        BenchmarkResultStatus::Warn => PhaseStatus::Warn,
        BenchmarkResultStatus::Fail => PhaseStatus::Fail,
        BenchmarkResultStatus::InsufficientData => PhaseStatus::InsufficientData,
    }
}

fn summary_of(phase: &BenchmarkPhaseResult) -> PhaseSummary {
    PhaseSummary {
        status: status_of(phase.status),
        p50: phase.p50(),
        p95: phase.p95(),
        max: phase.max(),
    }
}

/// `BenchmarkProfile` as the CLI prints it (enum members by their .NET name).
pub fn profile_info(profile: &BenchmarkProfile) -> ProfileInfo {
    ProfileInfo {
        id: profile.id.clone(),
        name: profile.name.clone(),
        description: profile.description.clone(),
        workload: profile.workload.name().to_owned(),
        loading_mode: profile.loading_mode.name().to_owned(),
        workers: profile.workers,
        next_window: profile.next_window,
        previous_window: profile.previous_window,
        iterations: profile.iterations,
        correctness_only: profile.correctness_only,
        is_file_action: profile.workload == BenchmarkWorkload::FileAction,
    }
}

fn executor_error(error: ExecutorError) -> ToolError {
    match error {
        ExecutorError::InvalidPath(inner) => ToolError::argument(inner.to_string()),
        ExecutorError::Service(inner) => ToolError::invalid_operation(inner.to_string()),
    }
}

/// The .NET type name the CLI's `catch` prints for an engine failure.
fn engine_error(error: BenchmarkError) -> ToolError {
    let message = error.to_string();
    match error {
        BenchmarkError::FolderRequired => ToolError::argument(message),
        BenchmarkError::DirectoryNotFound(_) => ToolError::directory_not_found(message),
        BenchmarkError::Profile(_) => ToolError::new("BenchmarkProfileException", message),
        BenchmarkError::Canceled => ToolError::new("OperationCanceledException", message),
        BenchmarkError::Operation(_) => ToolError::invalid_operation(message),
    }
}

fn images_lookup(profile_id: &str, workload: BenchmarkWorkload) -> Option<i32> {
    workload_runner::registry_images_per_sample(profile_id, workload)
}

impl<H: EngineHost> ProfileCatalog for EngineBackend<H> {
    fn all(&self) -> Vec<ProfileInfo> {
        profiles::all().iter().map(profile_info).collect()
    }
    fn runnable(&self) -> Vec<ProfileInfo> {
        profiles::runnable().iter().map(profile_info).collect()
    }
    fn find(&self, id: &str) -> Option<ProfileInfo> {
        profiles::find(id).map(profile_info)
    }
}

impl<H: EngineHost> BenchmarkBackend for EngineBackend<H> {
    fn run_profile(
        &mut self,
        folder: &str,
        profile: &ProfileInfo,
        files: &[String],
        progress: &mut dyn FnMut(&ProgressLine),
    ) -> Result<ProfileRun, ToolError> {
        let registry = profiles::find(&profile.id)
            .ok_or_else(|| ToolError::invalid_operation(format!("Unknown benchmark profile '{}'", profile.id)))?
            .clone();
        let parts = self.host.executor_parts()?;
        let executor = Arc::new(
            BenchmarkImageExecutor::new(
                &registry,
                files,
                parts.platform_codec,
                parts.payload_codec,
                parts.options,
            )
            .map_err(executor_error)?,
        );
        let recycle_bin = self.host.recycle_bin();
        let log = self.host.log();
        let machine = self.host.machine_name();
        let mut random = workload_runner::create_seeded_random(&registry.id);
        let cancel = CancelToken::new();

        // The logging flag is set for the run and restored afterwards (C# `using var loggingScope`).
        let host = &self.host;
        let scope = BenchmarkProfileScope::apply_logging(
            &registry,
            || host.logging_enabled(),
            |enabled| host.set_logging_enabled(enabled),
            None,
        );
        let (tx, rx) = mpsc::channel::<BenchmarkProgress>();
        let result = std::thread::scope(|s| {
            let worker = s.spawn(|| {
                let sink = move |p: BenchmarkProgress| {
                    let _ = tx.send(p);
                };
                let mut options = RunPreparedOptions::new(log.as_ref(), &cancel);
                options.progress = Some(&sink);
                options.machine = machine.as_deref();
                workload_runner::run_profile(folder, &registry, &executor, files, &mut random, &recycle_bin, &options)
            });
            // Progress arrives live, on this thread; the channel closes when the worker drops its sender.
            for p in rx {
                progress(&ProgressLine {
                    profile_id: p.profile_id,
                    completed: p.completed,
                    total: p.total,
                    message: p.message,
                });
            }
            worker.join()
        });
        drop(scope);
        executor.dispose();
        match result {
            Ok(Ok(report)) => self.store(report),
            Ok(Err(error)) => Err(engine_error(error)),
            Err(_) => Err(ToolError::invalid_operation("The benchmark worker thread panicked")),
        }
    }

    fn record_failed_profile(&mut self, folder_full_path: &str, profile: &ProfileInfo, message: &str) -> ProfileRun {
        let workload = BenchmarkWorkload::from_name(&profile.workload).unwrap_or(BenchmarkWorkload::Correctness);
        let failed = BenchmarkPhaseResult::new(
            profile.id.clone(),
            workload,
            Vec::new(),
            BenchmarkResultStatus::Fail,
            Some(message.to_owned()),
            None,
        );
        let report = BenchmarkReport {
            run_id: new_guid_n(),
            started_utc: BenchmarkTimestamp::now_utc(),
            folder: folder_full_path.to_owned(),
            phases: vec![failed.clone()],
            metrics: None,
            machine: self.host.machine_name().or_else(|| std::env::var("COMPUTERNAME").ok()),
        };
        let run = ProfileRun {
            run_id: report.run_id.clone(),
            phase: summary_of(&failed),
        };
        self.outcomes.push(failed);
        self.reports.push(report);
        run
    }

    fn report_json(&self, run_id: &str) -> Result<String, ToolError> {
        let report = self
            .reports
            .iter()
            .find(|r| r.run_id == run_id)
            .ok_or_else(|| ToolError::invalid_operation(format!("No benchmark report with run id '{run_id}'")))?;
        report
            .to_json(&images_lookup)
            .map_err(|e| ToolError::argument(e.to_string()))
    }

    fn batch_summary_json(&self, dataset: &DatasetManifest) -> Result<String, ToolError> {
        let summary = BenchmarkBatchSummary {
            dataset: BenchmarkDatasetManifest {
                folder: dataset.folder.clone(),
                files: dataset.files.clone(),
                unsupported_extensions: dataset
                    .unsupported_extensions
                    .iter()
                    .map(|(ext, n)| (ext.clone(), i32::try_from(*n).unwrap_or(i32::MAX)))
                    .collect(),
                file_limit: dataset.file_limit,
            },
            reports: self.reports.clone(),
            profile_outcomes: self.outcomes.clone(),
        };
        summary
            .to_json(&images_lookup)
            .map_err(|e| ToolError::argument(e.to_string()))
    }
}

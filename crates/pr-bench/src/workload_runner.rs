//! Port of `PhotoReview.Benchmarking/BenchmarkWorkloadRunner.cs` (C# baseline d92d6aae, 243 lines).
//!
//! One workload iteration's real behaviour, shared by the benchmark window (App, W8) and the CLI: the correctness
//! guard, the file-action mapping and the preload warm-up. Sync, thread based (R0003): a C# `Task` becomes a blocking
//! function, `Func<Task<(bool, Snapshot?)>>` becomes the engine's `PreparedMeasure`, `Parallel.ForEachAsync` becomes
//! `decode_async` handles.
//!
//! Contract: `contracts/W7-03c.md`. The sibling surfaces (engine, image executor, models) are the ones of the
//! W7-03a / W7-03b / W7-01 units, see the contract's "Depends on".

use std::collections::hash_map::RandomState;
use std::error::Error;
use std::fmt;
use std::hash::{BuildHasher, Hasher};
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use pr_core::abstractions::RecycleBin;
use pr_core::sync::CancelToken;
use pr_imaging::decoder_traits::DecodeError;
use pr_imaging::service::PreviewError;
use pr_imaging::service::handle::PreviewHandle;

use crate::engine::{BenchmarkError, MeasureOutcome, PreparedMeasure, RunPreparedOptions};
use crate::image_executor::BenchmarkImageExecutor;
use crate::models::{
    BenchmarkProfile, BenchmarkProfileError, BenchmarkProfileProblem, BenchmarkReport, BenchmarkWorkload,
};
use crate::profiles;

/// The "prepare one iteration" seam of [`run_profile_with`] (C# `Func<BenchmarkWorkload, int, CancellationToken,
/// Task<Func<Task<(bool, ReviewMetricsSnapshot?)>>>>`). The profile is fixed by the caller. The returned step is
/// `'static` and not `Send` (the engine's [`PreparedMeasure`]), so a step owns `Arc`s of what it needs.
pub type PrepareIteration<'a> =
    dyn FnMut(BenchmarkWorkload, i32, &CancelToken) -> Result<PreparedMeasure, BenchmarkError> + 'a;

/// Failures of this module that the engine's [`BenchmarkError`] has no variant for. They travel as
/// `BenchmarkError::Operation(Box::new(..))` (tests `downcast_ref::<WorkloadError>()`); cancellation is NEVER wrapped
/// (`BenchmarkError::Canceled`) and a refused profile is `BenchmarkError::Profile`.
#[derive(Debug)]
pub enum WorkloadError {
    /// C# `IOException` / `UnauthorizedAccessException` of `File.*` (scratch setup, move, copy, recycle bin).
    Io(io::Error),
    /// A decode failure that is not the tolerated race outcome (never a cancellation).
    Decode(PreviewError),
    /// Empty file list (see the contract, `prepare_iteration` step 6). The text is not part of the contract.
    InvalidArgument(String),
}

impl fmt::Display for WorkloadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "{error}"),
            Self::Decode(error) => write!(f, "{error}"),
            Self::InvalidArgument(text) => f.write_str(text),
        }
    }
}

impl Error for WorkloadError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Decode(error) => Some(error),
            Self::InvalidArgument(_) => None,
        }
    }
}

impl From<WorkloadError> for BenchmarkError {
    fn from(error: WorkloadError) -> Self {
        BenchmarkError::Operation(Box::new(error))
    }
}

fn io_error(error: io::Error) -> BenchmarkError {
    WorkloadError::Io(error).into()
}

fn invalid_argument(text: &str) -> BenchmarkError {
    WorkloadError::InvalidArgument(text.to_owned()).into()
}

/// A decode failure as the engine sees it: a cancellation is `Canceled`, anything else a [`WorkloadError::Decode`].
fn decode_failure(error: PreviewError) -> BenchmarkError {
    if error.is_canceled() {
        BenchmarkError::Canceled
    } else {
        WorkloadError::Decode(error).into()
    }
}

// ---------------------------------------------------------------------------------------------------------------
// System.Random(int seed) replica
// ---------------------------------------------------------------------------------------------------------------

/// Bit-exact replica of `new System.Random(seed)` on .NET 6+ (the legacy Knuth subtractive generator, `CompatPrng`):
/// only `next` and `next_max` are needed. The benchmark reports pin which files a profile touches, so the sequence
/// must equal .NET's (golden `bench-random`). Not thread safe, like the C# class.
#[derive(Debug, Clone)]
pub struct DotNetRandom {
    seed_array: [i32; 56],
    inext: usize,
    inextp: usize,
}

impl DotNetRandom {
    /// `new Random(seed)`. `int.MinValue` uses `int.MaxValue` as the subtraction, any other negative seed its
    /// absolute value. All `+`/`-` of the algorithm wrap (C# unchecked `int`): use `wrapping_*`.
    pub fn new(seed: i32) -> Self {
        const MSEED: i32 = 161_803_398;
        let subtraction = if seed == i32::MIN { i32::MAX } else { seed.abs() };
        let mut a = [0_i32; 56];
        let mut mj = MSEED.wrapping_sub(subtraction);
        a[55] = mj;
        let mut mk = 1_i32;
        let mut ii = 0_usize;
        for _ in 1..55 {
            ii += 21;
            if ii >= 55 {
                ii -= 55;
            }
            a[ii] = mk;
            mk = mj.wrapping_sub(mk);
            if mk < 0 {
                mk = mk.wrapping_add(i32::MAX);
            }
            mj = a[ii];
        }
        for _ in 1..5 {
            for i in 1..56 {
                let mut n = i + 30;
                if n >= 55 {
                    n -= 55;
                }
                a[i] = a[i].wrapping_sub(a[1 + n]);
                if a[i] < 0 {
                    a[i] = a[i].wrapping_add(i32::MAX);
                }
            }
        }
        Self {
            seed_array: a,
            inext: 0,
            inextp: 21,
        }
    }

    fn internal_sample(&mut self) -> i32 {
        self.inext += 1;
        if self.inext >= 56 {
            self.inext = 1;
        }
        self.inextp += 1;
        if self.inextp >= 56 {
            self.inextp = 1;
        }
        let mut r = self.seed_array[self.inext].wrapping_sub(self.seed_array[self.inextp]);
        if r == i32::MAX {
            r -= 1;
        }
        if r < 0 {
            r = r.wrapping_add(i32::MAX);
        }
        self.seed_array[self.inext] = r;
        r
    }

    /// `Random.Next()`: `InternalSample()`, in `0..i32::MAX` (never `i32::MAX`).
    // Mirrors `Random::Next`; this type is not an `Iterator`.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> i32 {
        self.internal_sample()
    }

    /// `Random.Next(maxValue)`: `(int)(Sample() * maxValue)`; `max_value == 0` gives 0 but still consumes one sample.
    ///
    /// # Panics
    /// When `max_value < 0` (C# `ArgumentOutOfRangeException`), BEFORE a sample is consumed. The runner never passes a value below 1.
    pub fn next_max(&mut self, max_value: i32) -> i32 {
        assert!(
            max_value >= 0,
            "maxValue must be non-negative (ArgumentOutOfRangeException)"
        );
        let sample = f64::from(self.internal_sample()) * (1.0 / f64::from(i32::MAX));
        (sample * f64::from(max_value)) as i32
    }
}

/// `CreateSeededRandom(profileId)`: `new Random(StableHash(profileId))`. Same profile id, same file sequence in every
/// process (C# `string.GetHashCode()` is randomized per process).
pub fn create_seeded_random(profile_id: &str) -> DotNetRandom {
    DotNetRandom::new(stable_hash(profile_id))
}

/// C# private `StableHash`: `hash = 17; for each UTF-16 code unit c: hash = hash * 31 + c` in UNCHECKED `int`
/// arithmetic (wrapping). Iterates `encode_utf16()`, not `chars()`: a character outside the BMP is two units.
pub(crate) fn stable_hash(value: &str) -> i32 {
    value
        .encode_utf16()
        .fold(17_i32, |hash, unit| hash.wrapping_mul(31).wrapping_add(i32::from(unit)))
}

// ---------------------------------------------------------------------------------------------------------------
// Pure selection helpers
// ---------------------------------------------------------------------------------------------------------------

/// Images decoded by one timed sample: 1 for FirstFrame, Preload, WarmNext and FileAction; otherwise
/// `min(max(1, profile.workers), max(1, file_count))`. `file_count` and `workers` may be zero or negative.
/// The `profile` is only read for `workers`.
pub fn images_per_sample(profile: &BenchmarkProfile, workload: BenchmarkWorkload, file_count: i32) -> i32 {
    match workload {
        BenchmarkWorkload::FirstFrame
        | BenchmarkWorkload::Preload
        | BenchmarkWorkload::WarmNext
        | BenchmarkWorkload::FileAction => 1,
        BenchmarkWorkload::Sequential | BenchmarkWorkload::Random | BenchmarkWorkload::Correctness => {
            profile.workers.max(1).min(file_count.max(1))
        }
    }
}

/// The `ImagesPerSampleLookup` of W7-01a (C# `BenchmarkProfiles.Find(id) is { } p ? ImagesPerSample(p, workload,
/// int.MaxValue) : 1`, minus the `: 1`): `Some(images_per_sample(p, workload, i32::MAX))` when
/// `profiles::find(profile_id)` (OrdinalIgnoreCase) knows the id, else `None`. The caller passes `&registry_images_per_sample`
/// where the models want a lookup, which keeps `models` free of a cycle with this module.
pub fn registry_images_per_sample(profile_id: &str, workload: BenchmarkWorkload) -> Option<i32> {
    profiles::find(profile_id).map(|profile| images_per_sample(profile, workload, i32::MAX))
}

/// File indices one parallel-decode iteration (Sequential, Random, Correctness) decodes; consumes `random`.
/// `count = min(max(1, workers), file_count)`; element `o` is `select_index(file_count, workload, iteration + o)`
/// with a WRAPPING add. `file_count <= 0` gives an empty vector (C#: empty for 0; negative is unreachable, a slice
/// length is never negative). The `random` draws happen in element order.
pub fn select_parallel_indices(
    file_count: i32,
    profile: &BenchmarkProfile,
    workload: BenchmarkWorkload,
    iteration: i32,
    random: &mut DotNetRandom,
) -> Vec<i32> {
    if file_count <= 0 {
        return Vec::new();
    }
    let count = profile.workers.max(1).min(file_count);
    (0..count)
        .map(|offset| select_index(file_count, workload, iteration.wrapping_add(offset), random))
        .collect()
}

/// C# private `SelectIndex`. Precondition `file_count >= 1` (C#: `% 0` throws `DivideByZeroException`).
/// FirstFrame -> 0; Random -> `random.next_max(file_count)`; WarmNext -> even iteration `iteration / 2 % n`, odd
/// `n - 1 - iteration / 2 % n`; Preload -> `iteration * 2 % n` (wrapping multiply); everything else (Sequential,
/// FileAction, Correctness) -> `iteration % n`. `%` is the truncating remainder, like C#.
pub(crate) fn select_index(
    file_count: i32,
    workload: BenchmarkWorkload,
    iteration: i32,
    random: &mut DotNetRandom,
) -> i32 {
    match workload {
        BenchmarkWorkload::FirstFrame => 0,
        BenchmarkWorkload::Random => random.next_max(file_count),
        BenchmarkWorkload::WarmNext => {
            let half = (iteration / 2) % file_count;
            if iteration % 2 == 0 {
                half
            } else {
                (file_count - 1).wrapping_sub(half)
            }
        }
        BenchmarkWorkload::Preload => iteration.wrapping_mul(2) % file_count,
        BenchmarkWorkload::Sequential | BenchmarkWorkload::FileAction | BenchmarkWorkload::Correctness => {
            iteration % file_count
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// File-action mapping (pure part) and scratch files
// ---------------------------------------------------------------------------------------------------------------

/// The mutation a file-action iteration performs while its decode is in flight.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileActionOp {
    /// `File.Move(temp, moved); File.Delete(moved)`.
    Move,
    /// `recycleBin.SendToRecycleBin(temp)`.
    Delete,
    /// `File.Copy(temp, copied, overwrite: true)`.
    Copy,
}

/// Profile id + iteration -> operation. `action-delete` -> Delete; `action-copy` -> Copy; `action-interleaved` ->
/// `iteration % 3`: 0 Move, 1 Delete, anything else Copy (a negative remainder is Copy); every other id, including
/// `action-move` and unknown ids, -> Move. Ids are matched case-sensitively and exactly.
pub fn file_action_op(profile_id: &str, iteration: i32) -> FileActionOp {
    match profile_id {
        "action-delete" => FileActionOp::Delete,
        "action-copy" => FileActionOp::Copy,
        "action-interleaved" => match iteration % 3 {
            0 => FileActionOp::Move,
            1 => FileActionOp::Delete,
            _ => FileActionOp::Copy,
        },
        _ => FileActionOp::Move,
    }
}

/// The scratch files of one file-action iteration: `<temp dir>\PhotoReview-Benchmark-Action-<32 lowercase hex>.bin`,
/// plus `<same>.moved` and `<same>.copy`.
#[derive(Debug, Clone)]
pub(crate) struct ActionScratch {
    pub(crate) temp: PathBuf,
    pub(crate) moved: PathBuf,
    pub(crate) copied: PathBuf,
}

static SCRATCH_COUNTER: AtomicU64 = AtomicU64::new(0);

impl ActionScratch {
    /// A fresh unique name (no `rand`/`uuid` dependency: e.g. two `RandomState` hashes, or pid + counter + time).
    pub(crate) fn new() -> Self {
        let counter = SCRATCH_COUNTER.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_nanos());
        let pid = std::process::id();
        let half = |salt: u8| {
            let mut hasher = RandomState::new().build_hasher();
            hasher.write_u64(counter);
            hasher.write_u32(pid);
            hasher.write_u128(nanos);
            hasher.write_u8(salt);
            hasher.finish()
        };
        let guid = format!("{:016x}{:016x}", half(0), half(1));
        let temp = std::env::temp_dir().join(format!("PhotoReview-Benchmark-Action-{guid}.bin"));
        let mut moved = temp.clone().into_os_string();
        moved.push(".moved");
        let mut copied = temp.clone().into_os_string();
        copied.push(".copy");
        Self {
            temp,
            moved: PathBuf::from(moved),
            copied: PathBuf::from(copied),
        }
    }

    /// Best-effort delete of the three files: a missing file or an `io::Error` is ignored (C# swallows
    /// `IOException` and `UnauthorizedAccessException` per file and goes on with the next one).
    pub(crate) fn cleanup(&self) {
        for path in [&self.temp, &self.moved, &self.copied] {
            // Best-effort cleanup of benchmark temp files: every failure here is an IO failure the C# also swallows.
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Removes the scratch files when the measure step finishes (any outcome) or is dropped without running.
struct ScratchGuard(ActionScratch);

impl Drop for ScratchGuard {
    fn drop(&mut self) {
        self.0.cleanup();
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Runner
// ---------------------------------------------------------------------------------------------------------------

/// `RunProfileAsync(folder, profile, executor, files, random, recycleBin, progress, ct)`: runs one profile end to end
/// with the setup/measure split (R2-F-14). Calls [`run_profile_with`] with a prepare step that forwards to
/// [`prepare_iteration`] and `file_count = files.len()` (saturating `i32`). `options` carries what the C# optional
/// arguments carried (`progress`, the time source, the log, the token); the token the steps see is `options.cancel`.
pub fn run_profile(
    folder: &str,
    profile: &BenchmarkProfile,
    executor: &Arc<BenchmarkImageExecutor>,
    files: &[String],
    random: &mut DotNetRandom,
    recycle_bin: &Arc<dyn RecycleBin>,
    options: &RunPreparedOptions<'_>,
) -> Result<BenchmarkReport, BenchmarkError> {
    let files_arc: Arc<[String]> = Arc::from(files);
    let file_count = i32::try_from(files.len()).unwrap_or(i32::MAX);
    let mut prepare = |workload: BenchmarkWorkload, iteration: i32, ct: &CancelToken| {
        prepare_iteration(
            executor,
            &files_arc,
            profile,
            workload,
            iteration,
            random,
            recycle_bin,
            ct,
        )
    };
    run_profile_with(folder, profile, &mut prepare, options, file_count)
}

/// Test seam (C# `internal RunProfileAsync(folder, profile, prepare, progress, timeProvider, ct, fileCount)`; pass
/// `i32::MAX` for the C# default). Runs `crate::engine::run_prepared` (its operation ignores the profile the engine
/// passes and forwards `workload, iteration, token` to `prepare`), then stamps
/// `images_per_sample = Some(images_per_sample(profile, phase.workload, file_count))` on EVERY phase (the phase's own
/// workload, the given profile). An engine error is returned as is, nothing is stamped.
pub fn run_profile_with(
    folder: &str,
    profile: &BenchmarkProfile,
    prepare: &mut PrepareIteration<'_>,
    options: &RunPreparedOptions<'_>,
    file_count: i32,
) -> Result<BenchmarkReport, BenchmarkError> {
    let mut operation = |_profile: &BenchmarkProfile, workload: BenchmarkWorkload, iteration: i32, ct: &CancelToken| {
        prepare(workload, iteration, ct)
    };
    let mut report = crate::engine::run_prepared(folder, profile, &mut operation, options)?;
    for phase in &mut report.phases {
        phase.images_per_sample = Some(images_per_sample(profile, phase.workload, file_count));
    }
    Ok(report)
}

/// `PrepareIterationAsync` (R2-F-14): does the untimed setup of one iteration and returns the measure step, the only
/// part the engine times. The step must be called exactly once (a file-action step also removes its scratch files,
/// even when it fails). The step owns clones of the `Arc`s and of the profile id (the engine's step type is `'static`).
///
/// Order of checks and branches (contract section 'prepare_iteration'):
/// 1. `profiles::is_not_implemented(profile)` -> `Err(BenchmarkError::Profile(..))` with problem `NotImplemented`.
/// 2. `FileAction` -> scratch copy + measure step (decode in flight while the mutation runs).
/// 3. `FirstFrame` -> `executor.evict_for_cold_decode(path)` now (untimed), decode in the step.
/// 4. `Preload` / `WarmNext` -> one index chosen now, step = decode + `warm_preload_around(center)`.
/// 5. otherwise (Sequential, Random, Correctness) -> indices chosen now, step = parallel decode of them.
///
/// Empty `files` is `Err(Operation(WorkloadError::InvalidArgument(..)))` for steps 2 to 5 (C# throws `IndexOutOfRange`,
/// `DivideByZero` or `ArgumentOutOfRange`); check it AFTER step 1. `random` is consumed here, never in the step.
// DIVERGENCE: C# fails the empty-folder cases from prepare (FirstFrame, Preload, WarmNext, FileAction) or from the
// step (Sequential, Random, Correctness); Rust fails every one at prepare. The engine treats both the same way.
#[allow(clippy::too_many_arguments)]
pub fn prepare_iteration(
    executor: &Arc<BenchmarkImageExecutor>,
    files: &Arc<[String]>,
    profile: &BenchmarkProfile,
    workload: BenchmarkWorkload,
    iteration: i32,
    random: &mut DotNetRandom,
    recycle_bin: &Arc<dyn RecycleBin>,
    ct: &CancelToken,
) -> Result<PreparedMeasure, BenchmarkError> {
    if profiles::is_not_implemented(profile) {
        // A decode-only stand-in would misreport these profiles as passing; refuse to run them.
        return Err(BenchmarkError::Profile(BenchmarkProfileError::new(
            BenchmarkProfileProblem::NotImplemented,
            profile.id.clone(),
            format!(
                "Benchmark profile '{}' does not implement a real {} check yet; \
                 a decode-only stand-in would misreport results, so it refuses to run.",
                profile.id, profile.name
            ),
        )));
    }

    if files.is_empty() {
        return Err(invalid_argument("The benchmark folder has no files to decode."));
    }

    match workload {
        BenchmarkWorkload::FileAction => prepare_file_action(executor, files, profile, iteration, recycle_bin, ct),
        BenchmarkWorkload::FirstFrame => {
            let index = usize::try_from(select_index(file_count_of(files), workload, iteration, random))
                .map_err(|_| invalid_argument("The selected file index is negative."))?;
            let path = files[index].clone();
            // Untimed: clearing the disk cache is not part of the first frame.
            executor.evict_for_cold_decode(&path).map_err(decode_failure)?;
            let executor = Arc::clone(executor);
            let ct = ct.clone();
            Ok(Box::new(move || {
                let image = executor.decode(&path, &ct).map_err(decode_failure)?;
                Ok(MeasureOutcome {
                    correct: image.pixel_width() > 0 && image.pixel_height() > 0,
                    metrics: Some(executor.metrics()),
                })
            }))
        }
        BenchmarkWorkload::Preload | BenchmarkWorkload::WarmNext => {
            // One navigation step per iteration, not N parallel decodes.
            let center = select_index(file_count_of(files), workload, iteration, random);
            let index =
                usize::try_from(center).map_err(|_| invalid_argument("The selected file index is negative."))?;
            let executor = Arc::clone(executor);
            let files = Arc::clone(files);
            let ct = ct.clone();
            Ok(Box::new(move || {
                let image = executor.decode(&files[index], &ct).map_err(decode_failure)?;
                executor.warm_preload_around(center);
                Ok(MeasureOutcome {
                    correct: image.pixel_width() > 0 && image.pixel_height() > 0,
                    metrics: Some(executor.metrics()),
                })
            }))
        }
        BenchmarkWorkload::Sequential | BenchmarkWorkload::Random | BenchmarkWorkload::Correctness => {
            let mut selected = Vec::new();
            for index in select_parallel_indices(file_count_of(files), profile, workload, iteration, random) {
                let index =
                    usize::try_from(index).map_err(|_| invalid_argument("The selected file index is negative."))?;
                selected.push(files[index].clone());
            }
            let executor = Arc::clone(executor);
            let ct = ct.clone();
            Ok(Box::new(move || {
                // Every item is in flight at once (C# `MaxDegreeOfParallelism = count == selected.Length`).
                let handles: Vec<Result<PreviewHandle, PreviewError>> =
                    selected.iter().map(|path| executor.decode_async(path, &ct)).collect();
                let mut all_correct = true;
                let mut first_failure: Option<PreviewError> = None;
                for started in handles {
                    match started.and_then(PreviewHandle::wait) {
                        Ok(image) => {
                            all_correct &= image.pixel_width() > 0 && image.pixel_height() > 0;
                        }
                        Err(error) => {
                            // DIVERGENCE: C# reports whichever failure happens first in time; Rust the lowest index.
                            first_failure.get_or_insert(error);
                        }
                    }
                }
                if let Some(error) = first_failure {
                    return Err(decode_failure(error));
                }
                Ok(MeasureOutcome {
                    correct: all_correct,
                    metrics: Some(executor.metrics()),
                })
            }))
        }
    }
}

/// `files.len()` as the C# `int` the selection helpers take (saturating).
fn file_count_of(files: &[String]) -> i32 {
    i32::try_from(files.len()).unwrap_or(i32::MAX)
}

/// C# private `PrepareFileActionAsync`: creates the [`ActionScratch`], copies `files[iteration % files.len()]` to
/// `scratch.temp` (full read + write, untimed; on any error `cleanup()` then the error), and returns the step.
fn prepare_file_action(
    executor: &Arc<BenchmarkImageExecutor>,
    files: &Arc<[String]>,
    profile: &BenchmarkProfile,
    iteration: i32,
    recycle_bin: &Arc<dyn RecycleBin>,
    ct: &CancelToken,
) -> Result<PreparedMeasure, BenchmarkError> {
    if files.is_empty() {
        return Err(invalid_argument("The benchmark folder has no files to decode."));
    }
    // `%` truncates: a negative iteration gives a negative index (C# `IndexOutOfRangeException`).
    let remainder = i64::from(iteration) % i64::try_from(files.len()).unwrap_or(i64::MAX);
    let Ok(index) = usize::try_from(remainder) else {
        return Err(invalid_argument(
            "The file-action iteration selects a negative file index.",
        ));
    };
    // C# evaluates `files[iteration % files.Length]` before `ReadAllBytesAsync(.., ct)` sees the token, so the index error wins.
    if ct.is_canceled() {
        return Err(BenchmarkError::Canceled);
    }

    // Dropping the guard (any error below, or the step finishing, or the step being dropped) removes the files.
    let guard = ScratchGuard(ActionScratch::new());
    // Untimed setup: a full read + write of the source photo (R2-F-14).
    let bytes = std::fs::read(&files[index]).map_err(io_error)?;
    std::fs::write(&guard.0.temp, bytes).map_err(io_error)?;

    let executor = Arc::clone(executor);
    let recycle_bin = Arc::clone(recycle_bin);
    let profile_id = profile.id.clone();
    let ct = ct.clone();
    Ok(Box::new(move || {
        let scratch = &guard.0;

        // Start the decode BEFORE mutating the file: awaiting it first would hide the lifetime race these
        // profiles exist to expose. A synchronous `decode_async` error is the decode OUTCOME, judged below.
        let started = match scratch.temp.to_str() {
            Some(temp_text) => executor.decode_async(temp_text, &ct),
            None => Err(PreviewError::Io(Arc::new(io::Error::new(
                io::ErrorKind::InvalidData,
                "scratch path is not valid UTF-8",
            )))),
        };

        let op = file_action_op(&profile_id, iteration);
        let mutation = match op {
            FileActionOp::Move => {
                std::fs::rename(&scratch.temp, &scratch.moved).and_then(|()| std::fs::remove_file(&scratch.moved))
            }
            FileActionOp::Delete => recycle_bin.send_to_recycle_bin(&scratch.temp),
            FileActionOp::Copy => std::fs::copy(&scratch.temp, &scratch.copied).map(|_| ()),
        };
        if let Err(error) = mutation {
            // Still observe the in-flight decode so it never runs against files the guard is about to delete.
            if let Ok(handle) = started {
                let _ = handle.wait();
            }
            return Err(io_error(error));
        }

        // Losing the race against the mutation is the expected outcome; cancellation still propagates.
        if let Err(error) = started.and_then(PreviewHandle::wait) {
            if error.is_canceled() {
                return Err(BenchmarkError::Canceled);
            }
            if !is_tolerated_race_failure(&error) {
                return Err(WorkloadError::Decode(error).into());
            }
        }

        // Move and delete must remove the source, copy must leave it in place.
        let source_exists = scratch.temp.exists();
        Ok(MeasureOutcome {
            correct: source_exists == (op == FileActionOp::Copy),
            metrics: Some(executor.metrics()),
        })
    }))
}

/// Whether a decode failure that raced the mutation is the expected outcome. C# swallows `IOException`,
/// `UnauthorizedAccessException`, `NotSupportedException`, `FormatException`: here `error.is_io_exception()` (covers
/// `SourceChanged`, `Io`, `SourceBytes(Io)`, `Key(SourceNoLongerExists)` and `Decode(e)` with an IO `e`), or
/// `Decode(e)` where `e.is_not_supported()` or `e.kind()` is `DecodeError::Format`. A cancellation
/// (`is_canceled()`), `Busy`, `InvalidData`, `InvalidOperation`, `OutOfMemory`, `DiskEntry`, `Panicked`, `Spawn` and
/// `Key(InvalidPath(Argument))` are never tolerated.
pub(crate) fn is_tolerated_race_failure(error: &PreviewError) -> bool {
    if error.is_canceled() {
        return false;
    }
    if error.is_io_exception() {
        return true;
    }
    match error {
        PreviewError::Decode(decode) => {
            decode.is_not_supported() || matches!(decode.kind(), DecodeError::Format { .. })
        }
        _ => false,
    }
}

#[cfg(test)]
mod unit_tests {
    //! Crate-private items of this module (`stable_hash`, `ActionScratch`, `is_tolerated_race_failure`), tested in place.
    //! Scratch files are only the names `ActionScratch::new` creates in `std::env::temp_dir()`.

    use std::fs;
    use std::sync::Arc;

    use pr_core::text::dotnet_path::PathError;
    use pr_imaging::cache::preview_file::PreviewCacheError;
    use pr_imaging::cache_key::CacheKeyError;
    use pr_imaging::decoder_traits::DecodeError;
    use pr_imaging::service::{PreviewError, SourceChangePhase};

    use super::*;

    // New: the C# StableHash of "" is the seed 17; "a" is 17 * 31 + 97.
    #[test]
    fn stable_hash_of_empty_and_single_unit() {
        assert_eq!(stable_hash(""), 17);
        assert_eq!(stable_hash("a"), 624);
    }

    // New: the hash wraps in unchecked int arithmetic (golden seed of preview-balanced).
    #[test]
    fn stable_hash_wraps_like_csharp_int() {
        assert_eq!(stable_hash("preview-balanced"), 758_877_054);
    }

    // New: a non-BMP character contributes two UTF-16 code units (surrogates 0xD83D, 0xDE00), not one.
    #[test]
    fn stable_hash_counts_utf16_code_units() {
        assert_eq!(stable_hash("\u{1F600}"), 17 * 31 * 31 + 0xD83D * 31 + 0xDE00);
    }

    // New: ActionScratch names follow the contract: `PhotoReview-Benchmark-Action-<32 lowercase hex>.bin` in the OS temp dir.
    #[test]
    fn action_scratch_names_are_unique_hex_names_in_temp_dir() {
        let first = ActionScratch::new();
        let second = ActionScratch::new();
        assert_ne!(first.temp, second.temp, "two scratch sets never share a name");

        let temp_dir = std::env::temp_dir();
        assert_eq!(first.temp.parent(), Some(temp_dir.as_path()));
        let name = first
            .temp
            .file_name()
            .and_then(|n| n.to_str())
            .expect("UTF-8 file name");
        let guid = name
            .strip_prefix("PhotoReview-Benchmark-Action-")
            .and_then(|rest| rest.strip_suffix(".bin"))
            .expect("the scratch name prefix and suffix");
        assert_eq!(guid.len(), 32);
        assert!(
            guid.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "lowercase hex: {guid}"
        );

        let temp_text = first.temp.to_str().expect("UTF-8 path");
        assert_eq!(first.moved.to_str(), Some(format!("{temp_text}.moved").as_str()));
        assert_eq!(first.copied.to_str(), Some(format!("{temp_text}.copy").as_str()));
    }

    // New: cleanup removes all three scratch files, and is a no-op (no panic) when none of them exists.
    #[test]
    fn action_scratch_cleanup_removes_the_three_files_and_tolerates_missing_ones() {
        let scratch = ActionScratch::new();
        fs::write(&scratch.temp, b"source").expect("create the temp copy");
        fs::write(&scratch.copied, b"copy").expect("create the copy");
        // `moved` deliberately absent: cleanup must go on past a missing file.

        scratch.cleanup();

        assert!(!scratch.temp.exists());
        assert!(!scratch.moved.exists());
        assert!(!scratch.copied.exists());
        ActionScratch::new().cleanup();
    }

    // New: the decode outcomes that are the expected loss of the race (contract: is_tolerated_race_failure table).
    #[test]
    fn race_failures_that_are_io_or_unsupported_are_tolerated() {
        let tolerated = [
            PreviewError::SourceChanged {
                path: "a.png".to_owned(),
                phase: SourceChangePhase::Decode,
            },
            PreviewError::Io(Arc::new(std::io::Error::other("gone"))),
            PreviewError::Key(CacheKeyError::SourceNoLongerExists {
                full_name: "C:\\photos\\a.png".to_owned(),
            }),
            PreviewError::Decode(Arc::new(DecodeError::FileNotFound {
                message: "gone".to_owned(),
            })),
            PreviewError::Decode(Arc::new(DecodeError::DirectoryNotFound {
                message: "gone".to_owned(),
            })),
            PreviewError::Decode(Arc::new(DecodeError::MemoryAdmission {
                message: "full".to_owned(),
            })),
            PreviewError::Decode(Arc::new(DecodeError::Io(std::io::Error::other("read")))),
            PreviewError::Decode(Arc::new(DecodeError::Format {
                message: "bad header".to_owned(),
            })),
            PreviewError::Decode(Arc::new(DecodeError::NotSupported {
                message: "no codec".to_owned(),
                user_text: None,
                inner_hresult: None,
            })),
            PreviewError::Decode(Arc::new(DecodeError::MissingCodec {
                message: "no codec".to_owned(),
                user_text: None,
            })),
        ];
        for error in &tolerated {
            assert!(is_tolerated_race_failure(error), "{error:?} must be tolerated");
        }
    }

    // New: everything else is a real failure, never swallowed by the race handling.
    #[test]
    fn real_failures_are_not_tolerated() {
        let not_tolerated = [
            PreviewError::Canceled,
            PreviewError::Decode(Arc::new(DecodeError::InvalidData {
                message: "corrupt".to_owned(),
            })),
            PreviewError::Decode(Arc::new(DecodeError::busy())),
            PreviewError::Decode(Arc::new(DecodeError::OutOfMemory {
                message: "oom".to_owned(),
            })),
            PreviewError::Key(CacheKeyError::InvalidPath(PathError::Argument)),
            PreviewError::InvalidPath(PathError::Argument),
            PreviewError::DiskEntry(Arc::new(PreviewCacheError::Canceled)),
            PreviewError::Panicked("boom".to_owned()),
            PreviewError::Spawn(Arc::new(std::io::Error::other("no thread"))),
        ];
        for error in &not_tolerated {
            assert!(!is_tolerated_race_failure(error), "{error:?} must not be tolerated");
        }
    }
}

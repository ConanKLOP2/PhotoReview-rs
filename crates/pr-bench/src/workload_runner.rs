//! Port of `PhotoReview.Benchmarking/BenchmarkWorkloadRunner.cs` (C# baseline d92d6aae, 243 lines).
//!
//! One workload iteration's real behaviour, shared by the benchmark window (App, W8) and the CLI: the correctness
//! guard, the file-action mapping and the preload warm-up. Sync, thread based (R0003): a C# `Task` becomes a blocking
//! function, `Func<Task<(bool, Snapshot?)>>` becomes the engine's `PreparedMeasure`, `Parallel.ForEachAsync` becomes
//! `decode_async` handles.
//!
//! Contract: `contracts/W7-03c.md`. Every body is `todo!()`; signatures are final. The sibling surfaces (engine,
//! image executor, models) are the ones of the W7-03a / W7-03b / W7-01 drafts, see the contract's "Depends on".

#![allow(unused_variables, dead_code)]

use std::error::Error;
use std::fmt;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;

use pr_core::abstractions::RecycleBin;
use pr_core::sync::CancelToken;
use pr_imaging::service::PreviewError;

use crate::engine::{BenchmarkError, PreparedMeasure, RunPreparedOptions};
use crate::image_executor::BenchmarkImageExecutor;
use crate::models::{BenchmarkProfile, BenchmarkReport, BenchmarkWorkload};

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
        todo!("W7-03c: CompatPrng constructor, contract section 'DotNetRandom'")
    }

    /// `Random.Next()`: `InternalSample()`, in `0..i32::MAX` (never `i32::MAX`).
    // Mirrors `Random::Next`; this type is not an `Iterator`.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> i32 {
        todo!("W7-03c")
    }

    /// `Random.Next(maxValue)`: `(int)(Sample() * maxValue)`; `max_value == 0` gives 0 but still consumes one sample.
    ///
    /// # Panics
    /// When `max_value < 0` (C# `ArgumentOutOfRangeException`), BEFORE a sample is consumed. The runner never passes a value below 1.
    pub fn next_max(&mut self, max_value: i32) -> i32 {
        todo!("W7-03c")
    }
}

/// `CreateSeededRandom(profileId)`: `new Random(StableHash(profileId))`. Same profile id, same file sequence in every
/// process (C# `string.GetHashCode()` is randomized per process).
pub fn create_seeded_random(profile_id: &str) -> DotNetRandom {
    todo!("W7-03c")
}

/// C# private `StableHash`: `hash = 17; for each UTF-16 code unit c: hash = hash * 31 + c` in UNCHECKED `int`
/// arithmetic (wrapping). Iterates `encode_utf16()`, not `chars()`: a character outside the BMP is two units.
pub(crate) fn stable_hash(value: &str) -> i32 {
    todo!("W7-03c")
}

// ---------------------------------------------------------------------------------------------------------------
// Pure selection helpers
// ---------------------------------------------------------------------------------------------------------------

/// Images decoded by one timed sample: 1 for FirstFrame, Preload, WarmNext and FileAction; otherwise
/// `min(max(1, profile.workers), max(1, file_count))`. `file_count` and `workers` may be zero or negative.
/// The `profile` is only read for `workers`.
pub fn images_per_sample(profile: &BenchmarkProfile, workload: BenchmarkWorkload, file_count: i32) -> i32 {
    todo!("W7-03c")
}

/// The `ImagesPerSampleLookup` of W7-01a (C# `BenchmarkProfiles.Find(id) is { } p ? ImagesPerSample(p, workload,
/// int.MaxValue) : 1`, minus the `: 1`): `Some(images_per_sample(p, workload, i32::MAX))` when
/// `profiles::find(profile_id)` (OrdinalIgnoreCase) knows the id, else `None`. The caller passes `&registry_images_per_sample`
/// where the models want a lookup, which keeps `models` free of a cycle with this module.
pub fn registry_images_per_sample(profile_id: &str, workload: BenchmarkWorkload) -> Option<i32> {
    todo!("W7-03c")
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
    todo!("W7-03c")
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
    todo!("W7-03c")
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
    todo!("W7-03c")
}

/// The scratch files of one file-action iteration: `<temp dir>\PhotoReview-Benchmark-Action-<32 lowercase hex>.bin`,
/// plus `<same>.moved` and `<same>.copy`.
#[derive(Debug, Clone)]
pub(crate) struct ActionScratch {
    pub(crate) temp: PathBuf,
    pub(crate) moved: PathBuf,
    pub(crate) copied: PathBuf,
}

impl ActionScratch {
    /// A fresh unique name (no `rand`/`uuid` dependency: e.g. two `RandomState` hashes, or pid + counter + time).
    pub(crate) fn new() -> Self {
        todo!("W7-03c")
    }

    /// Best-effort delete of the three files: a missing file or an `io::Error` is ignored (C# swallows
    /// `IOException` and `UnauthorizedAccessException` per file and goes on with the next one).
    pub(crate) fn cleanup(&self) {
        todo!("W7-03c")
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
    todo!("W7-03c")
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
    todo!("W7-03c")
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
    todo!("W7-03c")
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
    todo!("W7-03c")
}

/// Whether a decode failure that raced the mutation is the expected outcome. C# swallows `IOException`,
/// `UnauthorizedAccessException`, `NotSupportedException`, `FormatException`: here `error.is_io_exception()` (covers
/// `SourceChanged`, `Io`, `SourceBytes(Io)`, `Key(SourceNoLongerExists)` and `Decode(e)` with an IO `e`), or
/// `Decode(e)` where `e.is_not_supported()` or `e.kind()` is `DecodeError::Format`. A cancellation
/// (`is_canceled()`), `Busy`, `InvalidData`, `InvalidOperation`, `OutOfMemory`, `DiskEntry`, `Panicked`, `Spawn` and
/// `Key(InvalidPath(Argument))` are never tolerated.
pub(crate) fn is_tolerated_race_failure(error: &PreviewError) -> bool {
    todo!("W7-03c")
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

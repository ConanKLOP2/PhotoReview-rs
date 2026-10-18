//! Port of `PhotoReview.Benchmarking/BenchmarkRecycleBin.cs` (C# baseline d92d6aae, 32 lines), W7-02b.
//!
//! The "delete" benchmark workloads act on a temp COPY of a photo, but every iteration through the real Recycle Bin
//! would leave an item in the user's real bin. By default the "recycle" is therefore a direct file delete of the temp
//! copy; the opt-in environment variable measures the real shell operation and accepts the leftovers (timings of the
//! two modes are not comparable).
//!
//! AGENTS.md: no test may call the real bin. Tests use only the default deleter and `FakeRecycleBin` as the "real" bin.

use std::fs;
use std::io;
use std::path::Path;
use std::sync::Arc;

use pr_core::abstractions::RecycleBin;
use pr_core::time::DateTimeUtc;

/// Opt-in switch. DIVERGENCE (proposal in the contract, open question 1): the C# name is
/// `PHOTOREVIEW_BENCH_REAL_RECYCLE_BIN`; the Rust app uses the `PHOTOREVIEW_RS_*` prefix (R0002, D4) so a value set
/// for the C# app can never make the Rust benchmark touch the user's real bin.
pub const REAL_BIN_ENVIRONMENT_VARIABLE: &str = "PHOTOREVIEW_RS_BENCH_REAL_RECYCLE_BIN";

/// Pure core of `UseRealBin`: true only for exactly `Some("1")` (ordinal; no trim, no `true`/`yes`).
pub fn is_real_bin_opt_in(value: Option<&str>) -> bool {
    value == Some("1")
}

/// C# `UseRealBin`: reads [`REAL_BIN_ENVIRONMENT_VARIABLE`] (`std::env::var_os`; a non-Unicode value counts as unset).
pub fn use_real_bin() -> bool {
    let value = std::env::var_os(REAL_BIN_ENVIRONMENT_VARIABLE);
    is_real_bin_opt_in(value.as_deref().and_then(|v| v.to_str()))
}

/// C# `Create(realBin)`: the factory runs only when the opt-in is set.
pub fn create(real_bin: impl FnOnce() -> Arc<dyn RecycleBin>) -> Arc<dyn RecycleBin> {
    create_with(use_real_bin(), real_bin)
}

/// `create` with the switch given explicitly (testable without touching the process environment).
/// `use_real == true` returns exactly what `real_bin()` returned (same `Arc`); otherwise the temp-copy deleter and
/// `real_bin` is never called.
pub fn create_with(use_real: bool, real_bin: impl FnOnce() -> Arc<dyn RecycleBin>) -> Arc<dyn RecycleBin> {
    if use_real {
        real_bin()
    } else {
        Arc::new(TempCopyDeleter)
    }
}

/// C# private `TempCopyDeleter`: "recycle" = `File.Delete` of the benchmark's own temp copy.
#[derive(Debug)]
struct TempCopyDeleter;

impl RecycleBin for TempCopyDeleter {
    /// `File.Delete(path)` semantics, NOT `std::fs::remove_file`: see contract trap list (missing file in an existing
    /// folder = `Ok`, missing folder = `NotFound`, read-only file = `PermissionDenied`, directory path =
    /// `PermissionDenied`, empty path = `InvalidInput`).
    fn send_to_recycle_bin(&self, path: &Path) -> io::Result<()> {
        if path.as_os_str().is_empty() {
            return Err(io::Error::from(io::ErrorKind::InvalidInput));
        }
        match fs::symlink_metadata(path) {
            Ok(metadata) => {
                if metadata.is_dir() || metadata.permissions().readonly() {
                    return Err(io::Error::from(io::ErrorKind::PermissionDenied));
                }
                fs::remove_file(path)
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                // .NET: a missing file inside an existing folder is a no-op; a missing folder is an error.
                let parent_exists = match path.parent() {
                    Some(parent) if !parent.as_os_str().is_empty() => parent.is_dir(),
                    _ => true,
                };
                if parent_exists { Ok(()) } else { Err(err) }
            }
            Err(err) => Err(err),
        }
    }

    fn can_recycle(&self, _path: &Path) -> bool {
        true
    }

    fn fits_in_recycle_bin(&self, _path: &Path, _file_size: u64) -> bool {
        true
    }

    /// C# `NotSupportedException` => `ErrorKind::Unsupported`.
    fn delete_permanently(&self, _path: &Path) -> io::Result<()> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }

    fn try_restore(
        &self,
        _original_path: &Path,
        _expected_size: u64,
        _expected_last_write_utc: DateTimeUtc,
    ) -> io::Result<bool> {
        Ok(false)
    }
}

//! Port of `PhotoReview.Platform.Windows/RecycleBinCapacity.cs` capacity decision logic.

use std::io;
use std::path::Path;

use pr_core::abstractions::{Log, NullLog};

use crate::recycle_bin_settings::{DriveType, RecycleBinSettingsSource};

/// What the capacity check concluded about recycling one file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecycleCapacityVerdict {
    /// The bin is enabled for the volume and has enough capacity.
    Fits,
    /// Recycle Bin is disabled by policy or a `NukeOnDelete` setting.
    BinDisabled,
    /// The file exceeds the maximum usable capacity after applying the safety margin.
    TooLarge,
    /// Required settings or volume metadata were unavailable or malformed.
    Unknown,
}

/// Evaluates whether a Windows shell recycle operation will place the file in the Recycle Bin.
pub struct RecycleBinCapacityGuard;

impl RecycleBinCapacityGuard {
    /// Bytes in one mebibyte, matching Windows' `MaxCapacity` unit.
    pub const MEBIBYTE: i64 = 1024 * 1024;
    /// Space reserved for the Recycle Bin's `$I` record and cluster rounding.
    pub const SAFETY_MARGIN_BYTES: i64 = Self::MEBIBYTE;
    /// Default fallback divisor: 5% of the user's quota.
    pub const DEFAULT_CAPACITY_DIVISOR: i64 = 20;

    /// Returns an input error for blank paths, otherwise returns a fail-closed capacity verdict.
    pub fn evaluate(
        path: &str,
        file_size: Option<i64>,
        source: &dyn RecycleBinSettingsSource,
        log: Option<&dyn Log>,
    ) -> io::Result<RecycleCapacityVerdict> {
        if path.trim().is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "path cannot be empty or whitespace",
            ));
        }
        let logger = log.unwrap_or(&NullLog);
        match evaluate_core(path, file_size, source) {
            Ok(verdict) => Ok(verdict),
            Err(error) => {
                let name = Path::new(path).file_name().unwrap_or_default().to_string_lossy();
                logger.error(&format!("Recycle Bin settings could not be read: {name}"), Some(&error));
                Ok(RecycleCapacityVerdict::Unknown)
            }
        }
    }
}

fn evaluate_core(
    path: &str,
    file_size: Option<i64>,
    source: &dyn RecycleBinSettingsSource,
) -> io::Result<RecycleCapacityVerdict> {
    let policy = source.read_policy()?;
    if policy.no_recycle_files || policy.legacy_global_nuke_on_delete {
        return Ok(RecycleCapacityVerdict::BinDisabled);
    }
    if source.get_volume_drive_type(path) != Some(DriveType::Fixed) {
        return Ok(RecycleCapacityVerdict::Unknown);
    }
    let Some(volume_guid) = source.get_volume_guid(path).filter(|guid| !guid.is_empty()) else {
        return Ok(RecycleCapacityVerdict::Unknown);
    };
    let volume = source.read_volume(&volume_guid)?;
    if volume.nuke_on_delete.is_some_and(|value| value != 0) {
        return Ok(RecycleCapacityVerdict::BinDisabled);
    }
    let Some(size) = file_size else {
        return Ok(RecycleCapacityVerdict::Fits);
    };
    if size < 0 {
        return Ok(RecycleCapacityVerdict::Unknown);
    }

    let capacity = if let Some(percent) = policy.max_size_percent {
        let Some(quota) = source.get_user_quota_bytes(path).filter(|quota| *quota > 0) else {
            return Ok(RecycleCapacityVerdict::Unknown);
        };
        let mut capacity = (quota / 100) * percent.clamp(0, 100);
        if let Some(volume_capacity_mb) = volume.max_capacity_mb {
            capacity = capacity.min(volume_capacity_mb * RecycleBinCapacityGuard::MEBIBYTE);
        }
        capacity
    } else if let Some(volume_capacity_mb) = volume.max_capacity_mb {
        if volume_capacity_mb <= 0 {
            return Ok(RecycleCapacityVerdict::Unknown);
        }
        volume_capacity_mb * RecycleBinCapacityGuard::MEBIBYTE
    } else {
        let Some(quota) = source.get_user_quota_bytes(path).filter(|quota| *quota > 0) else {
            return Ok(RecycleCapacityVerdict::Unknown);
        };
        quota / RecycleBinCapacityGuard::DEFAULT_CAPACITY_DIVISOR
    };

    if size > capacity - RecycleBinCapacityGuard::SAFETY_MARGIN_BYTES {
        Ok(RecycleCapacityVerdict::TooLarge)
    } else {
        Ok(RecycleCapacityVerdict::Fits)
    }
}

//! Port of `IRecycleBin.cs`.
//!
//! AGENTS.md: never run code that deletes from, sweeps or empties the user's real Recycle Bin, tests included. Tests
//! use `pr_test_support::FakeRecycleBin`.

use std::io;
use std::path::Path;

use crate::time::DateTimeUtc;

pub trait RecycleBin: Send + Sync {
    /// Moves `path` to the Recycle Bin (restorable).
    fn send_to_recycle_bin(&self, path: &Path) -> io::Result<()>;

    /// True when `path` is on a volume that really has a Recycle Bin (local fixed drive). Removable, network, UNC
    /// and unknown volumes return false and are refused by `send_to_recycle_bin` (R2-F-05). Default true (fakes).
    fn can_recycle(&self, path: &Path) -> bool {
        let _ = path;
        true
    }

    /// Permanent delete (not restorable). Only after the user enabled "allow permanent delete" AND confirmed (Q-R8);
    /// never for a path where [`can_recycle`](Self::can_recycle) is true. Default: unsupported.
    fn delete_permanently(&self, path: &Path) -> io::Result<()> {
        let _ = path;
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }

    /// Tries to restore `original_path` from the bin, matching the recorded size and last write time.
    fn try_restore(&self, original_path: &Path, expected_size: u64, expected_last_write_utc: DateTimeUtc) -> bool;

    /// F-WIN-2: true only when the bin of `path`'s volume is known to accept `file_size` bytes. False when that bin
    /// is off, too small, or unreadable: the shell would then delete PERMANENTLY without asking, so the caller must
    /// refuse. Only asked when [`can_recycle`](Self::can_recycle) is true. Default true (fakes).
    fn fits_in_recycle_bin(&self, path: &Path, file_size: u64) -> bool {
        let _ = (path, file_size);
        true
    }
}

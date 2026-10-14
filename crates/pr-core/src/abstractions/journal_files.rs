//! Port of `IJournalCompactionFiles.cs`: the two primitives journal compaction needs beyond [`FileSystem`]. Only the
//! real Windows file system has them; a journal built without them never compacts.
//!
//! [`FileSystem`]: super::FileSystem

use std::io;
use std::path::Path;

use super::ReadSeek;

pub trait JournalCompactionFiles: Send + Sync {
    /// Opens `path` for reading while DENYING writers (share Read | Delete): every journal append gets a sharing
    /// violation while the stream is open and retries. Fails with a sharing violation when a writer holds the file.
    fn open_read_deny_writers(&self, path: &Path) -> io::Result<Box<dyn ReadSeek>>;
    /// Creates `temp_path` (must not exist) as the replacement to write and then rename over the target.
    fn create_staged_replacement(&self, temp_path: &Path) -> io::Result<Box<dyn StagedReplacement>>;
}

/// A new file being written as the replacement of another. Dropping it without a successful
/// [`replace_atomically`](Self::replace_atomically) deletes it; the target is untouched until that call succeeds.
pub trait StagedReplacement: Send {
    fn write(&mut self, bytes: &[u8]) -> io::Result<()>;
    /// Flushes everything written so far to disk (fsync), whatever the journal durability mode.
    fn flush_to_disk(&mut self) -> io::Result<()>;
    /// Renames this file over `destination` in one atomic operation (POSIX-semantics rename): open handles to the old
    /// file keep reading the old content. On failure the destination is unchanged. A handle denying delete sharing
    /// (an append in progress) makes it fail.
    fn replace_atomically(&mut self, destination: &Path) -> io::Result<()>;
}

//! Port of `ISourceReader.cs` + `PhysicalSourceReader.cs` (Q-R29 C-2): the single choke point every source-image
//! byte read goes through, so a perf-harness decorator can pace foreground and preload reads separately.

use std::fs::{File, OpenOptions};
use std::io::{self, BufReader};
use std::path::Path;

use super::ReadSeek;

/// Which lane a read belongs to. Metadata only: it never changes the bytes returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceReadPriority {
    /// The photo on screen (or about to be): the user is waiting.
    Viewer,
    /// Whole-folder preload read-ahead.
    Preload,
}

pub trait SourceReader: Send + Sync {
    /// Opens `path` for a sequential shared read of source bytes. C# default buffer: 1 MiB.
    fn open_source(
        &self,
        path: &Path,
        priority: SourceReadPriority,
        buffer_size: usize,
    ) -> io::Result<Box<dyn ReadSeek>>;
}

/// Production reader: the real file with share ReadWrite | Delete and SequentialScan (INV-8), ignoring priority.
#[derive(Debug, Default, Clone, Copy)]
pub struct PhysicalSourceReader;

impl SourceReader for PhysicalSourceReader {
    fn open_source(
        &self,
        path: &Path,
        _priority: SourceReadPriority,
        buffer_size: usize,
    ) -> io::Result<Box<dyn ReadSeek>> {
        Ok(Box::new(BufReader::with_capacity(
            buffer_size.max(1),
            open_shared_sequential(path)?,
        )))
    }
}

/// `FileMode.Open, FileAccess.Read, FileShare.ReadWrite | FileShare.Delete, FileOptions.SequentialScan`.
pub(crate) fn open_shared_sequential(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_SHARE_READ: u32 = 0x1;
        const FILE_SHARE_WRITE: u32 = 0x2;
        const FILE_SHARE_DELETE: u32 = 0x4;
        const FILE_FLAG_SEQUENTIAL_SCAN: u32 = 0x0800_0000;
        options.share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE);
        options.custom_flags(FILE_FLAG_SEQUENTIAL_SCAN);
    }
    options.open(path)
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use super::*;

    #[test]
    fn open_source_lets_others_rename_and_delete_while_open() {
        let dir = std::env::temp_dir().join(format!("pr-core-source-reader-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.bin");
        std::fs::write(&path, b"bytes").unwrap();

        let mut reader = PhysicalSourceReader
            .open_source(&path, SourceReadPriority::Viewer, 4096)
            .unwrap();
        let moved = dir.join("b.bin");
        std::fs::rename(&path, &moved).expect("INV-8: an open source must not block a move");
        std::fs::remove_file(&moved).expect("INV-8: an open source must not block a delete");
        let mut text = String::new();
        reader.read_to_string(&mut text).unwrap();
        assert_eq!(text, "bytes");

        drop(reader);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// INV-8 (cl-fsflush): the handle carries the SequentialScan hint, read back as `FILE_SEQUENTIAL_ONLY` (0x4) of
    /// `NtQueryInformationFile(FileModeInformation)`; a plain std open does not (so the query tells them apart).
    #[cfg(windows)]
    #[test]
    fn open_shared_sequential_sets_the_sequential_scan_mode() {
        use std::os::windows::io::AsRawHandle;

        #[repr(C)]
        struct IoStatusBlock {
            status: usize,
            information: usize,
        }
        #[link(name = "ntdll")]
        unsafe extern "system" {
            fn NtQueryInformationFile(
                handle: *mut std::ffi::c_void,
                io_status: *mut IoStatusBlock,
                info: *mut std::ffi::c_void,
                length: u32,
                class: u32,
            ) -> i32;
        }
        fn mode_of(file: &File) -> u32 {
            let mut io = IoStatusBlock {
                status: 0,
                information: 0,
            };
            let mut mode = 0u32;
            // SAFETY: the handle outlives the call (borrowed from `file`); `io` and `mode` are live locals and the
            // class FileModeInformation (16) writes a single ULONG.
            let status =
                unsafe { NtQueryInformationFile(file.as_raw_handle(), &mut io, (&mut mode as *mut u32).cast(), 4, 16) };
            assert_eq!(status, 0);
            mode
        }

        let dir = std::env::temp_dir().join(format!("pr-core-source-reader-seq-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.bin");
        std::fs::write(&path, b"bytes").unwrap();
        assert_eq!(mode_of(&open_shared_sequential(&path).unwrap()) & 0x4, 0x4);
        assert_eq!(mode_of(&File::open(&path).unwrap()) & 0x4, 0);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

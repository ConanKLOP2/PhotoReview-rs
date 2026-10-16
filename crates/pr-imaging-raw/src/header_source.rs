//! Port of `PhotoReview.Imaging.Raw/SourceRawHeaderSource.cs` (C# baseline d92d6aae, 159 lines): the production
//! [`RawHeaderSource`] over a seekable source stream. Reads 64 KiB-aligned blocks on demand, caches every block read for the
//! lifetime of the source and charges them to the header budget (`RawContainerLimits::MAX_HEADER_BYTES`, W5-01a) so a
//! hostile or circular container fails closed.
//!
//! The in-memory twin (`InMemoryRawHeaderSource`, W5-01b) copies these budget rules; the shared golden `raw-header-budget`
//! (W5-01b) runs against both.

use std::collections::HashMap;
use std::fmt;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

use pr_core::abstractions::{ReadSeek, SourceReadPriority, SourceReader};

// Defined by W5-01b (`crate::contracts`, re-exported at the crate root by the lead).
use crate::formats::limits::MAX_HEADER_BYTES;
use crate::{RAW_HEADER_BLOCK_SIZE, RawHeaderError, RawHeaderSource};

/// `SourceRawHeaderSource`: random-access window reads over a seekable stream, block cached and budget charged.
///
/// Invariants (C# fields): `block_cache` maps a block index to the bytes read for it (every block but the last holds exactly
/// [`RAW_HEADER_BLOCK_SIZE`] bytes; the last holds `length - index * BLOCK_SIZE`); `total_bytes_read` is the sum of the
/// lengths of the cached blocks and never exceeds `MAX_HEADER_BYTES`; `multi_block_buffer` is the buffer a read that spans
/// several blocks is assembled in (grown, never shrunk, reused by the next multi-block read).
pub struct SourceRawHeaderSource {
    /// `None` after [`SourceRawHeaderSource::dispose`] (C# `_stream.Dispose()`); a read then fails like a read of a closed stream.
    stream: Option<Box<dyn ReadSeek>>,
    length: i64,
    block_cache: HashMap<i64, Vec<u8>>,
    multi_block_buffer: Vec<u8>,
    total_bytes_read: i64,
}

impl fmt::Debug for SourceRawHeaderSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SourceRawHeaderSource")
            .field("length", &self.length)
            .field("open", &self.stream.is_some())
            .field("cached_blocks", &self.block_cache.len())
            .field("total_bytes_read", &self.total_bytes_read)
            .finish_non_exhaustive()
    }
}

impl SourceRawHeaderSource {
    /// C# `public const int BlockSize = 64 * 1024`.
    pub const BLOCK_SIZE: usize = RAW_HEADER_BLOCK_SIZE;

    /// C# `SourceRawHeaderSource(string path, ISourceReader sourceReader, SourceReadPriority priority = Viewer)`: opens
    /// `path` through `source_reader.open_source(path, priority, BLOCK_SIZE)` (buffer size 65536, pinned by a test) and reads the
    /// length. Callers that want the C# default pass `SourceReadPriority::Viewer`.
    ///
    /// Errors are plain `io::Error`s, NOT [`RawHeaderError`]: the C# constructor lets the `IOException`s of `OpenSource` and
    /// `Length` escape unwrapped and `RawDecoder` reports them as real I/O failures. A path that is empty or only whitespace
    /// (`ArgumentException.ThrowIfNullOrWhiteSpace`) is `io::ErrorKind::InvalidInput`. The opened stream is dropped (closed)
    /// when the length cannot be read, so a failed constructor leaks no handle.
    pub fn open(path: &Path, source_reader: &dyn SourceReader, priority: SourceReadPriority) -> io::Result<Self> {
        if path.to_string_lossy().trim().is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "The value cannot be an empty string or composed entirely of whitespace. (Parameter 'path')",
            ));
        }
        let stream = source_reader.open_source(path, priority, Self::BLOCK_SIZE)?;
        Self::from_stream(stream)
    }

    /// C# `SourceRawHeaderSource(Stream stream)`: takes ownership of `stream` and reads its length (`seek(End(0))`; a
    /// length that does not fit `i64` is `InvalidData`). On error the stream is dropped, as in C# (`LengthOrDispose`).
    pub fn from_stream(mut stream: Box<dyn ReadSeek>) -> io::Result<Self> {
        // On error `stream` is dropped here, which closes it (C# `LengthOrDispose`).
        let end = stream.seek(SeekFrom::End(0))?;
        let length = i64::try_from(end).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        Ok(Self {
            stream: Some(stream),
            length,
            block_cache: HashMap::new(),
            multi_block_buffer: Vec::new(),
            total_bytes_read: 0,
        })
    }

    /// C# `TotalBytesRead`: bytes read from the underlying stream so far (whole blocks; the last block counts only its bytes).
    /// Short or failed block reads charge nothing.
    pub fn total_bytes_read(&self) -> i64 {
        self.total_bytes_read
    }

    /// C# `Dispose()`: closes the stream and clears the block cache. Idempotent. A later `read` is a fresh stream read and
    /// fails as `InvalidData` with an I/O inner error (C# `ObjectDisposedException` wrapped), never serving cached blocks;
    /// `length()` and `total_bytes_read()` keep their values. Rust also closes on `Drop`; this method exists so tests can
    /// observe the post-dispose behaviour.
    pub fn dispose(&mut self) {
        self.stream = None;
        self.block_cache.clear();
    }

    /// C# `GetOrReadBlock` made fallible-in-place: makes sure `block_index` is in `block_cache`. Order of checks and messages
    /// (kept 1:1, the golden compares `Display` text):
    /// 1. cached => nothing to do; `bytes_to_read = min(BLOCK_SIZE, length - block_index * BLOCK_SIZE)`.
    /// 2. `bytes_to_read > MAX_HEADER_BYTES - total_bytes_read` => `InvalidData` "RAW header read exceeded hard limit of {MAX_HEADER_BYTES} bytes."
    /// 3. seek to the block start, then read until `bytes_to_read` bytes or a 0-byte read; `io::ErrorKind::Interrupted` is
    ///    retried (std `read_exact` behaviour); any other `io::Error` (from seek or read, or a closed stream) =>
    ///    `invalid_data_io("Unable to read RAW header block at {block_start}: {io error Display}", err)`. (Deviation: C# wraps only
    ///    `IOException`/`ObjectDisposedException`; every Rust `io::Error` is wrapped, see contract Semantics 5b.)
    /// 4. fewer bytes than `bytes_to_read` => `InvalidData` (no inner) "Unexpected EOF while reading block at {block_start}."
    /// 5. only now `total_bytes_read += bytes_to_read` and the block is inserted (a failed read charges and caches nothing).
    fn ensure_block(&mut self, block_index: i64) -> Result<(), RawHeaderError> {
        if self.block_cache.contains_key(&block_index) {
            return Ok(());
        }
        // block_index <= offset / BLOCK_SIZE, so the product cannot overflow.
        let block_start = block_index * Self::BLOCK_SIZE as i64;
        let remaining_in_source = self.length - block_start;
        let bytes_to_read = remaining_in_source.min(Self::BLOCK_SIZE as i64);
        if bytes_to_read > i64::from(MAX_HEADER_BYTES) - self.total_bytes_read {
            return Err(RawHeaderError::invalid_data(format!(
                "RAW header read exceeded hard limit of {MAX_HEADER_BYTES} bytes."
            )));
        }
        // 0 <= bytes_to_read <= 65 536 (the caller only asks for blocks inside the source).
        let wanted = usize::try_from(bytes_to_read).unwrap_or(0);
        let mut block = vec![0u8; wanted];
        let wrap = |err: io::Error| {
            RawHeaderError::invalid_data_io(format!("Unable to read RAW header block at {block_start}: {err}"), err)
        };
        let Some(stream) = self.stream.as_mut() else {
            return Err(wrap(io::Error::new(io::ErrorKind::BrokenPipe, "stream is closed")));
        };
        stream.seek(SeekFrom::Start(block_start as u64)).map_err(wrap)?;
        let mut total = 0usize;
        while total < wanted {
            match stream.read(&mut block[total..]) {
                Ok(0) => break,
                Ok(n) => total += n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(wrap(e)),
            }
        }
        if total != wanted {
            return Err(RawHeaderError::invalid_data(format!(
                "Unexpected EOF while reading block at {block_start}."
            )));
        }
        self.total_bytes_read += bytes_to_read;
        self.block_cache.insert(block_index, block);
        Ok(())
    }
}

impl RawHeaderSource for SourceRawHeaderSource {
    fn length(&self) -> i64 {
        self.length
    }

    /// C# `Read(long offset, int count)`. Checks in this order (messages are the C# ones, formatted with `{}`):
    /// 1. `offset < 0 || count < 0` => `ArgumentOutOfRange { param: "offset", message: "Offset and count must be non-negative." }`.
    /// 2. `offset > length - count` => `InvalidData` "Attempted to read past end of stream (offset: {offset}, count: {count}, length: {length})."
    /// 3. `count > MAX_HEADER_BYTES` => `InvalidData` "RAW header read of {count} bytes exceeds hard limit of {MAX_HEADER_BYTES} bytes."
    ///    (before anything is allocated)
    /// 4. `count == 0` => empty slice (no stream access, even after dispose).
    /// 5. one block => the slice of the cached block; several blocks => each block is `ensure_block`ed in order and copied into
    ///    `multi_block_buffer` (a block that fails midway leaves the earlier blocks charged and cached), the slice is
    ///    `multi_block_buffer[..count]`.
    fn read(&mut self, offset: i64, count: i32) -> Result<&[u8], RawHeaderError> {
        if offset < 0 || count < 0 {
            return Err(RawHeaderError::ArgumentOutOfRange {
                param: "offset",
                message: "Offset and count must be non-negative.".to_string(),
            });
        }
        let count_i64 = i64::from(count);
        if offset > self.length - count_i64 {
            return Err(RawHeaderError::invalid_data(format!(
                "Attempted to read past end of stream (offset: {offset}, count: {count}, length: {}).",
                self.length
            )));
        }
        if count > MAX_HEADER_BYTES {
            return Err(RawHeaderError::invalid_data(format!(
                "RAW header read of {count} bytes exceeds hard limit of {MAX_HEADER_BYTES} bytes."
            )));
        }
        if count == 0 {
            return Ok(&[]);
        }
        let block_size = Self::BLOCK_SIZE as i64;
        let start_block = offset / block_size;
        let end_block = (offset + count_i64 - 1) / block_size;
        let count_usize = count as usize;

        if start_block == end_block {
            self.ensure_block(start_block)?;
            let in_block = (offset % block_size) as usize;
            return match self.block_cache.get(&start_block) {
                Some(block) => Ok(&block[in_block..in_block + count_usize]),
                None => Err(RawHeaderError::invalid_data("RAW header block missing from cache.")),
            };
        }

        if self.multi_block_buffer.len() < count_usize {
            self.multi_block_buffer.resize(count_usize, 0);
        }
        let mut dest = 0usize;
        let mut remaining = count_usize;
        let mut current = offset;
        for b in start_block..=end_block {
            self.ensure_block(b)?;
            let Some(block) = self.block_cache.get(&b) else {
                return Err(RawHeaderError::invalid_data("RAW header block missing from cache."));
            };
            let in_block = (current % block_size) as usize;
            let n = remaining.min(block.len() - in_block);
            self.multi_block_buffer[dest..dest + n].copy_from_slice(&block[in_block..in_block + n]);
            dest += n;
            current += n as i64;
            remaining -= n;
        }
        Ok(&self.multi_block_buffer[..count_usize])
    }
}

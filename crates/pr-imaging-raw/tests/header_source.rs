//! Red tests for W5-12a (`header_source.rs`): `SourceRawHeaderSource`, the file-backed 64 KiB block header source with a hard
//! read budget (C# baseline d92d6aae, `src/PhotoReview.Imaging.Raw/SourceRawHeaderSource.cs`). Ports `RawContractsAndHeaderSourceTests`,
//! `SourceRawHeaderSourceMutationGapTests`, `RawHeaderSourceIoTests`, the `SourceRawHeaderSource` section of
//! `RawJpegFrameAndHeaderSourceGapTests`, the file-backed tests of `RawHeaderHostileInputTests` and the production half of
//! `RawReadInfoAndHeaderSourceParityTests`. Every ported test names its C# test in a `// C#:` comment.
//!
//! Goldens: `raw-header-budget` (12 cases: the W5-01b runner replayed over `SourceRawHeaderSource`) and `raw-header-source-io`
//! (27 cases; the stream shape and fault plan of each come from `tests/golden-inputs/raw-header-source-io.json`).
//!
//! Not ported here (contract W5-12a): `Read_Cancellation_IsNotWrapped` (no Rust twin: `ReadSeek` can only fail with `io::Error`,
//! and `RawHeaderSource::read` takes no `CancelToken`); `TryReadExif_FailingSource_ReturnsNullInsteadOfThrowing` (W5-11 `RawExif`);
//! `Decode_HeaderIoFailure_SurfacesTheRealIoErrorNotACorruptRawError` (W5-12b `RawDecoder`). The `ObjectDisposedException` row of
//! `Read_StreamIoFailure_...` has no twin of its own: it runs a second time with a closed-stream `io::ErrorKind`.
//! `GC.GetAllocatedBytesForCurrentThread` has no twin: `ZeroStream` counts the calls it receives instead.
//!
//! Stub bodies are `todo!()`, so every test that reaches the source fails until the implementation lands.

use std::error::Error as _;
use std::io::{self, Cursor, ErrorKind, Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use pr_core::abstractions::{PhysicalSourceReader, ReadSeek, SourceReadPriority, SourceReader};
use pr_imaging_raw::formats::limits;
use pr_imaging_raw::header_source::SourceRawHeaderSource;
use pr_imaging_raw::{InMemoryRawHeaderSource, RAW_HEADER_BLOCK_SIZE, RawHeaderError, RawHeaderSource};
use pr_test_support::TempRoot;
use pr_test_support::golden::{self, bool_of, i64_of, str_of};
use serde_json::Value;

/// C# `SourceRawHeaderSource.BlockSize` (64 KiB).
const BLOCK_USIZE: usize = SourceRawHeaderSource::BLOCK_SIZE;
const BLOCK: i64 = 65_536;
/// Bytes of the short last block of a 200 000-byte source (`200 000 - 3 * 65 536`).
const LAST_BLOCK_BYTES_200K: i64 = 3_392;

fn max_header() -> i64 {
    i64::from(limits::MAX_HEADER_BYTES)
}

fn count32(value: i64) -> i32 {
    i32::try_from(value).expect("test count fits i32")
}

/// `i % 251`: the pattern of `RawContractsAndHeaderSourceTests` and of the golden files.
fn mod251_bytes(length: usize) -> Vec<u8> {
    (0..length).map(|i| u8::try_from(i % 251).expect("below 251")).collect()
}

/// `(byte)(i * 7 + (i >> 16))`: the pattern of `RawJpegFrameAndHeaderSourceGapTests`.
fn pattern7(index: i64) -> u8 {
    u8::try_from((index.wrapping_mul(7).wrapping_add(index >> 16)) & 0xFF).expect("masked to a byte")
}

fn pattern7_bytes(length: usize) -> Vec<u8> {
    (0..length)
        .map(|i| pattern7(i64::try_from(i).expect("index fits i64")))
        .collect()
}

fn expected7(offset: i64, count: usize) -> Vec<u8> {
    (0..count)
        .map(|i| pattern7(offset + i64::try_from(i).expect("index fits i64")))
        .collect()
}

/// Mirrors C# `Seek`: `Begin`, `Current` and `End` relative to `end`; a target before the start is an error.
fn seek_target(pos: SeekFrom, current: u64, end: u64) -> io::Result<u64> {
    let target = match pos {
        SeekFrom::Start(n) => i128::from(n),
        SeekFrom::Current(d) => i128::from(current) + i128::from(d),
        SeekFrom::End(d) => i128::from(end) + i128::from(d),
    };
    u64::try_from(target).map_err(|_| io::Error::new(ErrorKind::InvalidInput, "seek before the start"))
}

/// Read counters and drop flag of a stream the source owns; the test keeps a clone of it.
#[derive(Debug)]
struct StreamProbe {
    reads: Arc<AtomicUsize>,
    dropped: Arc<AtomicBool>,
}

impl StreamProbe {
    fn reads(&self) -> usize {
        self.reads.load(Ordering::SeqCst)
    }

    fn dropped(&self) -> bool {
        self.dropped.load(Ordering::SeqCst)
    }
}

/// A read fault: every `read` call numbered `from_call` (0-based, counting from the stream's first read) or later fails.
#[derive(Debug, Clone, Copy)]
struct ReadFault {
    kind: ErrorKind,
    message: &'static str,
    from_call: usize,
}

/// Scripted `Read + Seek` double (`RawHeaderSourceIoTests.FaultyStream`, `RawJpegFrameAndHeaderSourceGapTests.ShortStream` and
/// the scripted stream of `raw-header-source-io`). `bytes` is what the stream holds; `declared_length` is what
/// `seek(End(0))` reports, so a stream may end before its declared length. `chunk` caps the bytes per `read` call.
#[derive(Debug)]
struct ScriptedStream {
    bytes: Vec<u8>,
    declared_length: u64,
    position: u64,
    chunk: usize,
    read_fault: Option<ReadFault>,
    length_fault: Option<&'static str>,
    interrupt_first_read: bool,
    probe: StreamProbe,
}

impl ScriptedStream {
    fn new(bytes: Vec<u8>, declared_length: u64) -> Self {
        Self {
            bytes,
            declared_length,
            position: 0,
            chunk: usize::MAX,
            read_fault: None,
            length_fault: None,
            interrupt_first_read: false,
            probe: StreamProbe {
                reads: Arc::new(AtomicUsize::new(0)),
                dropped: Arc::new(AtomicBool::new(false)),
            },
        }
    }

    /// A stream whose declared length is its byte count.
    fn exact(bytes: Vec<u8>) -> Self {
        let length = u64::try_from(bytes.len()).expect("length fits u64");
        Self::new(bytes, length)
    }

    fn chunked(mut self, chunk: usize) -> Self {
        self.chunk = chunk;
        self
    }

    fn failing_reads(mut self, fault: ReadFault) -> Self {
        self.read_fault = Some(fault);
        self
    }

    fn failing_length(mut self, message: &'static str) -> Self {
        self.length_fault = Some(message);
        self
    }

    fn interrupted_first_read(mut self) -> Self {
        self.interrupt_first_read = true;
        self
    }

    fn probe(&self) -> StreamProbe {
        StreamProbe {
            reads: Arc::clone(&self.probe.reads),
            dropped: Arc::clone(&self.probe.dropped),
        }
    }
}

impl Read for ScriptedStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let call = self.probe.reads.fetch_add(1, Ordering::SeqCst);
        if self.interrupt_first_read && call == 0 {
            return Err(io::Error::from(ErrorKind::Interrupted));
        }
        if let Some(fault) = self.read_fault
            && call >= fault.from_call
        {
            return Err(io::Error::new(fault.kind, fault.message));
        }
        let start = usize::try_from(self.position).unwrap_or(usize::MAX);
        if start >= self.bytes.len() {
            return Ok(0);
        }
        let n = buf.len().min(self.chunk).min(self.bytes.len() - start);
        buf[..n].copy_from_slice(&self.bytes[start..start + n]);
        self.position += u64::try_from(n).expect("fits u64");
        Ok(n)
    }
}

impl Seek for ScriptedStream {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        if let (SeekFrom::End(_), Some(message)) = (pos, self.length_fault) {
            return Err(io::Error::other(message));
        }
        self.position = seek_target(pos, self.position, self.declared_length)?;
        Ok(self.position)
    }
}

impl Drop for ScriptedStream {
    fn drop(&mut self) {
        self.probe.dropped.store(true, Ordering::SeqCst);
    }
}

/// `ZeroStream` of `RawHeaderHostileInputTests`: `length` virtual zero bytes that allocate nothing. Every read and seek is
/// counted so a test can tell that the source never touched the stream.
struct ZeroStream {
    length: u64,
    position: u64,
    calls: Arc<AtomicUsize>,
}

impl Read for ZeroStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let remaining = usize::try_from(self.length.saturating_sub(self.position)).unwrap_or(usize::MAX);
        let n = buf.len().min(remaining);
        buf[..n].fill(0);
        self.position += u64::try_from(n).expect("fits u64");
        Ok(n)
    }
}

impl Seek for ZeroStream {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.position = seek_target(pos, self.position, self.length)?;
        Ok(self.position)
    }
}

/// One `open_source` call as the source made it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct OpenCall {
    path: String,
    priority: SourceReadPriority,
    buffer_size: usize,
}

/// `ISourceReader` double (`RawHeaderSourceIoTests.FixedSourceReader` plus the recording the golden needs): records every
/// `open_source` call and hands out the stream it holds.
#[derive(Default)]
struct RecordingReader {
    calls: Mutex<Vec<OpenCall>>,
    held: Mutex<Option<Box<dyn ReadSeek>>>,
}

impl RecordingReader {
    fn hold(&self, stream: Box<dyn ReadSeek>) {
        *self.held.lock().expect("held lock") = Some(stream);
    }

    fn calls(&self) -> Vec<OpenCall> {
        self.calls.lock().expect("calls lock").clone()
    }
}

impl SourceReader for RecordingReader {
    fn open_source(
        &self,
        path: &Path,
        priority: SourceReadPriority,
        buffer_size: usize,
    ) -> io::Result<Box<dyn ReadSeek>> {
        self.calls.lock().expect("calls lock").push(OpenCall {
            path: path.to_string_lossy().into_owned(),
            priority,
            buffer_size,
        });
        self.held
            .lock()
            .expect("held lock")
            .take()
            .ok_or_else(|| io::Error::other("no prepared stream"))
    }
}

fn source_over(bytes: Vec<u8>) -> SourceRawHeaderSource {
    SourceRawHeaderSource::from_stream(Box::new(Cursor::new(bytes))).expect("in-memory stream opens")
}

fn source_from(stream: ScriptedStream) -> SourceRawHeaderSource {
    SourceRawHeaderSource::from_stream(Box::new(stream)).expect("length read succeeds")
}

/// Copies the bytes of one read out of the source (the returned slice borrows the source).
fn read_vec(source: &mut SourceRawHeaderSource, offset: i64, count: i32) -> Vec<u8> {
    source.read(offset, count).expect("read succeeds").to_vec()
}

/// Owned summary of one successful read, as the golden files report it.
#[derive(Debug)]
struct ReadSummary {
    len: i64,
    first: Option<i64>,
    last: Option<i64>,
    sum: i64,
}

impl ReadSummary {
    fn of(slice: &[u8]) -> Self {
        Self {
            len: i64::try_from(slice.len()).expect("length fits i64"),
            first: slice.first().map(|b| i64::from(*b)),
            last: slice.last().map(|b| i64::from(*b)),
            sum: slice.iter().map(|b| i64::from(*b)).sum(),
        }
    }
}

/// The golden's name of the error class (`InvalidDataException` and so on).
fn error_kind_name(error: &RawHeaderError) -> &'static str {
    match error {
        RawHeaderError::InvalidData { .. } => "InvalidDataException",
        RawHeaderError::ArgumentOutOfRange { .. } => "ArgumentOutOfRangeException",
        RawHeaderError::Canceled => "OperationCanceledException",
    }
}

fn error_param(error: &RawHeaderError) -> Option<&'static str> {
    match error {
        RawHeaderError::ArgumentOutOfRange { param, .. } => Some(*param),
        _ => None,
    }
}

/// The golden's offset. The oracle printed `i64::MAX` as the double `9223372036854776000`, which does not fit `i64`.
fn offset_of(step: &Value) -> i64 {
    step["offset"].as_i64().unwrap_or(i64::MAX)
}

/// Replays one `read` step of a golden case: the result (ok/len/first/last/sum or the error class, parameter, message and
/// inner error) and `total_bytes_read` after the step.
fn check_read_step(source: &mut SourceRawHeaderSource, step: &Value, at: &str) {
    let offset = offset_of(step);
    let count = count32(i64_of(step, "count"));
    let outcome = source.read(offset, count).map(ReadSummary::of);
    match outcome {
        Ok(summary) => {
            assert!(bool_of(step, "ok"), "{at}: expected an error, got {summary:?}");
            assert_eq!(summary.len, i64_of(step, "len"), "{at}: len");
            assert_eq!(summary.first, step["first"].as_i64(), "{at}: first");
            assert_eq!(summary.last, step["last"].as_i64(), "{at}: last");
            assert_eq!(summary.sum, i64_of(step, "sum"), "{at}: sum");
        }
        Err(error) => {
            assert!(!bool_of(step, "ok"), "{at}: expected Ok, got {error}");
            assert_eq!(error_kind_name(&error), str_of(step, "error"), "{at}: error class");
            assert_eq!(error_param(&error), step["param"].as_str(), "{at}: ParamName");
            if let Some(message) = step["message"].as_str() {
                assert_eq!(error.to_string(), message, "{at}: message");
            }
            match &step["inner"] {
                Value::Null => assert!(error.source().is_none(), "{at}: unexpected inner error"),
                _ => assert!(
                    error.is_io_failure() && error.source().is_some(),
                    "{at}: an I/O inner error is expected"
                ),
            }
        }
    }
    assert_eq!(
        source.total_bytes_read(),
        i64_of(step, "total_bytes_read"),
        "{at}: total_bytes_read"
    );
}

// ---------------------------------------------------------------- RawContractsAndHeaderSourceTests

// C#: RawContractsAndHeaderSourceTests.SourceRawHeaderSource_ReadsOnlyRequestedBlocks_AndCaches
#[test]
fn source_raw_header_source_reads_only_requested_blocks_and_caches() {
    let mut source = source_over(mod251_bytes(256 * 1024));
    assert_eq!(source.length(), 256 * 1024);
    assert_eq!(source.total_bytes_read(), 0);

    // Read 10 bytes in block 0.
    assert_eq!(read_vec(&mut source, 0, 10).len(), 10);
    assert_eq!(source.total_bytes_read(), BLOCK);

    // Read 10 bytes within block 0 again: served from the cache, no new read.
    assert_eq!(read_vec(&mut source, 100, 10).len(), 10);
    assert_eq!(source.total_bytes_read(), BLOCK);

    // Read crossing the boundary from block 0 into block 1.
    assert_eq!(read_vec(&mut source, BLOCK - 5, 20).len(), 20);
    assert_eq!(source.total_bytes_read(), BLOCK * 2);
}

// C#: RawContractsAndHeaderSourceTests.SourceRawHeaderSource_ThrowsOnPastEof
#[test]
fn source_raw_header_source_throws_on_past_eof() {
    let mut source = source_over(vec![0_u8; 100]);
    let error = source
        .read(95, 10)
        .expect_err("95 + 10 is past the end of a 100-byte source");
    assert!(matches!(error, RawHeaderError::InvalidData { .. }), "got {error:?}");
}

// ---------------------------------------------------------------- SourceRawHeaderSourceMutationGapTests

fn new_source(length: usize) -> SourceRawHeaderSource {
    source_over(vec![0_u8; length])
}

// C#: SourceRawHeaderSourceMutationGapTests.Read_NegativeOffsetOrCount_IsRejectedByTheArgumentCheckItself
#[test]
fn read_negative_offset_or_count_is_rejected_by_the_argument_check_itself() {
    for (offset, count) in [(-1_i64, 1_i32), (0, -1)] {
        let mut source = new_source(1000);
        let error = source
            .read(offset, count)
            .expect_err("a negative offset or count is rejected");
        match error {
            RawHeaderError::ArgumentOutOfRange { param, .. } => {
                assert_eq!(param, "offset", "offset {offset}, count {count}: ParamName");
            }
            other => panic!("offset {offset}, count {count}: expected ArgumentOutOfRange, got {other:?}"),
        }
    }
}

// C#: SourceRawHeaderSourceMutationGapTests.Read_ZeroCountInsideTheStream_ReturnsAnEmptySpan
#[test]
fn read_zero_count_inside_the_stream_returns_an_empty_span() {
    for offset in [10_i64, 1000] {
        // 1000 is exactly the end of the stream.
        let mut source = new_source(1000);
        let span = source
            .read(offset, 0)
            .expect("a zero-count read inside the stream is Ok");
        assert!(span.is_empty(), "offset {offset}");
    }
}

// C#: SourceRawHeaderSourceMutationGapTests.Read_AfterDispose_DoesNotServeBlocksThatWereCachedBefore
#[test]
fn read_after_dispose_does_not_serve_blocks_that_were_cached_before() {
    let mut source = new_source(1000);
    assert_eq!(source.read(0, 16).expect("block 0 is read").len(), 16);

    source.dispose();

    // The cache went with the stream: a read is a fresh stream read and fails like any use after dispose.
    let error = source.read(0, 16).expect_err("a read after dispose fails");
    assert!(matches!(error, RawHeaderError::InvalidData { .. }), "got {error:?}");
    assert!(error.is_io_failure(), "the closed stream is the inner error");
}

// ---------------------------------------------------------------- RawHeaderSourceIoTests

// C#: RawHeaderSourceIoTests.Constructor_LengthThrows_DisposesTheStream_PathOverload
#[test]
fn constructor_length_throws_disposes_the_stream_path_overload() {
    let stream = ScriptedStream::exact(vec![0_u8; 10]).failing_length("no length");
    let probe = stream.probe();
    let reader = RecordingReader::default();
    reader.hold(Box::new(stream));

    let result = SourceRawHeaderSource::open(Path::new("x.dng"), &reader, SourceReadPriority::Viewer);

    assert!(result.is_err(), "a failing length fails the constructor");
    assert!(probe.dropped(), "the stream is disposed when the length cannot be read");
}

// C#: RawHeaderSourceIoTests.Constructor_LengthThrows_DisposesTheStream_StreamOverload
#[test]
fn constructor_length_throws_disposes_the_stream_stream_overload() {
    let stream = ScriptedStream::exact(vec![0_u8; 10]).failing_length("no length");
    let probe = stream.probe();

    let result = SourceRawHeaderSource::from_stream(Box::new(stream));

    assert!(result.is_err(), "a failing length fails the constructor");
    assert!(probe.dropped(), "the stream is disposed when the length cannot be read");
}

// C#: RawHeaderSourceIoTests.Read_StreamIoFailure_BecomesInvalidDataExceptionWithTheInnerException (theory)
// The IOException row is `(Other, "disk")`. The ObjectDisposedException row has no Rust twin of its own: a closed stream is an
// `io::Error` too, so the second row is a closed-stream error kind. The inner error is the same `io::Error`: compare its kind
// and text through `std::error::Error::source`.
#[test]
fn read_stream_io_failure_becomes_invalid_data_with_the_inner_error() {
    let rows = [(ErrorKind::Other, "disk"), (ErrorKind::BrokenPipe, "stream closed")];
    for (kind, message) in rows {
        let stream = ScriptedStream::exact(vec![0_u8; 1000]).failing_reads(ReadFault {
            kind,
            message,
            from_call: 0,
        });
        let mut source = source_from(stream);

        let error = source.read(0, 16).expect_err("the failing block read fails");

        assert!(
            matches!(error, RawHeaderError::InvalidData { .. }),
            "{kind:?}: got {error:?}"
        );
        assert!(error.is_io_failure(), "{kind:?}: the inner I/O error is kept");
        let inner = error
            .source()
            .and_then(|e| e.downcast_ref::<io::Error>())
            .expect("the InvalidData carries the io::Error");
        assert_eq!(inner.kind(), kind, "{kind:?}: inner kind");
        assert_eq!(inner.to_string(), message, "{kind:?}: inner text");
    }
}

// ---------------------------------------------------------------- RawJpegFrameAndHeaderSourceGapTests (SourceRawHeaderSource)

// C#: RawJpegFrameAndHeaderSourceGapTests.Read_SuccessiveMultiBlockReadsOfDifferentSizes_EachReturnTheirOwnBytes
#[test]
fn read_successive_multi_block_reads_of_different_sizes_each_return_their_own_bytes() {
    let block = BLOCK;
    let mut source = source_from(ScriptedStream::exact(pattern7_bytes(3 * BLOCK_USIZE)));

    // Large span over blocks 0-2, then smaller spans over 0-1 and 1-2 (the shared buffer is reused), then large again.
    assert_eq!(
        read_vec(&mut source, block - 100, count32(block + 200)),
        expected7(block - 100, usize::try_from(block + 200).expect("fits usize"))
    );
    assert_eq!(read_vec(&mut source, block - 10, 20), expected7(block - 10, 20));
    assert_eq!(
        read_vec(&mut source, (2 * block) - 5, 10),
        expected7((2 * block) - 5, 10)
    );
    assert_eq!(
        read_vec(&mut source, 10, count32((2 * block) + 50)),
        expected7(10, usize::try_from((2 * block) + 50).expect("fits usize"))
    );
}

// C#: RawJpegFrameAndHeaderSourceGapTests.Read_SecondMultiBlockRead_DoesNotChangeTheBytesAlreadyCopiedByTheCaller
#[test]
fn read_second_multi_block_read_does_not_change_the_bytes_already_copied_by_the_caller() {
    let block = BLOCK;
    let mut source = source_from(ScriptedStream::exact(pattern7_bytes(3 * BLOCK_USIZE)));

    // The multi-block span is a view over one shared buffer, so the caller copies the first result (`to_vec`) before the
    // second read. The borrow checker already forbids keeping the first slice across the second call.
    let first = read_vec(&mut source, block - 50, 100);
    let second = read_vec(&mut source, (2 * block) - 50, 100);

    assert_eq!(first, expected7(block - 50, 100));
    assert_eq!(second, expected7((2 * block) - 50, 100));
}

// C#: RawJpegFrameAndHeaderSourceGapTests.Read_StreamEndsBeforeItsDeclaredLength_ThrowsInvalidDataAndChargesNothingForTheShortBlock
#[test]
fn read_stream_ends_before_its_declared_length_throws_invalid_data_and_charges_nothing_for_the_short_block() {
    let block = BLOCK;
    // Holds one whole block and 4000 bytes of the next; reports two whole blocks.
    let stream = ScriptedStream::new(pattern7_bytes(BLOCK_USIZE + 4000), 2 * BLOCK_USIZE as u64);
    let mut source = source_from(stream);

    assert_eq!(read_vec(&mut source, 0, 16), expected7(0, 16)); // block 0 is whole
    let error = source
        .read(block + 10, 16)
        .expect_err("block 1 holds 4000 of 65536 bytes");
    assert!(matches!(error, RawHeaderError::InvalidData { .. }), "got {error:?}");
    assert!(!error.is_io_failure(), "a truncated file has no inner I/O error");
    assert!(error.source().is_none());
    assert_eq!(source.total_bytes_read(), block, "the short block is not charged");
}

// C#: RawJpegFrameAndHeaderSourceGapTests.Read_MultiBlockReadCrossingTheShortBlock_ThrowsInvalidData
#[test]
fn read_multi_block_read_crossing_the_short_block_throws_invalid_data() {
    let stream = ScriptedStream::new(pattern7_bytes(BLOCK_USIZE + 4000), 2 * BLOCK_USIZE as u64);
    let mut source = source_from(stream);

    let error = source.read(BLOCK - 8, 16).expect_err("the read spans the short block");
    assert!(matches!(error, RawHeaderError::InvalidData { .. }), "got {error:?}");
}

// ---------------------------------------------------------------- RawHeaderHostileInputTests (file-backed tests)

// C#: RawHeaderHostileInputTests.SourceRawHeaderSource_Read_OffsetPlusCountWrapping_ThrowsInvalidData (theory)
#[test]
fn source_raw_header_source_read_offset_plus_count_wrapping_throws_invalid_data() {
    let rows = [
        (i64::MAX, 1_i32),
        (i64::MAX - 1, 4),
        (i64::MAX - 3, i32::MAX),
        (1, i32::MAX),
    ];
    for (offset, count) in rows {
        let mut source = source_over(vec![0_u8; 64]);
        let error = source
            .read(offset, count)
            .expect_err("offset + count overflows or passes the end");
        assert!(
            matches!(error, RawHeaderError::InvalidData { .. }),
            "offset {offset}, count {count}: got {error:?}"
        );
    }
}

// C#: RawHeaderHostileInputTests.SourceRawHeaderSource_Read_CountAboveBudget_ThrowsBeforeAllocatingTheBuffer
// The C# allocation assertion (`GC.GetAllocatedBytesForCurrentThread`) has no Rust twin. The contract's replacement: after the
// constructor the stream is never read or sought, and the rejected read leaves `total_bytes_read` at 0.
#[test]
fn source_raw_header_source_read_count_above_budget_throws_before_allocating_the_buffer() {
    // 1 GiB of virtual zeros: the length check passes, so only the budget check can reject the read.
    let calls = Arc::new(AtomicUsize::new(0));
    let stream = ZeroStream {
        length: 1_u64 << 30,
        position: 0,
        calls: Arc::clone(&calls),
    };
    let mut source = SourceRawHeaderSource::from_stream(Box::new(stream)).expect("length read succeeds");
    let after_construction = calls.load(Ordering::SeqCst);
    let count = 64 * 1024 * 1024;

    let error = source
        .read(0, count)
        .expect_err("a 64 MiB read is above the 8 MiB budget");

    assert!(matches!(error, RawHeaderError::InvalidData { .. }), "got {error:?}");
    assert_eq!(
        calls.load(Ordering::SeqCst),
        after_construction,
        "the stream was read or sought for a read that the budget rejects"
    );
    assert_eq!(source.total_bytes_read(), 0);
}

// C#: RawHeaderHostileInputTests.SourceRawHeaderSource_Read_ExactlyMaxHeaderBytes_IsNotRejectedUpFront
#[test]
fn source_raw_header_source_read_exactly_max_header_bytes_is_not_rejected_up_front() {
    let stream = ZeroStream {
        length: 1_u64 << 30,
        position: 0,
        calls: Arc::new(AtomicUsize::new(0)),
    };
    let mut source = SourceRawHeaderSource::from_stream(Box::new(stream)).expect("length read succeeds");

    let span = source
        .read(0, count32(max_header()))
        .expect("exactly the budget is allowed");

    assert_eq!(i64::try_from(span.len()).expect("fits i64"), max_header());
}

// C#: RawHeaderHostileInputTests.InMemoryRawHeaderSource_TotalBytesRead_MatchesSourceRawHeaderSourceForTheSameReads
// Run here because this unit is the one that has both source types in scope (contract W5-12a).
#[test]
fn in_memory_raw_header_source_total_bytes_read_matches_source_raw_header_source_for_the_same_reads() {
    let bytes = vec![0_u8; 300_000];
    let mut memory = InMemoryRawHeaderSource::new(bytes.clone());
    let mut stream = source_over(bytes);

    for (offset, count) in [(0_i64, 8_i32), (100, 4), (70_000, 10), (65_530, 20), (299_990, 10)] {
        memory.read(offset, count).expect("in-memory read succeeds");
        stream.read(offset, count).expect("file-backed read succeeds");
    }

    assert_eq!(stream.total_bytes_read(), memory.total_bytes_read());
}

// ---------------------------------------------------------------- RawReadInfoAndHeaderSourceParityTests (production half)

// C#: RawReadInfoAndHeaderSourceParityTests.Read_MultiBlockReadThatExhaustsTheBudget_KeepsTheEarlierBlocksChargedLikeProduction
// Both halves run: the in-memory source and the production source over the same 8 MiB + 2 blocks (130 blocks; the C# comment
// says 9 MiB, it is not).
#[test]
fn read_multi_block_read_that_exhausts_the_budget_keeps_the_earlier_blocks_charged_like_production() {
    let length = usize::try_from(max_header() + 2 * BLOCK).expect("fits usize");
    let bytes = vec![0_u8; length];
    let mut memory = InMemoryRawHeaderSource::new(bytes.clone());
    let mut production = source_over(bytes);

    // Charge 126 blocks (MaxHeaderBytes / Block = 128, minus 2), then attempt a read spanning 3 untouched blocks.
    let blocks_before = (max_header() / BLOCK) - 2;
    let offset = blocks_before * BLOCK;
    memory
        .read(0, count32(offset))
        .expect("charging 126 blocks is within the budget");
    production
        .read(0, count32(offset))
        .expect("charging 126 blocks is within the budget");

    let memory_failed = memory.read(offset, count32(3 * BLOCK));
    assert!(
        matches!(memory_failed, Err(RawHeaderError::InvalidData { .. })),
        "in-memory: got {memory_failed:?}"
    );
    let production_failed = production.read(offset, count32(3 * BLOCK));
    assert!(
        matches!(production_failed, Err(RawHeaderError::InvalidData { .. })),
        "production: got {production_failed:?}"
    );
    assert_eq!(production.total_bytes_read(), memory.total_bytes_read());
    assert_eq!(
        production.total_bytes_read(),
        max_header(),
        "the two blocks charged before the failure stay charged"
    );

    // The blocks kept by the failed read are free to re-read.
    memory
        .read(offset, count32(2 * BLOCK))
        .expect("re-reading the kept blocks is free");
    production
        .read(offset, count32(2 * BLOCK))
        .expect("re-reading the kept blocks is free");
    assert_eq!(production.total_bytes_read(), memory.total_bytes_read());
}

// ---------------------------------------------------------------- New Rust tests (no C# twin)

/// Pins `BLOCK_SIZE` (C# `public const int BlockSize = 64 * 1024`) and that the alias equals `RAW_HEADER_BLOCK_SIZE`.
#[test]
fn block_size_is_64_kib_and_equals_raw_header_block_size() {
    assert_eq!(SourceRawHeaderSource::BLOCK_SIZE, 65_536);
    assert_eq!(SourceRawHeaderSource::BLOCK_SIZE, RAW_HEADER_BLOCK_SIZE);
}

/// C# `SourceRawHeaderSource(path, sourceReader, priority)` opens through `OpenSource(path, priority, BlockSize)`: the reader
/// gets the caller's priority and a 65 536-byte buffer.
#[test]
fn open_hands_the_reader_the_block_size_buffer_and_the_callers_priority() {
    let reader = RecordingReader::default();
    reader.hold(Box::new(ScriptedStream::exact(vec![0_u8; 10])));

    let source = SourceRawHeaderSource::open(Path::new("a.dng"), &reader, SourceReadPriority::Preload)
        .expect("the held stream opens");

    assert_eq!(source.length(), 10);
    assert_eq!(
        reader.calls(),
        vec![OpenCall {
            path: "a.dng".to_owned(),
            priority: SourceReadPriority::Preload,
            buffer_size: 65_536,
        }]
    );
}

/// `open` of a missing file through `PhysicalSourceReader` is the plain `io::Error` of kind `NotFound`, not a `RawHeaderError`
/// (RawDecoder shows it as an I/O failure).
#[test]
fn open_of_a_missing_file_through_physical_source_reader_is_a_plain_not_found_io_error() {
    let temp = TempRoot::new("header-source-missing");
    let path = temp.join("missing.dng");

    let error = SourceRawHeaderSource::open(&path, &PhysicalSourceReader, SourceReadPriority::Viewer)
        .expect_err("a missing file cannot be opened");

    assert_eq!(error.kind(), ErrorKind::NotFound);
}

/// `ArgumentException.ThrowIfNullOrWhiteSpace(path)` runs first: an empty or whitespace path is `InvalidInput` and the reader is
/// never called.
#[test]
fn open_of_an_empty_or_whitespace_path_is_invalid_input_and_never_calls_the_reader() {
    for path in ["", "   ", "\t \n"] {
        let reader = RecordingReader::default();

        let error = SourceRawHeaderSource::open(Path::new(path), &reader, SourceReadPriority::Viewer)
            .expect_err("a blank path is rejected");

        assert_eq!(error.kind(), ErrorKind::InvalidInput, "path {path:?}");
        assert!(
            reader.calls().is_empty(),
            "path {path:?}: the reader must not be called"
        );
    }
}

/// `open` over a real file (no `#[ignore]`: plain `std::fs`) reads the same bytes as the `Cursor` version.
#[test]
fn open_over_a_real_temp_file_reads_the_same_bytes_as_the_cursor_version() {
    let temp = TempRoot::new("header-source-real");
    let path = temp.join("sample.dng");
    let bytes = mod251_bytes(200_000);
    std::fs::write(&path, &bytes).expect("write the sample file");

    let mut file_source = SourceRawHeaderSource::open(&path, &PhysicalSourceReader, SourceReadPriority::Viewer)
        .expect("the sample file opens");
    let mut cursor_source = source_over(bytes);
    assert_eq!(file_source.length(), cursor_source.length());

    for (offset, count) in [(0_i64, 10_i32), (65_530, 20), (131_070, 4_000), (150_000, 40_000)] {
        assert_eq!(
            read_vec(&mut file_source, offset, count),
            read_vec(&mut cursor_source, offset, count),
            "offset {offset}, count {count}"
        );
    }
    assert_eq!(file_source.total_bytes_read(), cursor_source.total_bytes_read());

    // Drop the source before the temp folder (its file handle would otherwise block the removal on Windows).
    drop(file_source);
}

/// An `Interrupted` error from the stream is retried: a stream that fails once with `Interrupted` and then works still yields the
/// block. Rust-only deviation (contract Semantics 6): .NET has no such error.
#[test]
fn interrupted_stream_read_is_retried_and_still_yields_the_block() {
    let stream = ScriptedStream::exact(mod251_bytes(1000)).interrupted_first_read();
    let mut source = source_from(stream);

    assert_eq!(read_vec(&mut source, 0, 16), mod251_bytes(16));
    assert_eq!(source.total_bytes_read(), 1000, "the 1000-byte block is charged once");
}

/// Never a panic: a deterministic xorshift64 sequence of 5 000 `(offset, count)` pairs (offsets and counts in `-10..3*BLOCK`,
/// plus `i32::MIN` and `i32::MAX`) over a 200 000-byte source. Every `Ok` slice has exactly `count` bytes equal to the pattern,
/// and `total_bytes_read` stays within the budget and is block-aligned except when it includes the short last block.
#[test]
fn fuzz_offset_count_pairs_never_panic_and_return_exact_pattern_slices() {
    let length = 200_000_usize;
    let mut source = source_over(mod251_bytes(length));
    let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
    let span = u64::try_from(3 * BLOCK + 10).expect("fits u64");

    for _ in 0..5_000 {
        let draw = xorshift64(&mut state);
        let offset = i64::try_from(draw % span).expect("fits i64") - 10;
        let count = if draw >> 60 == 0 {
            if xorshift64(&mut state) % 2 == 0 {
                i32::MIN
            } else {
                i32::MAX
            }
        } else {
            i32::try_from(xorshift64(&mut state) % span).expect("fits i32") - 10
        };

        let outcome = source.read(offset, count).map(|slice| {
            let start = usize::try_from(offset).unwrap_or(0);
            let exact = slice
                .iter()
                .enumerate()
                .all(|(i, byte)| *byte == u8::try_from((start + i) % 251).expect("below 251"));
            (slice.len(), exact)
        });
        if let Ok((len, exact)) = outcome {
            assert_eq!(
                i64::try_from(len).expect("fits i64"),
                i64::from(count),
                "offset {offset}, count {count}: Ok length"
            );
            assert!(exact, "offset {offset}, count {count}: bytes are not the pattern");
        }

        let total = source.total_bytes_read();
        assert!(
            total <= max_header(),
            "offset {offset}, count {count}: total {total} above the budget"
        );
        assert!(
            total % BLOCK == 0 || total % BLOCK == LAST_BLOCK_BYTES_200K,
            "offset {offset}, count {count}: total {total} is not block-aligned"
        );
    }
}

/// Deterministic xorshift64 (no `rand` dependency; a failing pair reproduces from the seed).
fn xorshift64(state: &mut u64) -> u64 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *state = x;
    x
}

// ---------------------------------------------------------------- goldens

/// Golden `raw-header-budget.json`: each case replays its `steps` against `SourceRawHeaderSource` over bytes `i % 251`
/// (the same runner as the in-memory side, `contracts.rs`).
// C#: RawReadInfoAndHeaderSourceParityTests (budget semantics), SourceRawHeaderSourceMutationGapTests
#[test]
fn golden_raw_header_budget_cases_over_source_raw_header_source() {
    let golden_file = golden::load("raw-header-budget");
    assert_eq!(
        golden_file.cases.len(),
        12,
        "golden raw-header-budget must carry 12 cases"
    );
    for case in &golden_file.cases {
        let name = str_of(case, "name");
        let length = usize::try_from(i64_of(case, "length")).expect("length fits usize");
        let mut source = source_over(mod251_bytes(length));
        for (index, step) in case["steps"].as_array().expect("steps").iter().enumerate() {
            check_read_step(&mut source, step, &format!("{name} step {index}"));
        }
    }
}

/// Input of every `raw-header-source-io` case: how the stream or the reader was built (`tests/golden-inputs/`).
fn golden_io_inputs() -> Vec<Value> {
    let path = golden::repo_root()
        .join("tests")
        .join("golden-inputs")
        .join("raw-header-source-io.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let doc: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
    doc["cases"].as_array().expect("cases array").clone()
}

fn golden_io_input<'a>(inputs: &'a [Value], name: &str) -> &'a Value {
    inputs
        .iter()
        .find(|case| case["name"].as_str() == Some(name))
        .unwrap_or_else(|| panic!("no golden input named {name}"))
}

fn priority_named(name: &str) -> SourceReadPriority {
    match name {
        "Viewer" => SourceReadPriority::Viewer,
        "Preload" => SourceReadPriority::Preload,
        other => panic!("unknown SourceReadPriority in input: {other}"),
    }
}

/// The scripted stream a golden input describes.
fn stream_for_input(input: &Value) -> ScriptedStream {
    let length = usize::try_from(i64_of(input, "length")).expect("length fits usize");
    let declared = input["declared_length"]
        .as_u64()
        .unwrap_or(u64::try_from(length).expect("length fits u64"));
    let mut stream = ScriptedStream::new(mod251_bytes(length), declared);
    if let Some(chunk) = input["chunk"].as_u64() {
        stream = stream.chunked(usize::try_from(chunk).expect("chunk fits usize"));
    }
    if input["length_fault"].as_bool() == Some(true) {
        stream = stream.failing_length("no length");
    }
    if let Some(fault) = input["read_fault"].as_str() {
        let from_call = usize::try_from(input["fault_after"].as_u64().unwrap_or(0)).expect("fits usize");
        let (kind, message) = match fault {
            "io" => (ErrorKind::Other, "disk"),
            // C# ObjectDisposedException: a closed stream's error. Its text is not compared (golden contract).
            "disposed" => (ErrorKind::BrokenPipe, "stream disposed"),
            other => panic!("unknown read_fault in input: {other}"),
        };
        stream = stream.failing_reads(ReadFault {
            kind,
            message,
            from_call,
        });
    }
    stream
}

/// Golden `raw-header-source-io.json`: 27 cases over the constructor (stream and path overloads), chunked and faulty streams,
/// `Dispose`, argument checks and the budget. Compares, per case, the constructor outcome, the `OpenSource` arguments, each step
/// (see `check_read_step`), the stream read calls, and whether the stream was disposed.
// C#: RawHeaderSourceIoTests, RawJpegFrameAndHeaderSourceGapTests (SourceRawHeaderSource), SourceRawHeaderSourceMutationGapTests
#[test]
fn golden_raw_header_source_io_cases() {
    let golden_file = golden::load("raw-header-source-io");
    assert_eq!(
        golden_file.cases.len(),
        27,
        "golden raw-header-source-io must carry 27 cases"
    );
    let inputs = golden_io_inputs();

    for case in &golden_file.cases {
        let name = str_of(case, "name");
        let input = golden_io_input(&inputs, name);
        let stream = stream_for_input(input);
        let probe = stream.probe();

        // The path overload gets the stream through the reader. Declared before the source so the stream is still owned by
        // the reader when a constructor refuses the path before reading it.
        let reader = RecordingReader::default();
        let built = if input["ctor"].as_str() == Some("path") {
            reader.hold(Box::new(stream));
            let path = str_of(input, "path");
            let priority = priority_named(input["priority"].as_str().unwrap_or("Viewer"));
            SourceRawHeaderSource::open(Path::new(path), &reader, priority)
        } else {
            SourceRawHeaderSource::from_stream(Box::new(stream))
        };

        let mut source = match (case["ctor_error"].as_str(), built) {
            (None, Ok(source)) => Some(source),
            (Some("ArgumentException"), Err(error)) => {
                assert_eq!(error.kind(), ErrorKind::InvalidInput, "{name}: constructor error kind");
                None
            }
            (Some("IOException"), Err(error)) => {
                assert_eq!(
                    error.to_string(),
                    str_of(case, "ctor_message"),
                    "{name}: constructor message"
                );
                None
            }
            (expected, got) => panic!(
                "{name}: constructor expected {expected:?}, got {}",
                if got.is_ok() { "Ok" } else { "Err" }
            ),
        };

        match case["open_source"].as_object() {
            None => assert!(reader.calls().is_empty(), "{name}: the reader must not be called"),
            Some(expected) => {
                let calls = reader.calls();
                assert_eq!(calls.len(), 1, "{name}: one OpenSource call");
                assert_eq!(calls[0].path, expected["path"].as_str().expect("path"), "{name}: path");
                assert_eq!(
                    calls[0].priority,
                    priority_named(expected["priority"].as_str().expect("priority")),
                    "{name}: priority"
                );
                assert_eq!(
                    i64::try_from(calls[0].buffer_size).expect("fits i64"),
                    expected["buffer_size"].as_i64().expect("buffer_size"),
                    "{name}: buffer_size"
                );
            }
        }

        if let Some(source) = source.as_mut() {
            for (index, step) in case["steps"].as_array().expect("steps").iter().enumerate() {
                let at = format!("{name} step {index}");
                match step["op"].as_str().expect("op") {
                    "read" => check_read_step(source, step, &at),
                    "dispose" => {
                        source.dispose();
                        assert_eq!(
                            probe.dropped(),
                            bool_of(step, "stream_disposed"),
                            "{at}: stream_disposed"
                        );
                    }
                    other => panic!("{at}: unknown op {other}"),
                }
            }
        }

        assert_eq!(
            i64::try_from(probe.reads()).expect("fits i64"),
            i64_of(case, "stream_reads"),
            "{name}: stream read calls"
        );
        drop(source);
        assert_eq!(
            probe.dropped(),
            bool_of(case, "stream_disposed"),
            "{name}: stream_disposed"
        );
    }
}

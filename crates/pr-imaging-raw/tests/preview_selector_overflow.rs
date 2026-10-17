//! Overflow-hardening tests for W5-11b: a source declaring a length near `i64::MAX` must never panic the JPEG walk.
use pr_imaging_raw::preview_selector::{try_read_jpeg_frame, walk_jpeg_header};
use pr_imaging_raw::{RawHeaderError, RawHeaderSource};

/// Declares `i64::MAX` bytes; serves `FF D8` for every 2+ byte read and `FF` filler after.
struct HugeSource {
    buf: Vec<u8>,
}

impl RawHeaderSource for HugeSource {
    fn length(&self) -> i64 {
        i64::MAX
    }
    fn read(&mut self, offset: i64, count: i32) -> Result<&[u8], RawHeaderError> {
        if offset < 0 || count < 0 {
            return Err(RawHeaderError::ArgumentOutOfRange {
                param: "offset",
                message: "negative".into(),
            });
        }
        self.buf.clear();
        self.buf.resize(usize::try_from(count).unwrap_or(0), 0xFF);
        if count >= 2 {
            self.buf[1] = 0xD8;
        }
        Ok(&self.buf)
    }
}

#[test]
fn near_i64_max_offset_does_not_panic() {
    let mut src = HugeSource { buf: Vec::new() };
    let _ = try_read_jpeg_frame(&mut src, i64::MAX - 3, 4);
    let _ = walk_jpeg_header(&mut src, i64::MAX - 5, 6, true);
    let _ = walk_jpeg_header(&mut src, i64::MAX - 1, 1_000, false);
    let _ = walk_jpeg_header(&mut src, i64::MAX - 4, i64::MAX, true);
}

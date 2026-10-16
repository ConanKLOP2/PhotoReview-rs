//! W5-02b adversarial verification tests (kept out of the commit).
use pr_imaging_raw::jpeg_probe::{LossyFrame, try_read_lossy_frame};
use pr_imaging_raw::{InMemoryRawHeaderSource, RawHeaderError, RawHeaderSource};

struct IoSource {
    len: i64,
    reads: Vec<(i64, i32)>,
}
impl RawHeaderSource for IoSource {
    fn length(&self) -> i64 {
        self.len
    }
    fn read(&mut self, offset: i64, count: i32) -> Result<&[u8], RawHeaderError> {
        self.reads.push((offset, count));
        Err(RawHeaderError::invalid_data_io("x", std::io::Error::other("disk")))
    }
}

struct Recording {
    inner: InMemoryRawHeaderSource,
    reads: Vec<(i64, i32)>,
}
impl RawHeaderSource for Recording {
    fn length(&self) -> i64 {
        self.inner.length()
    }
    fn read(&mut self, offset: i64, count: i32) -> Result<&[u8], RawHeaderError> {
        self.reads.push((offset, count));
        self.inner.read(offset, count)
    }
}

#[test]
fn length_below_four_never_reads_even_with_valid_offset() {
    for length in [-1i64, 0, 1, 2, 3] {
        let mut s = IoSource { len: 64, reads: vec![] };
        let got = try_read_lossy_frame(&mut s, 0, length);
        assert!(matches!(got, Ok(None)), "length {length}: {got:?}");
        assert!(s.reads.is_empty(), "length {length} must not read");
    }
}

#[test]
fn length_four_reads_soi_and_propagates_io() {
    let mut s = IoSource { len: 64, reads: vec![] };
    let got = try_read_lossy_frame(&mut s, 0, 4);
    assert!(matches!(got, Err(ref e) if e.is_io_failure()), "{got:?}");
    assert_eq!(s.reads, vec![(0, 2)]);
}

#[test]
fn never_reads_beyond_range_end() {
    // SOI, APP0 (len 2), then 3 spare bytes in range; bytes after the range look like more segments.
    let mut bytes = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x02];
    bytes.extend_from_slice(&[0xFF, 0xE0, 0x00]); // 3 bytes: range ends here (end = 9)
    bytes.extend_from_slice(&[
        0x02, 0xFF, 0xE0, 0x00, 0x02, 0xFF, 0xC0, 0x00, 0x0B, 8, 0, 8, 0, 8, 1, 1, 0x11, 0,
    ]);
    let mut s = Recording {
        inner: InMemoryRawHeaderSource::new(bytes),
        reads: vec![],
    };
    let got = try_read_lossy_frame(&mut s, 0, 9);
    assert!(matches!(got, Ok(None)));
    for (o, c) in &s.reads {
        assert!(o + i64::from(*c) <= 9, "read ({o},{c}) beyond range end");
    }
}

#[test]
fn standalone_markers_after_first_position_advance_by_two() {
    let mut bytes = vec![0xFF, 0xD8, 0xFF, 0xD0, 0xFF, 0xD0, 0xFF, 0x01, 0xFF, 0xD3];
    bytes.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x0B, 8, 0x01, 0xE0, 0x02, 0x80, 1, 1, 0x11, 0]);
    for off in [0usize, 5] {
        let mut b = vec![0u8; off];
        b.extend_from_slice(&bytes);
        let mut s = InMemoryRawHeaderSource::new(b.clone());
        let got = try_read_lossy_frame(&mut s, off as i64, (b.len() - off) as i64);
        assert_eq!(
            got.unwrap(),
            Some(LossyFrame {
                width: 640,
                height: 480
            }),
            "offset {off}"
        );
    }
}

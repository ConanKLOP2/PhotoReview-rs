//! Regression tests: C# catches ONLY InvalidDataException (no I/O cause); Canceled and ArgumentOutOfRange propagate.
use pr_imaging_raw::tiff_nav::{compute_exif_block, reconcile_jpeg_size};
use pr_imaging_raw::{RawHeaderError, RawHeaderSource};

struct Fail(u8);
impl RawHeaderSource for Fail {
    fn length(&self) -> i64 {
        1000
    }
    fn read(&mut self, _o: i64, _c: i32) -> Result<&[u8], RawHeaderError> {
        Err(match self.0 {
            0 => RawHeaderError::Canceled,
            _ => RawHeaderError::ArgumentOutOfRange {
                param: "offset",
                message: "x".into(),
            },
        })
    }
}

#[test]
fn diff_compute_exif_block_propagates_canceled() {
    assert!(matches!(
        compute_exif_block(&mut Fail(0), true, 8),
        Err(RawHeaderError::Canceled)
    ));
}

#[test]
fn diff_compute_exif_block_propagates_argument_out_of_range() {
    assert!(matches!(
        compute_exif_block(&mut Fail(1), true, 8),
        Err(RawHeaderError::ArgumentOutOfRange { .. })
    ));
}

#[test]
fn diff_reconcile_propagates_canceled() {
    let (mut w, mut h) = (64, 48);
    let r = reconcile_jpeg_size(&mut Fail(0), 8, 100, &mut w, &mut h, |_, _, _| {
        Err(RawHeaderError::Canceled)
    });
    assert!(matches!(r, Err(RawHeaderError::Canceled)));
}

#[test]
fn diff_reconcile_propagates_argument_out_of_range() {
    let (mut w, mut h) = (64, 48);
    let r = reconcile_jpeg_size(&mut Fail(1), 8, 100, &mut w, &mut h, |_, _, _| {
        Err(RawHeaderError::ArgumentOutOfRange {
            param: "offset",
            message: "x".into(),
        })
    });
    assert!(matches!(r, Err(RawHeaderError::ArgumentOutOfRange { .. })));
}

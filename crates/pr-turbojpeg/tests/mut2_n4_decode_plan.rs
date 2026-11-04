//! N4 (2026-11-05): killers and run-based proofs for the scale plan of `TurboJpegDecoder::decode_bytes`
//! (`decoder.rs` 1095:29, 1095:59, 1125:35, 1125:52, 1125:84, 1126:47, 1126:62, 1129:34). No `turbojpeg.dll`: a fake
//! decompressor reports the source size and records the scaling factors, a counting fine scaler records how often it is asked to
//! resize. No golden file (the one JPEG fixture is only read as bytes for its header walk), no sleep.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use pr_imaging::decode_types::{DecodeBox, DecodeRequest};
use pr_imaging::decoder_traits::{DecodeError, ImageDecoder};
use pr_imaging::memory_headroom::{MemoryInfo, MemoryInfoSource};
use pr_imaging::pixel_memory::PixelBufferImageCodec;
use pr_imaging::pixels::{PixelBuffer, PixelLayout};
use pr_test_support::golden;
use pr_turbojpeg::decoder::{
    Decompressor, DecompressorFactory, EnglishTurboJpegTexts, FineScaleError, FineScaler, TurboJpegDecoder,
    scaled_size, select_scaling_factor,
};
use pr_turbojpeg::native::{TjParam, TjPixelFormat, TjScalingFactor};

#[derive(Default)]
struct Log {
    factors: Mutex<Vec<(i32, i32)>>,
}

struct Probe {
    log: Arc<Log>,
    width: i32,
    height: i32,
    refuse_scaling: bool,
}

impl Decompressor for Probe {
    fn set(&mut self, _param: TjParam, _value: i32) -> Result<i32, DecodeError> {
        Ok(0)
    }
    fn get(&mut self, param: TjParam) -> Result<i32, DecodeError> {
        Ok(match param {
            TjParam::JpegWidth => self.width,
            TjParam::JpegHeight => self.height,
            _ => 0,
        })
    }
    fn decompress_header(&mut self, _jpeg: &[u8]) -> Result<i32, DecodeError> {
        Ok(0)
    }
    fn set_scaling_factor(&mut self, factor: TjScalingFactor) -> Result<i32, DecodeError> {
        self.log.factors.lock().unwrap().push((factor.num, factor.denom));
        Ok(i32::from(self.refuse_scaling))
    }
    fn decompress8(
        &mut self,
        _jpeg: &[u8],
        _dst: &mut [u8],
        _pitch: i32,
        _pixel_format: TjPixelFormat,
    ) -> Result<i32, DecodeError> {
        Ok(0)
    }
    fn error_message(&mut self) -> Option<String> {
        None
    }
}

struct CountingScaler(Arc<AtomicUsize>);

impl FineScaler for CountingScaler {
    fn resize(&self, _source: &PixelBuffer, width: i32, height: i32) -> Result<PixelBuffer, FineScaleError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        PixelBuffer::allocate(width, height, PixelLayout::Bgr32)
            .map_err(|e| FineScaleError::Failed(DecodeError::Other { message: e.to_string() }))
    }
}

struct Outcome {
    width: i32,
    height: i32,
    downscaled: bool,
    first_factor: (i32, i32),
    fine_scale_calls: usize,
}

fn jpeg_bytes() -> Vec<u8> {
    let path = golden::repo_root().join("tests/fixtures/turbojpeg/parity-64x48.jpg");
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// Decodes the fixture bytes while the probe reports `width x height`.
fn run(
    jpeg: &[u8],
    (width, height): (i32, i32),
    refuse: bool,
    decode_box: DecodeBox,
    orientation: Option<i32>,
) -> Outcome {
    let log = Arc::new(Log::default());
    let calls = Arc::new(AtomicUsize::new(0));
    let factory: Arc<dyn DecompressorFactory> = {
        let log = Arc::clone(&log);
        Arc::new(move || -> Result<Box<dyn Decompressor>, DecodeError> {
            Ok(Box::new(Probe {
                log: Arc::clone(&log),
                width,
                height,
                refuse_scaling: refuse,
            }))
        })
    };
    let memory: Arc<dyn MemoryInfoSource> = Arc::new(|| MemoryInfo {
        total_available: 1 << 36,
        load: 0,
    });
    let decoder = TurboJpegDecoder::new(
        PixelBufferImageCodec::shared(),
        Arc::new(CountingScaler(Arc::clone(&calls))),
        memory,
        factory,
        Arc::new(EnglishTurboJpegTexts),
    );
    let mut request = DecodeRequest::with_box("p.jpg", decode_box).with_bytes(jpeg.to_vec().into());
    if let Some(value) = orientation {
        request = request.with_source_orientation(value);
    }
    let image = decoder
        .decode(&request)
        .unwrap_or_else(|e| panic!("{width}x{height}: {e:?}"));
    let first_factor = log
        .factors
        .lock()
        .unwrap()
        .first()
        .copied()
        .expect("one scaling factor request");
    Outcome {
        width: image.pixel_width(),
        height: image.pixel_height(),
        downscaled: image.downscaled(),
        first_factor,
        fine_scale_calls: calls.load(Ordering::SeqCst),
    }
}

/// decoder.rs:1125:35 (`downscale && (..)` -> `||`), 1125:52 and 1125:84 (`>` -> `>=`): a DCT factor that already gives the
/// target size does NOT call the fine scaler; a target between two factors does (once).
#[test]
fn an_exact_dct_factor_needs_no_fine_scale_and_a_target_between_factors_does() {
    let jpeg = jpeg_bytes();
    for (reported, target, factor) in [
        ((64, 48), (16, 12), (1, 4)),
        ((64, 48), (32, 24), (1, 2)),
        ((64, 48), (8, 6), (1, 8)),
        ((4000, 1), (2000, 1), (1, 2)),
    ] {
        let outcome = run(&jpeg, reported, false, DecodeBox::new(target.0, target.1), None);
        assert_eq!(outcome.first_factor, factor, "{reported:?} -> {target:?}");
        assert_eq!((outcome.width, outcome.height), target, "{reported:?} -> {target:?}");
        assert_eq!(outcome.fine_scale_calls, 0, "{reported:?} -> {target:?}");
        assert!(outcome.downscaled);
    }
    let between = run(&jpeg, (64, 48), false, DecodeBox::new(20, 15), None);
    assert_eq!(between.first_factor, (3, 8));
    assert_eq!((between.width, between.height, between.fine_scale_calls), (20, 15, 1));
    // No box: never a fine scale.
    let none = run(&jpeg, (64, 48), false, DecodeBox::new(0, 0), None);
    assert_eq!(
        (none.width, none.height, none.fine_scale_calls, none.downscaled),
        (64, 48, 0, false)
    );
}

/// decoder.rs:1095:29 and 1095:59 (`<` -> `<=`): a box equal to a 1 x 1 source must not ask for a DCT factor (with `<=` the
/// scaling-factor search runs and picks 1/8, whose output is also 1 x 1, but the request differs).
#[test]
fn a_box_equal_to_a_one_pixel_source_keeps_the_unscaled_factor() {
    let jpeg = jpeg_bytes();
    let outcome = run(&jpeg, (1, 1), false, DecodeBox::new(1, 1), None);
    assert_eq!(outcome.first_factor, (1, 1));
    assert_eq!((outcome.width, outcome.height, outcome.fine_scale_calls), (1, 1, 0));
    assert!(!outcome.downscaled);
}

/// decoder.rs:1126:47 (`&&` -> `||`), 1126:62 (`>` -> `>=`) and 1129:34 (`<` -> `>`): all three are redundant on the whole
/// plan domain. `apply_orientation` is the identity for an orientation outside 2..=8, `orientation` is 1 when it is not applied,
/// and `scaled_width < orig_width` implies one of the two target comparisons. Proven by a differential run against the C#
/// formulas over every combination of 6 source widths, 6 source heights, 6 box widths, 6 box heights, a refused or accepted
/// factor and 6 orientations (15 552 decodes): size, `downscaled`, first factor request and number of fine-scale calls equal
/// the model.
#[test]
fn the_scale_plan_equals_the_csharp_model_on_a_small_exhaustive_domain() {
    let jpeg = jpeg_bytes();
    let fixture_orientation = pr_turbojpeg::decoder::read_exif_orientation(&jpeg);
    let sizes = [1, 2, 5, 8, 17, 64];
    let boxes = [0, 1, 3, 8, 24, 64];
    let orientations = [None, Some(1), Some(5), Some(6), Some(8), Some(9)];
    for ow in sizes {
        for oh in sizes {
            for bw in boxes {
                for bh in boxes {
                    for refuse in [false, true] {
                        for orientation in orientations {
                            let outcome = run(&jpeg, (ow, oh), refuse, DecodeBox::new(bw, bh), orientation);
                            let label = format!("{ow}x{oh} box {bw}x{bh} refuse {refuse} orientation {orientation:?}");
                            let applied = orientation.unwrap_or(fixture_orientation);
                            let transposed = (5..=8).contains(&applied);
                            let downscale = bw > 0 || bh > 0;
                            let (tw, th) = DecodeBox::new(bw, bh).fit_stored(ow, oh, transposed);
                            let requested = if downscale && (tw < ow || th < oh) {
                                select_scaling_factor(ow, oh, tw, th)
                            } else {
                                TjScalingFactor::ONE
                            };
                            let used = if refuse { TjScalingFactor::ONE } else { requested };
                            let (sw, sh) = (scaled_size(ow, used) as i32, scaled_size(oh, used) as i32);
                            let fine = downscale && (sw > tw || sh > th);
                            let (mut w, mut h) = if fine { (tw, th) } else { (sw, sh) };
                            if transposed {
                                (w, h) = (h, w);
                            }
                            let downscaled = downscale && (sw < ow || tw < ow || th < oh);
                            assert_eq!(
                                outcome.first_factor,
                                (requested.num, requested.denom),
                                "{label}: factor"
                            );
                            assert_eq!((outcome.width, outcome.height), (w, h), "{label}: size");
                            assert_eq!(outcome.downscaled, downscaled, "{label}: downscaled");
                            assert_eq!(outcome.fine_scale_calls, usize::from(fine), "{label}: fine scale");
                        }
                    }
                }
            }
        }
    }
}

//! Core contract tests for W5-12b1. The full 50-row oracle suite and its shared fake implementations are added with the
//! golden wiring; these boundary tests keep the decoder's public entry points guarded while that package is integrated.

use std::sync::Arc;

use pr_imaging::decode_types::{DecodeRequest, DecodedImageView, ImageInfo};
use pr_imaging::decoder_traits::{DecodeError, ImageDecoder};
use pr_imaging_raw::decoder::RawDecoder;

struct NeverCalledDecoder;

impl ImageDecoder for NeverCalledDecoder {
    fn decode(&self, _: &DecodeRequest) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
        panic!("blank-path validation must happen before invoking the inner decoder")
    }

    fn read_info(&self, _: &str) -> Result<ImageInfo, DecodeError> {
        panic!("blank-path validation must happen before invoking the inner decoder")
    }
}

#[test]
fn decode_blank_path_returns_argument_error() {
    // C#: RawDecoderTests.Decode_UnsupportedRawContainer_CarriesTheLocalizedUnsupportedSentence (entry validation precedes I/O).
    let decoder = RawDecoder::new(Arc::new(NeverCalledDecoder));
    let error = match decoder.decode(&DecodeRequest::new("   ", 0)) {
        Ok(_) => panic!("whitespace path must be rejected"),
        Err(error) => error,
    };
    assert!(matches!(error.kind(), DecodeError::InvalidArgument { .. }));
    assert!(error.message().contains("Parameter 'path'"));
}

#[test]
fn read_info_blank_path_returns_argument_error() {
    // C#: RawDecoderTests.ReadInfo_UnsupportedRawContainer_CarriesTheLocalizedUnsupportedSentence (entry validation precedes I/O).
    let decoder = RawDecoder::new(Arc::new(NeverCalledDecoder));
    let error = decoder.read_info("\t\r\n").expect_err("whitespace path is invalid");
    assert!(matches!(error.kind(), DecodeError::InvalidArgument { .. }));
    assert!(error.message().contains("Parameter 'path'"));
}

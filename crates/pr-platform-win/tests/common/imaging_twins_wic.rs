//! Test-only port of PhotoReview.Imaging/Decoding/EmbeddedThumbnailReader.cs WIC calls.
//! This exercises the pure reader through real WIC; production composition still needs this adapter.
use pr_core::abstractions::ReadSeek;
use pr_imaging::embedded_thumbnail::{
    EmbeddedThumbnailBackend, EmbeddedThumbnailBitmap, EmbeddedThumbnailFrame, ThumbnailBackendError,
};
use pr_platform_win::wic::{
    decoder::SourceStreamBackend,
    exif_reader::{FrameMetadata, MetadataQueryReader, PropVariant, read_orientation},
    interop::*,
};
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::core::PCWSTR;

pub struct NativeBackend;
struct Frame {
    frame: Com<IWICBitmapFrameDecode>,
    _decoder: Com<IWICBitmapDecoder>,
    _stream: ComStream,
}
struct Bitmap(Com<IWICBitmapSource>);
fn error(e: impl std::fmt::Display) -> ThumbnailBackendError {
    ThumbnailBackendError(e.to_string())
}
impl EmbeddedThumbnailBackend for NativeBackend {
    fn open(
        &self,
        stream: Box<dyn ReadSeek>,
    ) -> Result<Option<Box<dyn EmbeddedThumbnailFrame>>, ThumbnailBackendError> {
        let stream = ComStream::new(Box::new(SourceStreamBackend::with_buffer_size(stream, 64 * 1024)));
        let factory = create_factory().map_err(error)?;
        let decoder =
            create_decoder_from_stream(&factory, &stream.istream(), WICDecodeMetadataCacheOnDemand).map_err(error)?;
        // SAFETY: decoder is live; the call has no pointer arguments.
        if unsafe { decoder.GetFrameCount() }.map_err(error)? == 0 {
            return Ok(None);
        }
        let frame = get_frame(&decoder, 0).map_err(error)?;
        Ok(Some(Box::new(Frame {
            frame,
            _decoder: decoder,
            _stream: stream,
        })))
    }
}
impl EmbeddedThumbnailFrame for Frame {
    fn main_size(&mut self) -> Result<(u32, u32), ThumbnailBackendError> {
        let (mut w, mut h) = (0, 0);
        // SAFETY: frame is live and both output pointers are exclusive valid u32s.
        unsafe { self.frame.GetSize(&mut w, &mut h) }.map_err(error)?;
        Ok((w, h))
    }
    fn thumbnail(&mut self) -> Option<Box<dyn EmbeddedThumbnailBitmap>> {
        get_thumbnail(&self.frame)
            .ok()
            .flatten()
            .map(|bitmap| Box::new(Bitmap(bitmap)) as Box<dyn EmbeddedThumbnailBitmap>)
    }
    fn orientation(&mut self) -> i32 {
        read_orientation(self)
    }
}
struct Query(Com<IWICMetadataQueryReader>);
impl FrameMetadata for Frame {
    type Reader = Query;
    fn metadata_query_reader(&self) -> Result<Query, i32> {
        get_metadata_query_reader(&self.frame)
            .map(Query)
            .map_err(|e| e.hresult())
    }
}
impl MetadataQueryReader for Query {
    fn get_metadata_by_name(&self, name: &str, value: &mut PropVariant) -> i32 {
        let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: live reader, terminated UTF-16 name and exclusive aligned PROPVARIANT output valid for this call.
        unsafe {
            self.0
                .GetMetadataByName(PCWSTR(wide.as_ptr()), value.as_mut_ptr().cast::<PROPVARIANT>())
        }
        .err()
        .map_or(0, |e| e.code().0)
    }
}
impl EmbeddedThumbnailBitmap for Bitmap {
    fn size(&mut self) -> Result<(u32, u32), ThumbnailBackendError> {
        let (mut w, mut h) = (0, 0);
        // SAFETY: source is live and output pointers are exclusive valid u32s.
        unsafe { self.0.GetSize(&mut w, &mut h) }.map_err(error)?;
        Ok((w, h))
    }
    fn copy_pbgra(&mut self, dst: &mut [u8], stride: usize) -> Result<(), ThumbnailBackendError> {
        let factory = create_factory().map_err(error)?;
        let converter = create_format_converter(&factory).map_err(error)?;
        // SAFETY: source and converter are live; GUID is static; null palette is documented for this conversion.
        unsafe {
            converter.Initialize(
                &*self.0,
                &GUID_WICPixelFormat32bppPBGRA,
                WICBitmapDitherTypeNone,
                None::<&IWICPalette>,
                0.0,
                WICBitmapPaletteTypeCustom,
            )
        }
        .map_err(error)?;
        let stride = u32::try_from(stride).map_err(error)?;
        // SAFETY: converter is live; null rectangle copies the whole image into the reader's sized exclusive buffer.
        unsafe { converter.CopyPixels(std::ptr::null(), stride, dst) }.map_err(error)
    }
}

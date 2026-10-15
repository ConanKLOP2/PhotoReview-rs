# Wave 4 plan: Imaging (decided 2026-10-15, baseline C# `d92d6aae`)

Supersedes the Wave 4 table of `MULTI-AGENT-PLAN.md` section 3: the file map there predates the Win32/Direct2D move
(C# ADR 0010). Sizes below are C# bytes at `d92d6aae`. Granularity rule: P-18. Starts when the Wave 3 PR is merged
(contract writing may start earlier: it only reads C# and writes `contracts/`).

## Scope and crates
- Pure logic (no OS): `pr_imaging` (crate exists, empty) and `pr_core::caching` where the C# type lives in Core.
- Windows imaging (WIC COM, `unsafe`): `pr_platform_win::wic` (AGENTS.md: Windows code lives in `pr-platform-win`
  and decoder crates). WPF types (`PhotoReview.Imaging.Wpf`, 5 files, about 30 KB) have no Rust twin: their job
  (bitmap decode, EXIF orientation, adapter) is covered by WIC + the pure orientation code (W4-02a).
- RAW, TurboJPEG, LibRaw are Wave 5 (they plug into `IImageDecoder`; W4-12 defines the seam they use).

## Sub-units (one agent each; tiers run in order, units inside a tier in parallel)

### Tier A: pure leaves (Haiku, test-first not required)
| WU | C# source | Rust | Notes |
|---|---|---|---|
| W4-01a | `ImageCacheKey.cs` (6 KB) | `pr_imaging::cache_key` | key text is persisted in cache files: byte-identical |
| W4-01b | `DecodeBox`, `Decoding/{DecodeRequest, DecodedImageSize, ImageInfo, DecodedImage, IDecodedImage, PixelLease, DecodeFailureSourceBytes}` | `pr_imaging::decode_types` | exceptions become one error enum |
| W4-01c | `Decoding/{DecoderBusyException, DecoderMemoryAdmissionException, MissingImageCodecException, ICancellableImageDecoder, IImageDecoder, IImageDecoderFactory, IPlatformImageCodec}` | `pr_imaging::decoder_traits` | traits only; Wave 5 implements |
| W4-02a | `Decoding/ExifOrientation.cs` | `pr_imaging::orientation` | pure transform table; also replaces `WpfExifOrientation` |
| W4-02b | `Decoding/EmbeddedThumbnailReader.cs` | `pr_imaging::embedded_thumbnail` | |
| W4-03a | `Metadata/TiffStructure.cs` | `pr_imaging::tiff` | fixture-driven goldens |
| W4-03b | `Metadata/ExifParser.cs` | `pr_imaging::exif_parser` | needs W4-03a |
| W4-04a | `Metadata/{ExifSummary, ExifSummaryCodec}.cs` | `pr_imaging::exif_summary` | codec is a persisted format |
| W4-04b | `Metadata/ExifQueryInterpreter.cs` | `pr_imaging::exif_query` | |
| W4-05a | `AdaptivePreviewPolicy.cs` | `pr_imaging::adaptive_preview` | |
| W4-05b | `PreloadWindow`, `PreloadOrderService`, `Preload/PreloadOptions` | `pr_imaging::preload_order` | |
| W4-06a | `Preload/NavigationPace.cs` | `pr_imaging::navigation_pace` | clock injected (`pr_core::time`) |
| W4-06b | `Preload/{PreloadBusyBackoff, PreviewSizeSampler}.cs` | `pr_imaging::preload_backoff` | |
| W4-07a | `Preload/RamBudgetPolicy.cs` | `pr_imaging::ram_budget` | Sonnet |
| W4-07b | `Decoding/MemoryHeadroom.cs` | `pr_imaging::memory_headroom` | OS query behind a trait |
| W4-16a | `Pixels/{PixelBuffer, PixelOps}.cs` | `pr_imaging::pixels` | NEW (missing from the old plan); Sonnet |
| W4-16b | `Pixels/NativePixelMemory.cs`, `Decoding/{PixelBufferImageCodec, ReadOnlyMemoryStreamFactory}.cs` | `pr_imaging::pixel_memory` | aligned alloc, unsafe with SAFETY |

### Tier B: caches and routing (Sonnet / Opus)
| WU | C# source | Rust | Model |
|---|---|---|---|
| W4-08a | `Caching/AtomicCacheFile.cs` | `pr_imaging::cache::atomic_file` | Sonnet |
| W4-08b | `Caching/PreviewCacheFile.cs` (16 KB) | `pr_imaging::cache::preview_file` | Sonnet, on-disk format |
| W4-09 | `Caching/DiskCacheStore.cs` (14 KB) | `pr_imaging::cache::disk_store` | **Opus**, INV-2 |
| W4-10a | `Caching/{SourceBytesCache, SourceBytesCachePolicy}.cs` | `pr_imaging::cache::source_bytes` | **Opus**, INV-2/INV-8 |
| W4-10b | `Caching/ThumbnailCache.cs` | `pr_imaging::cache::thumbnails` | **Opus** |
| W4-12a | `Decoding/ImageDecoderFactory.cs` | `pr_imaging::factory` | Sonnet, INV-12 |
| W4-12b | `Decoding/FallbackImageDecoder.cs` | `pr_imaging::fallback` | Sonnet |
| W4-12c | `Decoding/WebpHeicRoutingDecoder.cs` | `pr_imaging::webp_heic` | Sonnet |

### Tier C: Windows imaging (Opus for COM)
| WU | C# source | Rust | Model |
|---|---|---|---|
| W4-11a | `Decoding/Wic/WicInterop.cs` (25 KB) | `pr_platform_win::wic::interop` | **Opus**, COM `unsafe`, SAFETY per block |
| W4-11b | `Decoding/Wic/WicCodecAvailability.cs` | `pr_platform_win::wic::availability` | Sonnet |
| W4-11c | `Decoding/Wic/WicDirectDecoder.cs` (28 KB) | `pr_platform_win::wic::decoder` | **Opus** |
| W4-11d | `Decoding/Wic/WicExifReader.cs` | `pr_platform_win::wic::exif_reader` | Sonnet |
| W4-11e | `Pixels/WicPixelScaler.cs`, `Caching/{WicCacheImageReader, WicImageEncoder}.cs` | `pr_platform_win::wic::{scaler, cache_image}` | Sonnet |

### Tier D: orchestration (Opus; the contract writer splits further after reading)
| WU | C# source | Rust | Model |
|---|---|---|---|
| W4-14 | `Caching/PreviewImageService.cs` (57 KB), INV-1/INV-2 | `pr_imaging::service::*` (split by the writer into 14a state/policy, 14b decode path, 14c cache path) | **Opus** |
| W4-15 | `Preload/{PreloadScheduler, IPreloadTarget}.cs` (57 KB) | `pr_imaging::preload::*` (split into 15a queue/priority, 15b worker/cancel) | **Opus** |

## Order and gates
Tier A -> B -> C (W4-11 can start with A done; it only needs the traits W4-01c) -> D. Per unit: red tests,
implementation, adversarial verifier (Opus for red units), mutation gate on cache write paths (INV-2). One PR for
the wave. Oracle: pure units use `tools/oracle-cs` goldens; WIC units use real-WIC differential cases marked
`#[ignore = "native: needs WIC"]` and run on `windows-latest`.

## Proposals acted on (also in PROPOSED-DECISIONS.md)
See P-20..P-23.

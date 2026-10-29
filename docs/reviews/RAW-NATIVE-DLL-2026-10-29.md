# RAW native DLL run, 2026-10-29 (machine A, claim PARTIAL-raw-dll)

Status: report of a test run. Machine A had the pinned DLLs (staged with `tools/dev/stage-native.ps1` into `target/debug`,
`target/debug/deps`; `turbojpeg.dll` and `libraw.dll` additionally copied to the gitignored `native/x64/`) and WIC, and **no RAW
corpus** (no .cr2/.nef/.arw/.dng files). Command per binary:
`cargo test -p <crate> --locked --test <bin> -- --include-ignored --test-threads=1` (and `--lib`). cargo-mutants not run.

Note on vacuous passes: `pr-turbojpeg` `decoder_native` and the in-crate native test read `<repo>/native/x64/`. Without that
copy 40 of its 41 tests return early ("skipping") and report green. The first run was therefore vacuous; the table is the rerun
with `native/x64/` populated (contract W10-N1 describes both locations).

| Crate / binary | passed | failed | ignored | Notes |
|---|---|---|---|---|
| pr-libraw `--lib` | 1 | 0 | 0 | |
| pr-libraw `availability` | 30 | 0 | 0 | real `libraw.dll` + `kernel32.dll` |
| pr-libraw `decoder_native` | 25 | 0 | 0 | 17 corpus-gated cases print "skip: no RAW corpus" and return early (8 ran) |
| pr-libraw `native` | 38 | 0 | 0 | 7 corpus-gated cases skipped (31 ran) |
| pr-turbojpeg `--lib` | 6 | 0 | 0 | `compress8_frees_...` needs `native/x64/turbojpeg.dll` (red until copied: environment) |
| pr-turbojpeg `availability` | 14 | 0 | 0 | |
| pr-turbojpeg `native_golden` | 8 | 0 | 0 | |
| pr-turbojpeg `decoder_native` | 40 | 1 | 0 | FAIL `flavour_decodes_consistently` (see defects) |
| pr-imaging-raw `header_source` / `raf_reader` / `raw_exif` / `raw_nef` / `raw_reader_arw` | 26 / 50 / 36 / 68 / 57 | 0 | 0 | corpus-gated tests return early (not evidence); `--lib` 9/9 |
| pr-imaging `--lib` | 150 | 0 | 0 | |
| pr-imaging `adaptive_preview`, `cache_atomic_file`, `cache_disk_store`, `cache_key`, `memory_headroom`, `preview_file_repair`, `verify_navigation_pace` | 12, 24, 58, 14, 9, 5, 4 | 0 | 0 | `verify_navigation_pace` includes the perf case (328 s, loaded machine) |
| pr-imaging `embedded_thumbnail` | 27 | 17 | 0 | all 17 `todo!()` native seam (`native_backend()`) |
| pr-imaging `thumbnail_cache` | 46 | 9 | 0 | all 9 `todo!()` seams (`CacheImageReader`/`PngEncoder`) |
| pr-imaging `cache_preview_file` | 46 | 4 | 0 | all 4 `todo!()`, twins belong to `pr-platform-win/tests/preview_cache_wic.rs` |
| pr-imaging `preview_service_disk` | 55 | 1 | 0 | `broken_entry_is_a_miss_jpeg_header_mangled_eoi`: fake decoder accepts the mangled entry, real WIC JPEG codec not wired |
| pr-platform-win `preview_cache_wic` | 4 | 0 | 0 | |
| pr-platform-win `wic_availability` / `wic_cache_adapters` / `wic_cache_image` / `wic_scaler` | 14 / 8 / 39 / 10 | 0 | 0 | |
| pr-platform-win `wic_decoder` | 41 | 1 | 0 | the 1 is `#[ignore = "blocked: W4-11c OQ1"]` localized sentence test, expected red |
| pr-platform-win `wic_decoder_native` / `wic_decoder_buffers` / `wic_exif_reader` / `wic_interop` / `wic_repair1` | 70 / 6 / 26 / 76 / 14 | 0 | 0 | real WIC |

## Defects and findings

1. `pr-turbojpeg` `decoder_native::flavour_decodes_consistently`: the `cmyk` flavour fails with "TurboJPEG decompression failed:
   Unsupported color conversion request" (the `ycck` flavour is never reached, the loop panics first). C#
   `TurboJpegSyntheticFormatTests.Flavour_DecodesConsistently` (7eaee2a2) lets TurboJpeg decline CMYK/YCCK (`InvalidDataException`/
   `NotSupportedException`) and checks size through the Turbo->Wpf fallback chain. The Rust twin calls the bare decoder and expects
   success. Assessment: mis-ported test (open), production decline matches C#. Assertion not weakened, nothing fixed here.
2. 30 pr-imaging native twins (17 + 9 + 4) are `todo!()` by design (dev-dependency cycle with pr-platform-win); their real
   counterparts pass in pr-platform-win, but the pr-imaging twins are unwired. 1 more (`preview_service_disk`) needs a real WIC
   JPEG payload codec in pr-imaging tests. Open test-wiring gaps, no production defect shown.
3. `compress8_frees_the_library_allocated_output_exactly_once` (turbojpeg lib) panics "missing native\x64\turbojpeg.dll" unless the
   DLL is also in `native/x64/`: environment/staging behaviour; `stage-native.ps1 -Destination` does not populate it.

## NOT run / still open

- Any RAW corpus case (24 libraw cases plus the imaging-raw corpus tests): no corpus on this machine (human-provided gap, P-30);
  strict mode (`PHOTOREVIEW_RAW_CORPUS_STRICT`) was not run.
- cargo-mutants (X-raw-mutation-w5 and all mutation gaps); GPU/Recycle Bin/desktop native tests; pr-platform-win
  `wic::decoder` in-crate `--lib` tests (not part of this run).
- No row was moved to Done: every row keeps a named gap (corpus, mutation, unwired twins or the open defect).

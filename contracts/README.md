# Work-unit contracts

One file per work unit (`W<wave>-<n>.md`), written by the Contract Writer before any porter starts
(docs/MULTI-AGENT-PLAN.md §2). The stub each contract names is already in the tree with `todo!()` bodies and
compiles with the rest of the workspace; a porter fills only the files under "Owned files".

**Lead decisions below override anything a contract says.** Porters read this file first, then their contract.

## Wave 1 roster

| WU | Unit | Rust | Porter | Golden |
|---|---|---|---|---|
| W1-01 | Model enums | `pr_core::model::enums` | Haiku | `model-enums` |
| W1-02 | Lenient enum/flag JSON | `pr_core::model::serde_ext` | **Sonnet** | `lenient-enum`, `flag-enum-json` |
| W1-03 | Flag fields | `pr_core::model::fields` | Haiku | `flag-fields` |
| W1-04 | Shortcuts, ReviewAction | `pr_core::settings::shortcuts` | Haiku | `shortcut-canonical` |
| W1-05 | PerformanceOptions | `pr_core::settings::perf_options` | — (constants, complete in stub; test porter only) | `perf-options` |
| W1-06 | Natural comparer | `pr_core::catalog::natural_cmp` | Haiku | `natural-cmp`, `natural-key` |
| W1-07 | Catalog types, ExplorerReason | `pr_core::catalog::{types, explorer_reason}` | Haiku | `image-file-types` |
| W1-08 | BoundedLruCache | `pr_core::caching::lru` | Haiku | `lru-trace` |
| W1-09 | File-action records | `pr_core::file_actions::types` | Haiku | — |
| W1-10 | AppVersion, update URL policy | `pr_core::updates::version` | Haiku; `validate_update_url` **Sonnet** (D6) | `app-version`, `update-url-policy` |
| W1-11 | LocTemplate | `pr_l10n::template` | Haiku | `loc-template`, `loc-render`, `loc-pseudo` |
| W1-12 | GenerationClock | `pr_core::catalog::generation` | **Sonnet** | — |
| W1-13 | Journal/FS errors | `pr_core::file_actions::errors` | Haiku | `path-root`, `journal-error-text` |
| W1-14 | Journal line parse/write | `pr_core::file_actions::journal_line` | Haiku (escalate fast) | `journal-lines` (mandatory) |
| W1-15 | Percentile, DiagOptions | `pr_core::diagnostics::{bench_stats, diag_options}` | Haiku | `percentile`, `diag-options` |
| W1-16 | Perf CSV reader | `pr_perf::csv` | Haiku | `perf-csv` |
| W1-17 | Perf stats | `pr_perf::stats` | Haiku | `perf-stats` |
| W1-18 | Perf rules | `pr_perf::rules` | Haiku | `perf-rules` |
| W1-19 | Perf navigation | `pr_perf::nav` | Haiku | `perf-nav` |
| W1-20 | Perf report writer | `pr_perf::report` | **Sonnet** | `perf-report` |
| W1-21 | Perf analyze pipeline | `pr_perf::analyze` | **Sonnet** | `perf-analyze` |
| W1-22 | VBlankEstimator | `pr_core::display::vblank_estimator` | Haiku | `vblank` |

All units can run in parallel: every cross-unit type is already defined (complete data, `todo!()` functions) in
the stubs. A unit's tests may only call functions of OTHER units once those are merged; until then test your own
functions only.

## Lead decisions (Wave 1)

- **D1 Paths in records are `String`.** Catalog, journal, session and file-action records mirror C# `string` (journaled
  and compared as text). The file-system boundary (`pr_core::abstractions::FileSystem`) takes `&Path`; convert with
  `Path::new(&s)`. `ExplorerViewSnapshot` was changed to `String` for the same reason.
- **D2 `JournalEntry` and `JournalGroupMember` live in W1-14** (`file_actions::journal_line`, timestamps as
  `JournalDateTime`). W1-09 re-exports them and no longer defines `JournalGroupMember`; its contract text about that
  type is superseded.
- **D3 Counters keep the C# integer type.** `GenerationClock` uses `AtomicI64` with wrapping increments because C#
  uses `long`; R0003's "AtomicU64" means "an atomic of the C# width".
- **D4 Diagnostic variables use the Rust prefix** `PHOTOREVIEW_RS_DIAG_*` (R0002): they change behaviour, so a value
  set to diagnose the C# app must not leak into this one. The W1-15 golden is keyed by the C# names; its test maps
  `PHOTOREVIEW_DIAG_X` → `PHOTOREVIEW_RS_DIAG_X`. (W1-15's contract text saying "C# names on purpose" is superseded.)
- **D5 Accepted divergences** (record them in the unit's tests as `// DIVERGENCE:` comments, no golden case):
  serde_json cannot read a bare `-0` number token as an integer where System.Text.Json reads 0 (W1-02; W1-14 handles
  it with `raw_value`); W1-14 converts an offset timestamp to UTC (C# makes a machine-dependent Local value) and
  rejects invalid UTF-8 in a property name; W1-11 `describe` strips Rust's ` (os error N)` suffix so texts match .NET;
  W1-21 reads UTF-8 CSVs only, numbers like `1e400` in JSON side files are rejected; non-coded journal error text is
  `err.to_string()` (free text, never parsed).
- **D6 W1-10 `validate_update_url` goes to Sonnet** from the start (hand-written `System.Uri` subset, 127 golden cases);
  the rest of W1-10 stays Haiku.
- **D7 W1-20 and W1-21 go to Sonnet** (byte-identical JSON writer, many ordering decisions), as their contracts propose.
- **D8 Helpers are where their first unit put them** (`catalog::types::get_extension`, `file_actions::errors::
  get_path_root`, `pr_perf::rules::{fmt_fixed, fmt_double}`). Wave 2 hoists shared ones into a lead-owned module.
- **D9 Output files keep CRLF** where C# wrote them on Windows (perf report, indented JSON): byte-identical wins.
- **D10 Things deferred to later waves:** `ReviewAction` deserialization and `CanonicalizeAll(AppSettings)` (W2-01/02);
  `CreatePseudo`/`CreateShowKeys` (Localizer, W2-04); `PerfCsvListener` writer (W2-17); a Windows-locale
  `NumberCulture` lookup (W8, UI).
- **D11 Temporary `#![allow(unused_variables, dead_code)]`** at the top of a stub is allowed only while it has
  `todo!()` bodies; the porter removes it in the same commit that removes the last `todo!()`.

## Wave 2 roster (dependency order)

Merge in this order; a porter may start once its "Depends on" units are merged (stub types already compile).

| WU | Unit | Rust | Porter | Goldens |
|---|---|---|---|---|
| W2-00 | .NET `Path.*` helpers | `pr_core::text::dotnet_path` | Sonnet | `dotnet-path` |
| W2-20 | ContextMenuLayout | `pr_core::settings::context_menu` | Sonnet | `context-menu` |
| W2-01 | AppSettings + config JSON | `pr_core::settings::app_settings` | Sonnet | `settings-json` |
| W2-02 | Normalizer / validator | `pr_core::settings::normalizer` | Sonnet | `settings-normalize`, `-validate`, `-repair-text` |
| W2-03 | SettingsStore (INV-11) | `pr_core::settings::store` | Sonnet | `settings-store`, `settings-parse-text` |
| W2-04 | Localization core + keygen | `pr_l10n::{catalog,loader,localizer}`, `build.rs` | Sonnet | `l10n-*` |
| W2-05..11 | Catalog services | `pr_core::catalog::*`, `file_actions::{duplicates,destination_policy}` | Sonnet (W2-11 Haiku) | per unit |
| W2-12 | SessionStore | `pr_core::session::store` | Sonnet | `session-key`, `session-json-*` |
| W2-13 | SessionWriter | `pr_core::session::writer` | **Opus** | `session-writer-sequence` |
| W2-14 | PhysicalFileSystem, Counting | `pr_core::io::{physical,counting}` | **Opus** (`physical.rs`) | `physical-fs-*` |
| W2-15 | NavigationStatWorker | `pr_core::io::navigation_stat` | Sonnet | `navigation-stat-worker` |
| W2-16 | FileLog (INV-10) | `pr_core::diagnostics::file_log` | Sonnet | `file-log-*` |
| W2-17 | Perf diagnostics | `pr_core::diagnostics::{metrics,perf,perf_csv}` | Sonnet | `perf-*`, `review-metrics` |
| W2-18 | UpdateChecker | `pr_core::updates::checker` | Sonnet | `update-checker` |
| W2-19 | Instance forwarding | `pr_core::instance::*` | Sonnet | `forwarded-path` |

## Lead decisions (Wave 2)

- **D12 Merge order:** W2-00 -> W2-20 -> W2-01 -> W2-02/W2-03; W2-04 after W2-00. Everything else is independent.
- **D13 `SettingsTexts`** stays a `pr-core` trait; the catalog-backed implementation is supplied by the App composition
  root (not W2-04). W2-01 goldens only need the English implementation.
- **D14 Test ownership:** the per-property `*SettingsTests` that go through the normalizer belong to W2-02; those
  through store/parse belong to W2-03. W2-01 keeps reader/writer tests only.
- **D15 Module ownership:** `pr_core::io` (mod.rs) is lead-owned; W2-14 and W2-15 only fill their own files.
- **D16 Local time** (W2-16): inject a `LocalTimeSource`; `pr-core` takes no `windows-sys` dependency.
- **D17 HTTP transport** (W2-18): behind a trait, real transport lands in `pr-platform-win` later; endpoint and allow-list
  stay those of C# for parity.
- **D18 `sha2`** crate is the SHA-256 for W2-17.
- **D19 Golden determinism:** oracle output must not depend on cwd, clock or file times. Removed from goldens:
  `session-key` case with folder `"	"` (resolves against cwd); `physical-fs-ops` `write_ticks` now null (copy
  time). Equality of times is still checked by `same_write_time`.
- **D20 Accepted divergence (W2-04):** invalid UTF-8 in a language file becomes a "cannot read" warning in Rust
  (`read_all_text` returns InvalidData) where C# substitutes U+FFFD. Record `// DIVERGENCE:` and a `Different` row.

## Wave 3 roster (data safety, dependency order)

Order: W3-01 -> W3-02 / W3-03 -> W3-04 -> W3-05 / W3-06 -> W3-07 / W3-08. W3-02 owns the shared journal line rules
(`journal_rules`), so in practice W3-02 merges before W3-01 bodies; W3-04 needs W3-08 and the W3-05 gate trait.

| WU | Unit | Rust | Porter | Goldens |
|---|---|---|---|---|
| W3-01 | OperationJournal + JournalTransaction | `pr_core::file_actions::{journal, transaction}` | **Opus** | `journal-stream-lines`, `journal-ops`, `journal-committed-moves`, `journal-compact`, `journal-transaction` |
| W3-02 | Compaction plan + physical files + line rules | `file_actions::{compaction, journal_rules}`, `io::journal_compaction_files` | **Opus** | `journal-compaction-plan`, `journal-tail-committed-move` |
| W3-03 | Reconcile + startup recovery | `file_actions::{reconcile, startup_recovery}` | **Opus** | `journal-reconcile`, `journal-startup-recovery` |
| W3-04 | FileActionService | `file_actions::service` | **Opus** | `file-action-scenarios` |
| W3-05 | UndoService | `file_actions::undo` | **Opus** | `undo-scenarios` |
| W3-06 | RecoveryRetryService | `file_actions::recovery_retry` | **Opus** | `recovery-retry` |
| W3-07 | RecoveryFileCheck | `file_actions::recovery_check` | Sonnet | `recovery-check`, `recovery-decide` |
| W3-08 | PartialDestinationCleanup | `file_actions::partial_cleanup` | Sonnet | `partial-cleanup` |

## Lead decisions (Wave 3)

- **D21 Test-first:** Haiku ports every C# test of a unit to a red Rust test first; only then does the porter write
  bodies (docs/MULTI-AGENT-PLAN.md, Wave 3). Stubs are `todo!()`, so this PR adds no tests.
- **D22 Gate:** a unit is done only with an Opus verifier (not Sonnet) and `cargo mutants -p pr-core -f
  file_actions/**` with no surviving mutant on the journal write path.
- **D23 Split of the journal:** W3-02 owns `journal_rules` and the compaction plan; W3-03 owns `reconcile.rs` and the
  `ReconcileJournal` trait (`OperationJournal` only delegates); `JournalCompactionOutcome/Result` are defined once, in
  `journal.rs`. The C# reentrant journal gate is kept.
- **D24 Merge order for journal units:** W3-02 -> W3-01 -> W3-03; the 3 observational-equivalence tests of W3-02 go in a
  follow-up PR after W3-01.
- **D25 Gate trait:** `undo::FileActionGate` is the single INV-4 trait; W3-04 implements it, W3-06 uses it.
- **D26 Open at porter kick-off (not decided in this PR):** (a) flush seam for PowerLossSafe (`AppendStream`, W3-01
  OQ1); (b) stat seam `FileSystem::try_get_file_stat` (W3-04/06/07, recommended A: one place for every stat);
  (c) `RecycleBin::try_restore` bool vs `io::Result<bool>` (W3-05 Q1, recommended A); (d) FaultInjectionFileSystem and
  hook fakes in `pr-test-support`; (e) sync `operations-legacy.jsonl` and `journal-older-build/*.jsonl` fixtures;
  (f) a W3-09 integration unit for the fault/crash matrices. Each is spelled out in the unit contract.
  **Resolved as proposals P-11..P-16 in `docs/PROPOSED-DECISIONS.md`** (all take the recommended option; the porters act on them).
  **Landed on `integration/wave-3` (lead foundation, before the red-test pass):**
  (a) `abstractions::AppendStream { fn as_file_mut(&mut self) -> Option<&mut File> }`, `FileSystem::open_append -> io::Result<Box<dyn AppendStream>>`;
  (b) `FileSystem::try_get_file_stat(&Path) -> io::Result<Option<FileStat>>` (W3-07 P1 lists the overrides; `io::physical::is_stat_missing`);
  (c) `RecycleBin::try_restore(..) -> io::Result<bool>`; `FakeRecycleBin::{set_send_hook, set_delete_permanently_hook, set_restore_hook, restore_calls}`;
  (d) `pr_test_support::{HookFileSystem, FaultInjectionFileSystem}` (+ `FaultKind`, `FaultPlan`, `ProcessCrashed`, `is_process_crash`,
  `FsEvent`, hook type aliases) and `FakeFileSystem::peek_stat`. Units use these instead of the local `HookFileSystem` /
  `HookRecycleBin` / `ScriptedFs` copies their contracts describe (a unit-specific scripted fake may still WRAP them);
  `CrashPointFileSystem`, `FaultableFileSystem` and the `FaultInjectionMatrix`/`Rig` helpers are NOT ported yet (W3-09);
  (e) fixtures `tests/fixtures/operations-legacy.jsonl` and `tests/fixtures/journal-older-build/*.jsonl` synced (SYNC-LEDGER).
- **D27 Golden determinism:** all 15 Wave 3 goldens were generated twice and compared byte for byte. Paths in them are
  virtual (`C:\Users\test\...`); `recovery-check` has two physical cases that name a missing file / missing UNC host.
- **D28 Stubs outrank draft prose:** where a contract still names a helper the wired stubs lack, the stub wins and the
  contract is fixed in the porter's PR.

## Wave 4 roster (imaging, tiers run in order, units inside a tier in parallel)

Source of truth for scope and order: `docs/WAVE-4-PLAN.md`. Every unit below has its contract (`contracts/W4-*.md`),
a stub with `todo!()` bodies in its crate path, an oracle file in `tools/oracle-cs/Commands/`, inputs in
`tests/golden-inputs/` and goldens in `tests/golden/`. Counts are golden cases.

### Tier A: pure leaves
| WU | Unit | Rust (`pr_imaging::` unless noted) | Porter | Goldens |
|---|---|---|---|---|
| W4-01a | ImageCacheKey | `cache_key` | Haiku | `image-cache-key` (91) |
| W4-01b | DecodeBox + decode types | `decode_types` | Haiku | `decode-box` (152), `decode-types` (72) |
| W4-01c | Decoder traits + error enum | `decoder_traits` | Haiku | `decoder-exceptions` (15) |
| W4-02a | ExifOrientation + transform table | `orientation` | Haiku | `orientation-values` (2), `orientation-read-wic` (21), `orientation-pixels` (47) |
| W4-02b | EmbeddedThumbnailReader | `embedded_thumbnail` | Haiku | `embedded-thumbnail-size` (21), `-fixtures` (36), `-read` (38) |
| W4-03a | TiffStructure | `tiff` | Haiku | `tiff-structure` (212) |
| W4-03b | ExifParser | `exif_parser` | Haiku | `exif-parser` (146) |
| W4-04a | ExifSummary + codec | `exif_summary` | Haiku | `exif-summary-{create,text,date,encode,decode}` (38/40/49/22/46) |
| W4-04b | ExifQueryInterpreter | `exif_query` | Haiku | `exif-query` (64) |
| W4-05a | AdaptivePreviewPolicy | `adaptive_preview` | Haiku | `adaptive-preview-box` (38), `-quantize` (24) |
| W4-05b | PreloadWindow, order, options | `preload_order` | Haiku | `preload-order` (37), `-window` (13), `-options` (3) |
| W4-06a | NavigationPace | `navigation_pace` | Haiku | `navigation-pace` (30) |
| W4-06b | PreloadBusyBackoff, PreviewSizeSampler | `preload_backoff` | Haiku | `preload-backoff-script` (18), `preview-size-sampler-script` (9) |
| W4-07a | RamBudgetPolicy | `ram_budget` | **Sonnet** | `ram-budget-scalar` (92), `-estimate` (53) |
| W4-07b | MemoryHeadroom | `memory_headroom` | Haiku | `memory-headroom` (59) |
| W4-16a | PixelBuffer, PixelOps | `pixels` | **Sonnet** | `pixel-buffer` (21), `pixel-ops` (476) |
| W4-16b | NativePixelMemory, pixel codec, read-only stream | `pixel_memory` | **Sonnet** (Opus verifier: `unsafe`) | `native-memory` (14), `pixel-codec` (8), `readonly-memory-stream` (11) |

### Tier B: caches and routing
| WU | Unit | Rust | Porter | Goldens |
|---|---|---|---|---|
| W4-08a | AtomicCacheFile | `cache::atomic_file` | Sonnet | `atomic-cache-file` (17) |
| W4-08b | PreviewCacheFile (on-disk format) | `cache::preview_file` | Sonnet | `preview-entry-read` (71), `-write` (30) |
| W4-09 | DiskCacheStore | `cache::disk_store` | **Opus** (INV-2, mutation gate) | `disk-cache-{prune,passes,note-accessed,stale-temp,delete}` (27/17/12/15/15), `cache-entry-failure` (32) |
| W4-10a | SourceBytesCache + policy | `cache::source_bytes` | **Opus** (INV-2, INV-8) | `source-bytes-scenarios` (40) |
| W4-10b | ThumbnailCache | `cache::thumbnails` | **Opus** | `thumbnail-cache-key` (22), `thumbnail-disk-decode` (15) |
| W4-12a | ImageDecoderFactory | `factory` | Sonnet | `decoder-factory` (21) |
| W4-12b | FallbackImageDecoder | `fallback` | Sonnet (INV-12) | `fallback-is-fallbackable` (26), `fallback-decoder` (55) |
| W4-12c | WebpHeicRoutingDecoder | `webp_heic` | Sonnet | `webp-heic-routing` (71) |

### Tier C: Windows imaging (`pr_platform_win::wic`, `unsafe` with SAFETY per block)
| WU | Unit | Rust | Porter | Goldens |
|---|---|---|---|---|
| W4-11a | WicInterop (COM boundary, IStream) | `wic::interop` | **Opus** | `wic-interop-constants` (5), `wic-hresult-exception` (7), `wic-managed-istream` (33) |
| W4-11b | WicCodecAvailability | `wic::availability` | Sonnet | `wic-codec-probe` (16) |
| W4-11c | WicDirectDecoder | `wic::decoder` | **Opus** | `wic-direct-{internals,fixtures,decode,read-info}` (96/41/149/26) |
| W4-11d | WicExifReader | `wic::exif_reader` | Sonnet | `wic-exif-values` (63), `wic-exif-variant` (59) |
| W4-11e | WicPixelScaler, WicImageEncoder, WicCacheImageReader | `wic::{scaler, cache_image}` | Sonnet | `wic-scale-resize` (15), `wic-bgrx-to-bgr` (13), `wic-cache-classify` (24), `wic-cache-codec` (31) |

### Tier D: orchestration (split by the contract writer)
| WU | Unit | Rust | Porter | Goldens |
|---|---|---|---|---|
| W4-14a | PreviewImageService state, keys, RAM cache, epoch | `service::{mod, deps}` | **Opus** (INV-1, INV-2) | `preview-budget-line` (49), `preview-current-key` (32) |
| W4-14b | In-flight dedup, viewer slots, decode, original decode | `service::{handle, decode}` | **Opus** | `preview-service-sequence` (33) |
| W4-14c | Disk-cache path, persist workers, cleanup, clear disk | `service::disk` | **Opus** | `preview-disk-cache-path` (36) |
| W4-14d | `impl PreloadTarget for PreviewImageService` + 8 `PreloadSchedulerTests` (NEW, D43) | `service::mod`, `tests/preview_service_preload.rs` | Sonnet, Opus verifier | - |
| W4-15a | Preload policy, queue, target trait | `preload::{mod, policy, queue, target}` | **Opus** | `preload-policy` (213) |
| W4-15b | Scheduler, worker pool, timer | `preload::{scheduler, worker, timer}` | **Opus** | `preload-scheduler-script` (41) |

Merge order (bodies; the stubs already compile together): 16b -> 16a; 02a, 03a -> 03b; 04a -> 04b; 01a/01b/01c; 05a, 05b,
06a, 06b, 07a, 07b; then Tier B with 08a -> 08b -> 09 -> 10a -> 10b and 12a/12b/12c; Tier C with 11a -> 11b, 11d -> 11c ->
11e; Tier D with 14a -> 14b -> 14c -> 14d and 15a -> 15b. The Wave 4 contracts still mention
`contracts/drafts-w4/<unit>/...`: that folder is gone; the stub is at its crate path, the oracle at
`tools/oracle-cs/Commands/<unit>.cs`, the inputs under `tests/golden-inputs/`.

## Lead decisions (Wave 4)

Decided and wired in this PR (the stubs show the result; D28 applies):

- **D29 Decoded image names:** W4-01b wins. `PlatformImage` is the W4-01b enum (`Pixels(Arc<PixelBuffer>)` | `Opaque`),
  the trait is `DecodedImageView`, `DecodedImage` is the struct. W4-01c is edited to match: `Arc<dyn DecodedImageView>`
  and `PlatformImageCodec::to_pixels<'a>(&self, &'a PlatformImage) -> Result<PixelLease<'a>, _>`.
- **D30 `ExifRational`** is defined once, in `tiff` (W4-03a returns it); `exif_summary` re-exports it. `ExifSummary::create`
  takes `RawExifTags` (no 8 positional parameters). W4-03b builds that struct.
- **D31 No placeholders:** every sibling-unit stand-in (`DecodeBox`, `ImageCacheKey`, `PixelBuffer`, `NativePixelMemory`,
  `ExifSummary`, `DecodedImage`, `PlatformImageCodec`, `WicCodecSupport`) was replaced by the real type.
  `service/deps.rs` is re-exports only.
- **D32 Cache seams:** `CacheImageReader`/`CacheImageInfo` (C# `WicCacheImageReader.Decode`) live in `cache::thumbnails`
  for now; `PngEncoder` (`disk_store`) and `PreviewPayloadCodec` (`preview_file`) stay two traits. Decide at W4-11e kick-off
  whether to merge them into one `cache::CacheImageEncoder` (W4-09 OQ1, W4-11e OQ6).
- **D33 SHA-256:** `pr_core::session::store::sha256` is now `pub` and is THE helper (W4-10b OQ1, W4-14c OQ1); no `sha2`.
- **D34 Key normalisation:** `cache_key::normalize_key_path` (full path + upper-invariant) is added as a stub fn;
  W4-14a uses it for eviction (W4-14a OQ2).
- **D35 `WicCodecSupport`** has one home, `pr_imaging::webp_heic`; `wic::availability` re-exports it
  (`pr-platform-win` depends on `pr-imaging`).
- **D36 `windows` crate** 0.62 (+ `windows-core`) is a workspace dependency used only by `pr-platform-win`
  (`#[cfg(windows)] pub mod wic`); features: Foundation, Graphics_Imaging, System_Com, System_Com_StructuredStorage,
  System_Variant, System_Ole, Media_MediaFoundation (P-22).
- **D37 `unsafe` in pr-imaging:** only `pixel_memory` (`std::alloc`, 64-byte aligned, one SAFETY comment per block), as the
  plan row says. Everything else in `pr-imaging` stays safe; the WIC FFI is in `pr-platform-win`.
- **D38 Oracle wiring:** `tools/oracle.ps1` also builds `PhotoReview.Imaging` from the archive; `OracleCs.csproj` references
  it and sets `UseWPF` (fixture encoders), `AllowUnsafeBlocks` and the three implicit usings WPF removes. Internals are
  reached by reflection; the C# repo is not touched. In golden inputs the source kind key is `source_kind` (`kind` is the
  case discriminator of `image-cache-key`).
- **D39 Fixtures:** `tests/fixtures/embedded-thumbnail/` and `tests/fixtures/wic-direct/` (235 KB) are generated by the
  oracle (`embedded-thumbnail-fixtures`, `wic-direct-fixtures`) and committed; they were byte-identical across two runs.
  Pixel hashes of real WIC output depend on the Windows build: the native Rust tests compare them only when
  `windowscodecs_version` matches the golden, otherwise per sample with tolerance 3 (W4-11c T11).
- **D40 Determinism:** all 70 goldens were produced twice and compared byte for byte. The two that differed
  (`thumbnail-disk-decode`: temp path in an error message; `preview-disk-cache-path`: source-kind key) were fixed in the
  oracle/inputs and re-checked. No pre-existing golden changed.
- **D41 No `pr-l10n` dependency in `pr-imaging`:** localized sentences are injected (the D13 pattern: `UserText` closure or
  texts trait supplied by the composition root). Applies to W4-10a, W4-12c, W4-14.
- **D42 Test ownership that no draft claimed:** `EmbeddedThumbnailMutationTests.MutatedApp1_NeverThrows` -> W4-02b;
  `WicPixelScalerTests` -> W4-11e (the W4-16a/16b/08b drafts also listed it); `WicDirectFactoryFailureTests` (5) -> W4-11a
  (`create_factory`/`check_factory_result` are owned there); `FallbackExceptionCoverageTests` -> W4-12b;
  `PreviewCacheCrossVersionTests.NewPngWrite_MatchesOldPngWrite` -> a native W4-11e test on bytes captured by the oracle;
  `ThumbnailCacheUnobservedFaultTests` (3) have no Rust twin (no unobserved task exceptions), replaced by a Rust-only test;
  the `ReadGcMemoryInfo_*` twin lives in `pr-platform-win` next to `WindowsMemoryProbe` (W4-07b).
- **D43 New unit W4-14d:** `impl PreloadTarget for PreviewImageService` and the 8 `PreloadSchedulerTests` (after W4-14b and
  W4-15b). The `PreviewError -> DecodeError` mapping for `PreloadTarget::preload` is rebuilt per variant (`DecodeError` is
  not `Clone`). `PreviewError::DiskEntry(Arc<PreviewCacheError>)` replaces `DiskEntryCanceled` so the `OutOfMemory` of
  W4-08b reaches the caller as in C#.
- **D44 Scheduler thread model (W4-15b):** the first steps of a run execute on the caller thread, then one
  `pr-preload-run` thread per run plus a fixed pool of `max(1, workers)`; the snapshot callback runs under the gate and must
  not call back into the scheduler. `build_preload_order` returns an eager `Vec<i32>` (measure in Wave 8).

To apply at porter kick-off (not wired here because they change a unit's behaviour; the porter of the named unit owns it):

- **A1 (W4-01c, needed by W4-11c and W4-12c):** `DecodeError` gets `user_text: Option<UserText>` on `NotSupported` and
  `MissingCodec` plus `not_supported_localized`/`missing_codec_localized`/`describe`/`is_localized`.
- **A2 (W4-01c, needed by W4-12b):** `DecodeError::WithSourceBytes { error: Box<DecodeError>, bytes }` with
  `attach_source_bytes`/`source_bytes`/`kind`, so `DecodeFailureSourceBytes` (C# `Exception.Data`) survives the fallback.
- **A3 (W4-04b):** `exif_query::as_integer` becomes `pub` (W4-11d needs it from `pr-platform-win`).
- **A4 (W4-15a):** `pr_core::diagnostics::diag_options::parse_preload_workers` becomes `pub`; `policy::parse_diag_workers`
  calls it.
- **A5 (W4-09):** `validate_search_pattern`, the UTF-16 pattern matcher and a lazy lister become `pub` in
  `pr_core::io::physical` (additive, W2-14); otherwise W4-09 records a DIVERGENCE at the stale sweep.
- **A6 (W4-11a):** `ensure_com_initialized()` (MTA) is called by `create_factory`, so every WIC chain runs in an MTA as on
  CLR threads; W4-11b/c/e do not initialise COM themselves.
- **A7 (W4-14b):** `pr_core::sync::CancelToken::wait_timeout` (condvar) replaces the 10 ms polling; until then polling.
- **Parity choices kept (INV-1, no behaviour change in this wave):** `FallbackImageDecoder` and the WebP/HEIC router do
  not forward `as_cancellable` (C# parity); the relabel wrapper drops `source_bytes_read` and `embedded_preview_size`;
  the 0x0 underflow quirk of `AdaptivePreviewPolicy` and the negative-window quirks of `PreloadWindow` stay (QUIRK-*
  goldens); `SourceBytesCache` buffers are zeroed and an allocation failure aborts (record in PARITY I-10).

## Wave 5 roster (RAW, TurboJPEG, LibRaw; dependency-graph scheduling, P-28)

Source of truth for the original scope: `docs/MULTI-AGENT-PLAN.md` section 3 (rows W5-01..W5-17, split into sub-units by the contract writers, P-18).
This roster records the port plan; it is not a live list of stubs or remaining work. Contracts are in `contracts/W5-*.md`,
oracle commands in `tools/oracle-cs/Commands/` (except W5-13b and W5-15a, host-dependent FFI gates without a golden), with
inputs in `tests/golden-inputs/` and outputs in `tests/golden/`. Counts are golden cases. The tier ordered the original schedule;
current implementation and validation status are recorded in `docs/PARITY.md` and the W5 final status below.

### Tier A: leaves (no Wave 5 dependency)
| WU | Unit | Rust (`pr_imaging_raw::` unless noted) | Porter | Goldens |
|---|---|---|---|---|
| W5-01a | RAW vocabulary: `RawFormat`, `RawFileTypes`, `RawContainerInfo`, limits | `formats` | Haiku | `raw-formats` (135) |
| W5-01b | Contract types: `EmbeddedPreview`, `ExifBlock`, header traits, header error, `InMemoryRawHeaderSource` | `contracts` | Haiku | `raw-contract-records` (12), `raw-header-budget` (12) |
| W5-02b | `JpegMarkerProbe` | `jpeg_probe` | Sonnet | `jpeg-marker-probe` (86) |
| W5-10a | `BmffBoxNavigator` | `bmff` | Sonnet | `bmff-navigator` (127) |
| W5-12a | `SourceRawHeaderSource` (64 KiB blocks, 8 MiB header budget) | `header_source` | Sonnet | `raw-header-source-io` (27) |

| WU | Unit | Rust (`pr_turbojpeg::`) | Porter | Goldens |
|---|---|---|---|---|
| W5-13a | TurboJPEG FFI: run-time loading, `TurboJpegHandle`, `unsafe` | `native` | Sonnet (P-34) | `turbojpeg-native-types` (46), `turbojpeg-native-handle` (23), `turbojpeg-native-decode` (146) |
| W5-14a | `PixelAreaResampler` | `area_resampler` | Haiku | `pixel-area-resample` (78) |

| WU | Unit | Rust (`pr_libraw::`) | Porter | Goldens |
|---|---|---|---|---|
| W5-16a | `FullDecodeGate` (process-wide admission, INV-8) | `full_decode_gate` | Sonnet (P-34) | `full-decode-gate` (30) |
| W5-16b | `DecodeMemoryGuard` | `memory_guard` | Sonnet | `libraw-memory-guard` (359) |
| W5-17 | `RgbBgraResampler` | `resampler` | Haiku | `rgb-bgra-resample` (176), `rgb-bgra-validate` (113) |

### Tier B: shared parsers and gates (depend on Tier A)
| WU | Unit | Rust (`pr_imaging_raw::` unless noted) | Porter | Goldens |
|---|---|---|---|---|
| W5-02a | `TiffHeaderNavigator` (`reconcile_jpeg_size` takes the frame walker as a closure, D47) | `tiff_nav` | Sonnet | `tiff-navigator` (538) |
| W5-11b | `PreviewSelector`, `try_read_jpeg_frame`, `is_adobe_rgb_exif` | `preview_selector` | Sonnet | `preview-selector-select` (160), `preview-selector-jpeg-frame` (142), `adobe-rgb-exif` (61) |

| WU | Unit | Rust (`pr_turbojpeg::`) | Porter | Goldens |
|---|---|---|---|---|
| W5-13b | `TurboJpegAvailability` probe (host-dependent, no golden) | `availability` | Sonnet | - |

| WU | Unit | Rust (`pr_libraw::`) | Porter | Goldens |
|---|---|---|---|---|
| W5-15a | LibRaw FFI: run-time loading, `LibRawHandle`, layout proof, progress callback (no golden) | `native` | Sonnet (P-34) | - |
| W5-15b | `LibRawAvailability` probe (merge with W5-15a as one pair) | `availability` | Sonnet | `libraw-version` (162) |

### Tier C: container readers and consumers of Tier B
| WU | Unit | Rust (`pr_imaging_raw::` unless noted) | Porter | Goldens |
|---|---|---|---|---|
| W5-03 | `Cr2ContainerReader` | `readers::cr2` | Haiku (Sonnet if `bytes_read` parity does not converge) | `cr2-container` (153), `cr2-container-corpus` (0) |
| W5-04 | `NefContainerReader` | `readers::nef` | Haiku (escalate to Sonnet at once on queue-order / maker-note / crop failures) | `nef-container` (246) |
| W5-05 | `OrfContainerReader` | `readers::orf` | Haiku (Sonnet after one failed round) | `orf-container` (233) |
| W5-06 | `DngContainerReader` (rated yellow; the plan said Haiku) | `readers::dng` | Sonnet | `raw-dng-synthetic` (258), `raw-dng-corpus` (1) |
| W5-07 | `ArwContainerReader` | `readers::arw` | Haiku (Sonnet after one failed round) | `raw-arw-reader` (181) |
| W5-08 | `Rw2ContainerReader` | `readers::rw2` | Sonnet | `rw2-container` (235), `rw2-corpus` (3) |
| W5-09 | `RafContainerReader` | `readers::raf` | Haiku | `raf-container` (123), `raf-can-read` (38), `raf-corpus` (3) |
| W5-10b | `Cr3ContainerReader` | `readers::cr3` | Sonnet | `raw-cr3-reader` (321) |
| W5-11c | `RawExif` | `raw_exif` | Sonnet | `raw-exif` (82), `raw-exif-corpus` (0) |

| WU | Unit | Rust (`pr_turbojpeg::`) | Porter | Goldens |
|---|---|---|---|---|
| W5-14b | `TurboJpegDecoder` (injected `Decompressor`, `FineScaler`, memory seams) | `decoder` | Sonnet | `turbojpeg-fixtures` (18), `turbojpeg-pure` (847), `turbojpeg-decode` (184), `turbojpeg-read-info` (43) |

| WU | Unit | Rust (`pr_libraw::`) | Porter | Goldens |
|---|---|---|---|---|
| W5-16c | `LibRawDecoder` (seam traits + `NativeLibRaw` adapter) | `decoder` | Sonnet (P-34) (verifier Sonnet) | `libraw-decoder` (155), `libraw-decoder-native` (35) |

### Tier D: registry and routing
| WU | Unit | Rust (`pr_imaging_raw::` unless noted) | Porter | Goldens |
|---|---|---|---|---|
| W5-11a | `RawContainerReaderRegistry`, `RawJpegIccProfile` (asset `assets/AdobeCompat-v2.icc`) | `registry` | Haiku | `raw-registry-find` (49), `raw-icc-ensure` (44) |
| W5-12c | `FormatRoutingDecoder` | `routing` | Sonnet | `raw-routing` (116) |

### Tier E: `RawDecoder`
| WU | Unit | Rust (`pr_imaging_raw::` unless noted) | Porter | Goldens |
|---|---|---|---|---|
| W5-12b1 | `RawDecoder` core: `read_info`, `decode`, preview range reads | `decoder` | Sonnet (verifier Sonnet: INV-1 identity, INV-8) | `raw-decoder-core` (50) |
| W5-12b2 | `RawDecoder` fallback chain (child module) | `decoder::fallback` | Sonnet | `raw-decoder-fallback` (75) |

Original dependency graph (direct, merge order inside a crate): 01a + 01b (one type cycle, merge together) -> 02a, 02b, 10a, 12a, 11b ->
readers 03..09 and 10b (each needs 02a/02b and, for the size-reconcile tests, 11b), 11c -> 11a (registry names every reader) ->
12c, 12b1/12b2 (12b needs 11a, 11b, 11c, 12a). `pr-turbojpeg`: 13a, 14a -> 14b; 13b needs 13a. `pr-libraw`: 16b -> 15a + 15b;
16a, 16b, 17, 15a -> 16c. The three crates were independent of each other except for the `pr-libraw` dev-dependency on
`pr-imaging-raw` (W5-16c native twin). Some Wave 5 contracts retain historical draft paths; `contracts/drafts-w5/<unit>/...`
is not the current source location. Implementations are at their crate paths, oracle commands at
`tools/oracle-cs/Commands/<unit>.cs`, and inputs under `tests/golden-inputs/`.

## Lead decisions (Wave 5)

Decided and wired in this PR (the stubs show the result; D28 applies):

- **D45 One owner for the shared RAW types:** W5-01b (`contracts`) owns `EmbeddedPreview`, `ExifBlock`, `EmbeddedPreviewKind`,
  `PreviewColorSpace`, the `RawHeaderError` enum (`InvalidData { message, io }`, `ArgumentOutOfRange`, `Canceled`) with the
  method `is_io_failure()`, and `RawHeaderSource::read(&mut self, offset: i64, count: i32) -> Result<&[u8], RawHeaderError>`.
  W5-01a (`formats`) keeps `RawFormat`, `RawContainerInfo`, `RAW_EXTENSIONS`/`is_raw_extension` and `formats::limits`; its
  temporary copies of those types, `RawHeaderCause`, the struct `RawHeaderError` and the free `is_io_failure` were deleted.
  The crate root re-exports both. W5-01a's contract text about those types is superseded by this entry.
- **D46 Module paths:** limits are `crate::formats::limits`; the TIFF navigator is `crate::tiff_nav`; the marker probe is
  `crate::jpeg_probe`; readers are `crate::readers::*`. The draft names `crate::limits`, `crate::tiff::navigator`,
  `crate::tiff_navigator`, `jpeg_marker_probe`, `RawError`, `RawReadError` and `crate::header_source::RawHeaderError` were rewritten.
- **D47 Frame walker as a parameter:** `tiff_nav::reconcile_jpeg_size` takes the JPEG frame walker as a closure so W5-02a does
  not depend on W5-11b (which depends on the readers). Readers pass `preview_selector::try_read_jpeg_frame`. The synthetic
  reader tests that expect a JPEG frame size to win were scheduled after W5-11b; its implementation and reader tests are now present in the integrated W5 tree.
- **D48 Shared test builders:** pure byte builders (`TiffBytes`, `create_minimal_jpeg`, `SyntheticRawBuilder`, fuzz mutators,
  golden spec interpreter) live in `pr_test_support::raw` (no crate cycle); the first reader porter (W5-02a) creates them and
  adds the `SyntheticRawBuilder` round-trip test; all later units consume them. `InMemoryRawHeaderSource` stays in
  `pr-imaging-raw` (W5-01b). Each reader contract's "private copy in the test file" option is withdrawn.
- **D49 RAW corpus:** the corpus is not in the Rust repo, is not on this machine and downloading it needs the user's yes, so
  it was not fetched. Corpus tests read `PHOTOREVIEW_RAW_CORPUS_DIR` (default `tests/fixtures/raw-corpus/`, now in
  `.gitignore`) and skip by early return when a file is absent, unless `PHOTOREVIEW_RAW_CORPUS_STRICT` is `1`/`true` (then they
  panic naming the variable). The draft variable names `PHOTOREVIEW_RS_RAW_CORPUS` and `PHOTOREVIEW_RAW_CORPUS` are replaced by
  `PHOTOREVIEW_RAW_CORPUS_DIR` on the Rust side. The corpus goldens (`cr2-container-corpus`, `raw-dng-corpus`, `rw2-corpus`,
  `raf-corpus`, `raw-exif-corpus`, the corpus cases of `raw-arw-reader`, `raw-cr3-reader` and `libraw-decoder-native`) were
  generated without the corpus: zero cases, `present: false` or `skipped: true` for the corpus entries. Regenerate them once the
  corpus is fetched (proposal P-30).
- **D50 Native DLL loading:** `pr-turbojpeg::native` and `pr-libraw::native` declare `LoadLibraryExW`/`GetProcAddress`/
  `FreeLibrary` themselves in a private `#[cfg(windows)]` block (two ~10-line copies, no `libloading`, no `windows` feature, no new
  dependency). Hoist one shared copy later if a third site appears. Native tests are `#[ignore = "native"]` and load the DLL by
  absolute path from `native/x64/`.
- **D51 Seam ownership in `pr-libraw`:** `RawStructure` is defined in `memory_guard` (W5-16b) and re-exported by `native`;
  `ProcessedImageHeader`, `IMAGE_*`, the progress callback, `set_cancel_token`, `progress_verdict` and the `CheckCancellation_*`
  tests are W5-15a; the seam traits `LibRawApi`/`LibRawSession`/`ProcessedImage` and the `NativeLibRaw` adapter are W5-16c
  (`decoder.rs` has no `unsafe`). Sub-unit names: W5-16a = `FullDecodeGate`, W5-16b = `DecodeMemoryGuard`, W5-16c = `LibRawDecoder`
  (two sibling contract headers swap the labels; the file names are right).
- **D52 `pr-turbojpeg` seams:** `Decompressor::*` returns `Result<i32, DecodeError>` so P/Invoke exceptions propagate (INV-12);
  `compress8` and `create_compressor` stay in `native` and `TurboJpegSyntheticFormatTests` are ported natively (no extra oracle
  command); the fine scaler is injected (`FineScaler`) and its production adapter over `pr_platform_win::wic::scaler` is written in
  the composition root (W8); native tests find `native/x64/turbojpeg.dll` by absolute path (W5-13b's "next to the test exe"
  wording is superseded). W5-14a's note that the decoder calls `area_resampler::resize` is wrong: at `d92d6aae` the C# decoder
  fine-scales with the WIC scaler.
- **D53 `RawDecoder` split:** W5-12b is two sub-units, `decoder.rs` (12b1) and the child module `decoder/fallback.rs` (12b2, an
  `impl RawDecoder` block reading the private fields). W5-12b1 must also list the integration test
  `FormatRoutingDecoder_RoutesRawExtensionWhenEnabled` (router over a real `RawDecoder`). The integration tests
  `PreviewService_DoesNotPreReadWholeRawFileIntoSourceCache` and `RawPreloadBudgetSlowTests` have no owner: they go to a Wave 5
  integration unit W5-18 (proposal P-31).
- **D54 Parity choices kept:** `FormatRoutingDecoder` keeps `as_cancellable() == None` and takes a `RawRoutingTexts` trait (the
  `WebpHeicTexts` pattern); the range-end overflow in `PreviewSelector` reproduces C# wrap-around (3 golden cases flip if
  saturating arithmetic is chosen instead); `SourceRawHeaderSource` retries `ErrorKind::Interrupted` (Rust-only, as std
  `read_exact`); the p95 timing assert of `Fuzz_Cr2ContainerReader_Robust` is dropped (AGENTS.md bans timing asserts) and recorded as
  a DIVERGENCE by the porter; cancellation of a queued `FullDecodeGate` waiter polls every 10 ms until A7.
- **D55 Oracle wiring:** `tools/oracle.ps1` also builds `PhotoReview.Imaging.Raw`, `.TurboJpeg` and `.LibRaw` from the archive into
  the core folder, copies the git-ignored `native/x64/*.dll` of the C# checkout into the build tree (no tool downloads anything)
  and copies `turbojpeg.dll`/`libraw.dll` next to `oracle-cs.exe`; `OracleCs.csproj` references the three assemblies. Internal
  classes are reached by reflection; the C# repo is not touched. Oracle fixes found by compiling: a CR inside a doc comment of
  W5-11c, missing usings in W5-14b and W5-16c.
- **D56 Golden input fixes found by running the oracle:** patches that ran past the end of the data in `orf-container`
  (`note-with-1-bytes-before-the-end-of-file`, clipped to the file end) and `raw-exif` (`block-at-offset-clamped-by-source-end`);
  a JS-rounded `i64::MAX` in `raw-header-source-io` (`9223372036854776000` -> `9223372036854775807`); `truncate_to` 1100 > file
  size 755 in `raw-cr3-reader` (`real-layout-truncated-in-the-middle-of-the-track-jpeg`, now 673 = middle of the 163-byte track
  JPEG); the 3 corpus cases of `raw-arw-reader` are `optional`; `raf-corpus` records `present: false` without a corpus instead of
  throwing.
- **D57 Cargo:** `pr-imaging-raw`, `pr-turbojpeg` and `pr-libraw` get `[dev-dependencies]` `pr-test-support` and `serde_json` (so
  parallel porters never edit a manifest); `pr-imaging-raw` and `pr-turbojpeg` get `pr-platform-win` as a `cfg(windows)`
  dev-dependency (native twins, no cycle), `pr-libraw` gets `pr-imaging-raw` (W5-16c). No new external crate; `sha2` is not added
  (D33).
- **D58 PARITY ids** used by the Wave 5 rows: I-02, I-03, I-13, I-14, I-15, INV-8, INV-12; new rows `X-raw-limits`,
  `X-preview-selector`, `I-02b` (TurboJPEG native layer) and `I-15b` (LibRaw native layer).

To apply at porter kick-off (not wired here because they change a unit's behaviour; the porter of the named unit owns it):

- **A8 (W5-12b, W4-01c, W4-11c): CONFIRMED A (D59).** `DecodeError::NotSupported` retains an optional directly wrapped COM HRESULT;
  WIC ICC failures preserve it when their cause is COM. The three `requires:[inner_com_hresult]` fallback golden cases now run.
- **A9 (W5-12b):** fix the doc comment of `RawPreviewFallback` (W5-01b): `RawDecoder` classifies thumbnail failures with its own
  `IsRecoverablePreviewFailure` list (InvalidCast, Overflow and DllNotFound are not recoverable there), not with the W4-12b whitelist.
- **A10 (W5-16a):** `CancelToken::wait_timeout` (A7) replaces the 10 ms cancellation polling inside `wait_for_grant`.
- **A11 (W5-13a, W5-15a):** a lead-owned `tools/stage-native.ps1` (SHA-256 pin check; copies `turbojpeg.dll`, `libraw.dll` and the
  licence files next to the test and app executables) and a Rust-side `tools/fetch-native.ps1`; CI runs it before `--include-ignored`.

## Wave 6 roster (Windows platform)

| WU | Unit | Rust | Porter | Tests |
|---|---|---|---|---|
| W6-01a | Explorer COM interop and shell window source | `explorer_com_interop` | Sonnet | `contracts/W6-01a.md`; fake COM ownership/vtable tests |
| W6-01b | Explorer window selection and location policy | `explorer_window_selector` | Sonnet | `contracts/W6-01b.md`; fake iterator/window tests |
| W6-01c | Explorer STA provider, timeout and prefetch lifecycle | `explorer_order_provider` | Sonnet | `contracts/W6-01c.md`; deterministic seam tests, native smoke ignored |
| W6-01d | Explorer native view read and item ordering | `explorer_native_view` | Sonnet | `contracts/W6-01d.md`; fake-vtable tests |
| W6-02a | Recycle eligibility and permanent-delete guard | `recycle_eligibility` | Sonnet | `contracts/W6-02a-recycle-eligibility.md`; injected drive/mount fakes; depends on 02b query adapter |
| W6-02b | Recycle settings, shared volume queries, and capacity verdict | `recycle_bin_settings`, `recycle_bin_capacity` | Sonnet | `contracts/W6-02b-recycle-capacity.md`; registry/volume fakes; 02a consumes its mount-point seam |
| W6-02c | Windows Recycle Bin facade, guarded send/delete, and restore delegation | `recycle_bin` | Sonnet | `contracts/W6-02c-recycle-bin-facade.md`; fake-first action tests; restore backend injected by 02e |
| W6-02d | Recycle item matching and restore-verb selection | `recycle_candidate`, `restore_verb` | Sonnet | `contracts/W6-02d-recycle-restore-selection.md`; pure tests |
| W6-02e | Shell restore orchestration and bounded native integration | `recycle_bin_shell`, `recycle_restore` | Sonnet | `contracts/W6-02e-recycle-restore-com-and-native.md`; native tests only clean their own items |
| W6-03a | Instance mutex key generation | `instance_keys` | Sonnet | `contracts/W6-03a.md`; deterministic path tests |
| W6-03b | Forward pipe naming and ACL | `forward_pipe` | Sonnet | `contracts/W6-03b.md`; deterministic pipe names + bounded native ACL checks; consumes 03a |
| W6-03c | Forward server listener | `forward_server` | Sonnet | `contracts/W6-03c.md`; bounded named-pipe tests |
| W6-03d | Forward client operation | `forward_client` | Sonnet | `contracts/W6-03d.md`; fake outcomes + bounded client/server cases |
| W6-03e | Instance ownership scope coordinator | `instance_scope` | Sonnet | `contracts/W6-03e.md`; fake locks/forwarder + bounded native mutex cases |
| W6-04a | `WindowsDisplayClock` worker/state machine | `display_clock` | Sonnet (P-34) | `contracts/W6-04a.md`; deterministic fake-driven behavioral twins |
| W6-04b | `IVBlankDriver`, D3DKMT vblank driver, DWM fallback | `vblank_driver` | Sonnet (P-34) | `contracts/W6-04b.md`; pure ABI checks + one ignored native DWM smoke |
| W6-05 | Windows memory probe, natural comparer, dark title bar, live-operation registry | `memory_probe`, `natural_comparer`, `dark_title_bar`, `live_operation_registry` | Haiku (unsafe review by Sonnet) | `contracts/W6-05.md` |

W6-02b owns the shared volume/mount-point query adapter; W6-02a consumes it, W6-02c consumes both policy/settings units, and W6-02e implements the restore backend injected by W6-02c. W6-02d remains independent. `lib.rs` wiring is lead-owned for all W6-02 units.

W6-03a generates mutex names; W6-03b derives paired pipe names and ACLs; W6-03c/d consume the pipe contract; W6-03e composes keys, server, and client. W6-04b depends only on the existing `DisplayTiming` contract and is independently implementable. W6-04a depends on W6-04b's driver trait plus the existing `DisplayClock`, `DisplayTiming`, and `VBlankEstimator`; it owns no Core abstractions. The parent contract records the split and dependency.

## Wave 7 roster (benchmarking, `pr-bench`)

Dependency order (P-28): W7-01a/01b and W7-02a first; W7-02b after 02a; W7-03a after 01; W7-03b after 01 and the pr-imaging service;
W7-03c after 03a and 03b; W7-03d after 03c (its `EngineBackend` is lead-owned, see D69).

| WU | Unit | Rust (`crates/pr-bench/src/`) | Porter | Golden |
|---|---|---|---|---|
| W7-01a | BenchmarkModels (profile validation, phase statistics, ranking, report/batch JSON) | `models` | Sonnet | `bench-profile`, `bench-phase`, `bench-rank`, `bench-json` |
| W7-01b | BenchmarkProfiles (All/Runnable/Find), BenchmarkMetrics.Since | `profiles`, `metrics` | Haiku | `benchmark-profiles`, `benchmark-metrics-since` |
| W7-02a | SlowLinkFileSystem, SharedBandwidthLimiter, ThrottledReadStream | `slow_link_fs` | Sonnet | `slow-link-fs` |
| W7-02b | SlowLinkSourceReader, BenchmarkRecycleBin (temp-copy deleter) | `slow_link_reader`, `bench_recycle_bin` | Sonnet | `slow-link-source-reader`, `bench-recycle-bin` |
| W7-03a | BenchmarkEngine (prepared run, timing, progress) | `engine` | Sonnet | `bench-engine-run`, `bench-engine-format-ms` |
| W7-03b | BenchmarkImageExecutor, WindowedPreloadTarget, ProfileScope | `image_executor` | Sonnet | `bench-profile-scope`, `bench-windowed-target` |
| W7-03c | BenchmarkWorkloadRunner (seeded Random, file selection, ImagesPerSample) | `workload_runner` | Sonnet | `bench-random`, `bench-select-indices`, `bench-images-per-sample`, `bench-prepare-edge` |
| W7-03d | Benchmark CLI (dispatch, `--benchmark*`, list-profiles, usage, arguments, ToolPathGuard) | `cli`, `main.rs` | Sonnet | `cli-arguments`, `cli-tool-path-guard`, `cli-dataset-plan` |

## Lead decisions (Wave 7)

The recommended option of every open question in the eight contracts was taken (standing user instruction: never stop to ask);
they are logged as P-35..P-37 in `docs/PROPOSED-DECISIONS.md`.

- **D60 Wiring done by the lead:** `crates/pr-bench/src/lib.rs` declares all modules (`cli` is a lib module, `main.rs` is the thin binary);
  `Cargo.toml` adds `pr-platform-win` (production memory probe for the executor, W7-03b Q2) and dev-dependencies `pr-test-support`, `serde_json`.
  The stubs compile together clippy-clean as delivered; the stub-only `#![allow(unused_variables, dead_code)]` lines stay until each porter
  removes them with the last `todo!()`.
- **D61 Name reconciliation:** the validation error type is `BenchmarkProfileError` (W7-01a); `engine.rs` was changed from the assumed
  `BenchmarkProfileException`. The W7-03a compile shim and the W7-03c compile-check are not merged. `BenchmarkTimestamp` stays in `models.rs`.
- **D62 Lookup seam (W7-01a Q1):** `effective_images_per_sample`, `p95_per_image`, `p50_per_image`, `rank` and `to_json` take an
  `ImagesPerSampleLookup` callback, so `models.rs` does not depend on `profiles`/`workload_runner` and merge order does not matter.
- **D63 Profile fields:** `BenchmarkProfile` fields are the snake_case mirror of the C# record; `since()` returns `Result` (duplicate baseline
  keys, as C# `ToDictionary` throws); `runnable()` returns clones. `TargetWidth` belongs to W7-01a.
- **D64 R0002 names:** env var `PHOTOREVIEW_RS_BENCH_REAL_RECYCLE_BIN` (Different row, D4 rule); benchmark scratch parent
  `PhotoReviewRs-Benchmark-Cache`; the CLI `--cache-dir` guard protects both `%LOCALAPPDATA%\PhotoReviewRs` and `\PhotoReview`.
- **D65 Visibility:** `TimeSource` and `ThrottledReadStream` stay `pub` (external test crate and W7-02b use them). The `FileSystem` trait method
  `try_get_file_stat` (P-12) delays once like `get_file_stat`; a C# baseline sync that adds it must do the same.
- **D66 Executor:** sync trait `BenchmarkPreparedWorkloadExecutor`; `BenchmarkExecutorOptions::default().physical_memory_bytes == 0` (the host
  passes `ram_budget::get_physical_memory_bytes`; revisit in W7-03c); `decoder: None` is `NoDecoder`; `dispose` keeps the unbounded
  `shutdown_persist_workers()` (parity). Teardown threading is not differentially testable (fake-decoder Rust tests only).
- **D67 Accepted divergences** (tests carry `// DIVERGENCE:`): the parallel-step error is the lowest index, not the first in time; an empty file
  list is `InvalidArgument` at prepare; workload errors are a `WorkloadError` boxed in `BenchmarkError::Operation` (tests `downcast_ref`); a setup
  failure is per-profile in Rust but aborts the batch in C#; the CLI DONE line uses invariant culture (a C# `vi-VN` machine prints a comma);
  progress lines print synchronously in order (C# `Progress<T>` is asynchronous); console output is UTF-8; eager `Vec` listing instead of the
  lazy C# iterator.
- **D68 CLI scope:** only the benchmark modes (`--benchmark`, `--benchmark-all`, `--benchmark-actions`, `list-profiles`, usage). The ~11 other tools
  of `PhotoReview.Benchmark.Cli` (perf-analyze, compare/export, decoder-bench, io-decode-split, raw-survey, perf-session, startup/ui-next/explorer
  probes, preload-bench) go to `BenchmarkBackend::run_other_mode` (exit 2, "not available"). Proposed W7-03e (perf-analyze + NavSamplesCsv) and
  W7-03f (compare/export); the WPF-bound tools go to W8 or are dropped (P-36).
- **D69 CLI seam:** `BenchmarkBackend` trait; the lead writes `EngineBackend` after W7-01..W7-03c merge and swaps it for `UnwiredBackend` in `main.rs`.
- **D70 Oracle wiring:** `PhotoReview.Benchmarking.dll` is built from the baseline archive into the shared output folder (it pulls
  `Platform.Windows` in); the two internal CLI classes (`BenchmarkCliArguments`, `ToolPathGuard`) are compiled from the baseline source through
  `-p:CSharpSrcDir`. The `cli-process` golden (runs the baseline WPF-bound Cli exe) is NOT generated: its input was dropped and `CliProcess` is not
  dispatched; process-level behaviour is covered by hand-written expectations (P-37).

## Wave 8 batch B1 roster (pure leaves and goldens, `pr-app` / `pr-platform-win`)

Batch B1 of `docs/WAVE-8-PLAN.md` (P-38): 35 contracts (`contracts/W8-*.md`), stubs under `crates/pr-app/src/` (and `crates/pr-platform-win/src/shell_viewing/`),
22 oracle files (`tools/oracle-cs/Commands/W8-*.cs`) and 50 goldens (`tests/golden/`, inputs in `tests/golden-inputs/`). Ports start per P-28 when the direct
dependencies merge (the plan's Depends-on column plus the missing edges in D-W8B1-9). Nothing in B1 is ported yet: every stub body is `todo!()`.

| WU | Unit | Rust | Porter | Golden (cases) |
|---|---|---|---|---|
| W8-01a | Viewport contracts, snapshot, state, ArrowPanRule + Input primitives + ViewerStretchMode | `viewport::{types, state, arrow_pan}`, `input::primitives` | Haiku | `viewport-state` 11, `arrow-pan` 26, `viewport-snapshot-text` 6, `viewport-values` 17 |
| W8-01b | ViewportLayoutEngine | `viewport::layout` | Haiku (Sonnet if the net_max/-0 traps fail twice) | `viewport-layout-engine` 361 |
| W8-01c | RenderGeometry (pure) | `viewport::render_geometry` | Haiku | `render-geometry` 133 |
| W8-01d | FitViewController, ZoomMenuRules | `viewport::fit`, `menus::zoom_rules` | Haiku (Sonnet if the trace tests fail twice) | none (F) |
| W8-02a | ContextMenuModelBuilder, MenuContracts | `menus::{model, builder}` | Haiku | `menu-model` 196 |
| W8-02b | Exif/Status/TitleBar formatters, SettingsReloadPath | `viewmodels::format::*` | Haiku (`context.rs` to Sonnet if the .NET number formats fail twice) | `format-exif` 104, `format-status` 109, `format-title-bar` 39, `settings-reload-path` 216 |
| W8-03a | Windowing contracts, monitor layout, placement visibility, JSON store | `windowing::{contracts, monitor_layout, visibility, placement_store}` | Sonnet | `window-placement-rules` 66, `-visibility` 37, `-json` 111 |
| W8-03b | FullscreenController over `WindowOps` | `windowing::fullscreen` | Sonnet | none (F) |
| W8-04a | Localization service, language options, translation problems | `localization::{language_options, source, service, translation_problems}` | Haiku | `language-options` 7, `localization-service` 35, `translation-problems` 12 |
| W8-04b | TranslationExport | `localization::export` | Haiku | `translation-export` 39 |
| W8-04c | App services (clipboard/observer/prompt contracts, prewarm, preview state, frame trace, ShortcutKeyName, ViewportSizeSource, InitialViewportPredictor) | `services::*` | Sonnet | none (F + committed `key-names.v1.json`) |
| W8-04d | AppLog, BuildInfo, LaunchArguments, TaskLogging | `misc::{app_log, build_info, launch_args, task_logging}` | Haiku | none (F) |
| W8-04e | FileHashService, MainWindowConvergence, MainWindowHelpers | `misc::{file_hash, viewport_convergence, window_helpers}` | Sonnet | none (F) |
| W8-05a | FrameClock, ViewportOperationVersion, WheelMessageSource (pure), IImageSurface, ReviewCommand | `input::{frame_clock, operation_version, wheel_message, surface, review_command}` | Haiku | none (F) |
| W8-05b | KeyId | `input::key_id` | Haiku | `key-id` 886 |
| W8-05c | KeyIdMapping | `input::key_mapping` | Haiku (Sonnet if traps 1-4 fail) | `key-id-mapping` 762 |
| W8-05d | ShortcutRouter | `input::shortcut_router` | Sonnet | `shortcut-router` 9604 |
| W8-05e | MouseGestures, TouchpadGestures | `input::{mouse_gestures, touchpad_gestures}` | Haiku | `mouse-gestures` 235, `touchpad-gestures` 158 |
| W8-05f | KineticPan | `input::kinetic_pan` | Sonnet | `kinetic-velocity` 20, `kinetic-scroller` 31, `keyboard-pan` 47, `glide-clock` 14 |
| W8-06a | ViewerState | `viewmodels::viewer_state` | Sonnet | `viewer-state-trace` 31, `viewer-step-zoom` 33, `viewer-display-size` 17 |
| W8-06b | InfoOverlayViewModel, auto-hide policies, FadeTargetGate, ImageTransitionDecision | `viewmodels::info_overlay`, `coordinators::{autohide, fade_gate, transition}` | Haiku (Sonnet if the 12-state view model fails twice) | `info-overlay-view-model` 23, `info-overlay-autohide` 107, `toolbar-autohide` 91, `fade-target-gate` 8, `image-transition` 108 |
| W8-06d | Coordinator traits and adapters | `coordinators::traits` | Haiku | none (F) |
| W8-10a | ActionListJson, ActionProfilesPresenter | `presenters::{action_list_json, action_profiles}` | Haiku (`action_list_json.rs` to Sonnet if the escaping fails twice) | `action-list-json` 26, `action-profiles` 38, `action-profiles-io` 13 |
| W8-10b | SettingsFieldMap | `presenters::settings_field_map` | Sonnet (contract Q2) | `settings-field-map` 617 |
| W8-10c | SettingsFormState, secondary dialog presenters | `presenters::{settings_form_state, secondary}` | Haiku | none (F) |
| W8-10f | RecoveryFilter, RecoveryPresentation | `presenters::{recovery_filter, recovery_presentation}` | Haiku | `recovery-presentation` 48 |
| W8-10h | BenchmarkPresentation | `presenters::benchmark_presentation` | Haiku | `benchmark-presentation` 94 |
| W8-12a | Render contracts (traits), TiledGpuImage | `services::render::*` | Haiku | none (F) |
| W8-15a | Overlay contracts, model, element template, layer | `overlay::{contracts, model, element, layer}` | Haiku | none (F) |
| W8-15b | Animator, FrameDelayTimer, DarkPalette | `overlay::{animator, frame_delay, palette}` | Haiku | `overlay-animator` 27, `overlay-frame-delay` 10, `overlay-palette` 17 |
| W8-15c | ButtonElement, TextElement | `overlay::{button, text}` | Haiku | none (F) |
| W8-15d | PanelElement, CapturePairBadge, FolderInfoPanel | `overlay::{panel, capture_badge, folder_info}` | Haiku | none (F) |
| W8-15e | StatusPanel, ToolbarPanel, ToolsMenu, ZoomIndicatorPanel | `overlay::{status, toolbar, tools_menu, zoom_indicator}` | Haiku | none (F) |
| W8-16a | ScalingPolicy, ImageLayer, FrameStats, FramePacer, FrameLog | `pr_app::viewing::{scaling_policy, image_layer}`, `pr_platform_win::shell_viewing::{frame_stats, pacer, frame_log}` | Haiku | `scaling-policy` 17, `frame-stats` 12, `frame-pacer` 13, `frame-log` 10 |
| W8-16e | ScrollBarGeometry, ScrollBarNative | `pr_app::viewing::scrollbar_geometry`, `pr_platform_win::shell_viewing::scrollbar_native` | Haiku | `scrollbar-geometry` 133, `scrollbar-repeat-timing` 145 |

## Lead decisions (Wave 8 batch B1)

The recommended option of every open question in the 35 contracts was taken (standing user instruction: never stop to ask), unless a decision below says otherwise;
logged as P-39 in `docs/PROPOSED-DECISIONS.md`.

- **D-W8B1-1 Crate layering (blocks B3):** `pr-app` still depends on `pr-platform-win` (the R0001 spike). The B1 stubs respect that edge: logic that needs `pr_app`
  types lives in `pr_app::viewing` (`scaling_policy`, `image_layer`, `scrollbar_geometry`); the std-only or OS-only parts stay in
  `pr_platform_win::shell_viewing` (`frame_log`, `frame_stats`, `pacer`, `scrollbar_native`). Before any B3 unit (W8-12b onward: Direct2D/DirectWrite
  implementations of the `pr_app::services::render` traits) a layering PR must reverse the edge (option A of the W8-12a contract: `pr-platform-win` depends on
  `pr-app`; `main.rs`, the spike and the composition move to a thin package such as `pr-shell`). Not done here: it moves existing code.
  **DONE (layering PR):** `pr-platform-win` now depends on `pr-app`; `main.rs`, the Phase 0 spike (feature `ui-spike`, bin `r0001-spike`) and `composition` (with its two test files) moved to the new package `crates/pr-shell`. To break the cycle `pr-app -> pr-bench -> pr-platform-win`, `pr-bench` no longer names `WindowsMemoryProbe`: `BenchmarkExecutorOptions::memory_probe` (and `BenchmarkSessionDeps::memory_probe`) is injected by the host, `None` fails closed (the answer `WindowsMemoryProbe` gives when the API fails). The shell passes the real probe when it wires the benchmark presenter (W8-09b/W8-17).
- **D-W8B1-2 Overlay element API:** W8-15a owns the shape. `ElementNode` (template with hooks, now with required `as_any`/`as_any_mut`) is what elements implement;
  `OverlayElement` is the host surface from a blanket impl. The W8-15c/15d stubs were retargeted from their assumed single trait to `ElementNode`
  (`measure_core` returns `SizeD`, `render_core(&self)`, leaves get `find_at`/`find`); `ElementError::EmptyId` and the `Result` constructors live in `element.rs`.
  NOT adopted from 15c: `Result`-returning measure and `OverlayLayoutContext::text_renderer_id`; the 15c porter must reconcile layout-cache identity with 15a (open item).
  W8-15e composites own their typed children (15e O1) while W8-15d `PanelElement` owns `Box<dyn ElementNode>` children: confirm before porting 15e.
- **D-W8B1-3 Shared types:** `RectD`, `PointD`, `SizeD`, `PointerButton`, `KeyModifiers` live in `input::primitives` (W8-01a owns the file; 05a adds tests only);
  `ViewerStretchMode` in `viewport::types` (01d no longer ports it); `ColorF`, `ImageInterpolation`, `Matrix3x2`, `PixelRect`, `TileSpec` in
  `viewport::render_geometry` (01c), re-exported from `services::render`; `KeyId` is W8-05b's (the W8-05c placeholder `key_id.rs` was dropped, the 05c tables compile against 05b).
- **D-W8B1-4 Index files:** every `mod.rs` and the `lib.rs` module list are lead-owned and generated from the files present (`input`, `viewport`, `menus`, `viewmodels(/format)`,
  `windowing`, `localization`, `services(/render)`, `misc`, `coordinators`, `presenters`, `overlay`, `viewing`). `pr-app` gets dev-dependencies `pr-test-support` and `serde_json`;
  `pr-platform-win` enables the `windows` features `Win32_UI_WindowsAndMessaging` and `Win32_UI_Input_KeyboardAndMouse` outside `ui-spike`. The new pr-app modules contain no `unsafe` and no `windows` code.
- **D-W8B1-5 Windowing:** the `FullscreenControl` trait of W8-03a was deleted (nothing used it; 03b's controller needs a `WindowOps` on every call). The native `Win32MonitorLayout` is owned by W8-13k.
- **D-W8B1-6 Oracle wiring:** `tools/oracle.ps1` builds `App.Shared`, `Shell.Interop`, `Shell.Rendering` and `Shell.Win32` (assembly `PhotoReview.dll`) from the baseline archive into `$coreDir`;
  `OracleCs.csproj` references them plus `Platform.Windows`, adds the App.Shared global usings and the `CommunityToolkit.Mvvm` 8.4.0 and `Microsoft.Extensions.DependencyInjection` 10.0.0 packages,
  and compiles the internal baseline files (ViewModels formatters, Windowing, Presenters `SettingsFieldMap`/`SettingsFormState`/`ActionListJson`, Shell.Win32 `Viewing/*`) through `-p:CSharpSrcDir`
  (CS0436 warnings where a type also exists in a referenced assembly: the compiled-in copy wins, same source). `Program.cs` sets `Commands.FormatInputsDir` (the `vi` catalog lives in `assets/languages`).
- **D-W8B1-7 Oracle and input fixes found by the first runs:** `format-status.json` held `9223372036854776000` (a Node float) where `long.MaxValue` is meant: now `9223372036854775807` and `-9223372036854775808`;
  `benchmark-presentation.json` had three cases with two `name` keys: the profile override is now `profileName` (the oracle reads it); `W8-05f.cs` and `W8-15b.cs` add `c.DeepClone()` (node already had a parent);
  `W8-10a.cs` passes `(object?)null` for `Open(null)` and imports `PhotoReview.App` and `Core.IO`; `W8-10f.cs` imports `System.Runtime.CompilerServices`; `W8-15b.cs` loads assembly `PhotoReview`.
  All 50 commands ran twice with byte-identical output; no pre-existing golden changed; no command was dropped.
- **D-W8B1-8 Behaviour decisions (golden wins):** ArrowPanRule ports the file as it is at `d92d6aae` (the equal-step rule; K-27 `Different` and an ADR are owed by the 01a/05f porters);
  `Math.Round` is ties-to-even (`round_ties_even`), .NET `F`/`N` formats round half away from zero; `Math.Min/Max/Clamp` NaN and `-0` rules are reproduced, never `f64::min/max`;
  diagnostics text uses the invariant culture; the Rust env var is `PHOTOREVIEW_RS_FRAME_LOG` (R0002, Different row; `tools/diag/stall-experiment.ps1` is not in this repo);
  NaN equals NaN in golden comparisons; a golden that disagrees with a hand-derived table in a contract wins.
- **D-W8B1-9 Missing plan edges (add to `docs/WAVE-8-PLAN.md` when a unit is scheduled):** 04c needs 05b and 03a; 05d needs 04c and 01a; 10a needs 04c, 05b, 04d; 10b needs 10a and 10c;
  07g needs 01d; 15b needs 12a and 15a; 15c needs 12a, 01a, 01c; 15e needs 02a; 06a needs 04e for its `MainWindowHelpers` tests.
- **D-W8B1-10 Plan-row corrections:** W8-01a covers 299 lines (adds `InputPrimitives` 45 and `ViewerStretchMode` 11); W8-05a adds `input::operation_version` and `input::wheel_message` (pure part;
  `WheelMessageSource.Current()` is W8-14a); W8-16a adds `frame_log`; W8-16e's pure geometry is in `pr_app::viewing`, not `pr_platform_win`.
- **D-W8B1-11 Stub hygiene:** `Debug` impls were added by hand where a field is a closure or trait object (`TaskEnd`, `StartFault`, `AppLog`, `RecoveryTexts`, `FileActionServiceExecutor`, `PreloadControllerAdapter`)
  to satisfy `missing_debug_implementations`. The stub-only `#![allow(unused_variables, dead_code)]` lines stay until each porter removes them (D11).
- **D-W8B1-12 Open items for porters (not decided here):** the strict-JSON wrapper in W8-03a versus a `parse_strict` in `app_settings::json`; the lenient notes reader of W8-04b;
  `NumberCulture` has no group separator (W8-02b/10c/10f carry a parameter or trait); `.NET Enum.TryParse` folding of non-ASCII key names (W8-04c Q9); the Windows implementations of
  `RecoveryTimeFormatter`, `DateText`, `UtcOffsetSource`, `MonitorProbe` and the production `CultureLookup` have no owner yet (assign in B3/B4); `ViewerChanges` is a temporary change-set type in
  `viewmodels::viewer_state` until D-W8-1's shared `ChangeSet` exists; the unresolved W8-12a render-trait details (`as_oriented` seam, `DeviceRecreated` subscribe).

## Wave 8 batch B2 roster (coordinators, view models, presenters; `pr-app`, pure fakes-only units)

Batch B2 of `docs/WAVE-8-PLAN.md` (P-38): 26 contracts (`contracts/W8-*.md`), stubs under `crates/pr-app/src/`. Test kind is F for every unit: no oracle command and no golden. The contracts were first drafted by a low-effort Codex model and then reviewed against the C# source at `d92d6aae` (every draft contained `async`/futures or invented API and was rewritten). Ports start per P-28 when the direct dependencies merge. Every stub body is `todo!()`.

| WU | Unit | Rust | Porter |
|---|---|---|---|
| W8-05g | PointerInputController, wheel and zoom half | `input::pointer::{wheel, zoom}` | Sonnet |
| W8-05h | PointerInputController, click zoom and pan half | `input::pointer::{click_zoom, pan}` | Sonnet |
| W8-06c | CompareViewModel | `viewmodels::compare` | Sonnet |
| W8-06e | FileActionGate (INV-4 caller side) | `coordinators::file_action_gate` | Sonnet + manual review |
| W8-06f | SiblingFolderNavigator | `coordinators::sibling_navigator` | Sonnet |
| W8-07a, 07b | FolderLoadCoordinator (load, reload/skipped) | `coordinators::folder_load::{state, load, reload, skipped}` | Sonnet |
| W8-07c, 07d, 07e | FileActionController (recycle, actions/undo, move/copy) | `coordinators::file_actions::*` | Sonnet |
| W8-07f | DuplicateCleanupController | `coordinators::duplicate_cleanup` | Sonnet |
| W8-07g | ZoomDetailLoader | `coordinators::zoom_detail` | Sonnet |
| W8-07h..07k | ImagePresenter (state/zoom, present_a, present_b, missing/compare_pair/photo_info) | `coordinators::presenter::*` | Sonnet |
| W8-08a..08e | MainViewModel (state, navigation, commands, session, notify_move) | `viewmodels::main::*` | Sonnet |
| W8-09a | Composition: decoder providers, service factories, contracts | `composition::{decoders, factories, contracts}` | Sonnet |
| W8-10d, 10e | SettingsPresenter (load/validate, apply/shortcuts) | `presenters::settings::*` | Sonnet |
| W8-10g | RecoveryWindowPresenter (INV-5 on screen) | `presenters::recovery_window` | Sonnet |
| W8-10i | BenchmarkPresenter | `presenters::benchmark` | Sonnet |

## Lead decisions (Wave 8 batch B2)

The recommended option of each open question was taken (standing user instruction); logged as P-41 in `docs/PROPOSED-DECISIONS.md`.

- **D-W8B2-1 No async:** C# `async Task` coordinators are explicit step/continuation state machines driven from the UI thread (`*Step`/`*Event` enums, `WorkerPort`, `CompletionInbox`, generation tokens checked on every completion, INV-9). No `async fn`, no futures, no runtime. Every draft that used `async` was rewritten.
- **D-W8B2-2 Shared change set:** one `ChangeSet` in `viewmodels::main::state` (W8-08a); coordinators and the presenter return their own small change structs (`PresenterChanges`, `CompareChanges`). `PresentationBridge`/`ViewerHandle` (`Rc<RefCell>`) is the one allowed shared-ownership exception, scoped to the main view model.
- **D-W8B2-3 Per-call context:** `FolderLoadCoordinator` and `FileActionController` take the catalog and the sinks per call (`*Cx`), not by ownership; the view model holds the `FileActionGate` (the controller never calls it).
- **D-W8B2-4 Duplicate concepts (porters reconcile, decided in the first port PR):** `folder_load::state::WorkerPort` vs `zoom_detail::{BackgroundRunner, CompletionInbox}`; `TextSource` lives in `file_actions::recycle` but is used by 07f; `ZoomDetailRefreshResult` is declared in `viewport::fit` and `zoom_detail`.
- **D-W8B2-5 Image payload type:** `CompareViewModel` and `IPresentationSink::set_current_image` use `Arc<DecodedImage>`; `ImagePresenter` holds `PlatformImage`. The 07h porter reconciles this and applies the B1 stub changes listed in W8-07h (`set_current_image(&ImageChange)`, `take_viewer_zoom_change`).
- **D-W8B2-6 Proposed B1 stub changes (NOT applied here, listed in the contracts):** `IFileActionExecutor: Send + Sync`; completion callbacks `on_present_finished` / `on_reload_finished` on `IFileActionSink` / `IDuplicateCleanupSink`; drop the `handler` parameter of `hook_render_frame` / `yield_to_render` in `input::surface`; `LocalizationService::export_todo`; a UTC-to-local conversion helper (W8-08).
- **D-W8B2-7 Missing plan edges and open items:** W8-05g also needs 05f, 04e and 06a. Unresolved: `SettingsTexts` over the Localizer, the `ShellIntegration` registry and the HTTP transport (10e), the time format "T" (10g), a compare size-pair formatter (06c).
- **D-W8B2-8 Stub hygiene:** the stub-only allow lines additionally allow the clippy lints their doc text or boxed seams trigger (`doc_lazy_continuation`, `type_complexity`, `enum_variant_names`, `large_enum_variant`, `should_implement_trait`); the porter removes them. `Debug` is implemented on every public type. No `Cargo.toml` change.

## Wave 8 batch B3 (gfx group 11f-11h)

Group of batch B3 of `docs/WAVE-8-PLAN.md` (P-38): the graphics interop audit of `Shell.Interop/Graphics` (1 466 C# lines) as `pr_platform_win::gfx`. Three contracts (`contracts/W8-11f.md`, `W8-11g.md`, `W8-11h.md`),
seven stub files under `crates/pr-platform-win/src/gfx/` (every body `todo!()`), `pub mod gfx;` in `lib.rs`, seven `windows` features in `crates/pr-platform-win/Cargo.toml` (versions untouched). Test kind N for all three
(native tests `#[ignore = "native: ..."]`; the pure helpers carry ordinary tests). The rest of B3 (11a-11e, 12b-12j, 13a-13f) is written by other agents.

| WU | Unit | Rust | Porter | Depends on | Golden |
|---|---|---|---|---|---|
| W8-11f | ComInterop, Guids, RenderingAdditions, DWriteAdditions (280 lines) | `gfx::{com, guids, extras}` | Sonnet (red: manual PR review) | none | none (N) |
| W8-11g | D3D11, Dxgi (459 lines, audit + thin helpers) | `gfx::{d3d11, dxgi}` | Sonnet (red) | 11f | none (N) |
| W8-11h | D2D1, DWrite (727 lines, audit + thin helpers) | `gfx::{d2d, dwrite}` | Sonnet (red) | 11f, 11g (tests) | none (N) |

Audit result: no `GAP`. Every declaration of the four hand-written COM files (`D3D11`, `Dxgi`, `D2D1`, `DWrite`) has a windows-rs item; 218 `Reserved` vtable placeholders and all slot arithmetic are retired. The two raw-vtable calls of the
C# `*Additions` files (`FillRoundedRectangle`, `CreateEllipsisTrimmingSign`) exist natively in windows-rs 0.62.2. Facts proven in a scratch test while writing the contracts: the 27 C# IIDs equal the windows-rs IIDs; the struct
sizes and flag values the C# test `StructLayouts_MatchWin32Sdk` checks hold for the windows-rs types; the HRESULT constants match.

## Lead decisions (Wave 8 batch B3, gfx group)

The recommended option of each open question was taken (standing user instruction: never stop to ask); to be logged in `docs/PROPOSED-DECISIONS.md` by the lead when the batch is merged.

- **D-W8B3G-1 `gfx` is std + `windows` only:** no `pr_app` or `pr_core` type in a `gfx` signature (`pr-app` depends on `pr-platform-win`, D-W8B1-1). Window handles are `HWND` inside, `isize` at the `pr_app` boundary (`gfx::com::hwnd`).
- **D-W8B3G-2 One error type:** `gfx::com::GfxError { hresult: i32, operation: &'static str }` (`Copy`) for the whole layer; it is converted field by field into `RenderError::Device` by `pr_platform_win::render` (12b).
- **D-W8B3G-3 Production swap chain values are the C# ones** (`FLIP_DISCARD`, `STRETCH`, `IGNORE`, 2 buffers, `FRAME_LATENCY_WAITABLE_OBJECT`), not the R0001 spike's (`FLIP_SEQUENTIAL`, `NONE`, flags 0). `dxgi::window_swap_chain_desc` is pure and fixed;
  W8-12j builds its own description if the attended re-measure still shows the `EndDraw` stall. A null waitable handle after a successful `SetMaximumFrameLatency` is an error (C# stored 0).
- **D-W8B3G-4 One waitable timer:** `extras::WaitableTimer` serves `GpuKeepAlive` (12h) and the host `TimerFrameClock` (13); the host copy `PendingKernel32` is not ported; `CancelWaitableTimer` becomes `cancel()`. The timer is `!Send`
  (create it on the waiting thread).
- **D-W8B3G-5 `DeviceLoss` lives in `gfx::com`** (`is_device_lost`, `describe_hresult`, the HRESULT constants), not in the `DeviceResources` port, because 12b (device) and 12d (surface) both need it and the plan cuts `DeviceResources` at line 230.
- **D-W8B3G-6 Audit rule:** one table row per non-`Reserved` C# member (kept as thin helper / replaced by a windows-rs item / retired, with the reason); `Reserved` placeholders are one row per interface. Draw calls on the Direct2D device context are NOT wrapped
  (12e owns the frame state machine); helpers return owned windows-rs types.
- **D-W8B3G-7 `ComInterop.Wrap/QueryInterface/Release` are retired**, not re-created: `Interface::cast` and `Drop` replace them. The ordering that `ComInterop.Release` served (back buffer reference released before `ResizeBuffers`) is a documented rule (W8-11g trap T1), enforced
  by field order or explicit `take()` in 12b/12c.
- **D-W8B3G-8 `d3d11::create_device_on_adapter(index, flags)` is added** (no C# twin, proven by the R0001 spike) so the attended re-measure can choose the adapter; the environment variable is read in W8-12b.
- **D-W8B3G-9 Missing plan edges, one dependency, one rule (lead actions):** (a) W8-11h native tests need W8-11g (WARP device and swap chain); (b) W8-12b to 12i also depend on W8-11f (`GfxError`, `is_device_lost`); (c) W8-12e needs the crate `windows-numerics`
  (`Matrix3x2` for `SetTransform`, `Vector2` for `DrawTextLayout`, `Matrix4x4` in `DrawBitmap`; they are not re-exported by `windows` 0.62.2) in `[workspace.dependencies]` and `crates/pr-platform-win/Cargo.toml` (already locked at 0.3.1; the version choice is the
  lead's, not done here); (d) windows 0.62.2 marks the D3D11, DXGI, Direct2D and DirectWrite interfaces `Send`, so the owning render types of 12b/12c/12d/12e/12i must be `!Send` (`PhantomData<*const ()>`): the compiler does not enforce the UI-thread rule.

Open items (recommended defaults taken, listed in the contracts): hidden `STATIC` window for the swap chain tests with a composition swap chain fallback (W8-11g Q4); no `DXGI_PRESENT_ALLOW_TEARING` yet (11g Q2); safe `copy_from_memory` wrapper (11h Q1); handle-count leak smoke test (11h Q3);
attended R0001 re-measure before W8-12d (11g Q3). Not verified here: every native behaviour claimed as a test (swap chain resize failing while a back buffer reference lives, wrong-flags `ResizeBuffers`, hidden-window swap chain creation, timer behaviour): they are written from the
C# comments and DXGI rules, with a note in each test description on what to change if Windows disagrees.

## Wave 8 batch B3 roster (win32 group 11a-11e; `pr-platform-win`, `unsafe` with SAFETY per block)

Group 11a-11e of batch B3 of `docs/WAVE-8-PLAN.md` (P-38): 5 contracts (`contracts/W8-11a.md` .. `W8-11e.md`), stubs under `crates/pr-platform-win/src/win32/`, one oracle command (`win32-constants`, 597 cases) and its golden. Replaces `PhotoReview.Shell.Interop`
(`Win32/*.cs` 1 459 lines, `Shell/*.cs` 341 lines of the 3 270; the five units cover 1 800) with the `windows` crate plus a few hundred lines of constants and thin wrappers; 11f..11h (`gfx`) and 12b..13f are separate contract groups. Nothing is ported yet: every wrapper body is `todo!()`
(the constant tables are complete in the stubs). None of the five units needs a `pr_app` type, so D-W8B1-1 (the layering PR) does not block them.

| WU | Unit | Rust | Porter | Test kind / golden |
|---|---|---|---|---|
| W8-11a | `Structs`, `WindowMessages` (+ handle, error and string helpers shared by 11b..11e) | `win32::{messages, structs}` | Haiku | G: golden `win32-constants` (163 `WindowMessages` constants + 16 struct layouts; the same file serves 11b..11e) |
| W8-11b | `User32`, `Kernel32`, `Kernel32Loader`, `Gdi32`, `Shell32`, `Shcore`, `Dwmapi`, `Ole32` | `win32::api` | Sonnet | N (4 C# `InteropSignatureTests` native + new) + constants from the golden |
| W8-11c | `CommonControlsNative`, `Comctl32`, `ActivationContext` (+ the shell manifest) | `win32::{comctl, control_constants, activation_context}`, `shell.manifest` | Sonnet | N + pure (181 + 22 + 1 constants, manifest twin of `ShellAssemblyTests`) |
| W8-11d | `DialogNative`, `DialogChromeNative` | `win32::{dialog_native, dialog_chrome}` | Sonnet | N + pure (65 + 57 constants) |
| W8-11e | `KeyCaptureNative`, `SettingsDialogNative`, `StartupAdditions`, `IDataObject`, `IDropTarget`, `IFileOpenDialog` | `win32::{key_capture, settings_native, startup, shell_native}` | Sonnet | N (3 C# `ShellComInteropGraphicsTests` + new) + pure (enum and GUID audit against windows-rs) |

## Lead decisions (Wave 8 batch B3, win32 group)

Written by the contract writer; the lead may overrule. Where a question had a recommended default it was taken (standing instruction). Detail and reasons are in the contracts.

- **D-W8B3-1 One shared golden.** `tools/oracle-cs/Commands/W8-11a.cs` (command `win32-constants`, dispatch line in `Program.cs`) reflects over the internal types of `PhotoReview.Shell.Interop.dll` and writes every constant of 18 classes, every member of 4 enums and the size and field offsets of 16 structs
  (`tests/golden/win32-constants.json`, 545 const + 36 enum + 16 layout cases; the writer ran it twice, byte-identical, cached baseline build, 8 s). 11b..11e read it; none of them touches an oracle file. The class-to-unit assignment is in W8-11a "Golden package".
- **D-W8B3-2 Constants use the SDK names and the C# types; the stubs already hold the values.** 499 of 545 names were found in the `windows` crate by rule; 46 were mapped by hand (`<STRUCT>_SIZE` for byte sizes, spelling slips); every value that the crate also defines was compared and matched, except the four `TD_*_ICON` (C# `-1..-4`, SDK `0xFFFF..`: W8-11c note 3).
  A constant that C# repeats in a second class is defined once in `messages` and re-exported (`pub use`).
- **D-W8B3-3 Struct aliases, plain-integer handles.** The 16 C# structs are `type` aliases of the windows-rs structs under the C# names; compile-time `size_of`/`offset_of` asserts reproduce every golden number (including `packed(1)` `TASKDIALOGCONFIG`). Handles are `isize`, `Wparam` is `usize`.
- **D-W8B3-4 Wrapper conventions** (W8-11b notes 1 to 5, reused by 11c to 11e): `Result<_, Win32Error>` for `SetLastError` functions, `Result<_, Hresult>` for `HRESULT` ones, `&str` strings converted to NUL-terminated UTF-16, and `pub unsafe fn` exactly where an integer argument can become a pointer the callee dereferences
  (`send_message`, `post_message`, `def_window_proc`, `dispatch_message`, `set_window_long_ptr`, `register_class_ex`, `create_window_ex`, `create_dialog_indirect_param`, `is_dialog_message`, `set_clipboard_data`, `global_free`, `set_window_subclass`, `def_subclass_proc`, `task_dialog_indirect`).
- **D-W8B3-5 Plan row deviations.** `structs.rs` also holds the structs that other Interop files declare (`ScrollInfo`, `ActCtx`, `FormatEtc`, `StgMedium`, `PointL`, `ComdlgFilterSpec`) and the shared types (`Win32Error`, `Hresult`, `WndProc`, UTF-16 helpers) so 11b..11e depend on 11a only. `DialogChromeNative` is its own file (`dialog_chrome.rs`).
  11e gets a fourth file `shell_native.rs` for `ShellNative`/`OleNative`. 11c splits into `comctl.rs`, `control_constants.rs`, `activation_context.rs`. Plan edge correction: 11d needs 11a only in code (11b is a convention reference).
- **D-W8B3-6 Wrappers set the size field** (`cbSize`, `length`) of the structure they pass, so a forgotten size cannot fail the call.
- **D-W8B3-7 `EnumDisplayMonitors` returns a `Vec<MonitorEnumItem>`** (the unmanaged callback and `data` pointer stay internal).
- **D-W8B3-8 `TaskDialogIndirect` is bound at run time, only successful lookups are cached; the comctl32 v6 manifest is a file in the crate and a test-only `ActivationScope`.** Verified on this machine by parsing `System32\comctl32.dll`: the v5.82 a manifest-less process loads has no `TaskDialogIndirect` (a static import would keep every `cargo test` executable of the crate from starting) but does export
  `SetWindowSubclass`, `RemoveWindowSubclass`, `DefSubclassProc` and `InitCommonControlsEx` by name, so those are imported statically. Embedding the manifest in the app exe is W8-20a's (recommended: `/MANIFESTINPUT` link argument, no dependency).
- **D-W8B3-9 `draw_text` clears `DT_MODIFYSTRING`** (the OS must never write into the read-only text slice); pure helper with an exact-value test.
- **D-W8B3-10 Shell COM is not ported.** The 340 lines of hand-written vtables (`IDataObject`, `IDropTarget`, `IShellItem`, `IFileDialog`...) are replaced by the windows-rs interfaces; W8-11e carries the slot-by-slot audit table and ports only `ShellNative`/`OleNative`. `ComInterop` (W8-11f) is not needed by 11e.
- **D-W8B3-11 `windows` features** added to `crates/pr-platform-win/Cargo.toml` (target-specific dependency, no version change): `Win32_UI_Input`, `Win32_UI_Input_Ime`, `Win32_UI_Controls`, `Win32_UI_HiDpi`, `Win32_UI_Shell_Common`, `Win32_Graphics_Gdi`, `Win32_System_DataExchange`, `Win32_System_Memory`, `Win32_System_Ole`,
  `Win32_System_ApplicationInstallationAndServicing` (already present: `Win32_Foundation` via the workspace, `Win32_Graphics_Dwm`, `Win32_System_Com`, `Win32_System_LibraryLoader`, `Win32_System_Threading`, `Win32_UI_Shell`, `Win32_UI_WindowsAndMessaging`, `Win32_UI_Input_KeyboardAndMouse`). `Cargo.lock` is unchanged (`--locked` builds).
- **D-W8B3-12 Native test conventions.** Pure tests (golden constants, layouts, enum and GUID audits, manifest text, string helpers, `fn`-pointer shape checks) are NOT ignored; every test that calls the OS beyond `GetLastError` is `#[ignore = "native: needs desktop"]`, runs on its own thread, destroys what it creates and never shows a modal dialog or empties the clipboard;
  the one test that changes process-wide state (`SetProcessDpiAwarenessContext`) is alone in its test executable. The lead runs `--include-ignored` once on an interactive desktop before any of these rows is marked Done.
- **D-W8B3-13 Findings of the audit (no action needed, recorded):** (a) there is no GAP: every P/Invoke of `Win32/*.cs` exists in `windows` 0.62.2; (b) `Kernel32Loader` (`LoadLibraryEx`, `GetProcAddress` by ordinal) is used only by a C# test (uxtheme ordinals 135/136); (c) C# `NmListViewSize = 88` and `NmLvCustomDrawSize = 104` are wrong (64 and 136) and unused,
  ported with the C# value and flagged; (d) `Msg.Private` (offset 44) has no windows-rs field (tail padding); (e) the existing `dark_title_bar.rs` keeps its run-time binding of `DwmSetWindowAttribute`; the new `dwmapi` wrapper imports it statically (W8-11b open question 3).

## Wave 8 batch B3 (render group 12b-12e)

Group of batch B3 of `docs/WAVE-8-PLAN.md` (P-38): the Direct2D renderer core of `Shell.Rendering` (`DeviceResources` 456, `D2DRenderSurface` 236 + factory 13, `D2DDrawContext` 359 = 1 064 C# lines) as `pr_platform_win::render::{device, surface, draw_context}`.
Four contracts (`contracts/W8-12b.md` .. `W8-12e.md`), seven stub files under `crates/pr-platform-win/src/render/` (`mod.rs`, `device/{mod, create, loss, resources}.rs`, `surface.rs`, `draw_context.rs`; every body `todo!()`), `#[cfg(windows)] pub mod render;` in `lib.rs`.
Test kind N for all four (native tests `#[ignore = "native: needs desktop"]`); each unit also carries small PURE functions (`clamp_dimension`, `classify_end_draw`, `sync_interval`, `FrameState`, ...) so the verifier has something to mutate. The rest of unit 12 (12f..12j) is written by another agent.

| WU | Unit | Rust | Porter | Depends on | Golden |
|---|---|---|---|---|---|
| W8-12b | DeviceResources: state, creation, loss, teardown (about 290 C# lines) | `render::device::{mod, create, loss}` | Sonnet (red) | 11f, 11g, 11h | none (N) |
| W8-12c | DeviceResources: target, bitmaps, resize, probes, interpolation map (about 140 C# lines) | `render::device::resources` | Sonnet (red) | 12b, 11g, 11h | none (N) |
| W8-12d | D2DRenderSurface + D2DRenderSurfaceFactory (249 lines) | `render::surface` | Sonnet (red) | 12c, 11g, 11f, 12e (type) | none (N) |
| W8-12e | D2DDrawContext + INativeTextLayout (359 lines) | `render::draw_context` | Sonnet (red) | 12c, 12d, 01c, 01a, 12a, 11h | none (N) |

## Lead decisions (Wave 8 batch B3, render group)

The recommended option of each open question was taken (standing user instruction: never stop to ask); to be logged in `docs/PROPOSED-DECISIONS.md` by the lead when the batch is merged.

- **D-W8B3R-1 Interim types until the layering PR.** `pr-app` still depends on `pr-platform-win` (D-W8B1-1), so the render types cannot implement the `pr_app::services::render` traits or name `RectD`/`ColorF`/`GpuImage` yet. The stubs therefore use std/`windows`-only twins with the same names and fields:
  `RenderFault` (twin of `RenderError`, `device/mod.rs`), `ImageInterpolation` (`device/resources.rs`), `SurfaceOptions`, `PresentResult` (`surface.rs`), `PointD`, `RectD`, `ColorF`, `Matrix3x2` and the `&dyn Any` image argument (`draw_context.rs`). Each is replaced by an import when the layering PR lands
  (option A of the W8-12a contract: `pr-platform-win` depends on `pr-app`; `main.rs`, the spike and the composition move to a thin package), and the `impl RenderSurface / DrawContext / OrientedDrawContext / RenderSurfaceFactory` adapters are written then.
  **Recommended order: layering PR, then ports of 12d and 12e; 12b and 12c have no `pr_app` dependency beyond `RenderError` and may be ported first.** Closing the layering question is a lead action, not part of this batch.
- **D-W8B3R-2 `DeviceResources` is cut by responsibility, not at line 230.** The plan labels the halves "device creation, adapter choice, device loss" / "brushes, caches, text factories"; the second label matches nothing in the file (the brush is created in `CreateDevice`, the caches are 12f/12g, the text factories 12i), and the line cut
  would put `CreateDevice`/`CreateSwapChain` in 12c. 12b = state + getters + `Create*` + `CreateAll`/`CreateDevice`/`CreateSwapChain` + `EndDraw`/`Present` seams + `Recreate`/`Dispose`/`ReleaseAll`; 12c = `CreateBitmap`/`ReleaseBitmap`/`Resize`/`TouchGpu`/`Synchronize`/`CreateTarget`/`ReleaseTarget`/`InterpolationModes`.
  `create_all` ends with `create_target` (12c), so whole-object native tests of 12b turn green with 12c: **one porter, two stacked PRs**.
- **D-W8B3R-3 One shared object.** `DeviceResources` is `Rc<DeviceResources>` shared by the surface, the draw context and (12g) the cache, as in C#; every method takes `&self` (`RefCell<DeviceState>` plus `Cell` counters). It is `!Send`/`!Sync` through `PhantomData<*const ()>` because windows 0.62.2 marks the COM
  interfaces `Send` (D-W8B3G-9 d). The keep-warm thread (12h) builds its own instance on its own thread. COM interfaces are handed out as clones (`AddRef`); callers must not keep `target()` across `resize` (the back-buffer trap).
- **D-W8B3R-4 Frame protocol.** `begin_draw_context(&mut self)` returns `&mut D2dDrawContext` borrowed from the surface, so surface calls during a live context reference do not compile. The reference can still be dropped without `end_draw_and_present`, so the C# runtime checks stay (second begin, resize while drawing, end without begin).
  The draw context holds an `Rc<DeviceResources>` clone, not the surface. The C# owner-thread check has no Rust twin (`!Send`); "draw outside a frame" cannot be reached (`debug_assert` + skip). Recorded as Different-by-construction.
- **D-W8B3R-5 12e does not depend on 12g.** The C# image tests go through `GpuImageCache` + `D2DGpuImageBackend`. 12e tests build a `TiledGpuImage<ID2D1Bitmap1>` themselves (`compute_tiles` + `DeviceResources::create_bitmap` + `copy_from_memory`); the cache-driven tests (`TiledImage_ForcedSmallTiles...`, `Panorama30000x2000`, the cache half of
  `DeviceLost_RecreateTarget...`, `Cache_*`) belong to 12g.
- **D-W8B3R-6 Adapter override.** `PHOTOREVIEW_RS_GPU_ADAPTER=<index>` (read once per device creation in 12b, only when not `prefer_warp`) tries `d3d11::create_device_on_adapter(index)` first and falls through to the normal path on failure (D-W8B3G-8 left the variable to 12b). The C# has no such switch.
- **D-W8B3R-7 Missing plan edges and Cargo lead actions.** 12b needs 11f; 12c needs 11g and 11h; 12d needs 11g, 11f and the type `D2dDrawContext` of 12e; 12e needs 12d (tests), 12a, 01a, 11h. Cargo (NOT edited here, to avoid conflicts with the other B3 agents): `windows` feature `Win32_UI_HiDpi` for `cfg(windows)`
  (`GetDpiForWindow`, 12b; `GetClientRect` needs `Win32_UI_WindowsAndMessaging`, already on); `windows-numerics = "0.3"` as a workspace and `pr-platform-win` dependency (12e; already locked at 0.3.1; verified that `windows` 0.62.2 does not re-export `Matrix3x2`). The stubs did not need either.
- **D-W8B3R-8 Behaviour decisions.** `Resize` keeps its two-way result (`Ok(Resized)` / `Ok(BuffersFailed(hr))` / `Err`); an `EndDraw` or `Present` failure that is not a loss is `Ok(Failed)`, never `Err`; the unbalanced-stack error is raised after the frame was presented; `create_bitmap` takes a slice and rejects sizes below 1 (C# cast to `uint`);
  present flags stay 0 (no `DXGI_PRESENT_ALLOW_TEARING`, C# v1); diagnostics are `Duration` (C# `Stopwatch` ticks); the `EndDrawFault`/`PresentFault` seams are `pub(crate)` hooks that see the real HRESULT; `DeviceLoss` stays in `gfx::com` (D-W8B3G-5).
- **D-W8B3R-9 Pure seams for testability.** `clamp_dimension`, `normalize_dpi_scale`, `dpi_scale_from_window_dpi`, `parse_adapter_index`, `classify_end_draw`/`FrameEnd` (12b), `to_d2d_interpolation`, `buffers_failed` (12c), `sync_interval` (12d), `Matrix3x2::multiply`, `to_d2d_rect`, `to_d2d_color`, `FrameState` (12e) are plain functions
  with ordinary tests and mutation targets; everything that touches a COM object is native.
- **D-W8B3R-10 Test helpers are private per unit.** The hidden `STATIC` window helper and the `RenderTestImages` helpers are copied into each unit's test module (a shared `render/test_*.rs` needs lead wiring); native tests use `windows` directly for `CreateWindowEx`, not W8-11b.

Safety finding that refines the gfx wording: in windows 0.62.2 EVERY Direct2D draw call used by 12c/12d/12e (`BeginDraw`, `Clear`, `FillRectangle`, `DrawBitmap`, `DrawTextLayout`, `SetTransform`, `PushAxisAlignedClip`, `SetColor`, ...) is an `unsafe fn` over raw pointers, so 12e is a many-small-`unsafe`-blocks unit, each with `// SAFETY:`
(contract W8-12e "Safety rules"); the gfx helpers wrap only bitmap creation, copy and read-back.

Open items (recommended defaults taken, listed in the contracts): layering PR timing (all four); attended R0001 `EndDraw` re-measure before 12d, W8-12j decides (12d Q2); `allow_tearing` stays sync-0 only (12d Q3); `RenderFault` location until the layering PR (12b Q5); shared test helpers (12d Q5, 12e Q5).
Not verified here: every native behaviour claimed as a test (back-buffer-reference `DXGI_ERROR_INVALID_CALL` on `ResizeBuffers`, hidden-window swap chain on WARP, waitable object signalled after creation, `D2DERR_RECREATE_TARGET` injection results, pixel-exact output, `ClearStateAndFlush` ordering hang): written from the C# comments and tests and the DXGI rules, each with a note on what to change if Windows disagrees.

## Wave 8 batch B3 (render group 12f-12j)

Second group of batch B3 of `docs/WAVE-8-PLAN.md` (P-38): the rendering support of `Shell.Rendering` that does not need the device/surface/draw context (12b-12e are written by another contract writer in parallel). Five contracts (`contracts/W8-12f.md` .. `W8-12j.md`), eight stub files under
`crates/pr-platform-win/src/render/` (`image_cache/{mod,policy,upload}.rs`, `keep_alive.rs`, `text/{mod,renderer,layout_cache}.rs`, plus `render/mod.rs` declaring only `image_cache`, `keep_alive`, `text`), `#[cfg(windows)] pub mod render;` in `lib.rs`. Every body is `todo!()`; the stubs compile against the CURRENT crate graph
(`pr-app` depends on `pr-platform-win`), so they use `std`, `pr_imaging`, `pr_core` and `gfx` types only. W8-12j is contract-only (conditional, no C# source, no stub).

| WU | Unit | Rust | Porter | Test | Depends on |
|---|---|---|---|---|---|
| W8-12f | `GpuImageCache` lines 1-300: byte budget, LRU, current/retained protection, striped prefetch, device-loss clear | `render::image_cache::policy` | Sonnet | F (fakes, no GPU) | 12a |
| W8-12g | `GpuImageCache` lines 301-414: `D2DGpuImageBackend` (tiles, bitmap upload, stripe copy, release) | `render::image_cache::upload` | Sonnet (red) | N + pure helpers | 12f, 12c, 11f, 11h, 01c, layering PR |
| W8-12h | `GpuKeepAlive` 231 (WP21-STALL keep-warm thread) | `render::keep_alive` | Sonnet (red) | F+N | 12c, 11f |
| W8-12i | `Text/{DWriteTextRenderer 201, TextLayoutCache 197}` | `render::text::{renderer, layout_cache}` | Sonnet | F+N | 12a, 11f, 11h, (12c), layering PR for the trait impls |
| W8-12j | NEW (no C# source): swap-chain latency experiment, CONDITIONAL on an attended re-measure | none until triggered | Sonnet | N | 12d, 12e, 12h, 11g, 16a, 13g |

## Lead decisions (Wave 8 batch B3, render group 12f-12j)

The recommended option of each open question was taken (standing user instruction: never stop to ask); to be logged in `docs/PROPOSED-DECISIONS.md` by the lead when the batch is merged.

- **D-W8B3S-1 Cut of `GpuImageCache.cs`:** 12f = lines 1-300 (the whole class `GpuImageCache`, already written against an internal backend seam and a dispatcher, hence pure), 12g = lines 301-414 (`IGpuImageBackend` + `D2DGpuImageBackend`). The plan's cut at line 200 is inside `EvictOverBudget`.
- **D-W8B3S-2 Prefetch stripes are `StripeToken` jobs handed to a `StripeScheduler` trait**, not closures: `pr_core::abstractions::ui::UiAction` is `Send`, the cache (`Rc`, `RefCell`) is not. The host (W8-13d) posts a `Send` job at `UiPriority::Background` that calls `cache.run_stripe(token)`. The token carries an entry serial so a job queued for an evicted or cleared entry stays inert even if the same `PixelBuffer` id is cached again.
- **D-W8B3S-3 `pr_core::caching::lru::BoundedLruCache` is NOT reused by 12f** although the plan row lists it: it is thread-safe, evicts on insert and cannot protect entries; the C# cache keeps the current image and the retained ids over budget. 12f uses `HashMap` + `BTreeMap` stamps (the technique of `lru.rs`).
- **D-W8B3S-4 Placeholder seams until the layering PR (D-W8B1-1):** `CachedImage`, `GpuImageBackend`, `CacheError` (12f, permanent seams: the policy stays testable on fakes), `BitmapDevice` (12g), `KeepAliveDevice` (12h), local `TextTrimming`/`LayoutSize`/`TextError` (12i). Each contract lists the exact `impl` blocks the porter adds after the layering PR (`impl GpuImageBackend for D2dGpuImageBackend`, `impl GpuImageCache for D2dGpuImageCache`,
  `impl TextRenderer for DWriteTextRenderer`, `impl TextLayout for DWriteTextLayoutLease`). The layering PR itself is still a lead action and still blocks the trait glue, NOT the pure logic.
- **D-W8B3S-5 (DONE: `PrefetchStatus` in `pr_app::services::render::gpu_image`) The 12a `GpuImageCache` trait lacks `IsFullyUploaded` and `UploadCompleted`**, which `PresentationComposer` (W8-16c, C# lines 93, 137, 211, 285, 324) uses on the concrete class. Recommended (lead action, not done here, 12a is merged): a sub-trait `PrefetchStatus: GpuImageCache` in `pr_app::services::render::gpu_image` with `is_fully_uploaded`, `subscribe_upload_completed`, `unsubscribe_upload_completed`,
  implemented by the 12g adapter.
- **D-W8B3S-6 Missing plan edges (lead action):** 12g also depends on 11f, 11h, 01c, 12a and the layering PR (its draw-based native tests on 12d and 12e); 12h on 11f; 12i on 12a, 11f, 11h (and 12e for its ink/colour native tests); 12j on 12e, 12h, 11g, 16a and 13g (window style `WS_EX_NOREDIRECTIONBITMAP` for the DirectComposition mode).
- **D-W8B3S-7 12h reads no environment variable:** the plan row names `PHOTOREVIEW_RS_GPU_KEEPWARM*`, but in C# they are read by `Win32/Viewing/KeepWarm.cs` (W8-16d, `KeepWarmOptions`): `PHOTOREVIEW_GPU_KEEPWARM` (0/off/false/no), `_MS` (250-60000, default 5000), `_INTERVAL_MS` (5-100, default 20), renamed with the `PHOTOREVIEW_RS_` prefix (R0002). Row correction for the plan.
- **D-W8B3S-8 Keep-warm ticks are nanoseconds of a monotonic clock** (`monotonic_ticks`, `ticks_from_ms`, `TICKS_PER_SECOND`), not `Stopwatch` units; no consumer may assume `Stopwatch.Frequency`.
- **D-W8B3S-9 `GpuKeepAlive::wait_blocked` is the one added test hook** (no C# twin): it replaces `SpinWait.SpinUntil` polling (AGENTS.md forbids sleep polling) for "the thread is parked on the activation event after N waits".
- **D-W8B3S-10 `TextLayoutCache` has no lock:** the C# `lock` guarded a lease disposed from another thread; Rust leases and the renderer are `!Send` (`PhantomData<*const ()>`, because windows-rs 0.62.2 marks the DirectWrite interfaces `Send`, D-W8B3G-9 d).
- **D-W8B3S-11 W8-12j trigger:** it starts only if an ATTENDED re-measure of the PRODUCTION surface (12d with the C# swap chain values, D-W8B3G-3) WITH keep-warm (12h) still shows a windowed p95 above 10 ms (R0001 threshold; target < 8.5 ms). The R0001 spike used `FLIP_SEQUENTIAL`, no waitable flag and no keep-warm thread, and its 300-510 ms `EndDraw` stalls look like WP21-STALL (0.47 s, Intel UHD 630), so the spike
  numbers do not decide the question. Recommended first action after 12d/12e/12h merge: the cheap attended measurement.
- **D-W8B3S-12 W8-12j has one env switch (`PHOTOREVIEW_RS_SWAPCHAIN_EXPERIMENT`) with one change per value** (`sequential`, `buffers3`, `tearing`, `composition`; unset = production path, zero cost); the losing modes are deleted at the end, no permanent experiment matrix.

Open items (recommended defaults taken, listed in the contracts): shape of 12b/12c `DeviceResources` (12g `BitmapDevice` and 12h `KeepAliveDevice` assume `Rc<DeviceResources>` with `max_bitmap_size`, `generation`, a bitmap leak counter, `is_warp`, `touch_gpu`, a device-recreated subscription) is the other writer's decision; strong `Arc` vs `Weak` source buffer in `prefetch` (12f Q1);
`with_data` upload as create + `copy_from_memory` (12g Q3); locale source for the text renderer (12i Q2); per-call `String` clones in the layout cache key (12i Q1); who logs a disabled keep-alive (12h Q3).
Not verified here: every native behaviour claimed as a test (DirectWrite metrics ranges, ellipsis `IsTrimmed`, `CopyFromMemory` stripes across gutter tiles, `D2D1_ALPHA_MODE_IGNORE` read-back, `SetThreadPriority`, high-resolution timer sleeps, GPU idle behaviour) is written from the C# tests and the DXGI/DirectWrite rules, not run; the pure policies were not compiled as Rust tests (the stubs
compile, `cargo check` and `cargo clippy -D warnings` are clean); the 12j analysis relies on the numbers in `docs/adr/R0001-ui-framework.md`.

## Wave 8 batch B3 (host group 13a-13f)

Group 13a-13f of batch B3 of `docs/WAVE-8-PLAN.md` (P-38): the window host `pr_platform_win::host` from `PhotoReview.Shell.Win32/Hosting` (C# `FrameClockBase`, `TimerFrameClock`, `SwapChainFrameClock`, `ShellWindowContracts`, `ShellIcons`, `NativeRect`, `MessageLoop`, `Win32UiDispatcher`, `WndProcThunk`, `ShellWindow`; baseline `d92d6aae`).
Six contracts (`contracts/W8-13a.md` .. `W8-13f.md`), 17 stub files under `crates/pr-platform-win/src/host/` (every body `todo!()`; 14 are unit-owned, 3 lead-owned indexes), `#[cfg(windows)] pub mod host;` in `lib.rs`. No `Cargo.toml` change: every `windows` feature the host needs is already enabled. The main window `ShellMainWindow` (13g..13k) is batch B4.
Retired: `Win32UiSynchronizationContext` (80 lines, no `await` on the UI thread: R0003, D-W8-2), `PendingInterop/PendingNativeMethods` (102 lines, replaced by `host::native` plus the `win32`/`gfx` wrappers). Ports start per P-28; nothing in 13 is ported yet.

| WU | Unit | Rust | Porter | Test kind |
|---|---|---|---|---|
| W8-13a | `FrameClockBase`, `TimerFrameClock` (333 lines) | `host::{error, frame_clock::{base, timer}}` | Sonnet | F (fake clock; 12 ported + new) |
| W8-13b | `SwapChainFrameClock`, `ShellWindowContracts`, `ShellIcons`, `NativeRect` (248; `PendingNativeMethods` retired) | `host::{swap_chain_clock, contracts, icons, native}` | Sonnet | N + pure (5 + 4 ported, 12d smoke ignored) |
| W8-13c | `MessageLoop` (354) + the exception capture of `WndProcThunk` | `host::{message_loop, pending_panic}` | Sonnet (red: manual PR review) | F+N (fake dispatcher and OS; 9 native ports) |
| W8-13d | `Win32UiDispatcher` (389; `Win32UiSynchronizationContext` retired) | `host::ui_dispatcher` (+ `UiScheduler`/`UiDispatcher` of `pr_core`) | Sonnet (red) | F+N (pure queues and wake flag; 14 of 21 tests ported, 7 retired) |
| W8-13e | `WndProcThunk` (104), `ShellWindow` 1-250 and the helpers it calls | `host::{wndproc, shell_window::create}` | Sonnet (red) | N (fake `WindowTarget` + real window) |
| W8-13f | `ShellWindow` 251-516 minus those helpers | `host::shell_window::{messages, dpi}` | Sonnet (red) | N + pure (11 ported + leak test) |

## Lead decisions (Wave 8 batch B3, host group)

Written by the contract writer; the lead may overrule. Where a question had a recommended default it was taken (standing instruction). Detail and reasons are in the contracts. Verified while writing: the stubs pass `cargo check` and `cargo clippy -p pr-platform-win --all-targets --locked -- -D warnings` and `rustfmt --edition 2024`.

- **D-W8B3H-1 Interim twins until the layering PR (D-W8B1-1).** `pr-app` still depends on `pr-platform-win`, so `host` cannot name `pr_app::input::frame_clock::{FrameTick, FrameHandler, FrameHandlerId}` or `pr_app::input::primitives::{SizeD, PointD}`. The stubs define field-for-field twins (`host::frame_clock::base`, `host::contracts`);
  the layering PR replaces them by `pub use` and adds `impl pr_app::input::frame_clock::FrameClock for` a newtype around `Rc<dyn HostFrameClock>` (the orphan rule forbids it on `Rc`). `UiScheduler`, `UiDispatcher`, `UiPriority`, `UiAction`, `UiCompletion`, `DisplayClock`, `DisplayTiming`, `Log` are in `pr_core::abstractions` and need no twin.
  `WindowOps` (`pr_app::windowing::fullscreen`) is implemented by the real window code of W8-13k, not by 13a-13f.
- **D-W8B3H-2 Frame clocks are shared `&self` objects.** `FrameClockBase<P: ClockPolicy>` (policy = `TimerPolicy`, `SwapChainPolicy`) over a `RefCell`, used as `Rc<dyn HostFrameClock>` by the loop, windows and presenters; handlers are taken out of their slot while they run (no borrow held), with C# multicast snapshot semantics.
  Arming failures (`SetWaitableTimer`) are `Result` in the `try_*` forms; the trait forms (shaped like `pr_app`'s `FrameClock`) panic with the `HostError` text, which is what the C# `Win32Exception` escaping `Frame +=` does.
- **D-W8B3H-3 Naming.** C# `IShellWindow` is the trait `ShellWindowApi`; the concrete type keeps the C# name `ShellWindow`. Events are `subscribe(ShellWindowEvent, handler)` / `subscribe_closing` / `unsubscribe(id)` (ids are `u64`, precedent `FrameClock`).
- **D-W8B3H-4 Thread affinity and panics.** The host types are `!Send` (`Rc`, boxed `FnMut`), so the C# `VerifyAccess` runtime checks and their tests have no Rust twin (compile-time assertions instead); `Win32UiDispatcher` is the one cross-thread type and keeps a run-time `verify_access`.
  A panic must not unwind out of an `extern "system"` procedure: `pending_panic::guarded` captures the first payload per thread and the procedure returns 0 (no default processing); the owner resumes it with `resume_unwind` after the native call returns (`MessageLoop` after `DispatchMessage`, `ShellWindow` after `CreateWindowEx`, `SendMessage`, `DestroyWindow`, `ShowWindow`, `SetWindowPos`), exactly the C# capture/rethrow of `WndProcThunk`.
  The panic is not caught again (R0003: the UI thread does not catch panics); it reaches the top of the UI thread with its original payload.
- **D-W8B3H-5 W8-13b -> W8-12d is a soft edge.** `SwapChainFrameClock` takes the waitable handle as an `isize` (what `RenderSurface::frame_latency_wait_handle()` returns in contract W8-12d, branch `wave8/b3-contracts-render-a`, PR #87); only one ignored smoke test needs a real surface. The 12d API used there is an assumption (`create_window_surface(hwnd, SurfaceOptions)`, `frame_latency_wait_handle()`).
- **D-W8B3H-6 Message handlers are `&self`, `Rc<dyn WindowMessageHandler>`,** `try_handle -> Option<Lresult>`; a handler can be re-entered through a nested `SendMessage`, so state lives in `Cell`/`RefCell`. `WindowMessage` is `Copy`.
- **D-W8B3H-7 Loop seams.** `MessageLoop` talks to the dispatcher through `LoopDispatcher` and to the OS through `LoopOs` (`Win32LoopOs` is the real one), which breaks the C# cycle `MessageLoop` <-> `Win32UiDispatcher` and lets the pass order (the highest-risk logic of the host) be tested with fakes. Filters and clocks use ids/`Rc`s with snapshots during iteration; `RawMessageFilter` is `as_raw()` on the filter.
- **D-W8B3H-8 `Win32UiDispatcher` shape.** `Clone + Send + Sync` (an `Arc`), implements `pr_core::abstractions::{UiScheduler, UiDispatcher}` and `LoopDispatcher`; no `Drop` (the composition root calls `shutdown()` on the UI thread, C# `Dispose`). `InvokeAsync`/`YieldAsync` become `invoke`/`post_with_priority(.., Background)`; a shut-down dispatcher resolves `invoke` completions as dropped, never hangs.
  `UiQueues<T>` and `WakeFlag` are pure so the ordering and the single-wake-message protocol are tested without a window.
- **D-W8B3H-9 Window class names carry the Rust prefix:** `PhotoReviewRs.Shell.Dispatcher`, `PhotoReviewRs.Shell.Window` (C#: `PhotoReview.Shell.*`). A window class is process-local, so this is not a collision fix; it keeps UI automation and diagnostics that look windows up by class name apart (R0002 spirit). Not a behaviour difference; no `Different` row.
- **D-W8B3H-10 `post` catches a panicking action** (log via `Log::error`, the loop continues) and `invoke` reports it in the completion: the C# contract and tests (`Post_ActionThrows_IsLoggedAndTheLoopKeepsRunning`). Deviation from "the UI thread does not catch panics" (R0003) is confined to this one function; the alternative is 13d Q2 (needs a `Different` row only if chosen).
- **D-W8B3H-11 `dispatcher_wnd_proc` lives in `ui_dispatcher.rs`,** not in `wndproc.rs` (the C# `DispatcherProc` relocated), so 13d does not depend on 13e. Both procedures are declared with the `windows` parameter types so they fit `WNDCLASSEXW::lpfnWndProc` without a cast (the `isize` alias `win32::structs::WndProc` is a different type).
- **D-W8B3H-12 Trampoline over a `WindowTarget` trait.** The `GWLP_USERDATA` cookie is a thin pointer to a `Box<Rc<dyn WindowTarget>>` (C# `GCHandle`); `ShellWindowInner` implements the trait, so the trampoline, the NCCREATE binding, the NCDESTROY cleanup and the panic guard are tested with a fake target. The trampoline clones the `Rc` before calling, so a target may free its own cookie. Events use `EventList<H>` (snapshot ids, handler taken out while running).
- **D-W8B3H-13 13e/13f cut moved to call direction.** The plan cut at line 250 would make 13e call 13f (`ApplyInitialSize`, `ToPixels`, `OnFrame`, `OnDestroy`, lifecycle arms, `ensure_class_registered`, ...). 13e owns those (about 150 of the 266 lines of the second half); 13f owns `ProcessMessage`'s dispatch, `UpdateClientSize`, `ApplyDpiChange`, `UpdateActive`, `ApplyMinTrackSize`, `RaiseClosing`, `LowWord`/`HighWord`.
  13e writes the first `process_message` (lifecycle + `DefWindowProc`) in 13f's `messages.rs` so a window can be created before 13f lands. The lead may merge 13e and 13f into one PR without changing a signature.
- **D-W8B3H-14 Extra files not named by the plan,** all inside `host/`: `error.rs` (`HostError`, owned by 13a), `native.rs` (13b: `GetCapture`, `ValidateRect`, `WindowFromPoint`, `LoadImage` and the constants of the retired `PendingNativeMethods`), `pending_panic.rs` (13c). `HostError` replaces the C# `Win32Exception`/`InvalidOperationException`/`ArgumentException` of the host.
- **D-W8B3H-15 QPC helpers are not duplicated:** `host::frame_clock::qpc_frequency` delegates to `crate::display_clock::qpc_frequency` (`pub(crate)`), `qpc_now` to `display_clock::qpc_timestamp` (13a changes it to `pub(crate)`, the one edit outside the owned files). Arithmetic on QPC ticks that C# does unchecked is done in `i128`/`saturating` so debug builds do not panic and far deadlines do not wrap into "fire now".
- **D-W8B3H-16 Icons.** `ShellIcons` loads from the exe's `RT_GROUP_ICON` 32512 with `LR_SHARED`; the C# fallback to the test assembly module is dropped, tests use the `load_from(0, ..)`/`apply_with` seams. Embedding the icon in the Rust exe is a W8-20a build step (W8-13b Q3: lead decision on the resource crate).
- **D-W8B3H-17 `pending_panic.rs` is 13c's** (the loop is the first caller; 13d/13e use it): first payload wins per thread, `rethrow_pending` takes it and `resume_unwind`s, `guarded(on_panic, body)` is the guard every window procedure wraps its body in.
- **D-W8B3H-18 `ShellWindow` owner semantics.** `ShellWindow` is an `Rc<ShellWindowInner>` handle whose `Drop` is C# `Dispose` (destroys the window if it exists; the rethrow is skipped while panicking); the frame-clock subscription made by `invalidate` holds a `Weak`, so the clock never keeps a window alive.
- **D-W8B3H-19 Pointer-bearing messages are guarded.** `WM_DPICHANGED` (`RECT*`) and `WM_GETMINMAXINFO` (`MINMAXINFO*`) read `lParam` only when it is non-null and aligned (the C# only null-checked); two `unsafe` blocks with `// SAFETY:` in `process_message`.
- **D-W8B3H-20 Missing plan edges (lead actions):** 13c needs 13a (and 11f for `wait_for_handle`); 13b needs 11d (`send_message`); 13d needs 13a; 13e needs 13a, 13b and 11c/11d (wrappers `get_cursor_pos`, `set_window_text`, `screen_to_client`, `send_message`, `is_window`); 13f needs 13b; 13a needs 11f (`WaitableTimer`). 13b -> 12d is soft (D-W8B3H-5). 13d does not need 13e.

Open items (recommended defaults taken, listed in the contracts): trait-form arming failure panics vs deferred error (13a Q1); `HostFrameClock` as a second trait next to `pr_app`'s `FrameClock` (13a Q2); the exe icon resource and its build step (13b Q3); `LoopOs` seam (13c Q1); catching action panics in `post` (13d Q2); merging 13e and 13f (13e Q2, 13f Q1); the class-registration failure test (13e Q4).
Not verified here (exact): no native behaviour was run. Everything about `PeekMessage`/`MsgWaitForMultipleObjectsEx` ordering, `WM_DPICHANGED` re-entrancy through `SetWindowPos`, the 64-handle limit, `WM_SETCURSOR` return values, `LoadImage(LR_SHARED)` with a null module, `DestroyWindow` on a thread other than the creator, `WaitableTimer` cancel semantics, and the claim that a message-only window receives `WM_APP + 1` inside foreign modal loops
is written from the C# comments, the C# tests and the Win32 documentation, with the C# test as the oracle for the porter. The W8-12d API used by the 13b smoke test is read from the unmerged branch, not compiled against.

## Wave 8 batch B4 (viewing group 16b-16i)

Group 16b-16i of batch B4 of `docs/WAVE-8-PLAN.md` (P-38): the Win32 image presentation `pr_platform_win::shell_viewing` from `PhotoReview.Shell.Win32/Viewing` and `Startup/ShellPresentationSink` (C# `CompareLayer`, `CrossfadeLayer`, `PresentationComposer`, `PresentationPrefetcher`, `KeepWarm`, `ShellMainWindow.KeepWarm`, `ShellViewingRegistration`, `ScrollBars`, `ViewportController`, `Win32PresentationSink`, `ShellPresentationHub`; baseline `d92d6aae`). 16a and 16e (B1) are merged; the WPF sinks stay retired.
Seven contracts (`contracts/W8-16b.md`, `W8-16c.md`, `W8-16d.md`, `W8-16f.md`, `W8-16g.md`, `W8-16h.md`, `W8-16i.md`), 12 stub files under `crates/pr-platform-win/src/shell_viewing/` (every body `todo!()`: `compare`, `composer`, `crossfade`, `keep_warm`, `prefetcher`, `presentation_sink`, `viewport_controller`, `scrollbars/{mod,state,layout,input,paint}`; `mod.rs` is lead-owned and lists them), 10 rows in `docs/PARITY.md` ("Todo (contract W8-16x)"). No `Cargo.toml` change and no `lib.rs` change: the module and every `windows` feature are already there. Nothing is ported yet.

| WU | Unit | Rust | Porter | Test kind |
|---|---|---|---|---|
| W8-16b | `CompareLayer` (166), `CrossfadeLayer` (165) | `shell_viewing::{compare, crossfade}` | Sonnet (red: manual PR review) | F+N (9 + 9 ports; 8 native/golden rows) |
| W8-16c | `PresentationComposer` (354) | `shell_viewing::composer` | Sonnet (red) | F (24 + 4 stale-order ports; 5 native rows) |
| W8-16d | `PresentationPrefetcher` (181), `KeepWarm` (138), `ShellMainWindow.KeepWarm` (39), `ShellViewingRegistration` (24, retired) | `shell_viewing::{prefetcher, keep_warm}` | Sonnet | F (12 + 14 ports + glue tests) |
| W8-16f | `ScrollBars` colours, options, state, geometry queries (about 160 lines) | `shell_viewing::scrollbars::{state, layout}` | Sonnet | F (8 ports) |
| W8-16g | `ScrollBars` mouse handling, paging, painting (about 330 lines) | `shell_viewing::scrollbars::{input, paint}` | Sonnet | F+N (28 entries + 4 native) |
| W8-16h | `ViewportController` (283) | `shell_viewing::viewport_controller` | Sonnet (red) | F (23 ports) |
| W8-16i | `Win32PresentationSink` (103), `ShellPresentationHub` (89) | `shell_viewing::presentation_sink` | Sonnet | F (2 + 5 ports) |

## Lead decisions (Wave 8 batch B4, viewing group)

Written by the contract writer; the lead may overrule. Where a question had a recommended default it was taken (standing instruction). Detail and reasons are in the contracts. Verified while writing: the stubs pass `cargo check -p pr-platform-win --locked`, `cargo clippy -p pr-platform-win --all-targets --locked -- -D warnings` and `rustfmt --edition 2024`.

- **D-W8B4V-1 Types.** The viewing modules use the `pr_app` types in their own API (`RectD`, `PointD`, `SizeD`, `ColorF`, `ImageInterpolation`, the `services::render` traits, `PlatformImage`). The window API (`host::contracts::ShellWindowApi`) and the frame clock (`host::frame_clock::base`) still return the interim host twins (`SizeD`, `PointD`, `FrameTick`, D-W8B3H-1); they are converted field by field where they meet (`crossfade::to_app_tick`, a private `client_size` in the scroll bars, the viewport controller). When the twins are replaced by imports the conversions become identities and are deleted.
- **D-W8B4V-2 Plan cuts and edges (lead actions).** 16f/16g cut: see D-W8B4V-6. 16d does NOT depend on 16c (the prefetcher only produces ids for `retain`); it depends on 12g, 12h, 16a, 13b, 13e. 16i does NOT depend on 16c (the hub raises an event, the window feeds the composer); it depends on 06d and 07h. 16h depends on 01a, 01b, 05a, 01d, 13a, 13b, NOT on 16e or 05h. 16c also needs 12g (merged; `PrefetchStatus`), 12f (F-test cache), 13a. 16b also needs 12a, 12g, 15b (`Animator`), 13a. 16f/16g depend on 16h (the concrete controller) and 16b (`compare::layout::panel`); their tests run only after 16h is ported, so the port order is 16h, then 16b, 16c, 16d, 16f, 16g, 16i. Units 16b..16h also share `tests/common/viewing_fakes.rs` (D-W8B4V-17).
- **D-W8B4V-3 Errors.** A C# member that cannot throw returns a plain value; one that calls the GPU cache or the draw context returns `Result<_, RenderError>`; a use-after-dispose that C# reports with `ObjectDisposedException` is a small enum (`CrossfadeError::Disposed`, `ComposerError::Disposed`). No `unwrap`/`expect`/panic on the UI thread (R0003).
- **D-W8B4V-4 Posting `!Send` closures to the UI thread (cross-unit, needs W8-13j).** `UiAction` is `Send`, but the viewing objects are `Rc`-based. The composer's deadline (`ScheduleOnUi`) and the viewport controller's Render-priority jobs (`RenderPoster`) are therefore SEAMS the host implements: it keeps the `Box<dyn FnOnce()>` in a UI-thread-local queue and posts a `Send` wake job through `Win32UiDispatcher` (same idea as `StripeScheduler`, D-W8B3S-2). Nobody owns that helper yet: recommended in 13j (or a `post_local` on the merged `ui_dispatcher`, lead action).
- **D-W8B4V-5 Prefetcher threading.** `PrefetchHost` is `path_at` plus two requests the host fulfils (`start_lookup(LookupRequest)`: a worker does `lookup_cached`, then the host posts `apply_lookup(generation, found)` at Background priority; `schedule_retry(delay, RetryRequest)`: the host calls `run_retry`), instead of the C# `RunBackground`/`PostToUi`/`ScheduleOnUi` closures. The prefetcher stays pure; only `Send` data crosses threads.
- **D-W8B4V-6 `ScrollBars.cs` is split by responsibility, not at line 245.** The plan cut falls inside `OnMove`. 16f = colours, options, the struct with ALL fields, constructor, state accessors and every read-only geometry query (`current_layout`, `hit_test`, `hovered_thumb_axis`, `to_dip`, `is_over_page_region`); 16g = `TryHandle`, press / move / release, drag, paging, frame hook, `Draw`, `Dispose`. 16f also exposes `set_pointer_for_tests` (`#[doc(hidden)]`) so the hover rule is testable alone. The plan's line ranges (245 / 245) become about 160 / 330.
- **D-W8B4V-7 Objects owned by the window's handler list hold the window by `Weak`** (`ShellScrollBars`, the keep-warm controller's `is_window_active` closure), so there is no `Rc` cycle window <-> handler. Controllers the window does not own (`ViewportController`, composer) hold what they need strongly and hand `Weak` self references to the closures they give out.
- **D-W8B4V-8 `signed_low_word` / `signed_high_word`** (C# `InputNative`, W8-14a) are written as private twins in `scrollbars/input.rs`; the porter switches to the shared helper once `shell_input` exposes it.
- **D-W8B4V-9 No `RefCell` borrow across a callback.** Composer, crossfade layer, scroll bars and viewport controller call the window, the clock, the cache and host callbacks that can re-enter them (D-W8B3H-6); state is `Cell`/`RefCell`, borrowed for single reads/writes only, handlers run on snapshots.
- **D-W8B4V-10 Cross-thread marshalling of `Win32PresentationSink` is not ported.** The Rust sink and hub are `!Send` (`Rc`), so the ADR 0005 situation (an update from another thread, the FIFO `_queued` guard, `CrossThreadPresentCount`) cannot be written; 4 C# tests have no twin. PARITY row `D-presentation-sink-cross-thread` (`Different`, compile-time) proposed; the lead decides whether an ADR is still wanted (AGENTS.md).
- **D-W8B4V-11 `SinkCallbacks` twin (cross-unit, W8-09a/09b/17c).** `pr_shell::composition::contracts::{MainViewModelSinkCallbacks, PresentationSinkFactory}` cannot be named from `pr-platform-win` (`pr-shell` depends on it), and the 09a contract's "W8-16i implements `PresentationSinkFactory`" cannot hold. 16i defines `SinkCallbacks` (the four closures plus `take_viewer_zoom_change`, which the 4-closure struct lacks; W8-08a O1) and a `create` method; the composition root adapts. Alternative: move both types into `pr_app::coordinators` (pure move).
- **D-W8B4V-12 `Arc<PixelBuffer>` ownership.** The composer and the crossfade layer hold the buffers they draw as `Arc<PixelBuffer>`, the compare frame borrows them for one frame; `PixelBuffer::dispose` needs `&mut`, so "the owner disposed it under us" cannot happen. Four C# tests have no twin (`OutgoingBufferDisposedMidFade_*` x2, `PendingUpgrade_DisposedByItsOwner_IsDropped`, `BeginCrossfade_AfterTheDrawnBufferWasDisposed_HasNothingToFreeze`); the `is_disposed()` branches are ported and tested with buffers disposed before wrapping. INV-1 holds: every bitmap is the cache entry of `PixelBuffer::id()`.
- **D-W8B4V-13 `PrefetchCache` local trait.** `pr_app`'s `PrefetchStatus` has `is_fully_uploaded` and `UploadCompleted` but no `contains`; the prefetcher needs `contains` + `prefetch`. 16d defines a two-method trait and implements it for `D2dGpuImageCache` through `inner().contains`. Alternative: add `contains` to `PrefetchStatus` (merged `pr-app`, lead action).
- **D-W8B4V-14 Two tick domains.** Composer, crossfade, viewport controller and scroll bars use QPC ticks (`host::frame_clock::base::{qpc_now, qpc_frequency}`, the unit of `FrameTick::timestamp`); keep-warm uses the nanoseconds of `render::keep_alive::monotonic_ticks` (D-W8B3S-8). `Animator` durations are `TimeSpan` ticks (100 ns). The hook rendering time is `trunc(rendering_time_ms * 10_000)` 100 ns ticks (the `RenderFrameHandler` unit).
- **D-W8B4V-15 Keep-warm environment.** `KeepWarmOptions::from_environment(get)` reads `PHOTOREVIEW_RS_GPU_KEEPWARM`, `..._MS` (250-60000, default 5000), `..._INTERVAL_MS` (5-100, default 20) with .NET `int.TryParse` rules; `register_keep_warm` reads the process environment once (D-W8B3S-7). The glue `ShellMainWindow.KeepWarm.cs` (`register_keep_warm`, `KeepWarmSetup::dispose`) lives in 16d; `ShellViewingRegistration` (DI) is retired.
- **D-W8B4V-16 `ViewportLayoutEngine` and trait adapters.** `viewport::state::LayoutEngine` has no implementation in `pr-app`; 16h adds the unit struct `ViewportLayoutEngine` over `viewport::layout::{compute, clamp_offset}`. 16h exposes the `ImageSurface` and `FitSurface` members as inherent methods; the trait impls (C# `ViewportImageSurface`) are W8-14c.
- **D-W8B4V-17 Shared test fakes.** `crates/pr-platform-win/tests/common/viewing_fakes.rs` (fake window, fake render poster, fake host clock, recording draw context, policy-cache adapter, prefetch/keep-warm fakes, `ScrollBarRig`, fake perf log) is created by the first unit that ports (any of 16b/16f/16h) and extended by the others in sequence, never concurrently (precedent: W8-13e/13f `shell_window_fixture.rs`).

Cross-unit assumptions to confirm when the sibling contracts land (none was readable while this was written; the sibling worktrees were still empty): **13g/13i/13j** build the `KeepWarmRegistration`, call `register_keep_warm` and `dispose` first in `release_renderer`, construct `ShellScrollBars` before the input handler, call `bars.draw` after the composer and before the overlay, own the `ScheduleOnUi` / `RenderPoster` / `PrefetchHost` implementations (D-W8B4V-4/5), pass one scroll-bar thickness to `ViewportControllerOptions` and `ScrollBarOptions`, subscribe `hub.subscribe_image_changed` and call `hub.note_frame_presented`; **14a/14c** expose `signed_low_word` and implement `ImageSurface` / `FitSurface` over `Rc<ViewportController>`; **13d** keeps `Win32UiDispatcher::post_with_priority` as the way to wake the UI thread.

Open items (recommended defaults taken, listed in the contracts): `CompareFrame` borrowed vs owned (16b Q1); `Rc<dyn PrefetchStatus>` as the composer's cache type (16c Q2); `PrefetchCache` local trait vs a `contains` on `PrefetchStatus` (16d Q1); `SinkCallbacks` twin vs moving the callbacks into `pr_app` (16i Q1); the `Different` row for the dropped cross-thread guard (16i Q2); concrete `Rc<ViewportController>` in the scroll bars (16f Q1); local signed-word helpers (16g Q1); the re-entrant `WM_CAPTURECHANGED` order (16g Q5); `RenderPoster` as a seam (16h Q1).
Not verified here (exact): no behaviour was run. The stubs compile and pass clippy; every number copied from a C# test (layout rectangles, thumb sizes, opacity table, repeat timings) was read from the test source, not recomputed in Rust; the claims that `GpuKeepAlive::activate` is thread-safe, that the host frame clock tolerates unsubscribing from inside a handler, and that `D2dRenderSurface::begin_draw_context` yields a `DrawContext` usable with `D2dGpuImageCache` are taken from the merged sources and doc comments, not exercised; the .NET `int.TryParse` whitespace set and the `TimeSpan.FromMilliseconds` truncation (16h) are from .NET documentation, not from a run; the re-entrancy of `WM_CAPTURECHANGED` during `SetCapture` is unverified; whether the 12f policy cache over a fake backend can host the composer's F tests (the test adapter must implement `GpuImage` for its own fake image) is unverified.

## Wave 8 batch B4 (overlay group 15f-15j)

Group 15f-15j of batch B4 of `docs/WAVE-8-PLAN.md` (P-38): the Win32 parts of the overlay HUD, `PhotoReview.Shell.Win32/Overlay` (`OverlayHost`, `OverlayMessageHandler`, `OverlayNative`, `ShellOverlayServices`, `NativeTooltipHost`, `DeferredTooltips`, `ShellOverlay`, `ShellMainWindow.Overlay`; baseline `d92d6aae`; 1 223 C# lines in the five rows). Five contracts (`contracts/W8-15f.md` .. `W8-15j.md`), 13 stub files under `crates/pr-platform-win/src/shell_overlay/` (every body `todo!()`), `#[cfg(windows)] pub mod shell_overlay;` in `lib.rs`, one dependency edge in `crates/pr-platform-win/Cargo.toml` (`pr-l10n`, plus the one-line `Cargo.lock` change it causes). The pure element tree 15a-15e is `pr_app::overlay` (batch B1); the main window (13g-13k), input and menus (14), viewing (16) are written by other agents in parallel and are only READ here.

| WU | Unit | Rust | Porter | Test kind |
|---|---|---|---|---|
| W8-15f | `OverlayHost` (101), `OverlayMessageHandler` (111), `OverlayNative` (134, user32 half), `ShellOverlayServices` (19, retired) | `shell_overlay::{host, messages, native}` | Sonnet | F+N (8 host tests + handler tests with fakes; 4 native) |
| W8-15g | `NativeTooltipHost` (191), `DeferredTooltips` (79), tooltip half of `OverlayNative` | `shell_overlay::tooltips` | Sonnet | N + pure (6 native ports, 3 deferral ports, rect and constant tables) |
| W8-15h | `ShellOverlay` options, state, constructor, data entry points, `Arrange`, hover, `HitTest`, auto-hide and fade (about 400 of 675 lines) | `shell_overlay::overlay::{state, layout, hit_test, autohide}` | Sonnet (red: manual PR review) | F (21 + 3 ports) + native golden twin (4) |
| W8-15i | `ShellOverlay` press/release, wheel, keys, Tools menu, `Render`, `SyncTooltips` (about 290 lines) | `shell_overlay::overlay::{input, paint}` | Sonnet (red) | F (about 30 ports) + 3 native + 3 manual |
| W8-15j | `ShellMainWindow.Overlay` (284) | `shell_overlay::main_window` | Sonnet | F (with fakes) + 2 native |

## Lead decisions (Wave 8 batch B4, overlay group)

Written by the contract writer; the lead may overrule. Where a question had a recommended default it was taken (standing instruction). Detail and reasons are in the contracts. Verified while writing: the stubs pass `cargo check -p pr-platform-win --locked` and `cargo clippy -p pr-platform-win --all-targets --locked -- -D warnings` and `rustfmt --edition 2024`.

- **D-W8B4O-1 Module layout.** `shell_overlay::{host, messages, native, tooltips, main_window}` and `shell_overlay::overlay::{state, layout, hit_test, autohide, input, paint}`. Extra files not named by the plan: `native.rs` (15f: the user32 half of `OverlayNative`), `overlay/state.rs` (15h: the struct with all fields, options, seams; the name `core` was avoided because it shadows the crate) and `overlay/autohide.rs` (15h). `shell_overlay/mod.rs` and `overlay/mod.rs` are lead-owned indexes.
- **D-W8B4O-2 `ShellOverlayServices.cs` is retired** (19 lines: one DI registration of `NativeShellFolderDialog`). No container (W8-09). The folder dialog wiring for the toolbar button, the context menu and "Move/Copy to folder" is W8-18a plus the composition root (W8-20a).
- **D-W8B4O-3 `ShellOverlay` is cut by cohesion and call direction, not at C# line 338** (inside the doc of `PointerUp`). 15h: options, constructor, data entry points, `Arrange`, `PointerMoved/Left`, `HitTest`, and the auto-hide block (called by `update`/`pointer_moved`/`settings_changed`), `ApplyModel`, `Dispose` (about 400 lines). 15i: `Render`, `PointerDown/Up/Cancel`, `Wheel`, `KeyDown`, Tools menu, `SyncTooltips` (about 290 lines). One `impl ShellOverlay` block per file (the W8-13e/13f precedent). Signatures are unchanged if the lead reverts the cut.
- **D-W8B4O-4 The host owns the elements once; typed access by id.** C# kept `Toolbar`, `Status`... both as properties and in the host's roots. Rust: `OverlayHost` holds `Box<dyn ElementNode>` roots (`InfoOverlayHost` layer, then `ToolbarPanel`); `with_toolbar(|t| ..)`-style closure accessors replace the properties; `_hot`/`_pressed` become `OverlayButton` ids. `OverlayHost::opacity_store()` (an `OpacityStore` over the roots) replaces `Resolve`: only roots are animated. **Required amendment A1 (lead, `pr_app::overlay::layer`, about 15 lines): `LayerElement::child::<T>(index)` and `child_mut::<T>(index)`** (the `PanelElement` shape); fallback: a private info-layer composite in `ShellOverlay`. Optional A2: `ElementNode::find_mut` if a nested element is ever animated. **A3 (lead, `pr_app::viewmodels::main`): `MainViewModel::overlay_model(&self) -> OverlayModel`** (the owner of C# `OverlayModel.From`, 15a O2).
- **D-W8B4O-5 `OverlayFrameClock`**: the three members of `HostFrameClock` the overlay uses (`subscribe`, `unsubscribe`, `request_frame`) as a trait with an adapter `overlay_clock(Rc<dyn HostFrameClock>)`; `dyn`-to-`dyn` upcasting between unrelated traits does not exist. Tests use a 20-line fake.
- **D-W8B4O-6 `OverlayMenuHost` seam** with the exact signature of 14e's `PopupMenuHost` (`show(&self, &[MenuItemModel], PointD) -> Result<Option<String>, Box<dyn Error>>`, read from branch `wave8/b4-contracts-input`), so 15i does not depend on 14e; the lead replaces it by `pub use` or a blanket impl at merge.
- **D-W8B4O-7 No `RefCell` borrow across a callback.** `ShellOverlay` is an `Rc` with `&self` methods and one `RefCell<OverlayState>`; every method reads the injected closures first, mutates under a short borrow, collects effects (render request, clock request, tooltip, command and menu calls), releases, then runs them. The Tools menu is modal and the window keeps pumping messages inside it.
- **D-W8B4O-8 One frame subscription, fixed order.** `pr_app`'s `Animator` and `FrameDelayTimer` do not subscribe (W8-15b); `ShellOverlay` keeps one subscription while the animator or either timer runs. Order inside a frame: animator, toolbar timer, info timer (the C# tests need a fade started by a timer in frame N to start in frame N+1). The rare C# subscription-order case that starts a tween one frame earlier is not reproduced and not observable in the C# tests.
- **D-W8B4O-9 Commands are synchronous** (W8-15e O3, D-W8-2). C# `RunGuarded`/`ObserveFailure` (command name + exception to `OnCommandFailed`, cancellation is no failure) have no Rust twin in the overlay: the command implementation (`ViewModelOverlayCommands` -> `report_action_failed`) reports. `on_command_failed` stays for menu errors. Four C# tests are retargeted to 15j (`contracts/W8-15i.md` item 6).
- **D-W8B4O-10 Cross-thread hand-over.** `SettingsStore::subscribe_changed` and `Localizer` `on_current_changed` take `Send + Sync` closures that may run on any thread; they only record the event in `OverlayPending` and call `UiWake::wake`; the main window then calls `MainWindowOverlay::run_pending` on the UI thread (settings first, then language). Replaces the C# `_dispatcher.Post`.
- **D-W8B4O-11 Cargo edge `pr-platform-win -> pr-l10n`.** `shell_overlay` names `pr_l10n::localizer::Localizer` (the panels take `&Localizer`, W8-15e); `pr-app` does not re-export it. `crates/pr-platform-win/Cargo.toml` gets `pr-l10n.workspace = true` and `Cargo.lock` one line (no cycle: `pr-l10n` depends on `pr-core` only). Done with the stubs, so `--locked` builds.
- **D-W8B4O-12 `OverlayInput` trait** (15f `messages.rs`): the eight methods the message handler drives; `ShellOverlay` implements it (15i). The handler is testable with a recording fake and 15f does not wait for 15h/15i.
- **D-W8B4O-13 `LocalPoster`** (15g `tooltips.rs`): posts non-`Send` UI-thread jobs; `pr_core`'s `UiAction` is `Send`. The 13g stub (branch `wave8/b4-contracts-main`) declares an identical trait and one implementation `UiLocalPoster`; the lead re-exports it from `shell_overlay::tooltips` when 13g is on master. `DeferredTooltips` posts `create_now` at `Background` through a `Weak` job.
- **D-W8B4O-14 Cross-unit assumptions on the main window (13g-13k) and input (14), all open questions with defaults.** (a) `MainWindowOverlay` implements the 13g `MainWindowFeature` hook trait and is registered with `add_feature` (`draw_overlay`, `on_changes(&ChangeSet)`, `dispose`); (b) the window implements `UiWake` with its `coalesced_wake` and `OverlayViewModel` over `update_view_model`; (c) the Fit button's closure is mandatory (`OverlayDeps::fit`, supplied by 14c); the C# fallback (`OverlayFitSurface` + own `FitViewController`) is not ported; (d) handler order: overlay inserts FIRST, as 14c's contract states; (e) **finding**: the baseline never assigns `ShellInput.OverlayHitTest`/`ActivationKeyOwned`/`EscapeInterceptor` nor subscribes `PointerActivity`; the 14b downstream note "W8-15j sets OverlayHitTest..." is wrong and 15j does not set them.

Missing plan edges (lead actions): 15f needs 11c/11d/11e (wrappers), 13b (contracts), 12a, and 13e for one native test, NOT 15h/15i (D-W8B4O-12); 15g needs 11b, 11d, 13b; 15h needs 15b, 06b (done), 12i (golden twin), 13a/13b; 15i needs 15f (`OverlayInput`, `VK_*`), 15e, 12e/12i (native); 15j needs 15g, 15f, 08a-08e (`ChangeSet`), 13e/13f, 14c (Fit), 12i; 13g/13h are needed only for the final feature wiring. 14g depends on 15b only (the overlay palette), not on this group.

Still stubs on master, so these units cannot be PORTED before their dependencies: `PanelElement`, `FolderInfoPanel`, `CapturePairBadge` (15d); `StatusPanel`, `ToolbarPanel`, `ZoomIndicatorPanel`, `tools_menu::{build, execute}` (15e). Implemented on master: `contracts`, `element`, `layer`, `model` (15a), `animator`, `frame_delay`, `palette` (15b), `button`, `text` (15c), the policies and `FadeTargetGate` (06b), `MainViewModel` and `ChangeSet` (08a-08e), `D2dDrawContext` (12e), the text renderer (12i), `ShellWindow` (13e/13f), the comctl and user32 wrappers (11b-11e). 15f's handler, native and `OverlayNative` tests can be ported at once; 15f's host tests need 15d; 15g needs only 11c; 15h-15j need 15d and 15e.

Open items (recommended defaults taken, listed in the contracts): `client_to_screen` wrapper in 11d vs a local `unsafe` (15f Q3); `ICC_BAR_CLASSES` value (15g Q2); the cut of `ShellOverlay` (15h Q1); `LayerElement::child_mut` (15h Q3); command failure channel (15i Q1); Tab on the last button swallowed (15i Q4); `UiWake`/`LocalPoster` shape (15j Q1); `MainViewModel::overlay_model` (15j Q2); Fit fallback (15j Q3); `settings_store` as a concrete type (15j Q7).
Not verified here (exact): no native behaviour was run. Everything about `tooltips_class32` with the 72-byte `TOOLINFOW` under the comctl32 v6 manifest, `TTM_ADDTOOL` copying the text, `TTM_GETTOOLINFOW` returning the stored pixel rectangle, `TrackMouseEvent(TME_LEAVE)` posting `WM_MOUSELEAVE` for a cursor outside the window, `ReleaseCapture` sending `WM_CAPTURECHANGED` synchronously (the `releasing` guard), a `WM_MOUSEMOVE` reaching the window during `TrackPopupMenu`, the 2 DIP golden tolerance with DirectWrite on a machine without Segoe UI, and the cross-unit stubs read from three sibling branches (not compiled against) is written from the C# source, the C# tests and the Win32 documentation, with the C# test as the oracle for the porter. The `Localizer`/`SettingsStore` handler thread claims come from reading `pr_l10n::localizer` and `pr_core::settings::store`. The pure logic was not compiled as Rust tests (the stubs compile; `cargo check` and `clippy -D warnings` are clean).

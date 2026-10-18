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

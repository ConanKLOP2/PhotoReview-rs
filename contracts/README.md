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

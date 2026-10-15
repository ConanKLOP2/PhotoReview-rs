# Plan port bằng nhiều agent (ưu tiên Haiku)

**Ngày:** 2026-10-14 · **Phạm vi:** Phase 1–6 trong [PLAN.md](PLAN.md) · **Spec:** C# baseline trong [SYNC-LEDGER.md](SYNC-LEDGER.md)

## 0. Kết luận ngắn

**Được, nhưng không nên dùng Haiku cho 100% công việc.** Haiku hợp với các đơn vị nhỏ, đã có spec rõ, có golden
test để tự kiểm: chiếm khoảng 60–65% số đơn vị (model, enum, parser, codec, các policy thuần, RAW container reader,
PerfAnalysis, port test, cửa sổ phụ đơn giản). Các phần dễ âm thầm làm hỏng dữ liệu hoặc tốc độ phải để model mạnh
hơn: journal, file action, undo/recovery (INV-3/4/5/6), PreviewImageService/PreloadScheduler (INV-1/2), COM/FFI
`unsafe` (WIC, Shell, Recycle Bin, LibRaw, TurboJPEG), MainWindow/Presenter, cùng mọi việc merge, tích hợp và chốt hợp đồng.
Cách chia này đúng quy tắc trong AGENTS.md của repo C#: dùng model rẻ nhất đủ sức, và model mạnh nhất cho việc rủi ro.

Haiku làm được việc là nhờ ba thứ:
1. **Hợp đồng viết trước:** chữ ký Rust, danh sách test và file được phép sửa do model mạnh viết, Haiku chỉ điền phần thân.
2. **Golden test sinh từ chính code C#:** Haiku không cần hiểu sâu mà vẫn biết mình đúng hay sai.
3. **Không tin báo cáo của Haiku:** lead tự build và test lại trên kết quả đã merge.

---

## 1. Vai trò và model

| Vai trò | Model | Việc | Không được làm |
|---|---|---|---|
| **Lead / Integrator** | Opus (phiên chính) | Chia wave, viết hoặc duyệt hợp đồng, merge nhánh, chạy gate, cập nhật PARITY/task_on_progress, mở PR | Tin báo cáo "done" mà không chạy lại gate |
| **Contract Writer** | Opus cho đơn vị đỏ, Sonnet cho đơn vị vàng/xanh | Đọc C#, viết `contracts/WU-xxx.md` + stub Rust (chữ ký, doc comment, `todo!()` trong thân) + tên test | Viết phần thân implementation |
| **Golden Generator** | Sonnet | Viết, chạy `tools/oracle-cs` (mục 4) để sinh `tests/golden/**` | Sửa repo C# |
| **Test Porter** | **Haiku** | Port test C# của đơn vị thành test Rust (đỏ trước), dùng golden | Sửa code `src/` |
| **Porter** | **Haiku** (xanh), Sonnet (vàng), Opus (đỏ) | Điền thân hàm cho test xanh | Đổi chữ ký public, thêm dependency, viết `unsafe`, sửa file ngoài danh sách |
| **Verifier** | Sonnet | Đọc C# và Rust song song, tìm khác biệt hành vi, chạy mutation mẫu, trả verdict PASS/FAIL kèm bằng chứng | Tự sửa code (chỉ báo lỗi) |

Màu đơn vị:
- 🟢 **Xanh (Haiku port):** logic thuần, có golden, không I/O hoặc I/O qua trait đã có fake, không `unsafe`, không thread.
- 🟡 **Vàng (Sonnet port, Haiku port test):** có I/O thật, debounce, atomics, parse format trên đĩa, logic nhiều nhánh.
- 🔴 **Đỏ (Opus port, Haiku port test):** INV an toàn dữ liệu, concurrency nhiều thread, COM/FFI `unsafe`, đường nóng hiệu năng.

---

## 2. Đơn vị công việc (Work Unit, WU)

### 2.1 Quy tắc chia
- Mỗi WU port **≤ khoảng 400 dòng C#** (Haiku) hoặc ≤ khoảng 900 dòng (Sonnet/Opus), và tạo ra **1–3 file Rust do riêng WU đó sở hữu**.
- Một WU chỉ phụ thuộc API **đã merge vào master**, hoặc stub đã có trong hợp đồng của wave đó.
- Các file dùng chung (`lib.rs`/`mod.rs`, `Cargo.toml`, `PARITY.md`, `task_on_progress.md`) **chỉ lead sửa**. Contract PR của
  wave khai báo sẵn mọi `mod`, nên agent không bao giờ phải chạm vào chúng. Hai agent chạy song song vì vậy không thể conflict.

### 2.2 Vòng đời một WU

```text
[Contract] Opus/Sonnet ─► contracts/WU-xxx.md + stub (todo!()) ─► lead merge "contract PR" của wave
     │
     ├─► [Golden] Sonnet: tests/golden/<area>/WU-xxx/*.json   (nếu WU cần)
     │
     ├─► [Test port] Haiku: test Rust đỏ (fail vì todo!())
     │
     ├─► [Port] Haiku/Sonnet/Opus: điền thân hàm cho test xanh
     │        tự chạy: cargo fmt; cargo clippy -p <crate> --all-targets -- -D warnings; cargo test -p <crate>
     │        trả JSON report (mục 5.3)
     │
     ├─► [Verify] Sonnet: so C# và Rust, chạy mutation mẫu ─► PASS / FAIL(+bằng chứng)
     │        FAIL lần 1 → trả về Porter kèm diff cụ thể; FAIL lần 2 → nâng model (Haiku→Sonnet→Opus)
     │
     └─► [Integrate] Lead: merge vào integration/wave-N, chạy GATE (mục 6), cập nhật PARITY, mở PR
```

### 2.3 Mẫu hợp đồng `contracts/WU-xxx.md`

```markdown
# WU-xxx: <tên>
- Color: 🟢 | Porter: haiku | Test porter: haiku | Verifier: sonnet
- C# source (baseline e441d67a): src/PhotoReview.Core/Catalog/ManagedNaturalComparer.cs (143 lines)
- C# tests to port: tests/**/ManagedNaturalComparer*Tests.cs (list each file and test name)
- PARITY rows: K-.., S-.., INV-.. (rows this WU satisfies)
- Owned files (the ONLY files you may create/modify):
    crates/pr-core/src/catalog/natural_cmp.rs
    crates/pr-core/tests/natural_cmp.rs
- Read-only inputs: tests/golden/catalog/natural_cmp/*.json
- Public API (already stubbed, do NOT change signatures):
    pub fn compare_natural(a: &str, b: &str) -> std::cmp::Ordering
- Semantics notes (from contract writer): digits compared numerically, leading zeros ..., case-insensitive ordinal ...
- Traps checklist: §7 items 1, 3, 5
- Done when: all listed tests pass, clippy clean, every C# test has a Rust twin with `// C#: <Class>.<Method>`.
```

---

## 3. Danh sách WU theo wave

Ký hiệu cột Model: **P** = Porter, **T** = Test porter. Khi không ghi T thì Test porter là Haiku.
Số dòng là kích thước file C# ở baseline.

### Wave 0: Nền móng (tuần tự, không song song). Mọi wave sau phụ thuộc wave này.

| WU | Nội dung | Model | Ghi chú |
|---|---|---|---|
| W0-01 | ADR R0003 concurrency: thread pool, channel, `Generation(AtomicU64)`, quy tắc UI thread (C# ADR 0005), cancellation token | Opus | Phải có trước mọi WU vàng/đỏ |
| W0-02 | `pr-core::abstractions`: trait cho `IFileSystem`(138), `IClock`/`SystemClock`, `IDisplayClock`(123), `ILog`/`NullLog`, `IRecycleBin`, `IUiScheduler`/`ImmediateUiScheduler`, `ISourceReader`, `IMemoryProbe`, `IExplorerOrderProvider`, `ILiveOperationRegistry`(70), `IJournalCompactionFiles`, `INaturalComparer`, `IDialogService`, `IKeyNameValidator`, `IAppPaths` | Opus | Hình dạng trait quyết định mọi thứ sau, không giao Haiku |
| W0-03 | Crate `pr-test-support`: `FakeFileSystem`, `FakeClock`, `FakeRecycleBin` (không bao giờ đụng thùng rác thật), `FakeUiScheduler`, helper đọc golden | Sonnet | Port từ `tests/PhotoReview.TestSupport` |
| W0-04 | `tools/oracle-cs` + quy ước golden (mục 4) | Sonnet | Không sửa repo C# |
| W0-05 | Kiểu lỗi chung (`thiserror`), quy ước log, chọn crate: `serde`, `serde_json`, `thiserror`, `crossbeam-channel`, `rayon`, `windows`, `parking_lot` | Opus | Chỉ lead thêm vào `[workspace.dependencies]` |
| W0-06 | Contract PR cho Wave 1: khai báo `mod`, stub và `contracts/WU-*.md` của toàn wave | Opus+Sonnet | |
| W0-07 | Script `tools/wave.ps1`: tạo worktree cho từng WU, chạy gate, gom report JSON | Sonnet | |

### Wave 1: Lá của Core, gần như toàn Haiku (song song tối đa 8)

| WU | C# nguồn (dòng) | Rust | Màu | P |
|---|---|---|---|---|
| W1-01 | Model/*: `ImageSortMode`, `InitialViewMode`, `LoadingMode`, `DecoderBackend`, `RawPairMode`, `RawFullDecode`, `MouseWheelAction`, `ScalingQuality`, `ImageTransition`, `InstanceMode`, `JournalState`, `FileOperationType`, `JournalDurability`, `FitWidthAnchor`, `KeyboardZoomAnchor`, `KineticGlideSmoothing` (~300 tổng) | `pr-core::model` | 🟢 | Haiku |
| W1-02 | `LenientEnumConverter`(142), `FlagEnumJson`(66), `SettingsEnumConverter`(22), `JsonAliasAttribute`(10) → serde lenient deserialize | `pr-core::model::serde_ext` | 🟡 | Sonnet |
| W1-03 | `TitleBarFields`(54), `ExifInfoFields`(52) | `pr-core::model` | 🟢 | Haiku |
| W1-04 | `ShortcutMappings`(79), `ShortcutKeyCanonical`(66), `ReviewAction`(21) | `pr-core::settings::shortcuts` | 🟢 | Haiku |
| W1-05 | `PerformanceOptions`(49) | `pr-core::settings::perf` | 🟢 | Haiku |
| W1-06 | `ManagedNaturalComparer`(143) | `pr-core::catalog::natural_cmp` | 🟢 | Haiku |
| W1-07 | `ImageFileTypes`(45), `CatalogEntry`(46), `CaptureGroup`(62), `ExplorerReason`(45), `ExplorerOrderTypes`(39), `LatestExplorerSnapshot`(17) | `pr-core::catalog::types` | 🟢 | Haiku |
| W1-08 | `BoundedLruCache`(82) | `pr-core::caching::lru` | 🟢 | Haiku |
| W1-09 | `FileFingerprint`, `FileActionRequest/Result`, `CaptureGroupAction*`, `UndoResult`, `JournalGroupMember`, `CopyCreationProof`, `SkippedEntry` (~150) | `pr-core::file_actions::types` | 🟢 | Haiku |
| W1-10 | `AppVersion`(55), `UpdateUrlPolicy`(22), `UpdateCheckResult`(26) | `pr-core::updates` | 🟢 | Haiku |
| W1-11 | `LocTemplate`(172), `LocArg`, `UserFacingError`(45), `TranslatorModes`(63) | `pr-l10n::template` | 🟢 | Haiku |
| W1-12 | `GenerationClock`(77) | `pr-core::catalog::generation` | 🟡 | Sonnet |
| W1-13 | `JournalErrors`(128), `FileSystemErrors`(17), `RecycleBinCapacity` (Core, 23) | `pr-core::file_actions::errors` | 🟢 | Haiku |
| W1-14 | `JournalLineParser`(113) + golden các dòng journal thật | `pr-core::file_actions::journal_line` | 🟢 (golden bắt buộc) | Haiku |
| W1-15 | `BenchmarkStatistics`(15), `DiagOptions`(91) | `pr-core::diagnostics` | 🟢 | Haiku |
| W1-16…21 | PerfAnalysis: `PerfAnalyzeCsv`(156), `PerfAnalyzeStats`(249), `PerfAnalyzeRules`(201), `PerfAnalyzeNav`(368), `PerfAnalyzeReport`(183), `PerfAnalyze`(214)+`NaNAsNullDoubleConverter` | `pr-perf` | 🟢 (golden CSV→report) | Haiku |

**Gate Wave 1:** toàn bộ golden khớp, PARITY các dòng tương ứng là Done.

### Wave 2: Core tầng giữa

| WU | C# nguồn (dòng) | Màu | P |
|---|---|---|---|
| W2-01 | `AppSettings`(390) + `AppSettingsJsonContext` → struct serde, đúng tên field và default (S-*) | 🟡 | Sonnet |
| W2-02 | `SettingsNormalizer`(274), `SettingsValidator`(41), `SettingsLoadRepairText`(23), `SettingsParseResult` (golden: config hỏng → kết quả repair) | 🟢 | Haiku |
| W2-03 | `SettingsStore`(400), INV-11 (backup/reset, config bị khóa) | 🟡 | Sonnet |
| W2-04 | `LanguageCatalog`(213), `LanguageLoader`(144), `Localizer`(156), `BuiltInCatalog` + `build.rs` sinh key từ `assets/languages/en.json` | 🟡 | Sonnet (build.rs), Haiku (loader) |
| W2-05 | `ImageSortService`(118), `SiblingFolderService`(24) | 🟢 | Haiku |
| W2-06 | `CaptureGroupBuilder`(182) (JPG+RAW pairing) | 🟢 | Haiku |
| W2-07 | `ComparePairService`(130), `DuplicateFinder`(120) | 🟢 | Haiku |
| W2-08 | `ActionDestinationPolicy`(121) | 🟢 | Haiku |
| W2-09 | `ExplorerSnapshotValidator`(132), INV-7 | 🟡 | Sonnet |
| W2-10 | `ReviewCatalog`(529) | 🟡 | Sonnet |
| W2-11 | `DragDropInputService`(72) | 🟢 | Haiku |
| W2-12 | `SessionStore`(171), ghi atomic theo ADR 0007 | 🟡 | Sonnet |
| W2-13 | `SessionWriter`(292) (debounce, thread) | 🔴 | Opus |
| W2-14 | `PhysicalFileSystem`(340), `CountingFileSystem`(39), `PhysicalSourceReader`: share flag, SequentialScan (INV-8) | 🔴 | Opus |
| W2-15 | `NavigationStatWorker`(86) | 🟡 | Sonnet |
| W2-16 | `FileLog`(345), INV-10 (tắt thì không tạo file) | 🟡 | Sonnet |
| W2-17 | `PerfCsvListener`(367), `PhotoReviewPerf`(254), `ReviewMetrics`(183) | 🟢 | Haiku (format CSV có golden) |
| W2-18 | `UpdateChecker`(126) (HTTP qua trait, test bằng fake) | 🟢 | Haiku |
| W2-19 | `ForwardedPathProtocol`(89), `ForwardedOpenCoalescer`(80), `SecondInstanceHandoff`(31), `FolderOwnership`(29), `InstanceForwarding`(21) | 🟡 | Sonnet |

### Wave 3: Core an toàn dữ liệu (đỏ, Haiku chỉ port test)

Chạy theo kiểu **test trước:** Haiku port toàn bộ test C# của mỗi WU thành test Rust đỏ, sau đó Opus mới viết code.

| WU | C# nguồn (dòng) | Màu | P / T |
|---|---|---|---|
| W3-01 | `OperationJournal`(847), `JournalTransaction`(218): format dòng giống hệt, Prepared → Committed/Failed | 🔴 | Opus / Haiku |
| W3-02 | `JournalCompactionPlan`(126), `PhysicalJournalCompactionFiles`(124) | 🔴 | Opus / Haiku |
| W3-03 | `JournalStartupRecovery`(75) (ADR 0003, gồm cả "older builds") | 🔴 | Opus / Haiku |
| W3-04 | `FileActionService`(704): INV-3, INV-4 | 🔴 | Opus / Haiku |
| W3-05 | `UndoService`(656): INV-5, INV-6 | 🔴 | Opus / Haiku |
| W3-06 | `RecoveryRetryService`(355) | 🔴 | Opus / Haiku |
| W3-07 | `RecoveryFileCheck`(239): bảng verdict trong architecture.md | 🟡 | Sonnet / Haiku (test bảng) |
| W3-08 | `PartialDestinationCleanup`(29) | 🟡 | Sonnet |

**Gate Wave 3:** `cargo-mutants` trên `file_actions::*` không còn mutant sống ở đường ghi journal; oracle diff của chuỗi
action → journal khớp C#; Verifier chạy bằng Opus thay vì Sonnet.

### Wave 4: Imaging

| WU | C# nguồn (dòng) | Màu | P |
|---|---|---|---|
| W4-01 | `ImageCacheKey`(108), `DecodeBox`(49), `DecodeRequest`(44), `ImageInfo`, `DecodedImageSize`, các exception → enum lỗi | 🟢 | Haiku |
| W4-02 | `ExifOrientation`(105), `EmbeddedThumbnailReader`(92) | 🟢 | Haiku |
| W4-03 | `TiffStructure`(159), `ExifParser`(217) (golden: fixture → EXIF) | 🟢 | Haiku |
| W4-04 | `ExifSummary`(96), `ExifSummaryCodec`(137), `ExifQueryInterpreter`(80) | 🟢 | Haiku |
| W4-05 | `AdaptivePreviewPolicy`(58), `PreloadWindow`(29), `PreloadOrderService`(41), `PreloadOptions` | 🟢 | Haiku |
| W4-06 | `NavigationPace`(124), `PreloadBusyBackoff`(60), `PreviewSizeSampler`(41) | 🟢 | Haiku |
| W4-07 | `RamBudgetPolicy`(251), `MemoryHeadroom`(43) | 🟡 | Sonnet |
| W4-08 | `AtomicCacheFile`(57), `PreviewCacheFile`(340) (format file cache) | 🟡 | Sonnet |
| W4-09 | `DiskCacheStore`(321), INV-2 | 🔴 | Opus |
| W4-10 | `SourceBytesCache`(238)+policy, `ThumbnailCache`(253), INV-2/INV-8 | 🔴 | Opus |
| W4-11 | `WicInterop`(422) + `WicDirectDecoder`(752): COM `unsafe` | 🔴 | Opus |
| W4-12 | `FallbackImageDecoder`(122), `ImageDecoderFactory`(71), INV-12 | 🟡 | Sonnet |
| W4-13 | Thay `WpfBitmapImageDecoder`(254)/`WpfImageAdapter`/`WpfExifReader`/`WpfDecodedImage` bằng decoder fallback WIC tổng quát / crate `image` | 🟡 | Sonnet |
| W4-14 | `PreviewImageService`(889), INV-1/INV-2 | 🔴 | Opus |
| W4-15 | `PreloadScheduler`(827) | 🔴 | Opus |

### Wave 5: RAW + TurboJPEG + LibRaw

| WU | C# nguồn (dòng) | Màu | P |
|---|---|---|---|
| W5-01 | `RawFormat`, `RawFileTypes`, `RawContainerInfo`, `RawContainerLimits`, `EmbeddedPreview`, `ExifBlock`, `RawHeaderErrors`, các trait (~200) | 🟢 | Haiku |
| W5-02 | `TiffHeaderNavigator`(419), `JpegMarkerProbe`(79) | 🟡 | Sonnet (nền chung của các reader) |
| W5-03…08 | `Cr2`(148), `Nef`(266), `Orf`(258), `Dng`(217), `Arw`(170), `Rw2`(106) ContainerReader, mỗi cái 1 WU | 🟢 (golden: corpus RAW → offset/size preview) | Haiku |
| W5-09 | `RafContainerReader`(143) | 🟢 | Haiku |
| W5-10 | `BmffBoxNavigator`(118), `Cr3ContainerReader`(383) | 🟡 | Sonnet |
| W5-11 | `RawContainerReaderRegistry`(48), `PreviewSelector`(355), `RawExif`(107), `RawJpegIccProfile`(62) | 🟡 | Sonnet |
| W5-12 | `SourceRawHeaderSource`(159), `RawDecoder`(603), `FormatRoutingDecoder`(63) | 🟡 | Sonnet |
| W5-13 | TurboJPEG FFI: `TurboJpegNative`(136), `SafeTurboJpegHandle`, `TurboJpegAvailability`(56) | 🔴 | Opus |
| W5-14 | `TurboJpegDecoder`(656) | 🟡 | Sonnet (dựa trên W5-13) |
| W5-15 | LibRaw FFI: `LibRawNativeMethods`(193), `SafeLibRawHandle`, `LibRawAvailability`(154) | 🔴 | Opus |
| W5-16 | `LibRawDecoder`(468), `FullDecodeGate`(156), `DecodeMemoryGuard`(91) | 🔴 | Opus |
| W5-17 | `RgbBgraResampler`(209) (golden pixel) | 🟢 | Haiku |

### Wave 6: Platform Windows

| WU | C# nguồn (dòng) | Màu | P |
|---|---|---|---|
| W6-01 | `ComEnumeration`(120), `ExplorerComInterop`(65), `ExplorerWindowSelector`(83), `ExplorerOrderService`(509) | 🔴 | Opus |
| W6-02 | `WindowsRecycleBin`(452), `RecycleBinCapacity`(230): test chỉ dùng fake, test native chỉ xóa item của chính nó | 🔴 | Opus |
| W6-03 | `InstanceKeys`(68), `InstanceScope`(281), `InstanceForwardServer`(154), `InstanceForwardClient`(105) | 🟡 | Sonnet |
| W6-04 | `WindowsDisplayClock`(323) | 🟡 | Sonnet |
| W6-05 | `WindowsMemoryProbe`(56), `WindowsNaturalComparer`(39), `WindowsDarkTitleBar`(67), `WindowsLiveOperationRegistry`(75) | 🟢 | Haiku (lời gọi Win32 đơn giản; `unsafe` cần Sonnet duyệt) |

### Wave 7: Benchmarking

| WU | C# nguồn (dòng) | Màu | P |
|---|---|---|---|
| W7-01 | `BenchmarkModels`(146), `BenchmarkProfiles`(60), `BenchmarkMetrics`(54) | 🟢 | Haiku |
| W7-02 | `SlowLinkFileSystem`(186), `SlowLinkSourceReader`(35), `BenchmarkRecycleBin`(32) | 🟢 | Haiku |
| W7-03 | `BenchmarkEngine`(104), `BenchmarkImageExecutor`(234), `BenchmarkWorkloadRunner`(243) + CLI có output cùng format với `PhotoReview.Benchmark.Cli` | 🟡 | Sonnet |

### Wave 8: App/UI (chỉ bắt đầu sau khi R0001 đã chốt)

Chia lại chi tiết sau R0001, vì cấu trúc phụ thuộc framework. Khung dự kiến:

| WU | Nội dung | Màu | P |
|---|---|---|---|
| W8-01 | Composition root (AppHost), vòng đời app, single instance | 🔴 | Opus |
| W8-02 | `MainViewModel` + coordinator (`FolderLoadCoordinator`, INV-9) | 🔴 | Opus |
| W8-03 | `ImagePresenter` (choke point), canvas zoom/pan/Fit (ADR 0008), Compare | 🔴 | Opus |
| W8-04 | `ShortcutRouter` + input (K-* rows) | 🟡 | Sonnet |
| W8-05 | SettingsWindow (từng tab là 1 WU) | 🟢 | Haiku (layout spec + view model có sẵn) |
| W8-06 | ActionProfilesWindow, ClickZoomCustomDialog, DiagnosticsWindow | 🟢 | Haiku |
| W8-07 | RecoveryWindow + RecoveryPathPanel, BatchReviewWindow | 🟡 | Sonnet |
| W8-08 | BenchmarkWindow | 🟢 | Haiku |
| W8-09 | Menu chuột phải, title bar, thông báo lỗi (C-*, W-*) | 🟡 | Sonnet |

**Tổng (Wave 1–7):** khoảng 95 WU. Haiku port khoảng 55 WU, Sonnet khoảng 25, Opus khoảng 15. Haiku port test cho hầu hết WU, kể cả WU đỏ.

---

## 4. Oracle và golden test (để Haiku tự biết mình đúng hay sai)

- `tools/oracle-cs/`: một console app C# **nằm trong repo Rust**, tham chiếu DLL đã build của repo C#
  (`../PhotoReview/src/*/bin/Release/...`), nên **không sửa repo C#**. Mỗi lệnh con sinh golden cho một khu vực:
  `natural-cmp`, `settings-repair`, `journal-lines`, `exif`, `raw-preview`, `perf-report`, `sort`, `capture-groups`, ...
- Input lấy từ `tests/fixtures/` (đã sync), corpus RAW (`fetch-raw-samples`) và các bảng case viết tay trong `tests/golden-inputs/`.
- Output nằm ở `tests/golden/<area>/*.json`. Header ghi commit C# đã sinh ra nó. Khi đổi baseline thì sinh lại; diff golden = thay đổi hành vi cần port.
- Rust test đọc golden qua `pr-test-support::golden`. Haiku **không được sửa golden**, chỉ Golden Generator sửa.

---

## 5. Prompt và cách giao việc cho Haiku

### 5.1 Môi trường
- Mỗi agent làm trong một worktree riêng: `git worktree add .claude/worktrees/WU-xxx -b wu/WU-xxx origin/integration/wave-N`.
- Dùng chung `CARGO_TARGET_DIR` cho mỗi wave (`target-wave-N`) để không build lại từ đầu 8 lần. Chạy `cargo test -p <crate>`
  chứ không chạy `--workspace`, để giảm khóa build.
- Một lượt song song tối đa **8 Haiku + 2 Sonnet verifier** (giới hạn kích thước workflow mặc định khoảng 10 agent). Wave có
  nhiều hơn 8 WU thì chạy thành nhiều lượt.

### 5.2 Prompt mẫu cho Haiku Porter

```text
You are porting ONE unit of a C# app to Rust. Read these files first, in order:
  1. D:\MyProject\PhotoReview-rs\AGENTS.md
  2. contracts/WU-xxx.md   (your contract: owned files, API, tests, traps)
  3. The C# source files listed in the contract, read via:
     git -C D:\MyProject\PhotoReview show e441d67a:<path>
Rules:
  - Modify ONLY the files listed under "Owned files". Never touch Cargo.toml, lib.rs, mod.rs, docs/, tests/golden/.
  - Do not change any public signature from the stub. Do not add dependencies. Do not write `unsafe`.
  - Do not use unwrap()/expect() on file-system or user-data paths.
  - Keep C# behaviour exactly, including edge cases, error cases and ordering. When the C# code and your intuition
    disagree, the C# code wins.
  - If something cannot be done without breaking a rule (missing API, ambiguous semantics), STOP and write
    contracts/WU-xxx.BLOCKED.md explaining what and why. Do not guess.
Steps:
  1. Implement until these all pass, in this order:
       cargo fmt --all
       cargo clippy -p <crate> --all-targets -- -D warnings
       cargo test -p <crate>
  2. For each C# test listed in the contract, confirm a Rust test exists with a `// C#: Class.Method` comment.
  3. Commit on your branch with message "port(WU-xxx): <name>".
Final answer: ONLY the JSON report from §5.3.
```

### 5.3 Report bắt buộc (JSON)

```json
{
  "wu": "WU-xxx",
  "status": "done | blocked",
  "files_changed": ["..."],
  "commands": {"fmt": "ok", "clippy": "ok", "test": "ok 23 passed"},
  "csharp_tests_ported": 23,
  "csharp_tests_skipped": [{"name": "...", "reason": "..."}],
  "behaviour_doubts": ["chỗ nào không chắc giống C#"],
  "commit": "<sha>"
}
```
Lead chỉ dùng report để biết chạy gì tiếp. Pass hay không do gate quyết định.

### 5.4 Prompt mẫu cho Verifier (Sonnet)

```text
Adversarially verify WU-xxx. Compare the C# source (baseline e441d67a) with the Rust port on branch wu/WU-xxx.
Find behaviour differences: edge cases, error paths, ordering, string/path comparison, number formatting,
overflow, rounding, time handling. For each suspected difference, write a failing Rust test proving it.
Run 5-10 manual mutations of the Rust code (flip a condition, drop a branch); every mutation must make a test fail.
Answer: PASS, or FAIL with the failing tests and the surviving mutations. Do not fix the code.
```

### 5.5 Nâng model khi gặp khó
- Haiku FAIL lần 1: gửi lại đúng Haiku đó kèm test fail và diff.
- Haiku FAIL lần 2 hoặc BLOCKED: chuyển WU cho Sonnet, kèm nhánh của Haiku làm điểm xuất phát.
- Sonnet FAIL 2 lần, hoặc WU hóa ra đụng INV: chuyển cho Opus, đổi màu WU và ghi vào hợp đồng.
- Một wave có hơn 30% WU phải nâng model: dừng lại, viết hợp đồng chi tiết hơn, vì lỗi nằm ở hợp đồng chứ không ở model.

---

## 6. Gate tích hợp (lead chạy, không ngoại lệ)

Sau mỗi lượt merge vào `integration/wave-N` và trước khi mở PR vào `master`:
1. `cargo fmt --all --check`
2. `cargo clippy --workspace --all-targets --locked -- -D warnings`
3. `cargo test --workspace --locked`, giới hạn thời gian theo hang guard
4. `tools/sync-from-csharp.ps1 -Check`; golden không bị sửa ngoài Golden Generator (`git diff --stat tests/golden`)
5. Mỗi WU của wave có Verifier PASS
6. Wave 3 và các WU đỏ: `cargo mutants -p pr-core -f file_actions/**` không còn mutant sống ở đường ghi
7. Cập nhật PARITY.md (Status, Rust location, Proving test) cho mọi dòng WU đã đạt
8. Một PR cho mỗi wave (hoặc mỗi lượt), base `master`, không stacked

---

## 7. Bẫy khi dịch C# sang Rust (gắn vào mọi hợp đồng)

1. **So sánh chuỗi và path:** `OrdinalIgnoreCase` của C# khác `to_lowercase()` của Rust với Unicode. Dùng helper
   `pr_core::text::eq_ordinal_ignore_case` (Wave 0) cho path và tên file, không tự viết lại.
2. **Culture:** journal, log, CSV luôn theo invariant (dấu chấm thập phân, ISO 8601). Không bao giờ phụ thuộc locale.
3. **Số học:** C# `int` tràn thì wrap (unchecked), Rust ở debug thì panic. Kiểm tra từng phép cộng/nhân trên size, offset
   và byte count, dùng `checked_*`/`saturating_*` đúng như ý đồ C# (xem các bản sửa R16 "saturating" trong C#).
4. **Làm tròn và định dạng số:** `Math.Round` mặc định là banker's rounding, khác `f64::round`. `ToString("F2")`, `"N0"`: so với golden.
5. **Thời gian:** `DateTime` (local/UTC, tick 100 ns) và `DateTimeOffset` cần map sang `SystemTime`/`chrono` đúng như format trên đĩa.
6. **JSON:** đúng tên property (camelCase hay PascalCase như C#), field thiếu → default, field lạ → bỏ qua, enum lenient
   (W1-02). Khi round-trip không được làm rơi field.
7. **Path Windows:** `\\?\`, UNC, volume GUID, drive-relative (C# R32/R44); `Path.GetFullPath` ≠ `std::path::absolute` ở vài trường hợp. Có golden riêng.
8. **Null và rỗng:** C# `string?` và `""` thường được xử lý khác nhau. Giữ đúng phân biệt `None` và `Some("")`.
9. **Exception thành Result:** mỗi `catch (X) when (...)` trong C# phải thành một nhánh lỗi tương ứng, không được gộp.
10. **Thứ tự lặp:** `Dictionary` của C# giữ thứ tự chèn trong thực tế, `HashMap` của Rust thì không. Chỗ nào thứ tự lộ ra
    output thì dùng `IndexMap` hoặc `Vec`.

---

## 8. Chạy plan này như thế nào

- **Thủ công theo từng lượt:** lead (phiên chính) gọi Agent tool cho 8 WU một lúc, mỗi WU ở worktree riêng, rồi merge và chạy gate.
- **Bằng Workflow:** khi người dùng gõ rõ "chạy workflow wave N", lead dùng script workflow `pipeline(WUs, port → verify)`
  với `model: haiku` cho Porter và `model: sonnet` cho Verifier, schema bắt theo §5.3. Không tự chạy workflow khi chưa
  được yêu cầu, vì tốn token lớn.
- **Thứ tự:** Wave 0 → 1 → 2 → 3 (2 và 3 có thể gối nhau: test port của Wave 3 chạy song song với Wave 2) → 4 → 5 và 6 song song → 7 → 8.
- **Bản C# vẫn thay đổi:** đầu mỗi wave, lead kiểm SYNC-LEDGER. Nếu có PR C# đổi hành vi của WU chưa làm thì cập nhật hợp đồng; nếu WU đã làm thì tạo WU vá.

## 9. Ước lượng tương đối
- Tỷ lệ số WU theo model ở Wave 1–7: Haiku khoảng 60%, Sonnet khoảng 25%, Opus khoảng 15%. Chi phí token phần lớn vẫn nằm ở
  Opus (hợp đồng, WU đỏ, tích hợp), vì WU đỏ là các file lớn nhất.
- Wave 1 là wave thử: đo tỷ lệ Haiku PASS ngay lần đầu, rồi chỉnh hợp đồng và §7 trước khi mở rộng. Nếu tỷ lệ PASS lần đầu dưới 50% thì
  chuyển các WU 🟢 khó (parser, PerfAnalysis) sang Sonnet.

## 10. Bài học từ Wave 1 (2026-10-14)

- **Số vòng:** 22 WU cần 1 vòng port + 4 vòng sửa (khoảng 9 triệu token). 10/22 pass ngay vòng 1; phần còn lại chủ yếu là lỗ hổng test do verifier tìm bằng mutation, không phải lỗi hành vi (hành vi thật chỉ có ở W1-10, W1-14, W1-18, W1-21).
- **Thư mục build dùng chung giữa các worktree là lỗi nặng nhất:** agent thấy test panic ở dòng `todo!()` cũ, port đúng vẫn fail gate. Mỗi worktree một `CARGO_TARGET_DIR` (đã vào `wave.ps1`).
- **Phụ thuộc chéo:** hợp đồng nói "chỉ gọi hàm của unit mình" nhưng test của W1-03/17/19/21 gọi hàm unit khác. Chạy wave theo thứ tự phụ thuộc, hoặc tích hợp từng đợt rồi rebase nhánh còn lại (đã làm).
- **Verifier có harness .NET thật là thứ giá trị nhất:** nó tìm ra sai lệch mà không ai đọc code C# thấy được (junction, double BOM, ký tự bidi trong host, độ sâu JSON).
- **Mutation-testing vô tận:** mỗi vòng verifier tìm mutant mới. Quy tắc dừng: mutant tương đương không tính; còn lại là lỗ hổng test hoặc khác biệt hành vi thì phải đóng; sau 3 vòng cho phép lead tự thêm test nhỏ.
- **Nên sinh golden từ harness .NET khi hợp đồng nghi ngờ** (làm ở vòng 5 cho W1-21 và W1-10): hợp đồng viết tay có chỗ sai ("C# không đi theo junction").
- **Haiku:** làm tốt unit thuần dữ liệu và parser nhỏ; fail ở nơi có nhiều nhánh biên (W1-14, W1-18) hoặc phụ thuộc quirks .NET. Hợp đồng nên đánh dấu "quirk .NET" để chuyển Sonnet ngay từ đầu.

## 11. Bài học từ Wave 2 (2026-10-14)

- **Kết quả:** 21 WU (W2-00..W2-20), 3 đợt tích hợp, 21 nhánh đều merge. Verdict PASS chỉ có W2-15, W2-16; các unit còn lại FAIL do mutant sống hoặc khác biệt hành vi nhỏ và được merge kèm danh sách vấn đề mở (xem PR).
- **Test của chính unit có thể sai:** `numeric_path_trailing_whitespace_rules` (W2-20) khẳng định `"3 "` không parse được, trong khi .NET `Enum.TryParse` chấp nhận khoảng trắng cuối. Code đúng, test sai; gate workspace đỏ cho tới khi sửa test. Verifier phải chạy test của porter trên harness .NET, không chỉ đọc.
- **Test `#[ignore]` che lỗi:** 42 test io_physical bị ignore trong lần chạy workspace (cần W2-00 và chạy `--include-ignored`). Gate phải chạy thêm `--include-ignored` cho các unit native hoặc phụ thuộc.
- **Lỗi lặp lại ở quirk đường dẫn Windows:** dấu cách/chấm cuối đoạn đường dẫn (W2-14, W2-17), tiền tố NT `\??`, root UNC (W2-00). Hợp đồng nên liệt kê các case này và golden phải có chúng.
- **Mutant sống tập trung ở hằng số thời gian và nhánh cổng:** flush timer, dung lượng kênh, quy tắc năm nhuận, chặn 403 (W2-17, W2-18, W2-12, W2-13, W2-19). Cần test đúng giá trị biên của hằng số, không chỉ hành vi chung.
- **Xung đột chỉ ở `docs/PARITY.md`:** mọi nhánh cùng sửa file này. Giữ cả hai phía là đủ; lead nên sở hữu file và porter chỉ báo dòng cần thêm.
- **Thứ tự phụ thuộc:** W2-00 -> W2-01/W2-20 -> W2-02/03 vẫn đúng; W2-03 cần normalizer của W2-02 và W2-14 resolve cần W2-00 để test không ignored.

## 12. Bài học từ Wave 3 (2026-10-16)

- **Kết quả:** 9 WU (W3-01..W3-09) merge trên `integration/wave-3` theo 4 đợt (i1: W3-02/07/08; i2: W3-01/03; i3: W3-04; i4: W3-05/06) cộng W3-09 (chỉ test). Verdict PASS: W3-03, W3-04, W3-06. FAIL: W3-01 (mutation chưa chạy, 228/231 mutant chưa kiểm) và W3-05 (verifier chưa chạy xong cargo-mutants, 0/195). Hai unit này ghi `Partial` trong PARITY.md.
- **Seam do lead làm trước (P-11..P-15) cứu cả wave:** `AppendStream`, `try_get_file_stat`, `try_restore -> io::Result<bool>`, `FaultInjectionFileSystem`, fixture đồng bộ. Không unit nào phải tự bịa seam; nhưng W3-07 phải thu hẹp P-12 trên `PhysicalFileSystem` (file do lead sở hữu), nên seam cần có test golden vật lý ngay từ đầu.
- **Phụ thuộc vòng giữa các unit:** W3-01 cần W3-03 (reconcile), W3-04 cần W3-05 (undo). Test đỏ chỉ xanh sau khi cả cặp merge; thứ tự tích hợp đúng là 02/07/08 -> 01/03 -> 04 -> 05/06 -> 09. Hợp đồng nên ghi rõ "test X đỏ cho đến khi unit Y merge" thay vì để porter tự phát hiện.
- **Xung đột chỉ ở `docs/PARITY.md`** (lại như Wave 2): mọi merge đều giữ hàng Done mới, bỏ hàng Todo cũ. Lead nên sở hữu file này.
- **Mutation gate là nút thắt:** `cargo-mutants` trên `undo.rs` chạy khoảng 3 giờ (198 mutant); verifier không kịp. Cần ngân sách thời gian cho verifier hoặc chạy mutation theo file, song song, ngay trong vòng port.
- **Test `#[ignore]` vẫn nhiều:** journal 19, compaction 15, journal_reconcile 2, startup_recovery 6, 5 test INV-6 chờ D24; chưa chạy `--include-ignored` toàn bộ. Gate Wave 3 phải chạy thêm các binary native.
- **Flake dưới tải:** `diagnostics_file_log::flush_drained_set_while_entry_queued_waits_until_written` (W2) rớt ở một lần chạy workspace đầy đủ, xanh khi chạy riêng; đã có bản sửa giới hạn 10 s (e689f26 trên nhánh fix/w4-flaky).
- **Đồng bộ C# trôi:** `origin/master` của C# đi tiếp `d92d6aae` (trình sửa ngoài). Ghi vào SYNC-LEDGER và P-17; patch W2-S1 sau khi Wave 3 merge.

## 13. Granularity rule (P-18, 2026-10-15)

Split a unit when it has independent concerns: pure logic vs I/O, separate C# classes, separate files. Name sub-units
`W<wave>-<n>a`, `b`, ... in the contract; each has its own Owned files, goldens and verifier. Chores (docs, ignore
files, cleanup, PR bookkeeping) are Haiku sub-agents in the background. Never more than 8 concurrent cargo builds,
each with its own `CARGO_TARGET_DIR`.

# Kế hoạch port PhotoReview sang Rust

Mục tiêu: hành vi **y chang** bản C# (baseline trong [SYNC-LEDGER.md](SYNC-LEDGER.md)), tốc độ **bằng hoặc hơn**,
chạy song song với bản C# mà không bao giờ dùng chung dữ liệu ([R0002](adr/R0002-coexistence-with-csharp-app.md)).
Quy mô nguồn: khoảng 52K dòng C# trong 10 project, khoảng 123K dòng test, 10 cửa sổ, INV-1…12, 9 ADR.

## Stack

| Mảng | C# | Rust |
|---|---|---|
| UI | WPF | eframe/egui + wgpu (đề xuất), Slint là phương án khác; chốt bằng spike ([R0001](adr/R0001-ui-framework.md)) |
| Decode mặc định | WicDirect | WIC qua crate `windows` (ADR 0001) |
| Decode nhanh | TurboJPEG | libjpeg-turbo (DLL đã pin hash hoặc link tĩnh) |
| Fallback | WPF decoder | WIC tổng quát / crate `image`; cache key gồm cả backend (INV-12) |
| RAW preview | Imaging.Raw | Port nguyên parser |
| RAW sensor | P/Invoke libraw.dll | `pr-libraw` FFI, dynamic link (LGPL/CDDL) |
| Explorer order, Recycle | COM | crate `windows` (IFolderView, IFileOperation) |
| Concurrency | TPL | std threads + crossbeam-channel + rayon; không dùng tokio; generation = `AtomicU64` |
| i18n | JSON + source generator | Cùng JSON (sync) + `build.rs` sinh key |
| Settings/journal | System.Text.Json | serde, **cùng format** |
| Test | xUnit, Stryker | cargo test/nextest có timeout, cargo-mutants |

## Crate ↔ project C#

| Crate | Port từ |
|---|---|
| `pr-core` | PhotoReview.Core |
| `pr-l10n` | Core/Localization + Localization.Generator |
| `pr-imaging` | PhotoReview.Imaging |
| `pr-imaging-raw` | PhotoReview.Imaging.Raw |
| `pr-libraw` | PhotoReview.Imaging.LibRaw |
| `pr-turbojpeg` | PhotoReview.Imaging.TurboJpeg |
| `pr-platform-win` | PhotoReview.Platform.Windows |
| `pr-bench`, `pr-perf` | Benchmarking, PerfAnalysis |
| `pr-app` | PhotoReview.App (bin `PhotoReviewRs`) |

## Các phase (mỗi phase phải qua gate trước khi sang phase sau)

### Phase 0: Nền móng và spike
- [x] Repo, workspace, AGENTS.md, CI, ADR R0001 (đề xuất) và R0002 (đã chấp nhận)
- [x] Sync ngôn ngữ và fixture từ C# (`tools/sync-from-csharp.ps1`), baseline `e441d67a`
- [x] `pr_core::app_paths` + `identity` (tách dữ liệu khỏi bản C#) kèm test
- [x] Toolchain Rust 1.99.0 (pin trong `rust-toolchain.toml`), MSVC Build Tools; `Cargo.lock` + `--locked` trong CI; fmt/clippy/11 test xanh
- [x] Kiểm kê tính năng (khoảng 245 mục: K/C/S/W/F/I/P/INV/D) vào [PARITY.md](PARITY.md)
- [ ] Spike UI egui và Slint theo giao thức trong R0001, có số đo; chốt R0001
- [ ] Spike COM: đọc thứ tự Explorer cho folder khoảng 1800 file, mục tiêu khoảng 60 ms như C#
- [ ] ADR R0003 concurrency (thread, channel, generation, UI-thread affinity theo C# ADR 0005)
- **Gate:** R0001 và R0003 đã chốt, CI xanh

### Phase 1: Core (không UI)
Model → Settings (load/repair/backup, INV-11) → Catalog (scan, sort, fallback order, GenerationClock) → Session
(SessionWriter debounce, ghi atomic theo C# ADR 0007) → OperationJournal (Prepared/Committed/Failed, cùng format) →
FileActionService (INV-4) → Undo/RecoveryRetry/RecoveryFileCheck (bảng verdict) → JournalStartupRecovery (ADR 0003) →
Instance lock → Localization.
- **Gate:** test cho INV-3/4/5/6/11; oracle diff config sau repair, journal sau chuỗi action, thứ tự catalog; cargo-mutants trên journal/file action

### Phase 2: Imaging
ImageCacheKey (INV-1, INV-12) → decoder trait + WIC/TurboJPEG/fallback (INV-8) → RAM cache + memory pressure →
DiskCacheStore/ThumbnailCache/SourceBytesCache (INV-2) → PreloadScheduler → chế độ Fast/Preview/Original.
- **Gate:** oracle so kích thước, orientation và pixel hash; `pr-bench` xuất cùng format với Benchmark.Cli

### Phase 3: RAW
Parser (CR2/CR3/NEF/ARW/DNG/RAF/ORF/RW2), JPEG nhúng, RawPairMode, LibRaw decode khi zoom, thumbnail dự phòng.
- **Gate:** mọi mẫu trong `raw-samples.txt` của C# cho kết quả giống hệt

### Phase 4: Platform Windows
Thứ tự Explorer (prefetch, đường đọc nhanh, validator, INV-7/9), Recycle Bin (chỉ fake trong test), memory/monitor.

### Phase 5: Cửa sổ chính
Composition root, MainWindow (Fit, fullscreen, zoom, pan, menu chuột phải), ImagePresenter là choke point, luồng
mở file/folder (INV-9), luồng file action (INV-3), Compare, kéo thả, command line.
- **Gate:** kịch bản E2E tương đương test tích hợp C#

### Phase 6: Cửa sổ phụ
Settings → Recovery → ActionProfiles → BatchReview → ClickZoomCustomDialog → Diagnostics → Benchmark → kiểm tra update thủ công.

### Phase 7: Đạt parity và chuyển giao
PARITY 100% (hoặc có ADR cho mục khác biệt), benchmark bằng hoặc hơn C#, người dùng chạy thử thật 2 tuần, release
riêng. Có dừng bản C# hay không là quyết định của người dùng.

## Cơ chế đảm bảo "y chang"
1. [PARITY.md](PARITY.md): mỗi tính năng/INV có trạng thái và test chứng minh.
2. Oracle harness (dự kiến `tools/oracle`): chạy C# và Rust trên cùng fixture rồi diff output.
3. Test port giữ tên C# gốc trong comment.
4. Dùng chung JSON ngôn ngữ, kiểm tra không thiếu key.

## Đồng bộ khi C# còn thay đổi
Xem [SYNC-LEDGER.md](SYNC-LEDGER.md). Giai đoạn đầu, bản C# chỉ nên nhận sửa bug và an toàn dữ liệu.

## Rủi ro
| Rủi ro | Giảm thiểu |
|---|---|
| UI Rust không đạt tốc độ hoặc cảm giác như WPF | Spike có số đo trước khi viết nhiều code |
| Hai app tranh dữ liệu | R0002 + test |
| COM/Shell dễ lỗi (apartment, PIDL lifetime) | Spike, wrapper RAII, test native riêng |
| LGPL của LibRaw | Dynamic link, kiểm tra license tự động (cargo-deny) |
| Khối lượng lớn | Chia phase theo INV, oracle tự động, agent song song theo crate |

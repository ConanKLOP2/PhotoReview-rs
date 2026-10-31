# FIX-FFI-HARDENING 2026-10-31: xử lý phát hiện của ST-FFI-LIBS

Status: report (lane sửa). Nguồn: [ST-FFI-LIBS-2026-10-31.md](ST-FFI-LIBS-2026-10-31.md). Mỗi mục được kiểm trên mã thật và đối chiếu C# `7eaee2a2`.

## Kết quả theo mục

| Mục | Quyết định | Nội dung |
|---|---|---|
| M1 | Sửa | `pr-libraw` nạp bằng `APP_FOLDER_LOAD_FLAGS = 0x1100` (`LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS`) như `pr-turbojpeg`; bỏ `LOAD_WITH_ALTERED_SEARCH_PATH` (0x8). Test cờ thay test khoá 0x8. |
| L1 | Sửa | `has_interior_nul`; `LibraryHandle::load_from` từ chối đường dẫn có NUL (`LoadFailed`, lỗi 87); `open_wfile` trả `-1`. Test thuần cho `load_from` + `has_interior_nul`. `open_wfile` dùng chung helper nhưng cần DLL nên chưa có test riêng. |
| L2 | Sửa | `TurboJpegLibrary::has_decode_exports` (8 export đường giải mã); `availability::probe` lọc thư viện thiếu export thành "không nạp được". Test thuần `decode_exports_are_all_required`. Không kiểm phiên bản: libjpeg-turbo không có hàm lấy version, dựa hash pin khi đóng gói. |
| L5 | Bỏ (tương đương C#) | `validate_source_length` giữ `data_size >= w*h*c` đúng C# và golden; `data_size` hợp lệ có thể lớn hơn (ảnh 16 bit). Siết `==` sẽ sai với ảnh 16 bit. Không mất an toàn bộ nhớ (slice bị chặn bởi `rgb_length <= data_size`). |
| M4 | Hoãn | Cả `from_raw`/`wrap_init_result`/`with_destroy` (turbojpeg) lẫn `try_set_use_camera_wb_with`/`raw_structure_with`/`write_i32` (libraw) được gọi từ integration test (`tests/native.rs`, `tests/native_golden.rs`) là crate ngoài, nên `pub(crate)` không dùng được. Đổi thành `unsafe fn` thêm hàng chục khối `unsafe` trong test, làm đổi `unsafe-audit.baseline` (cấm sửa trong lane này). Cần thiết kế: feature `test-support` hoặc chuyển test vào crate. Không có caller production (grep). `write_i32` là phương thức của trait công khai `WhiteBalanceTarget`. |
| M2, M3 | Hoãn | Watchdog/tiến trình con/fuzz là thiết kế lớn. |
| L3, L4, L6 | Hoãn | Không có sửa 1-2 dòng an toàn. |
| SAFETY comment (3) | Sửa | `native.rs` đầu file (callback đã nằm trong file), `resolve_all!` (`api()` không tự kiểm, probe nằm ở composition), `read_i32` (điều kiện "mọi bản có `color.curve[0x10000]`", không có cổng version riêng). |

## Kiểm chứng

- Test thuần: `cargo test -p pr-libraw --test native` 30 pass, 9 ignored; `cargo test -p pr-turbojpeg --lib decode_exports` pass. "Đỏ trước" ở dạng lỗi biên dịch (hằng/hàm mới chưa tồn tại), không chạy riêng bản đỏ.
- Test native với DLL thật: sao chép `libraw.dll` (bản chỉ đọc ở `C:\MyProjects\PhotoReview\native\x64`) cạnh binary test trong `target/` của worktree rồi chạy `--ignored`: 9/9 pass (nạp qua `api()` bằng cờ 0x1100, gồm round-trip giải mã, `try_set_use_camera_wb` pinned, `version_string`). DLL đã xoá khỏi `target` sau đó. Chưa chạy native test của turbojpeg (thay đổi chỉ ở probe).
- `cargo fmt --check`, một clippy `--all-targets -D warnings` cho mỗi crate: sạch. `tools/dev/unsafe-audit.sh`: không vi phạm mới, baseline không đổi.

## PARITY

Không sửa. Hàng I-15 (`libraw.dll` chỉ nạp từ thư mục app) vẫn đúng, nay chặt hơn C# về DLL phụ thuộc.

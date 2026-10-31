# ST-PAINTER-TESTS: test painter 144 DPI, fill rect rỗng, ETCHEDHORZ/VERT, golden Settings theo tên (2026-10-31)

Nguồn việc: ST-GAPMAP2 mục 6, việc 9. Lane `st-painter-tests`, nhánh `feat/st-painter-tests`. Chỉ thêm test, không sửa mã production.

## File mới
- `crates/pr-platform-win/tests/dialogs_painter_dpi_empty.rs`: 9 test thuần + 3 test native `#[ignore = "native: needs desktop"]`.
- `crates/pr-platform-win/tests/settings_pages_golden_by_name.rs`: 8 test thuần, nạp golden theo tên.

## Đã chạy thật (máy này, `cargo test -p pr-platform-win --test ... -j 2`)
- `dialogs_painter_dpi_empty`: 9 passed, 3 ignored; chạy riêng `-- --ignored --test-threads=1`: 3 native passed (cửa sổ owner đặt ngoài màn hình tại -20000, `PrintWindow`, giống các file `dialogs_painter_*`).
- `settings_pages_golden_by_name`: 8 passed.
- `cargo fmt -p pr-platform-win` đã chạy. Chưa chạy clippy (theo giới hạn build), chờ CI.

## Nội dung và đối chiếu khoảng trống
1. **144 DPI** (X-chrome-painter-controls, open question 2 của W8-18m). Không tạo được cửa sổ 144 DPI thật trên máy bất kỳ (DPI cửa sổ là DPI của monitor), nên ghim cả chuỗi `paint_scale(144)` -> `scaled` -> `push_button_draw` / `group_box_layout` / `etched_shape` bằng dữ liệu thuần: `S(3)=4` (4.5 làm tròn về chẵn, không phải 5), `S(4)=6`, `S(6)=9`, `S(8)=12`, `scaled` không bao giờ dưới 1, độ dịch khi nhấn là 1 px không nhân tỉ lệ, dòng etched luôn dày 1 px. Biến thể native 144 DPI của C# vẫn KHÔNG port (cần monitor 144 DPI, việc của người).
2. **Fill rect rỗng** (X-chrome-painter-controls). `fill` là `pub(super)` và không có DC ghi lại, nên không gọi được trực tiếp. Làm hai lớp: (a) test thuần dùng đúng vị từ của C# `Fill` (`right <= left || bottom <= top`) để phân loại mọi hình chữ nhật mà `frame_rects`, `etched_shape`, `group_box_layout` giao cho `fill` (client 0x0, độ dày 0 hoặc âm, 1x1, dòng etched cạnh ngắn bằng 0 vẫn KHÔNG rỗng); (b) test native: control `SS_ETCHEDFRAME` kích thước 0x0 sau khi gắn painter không đổi pixel nào quanh nó. Đây là bằng chứng gián tiếp, không phải assertion trên chính hàm `fill`; muốn assertion trực tiếp cần mở `fill` ra `pub` hoặc thêm seam DC (không làm vì lane không sửa production).
3. **Fixture ETCHEDHORZ/VERT có client không rỗng** (X-chrome-painter-frame). Cửa sổ etched mới tạo bị hệ thống để mỏng 2 px, client rỗng; sau khi `move_window` mở rộng (120x12 / 12x120) client không rỗng và thực nghiệm đo được dòng `Dark.Border` đúng ở `height/2` (ngang) và `width/2` (dọc), hàng/cột kế bên không phải màu đường, vùng trên/trái dòng là `Dark.Window`. Cả 3 test native pass trên máy này.
4. **Golden từng trang Settings theo tên** (X-settings-ui-pages). 7 trang qua 6 golden (`settings-pages-diag-perf` có 2 trang): so key, id_base, mọi item (class, id, style, ex_style, text), mọi provider text, tập notes / key_boxes / warnings, số combo/slider và số phần tử cấp cao của cây layout. Tất cả khớp golden C#, kể cả text trang Shortcuts. Các test `golden_*` cũ không đọc JSON; các test mới mới thực sự đọc.

Giới hạn của golden-by-name: cây `elements` chỉ so số phần tử cấp cao, chưa so đệ quy `type`/`control_ids`/`children`; `ids` ngoài Diagnostics chưa so từng khoá. Tên hành động Shortcuts khớp golden (tiếng Anh), nên ghi chú "English action names until settings_texts()" trong PARITY chỉ còn là vấn đề bản địa hoá, không phải lệch golden.

## Đề xuất đổi PARITY (orchestrator quyết, lane này không sửa)
- **X-chrome-painter-controls (L644)**: bỏ "fill with an empty rectangle has no test" khỏi phần còn thiếu (đã có bằng chứng gián tiếp ở trên); giữ Partial vì test 144 DPI native chưa port và `fill` chưa có assertion trực tiếp. Chưa nâng Done.
- **X-chrome-painter-frame (L646)**: thay "SS_ETCHEDHORZ / SS_ETCHEDVERT empty client, only SS_ETCHEDFRAME measured" bằng "đo được cả ngang và dọc nhờ `move_window` sau khi tạo" (chạy tay một lần offscreen, chưa phải CI). Vẫn Partial vì `default_chrome` còn `NoPainter` đến 18h..18k.
- **X-settings-ui-pages (L651)**: "golden từng trang chưa nạp theo tên" đã xong (6 golden, 7 trang, 8 test). Còn lại: so đệ quy cây layout, bản địa hoá tên hành động Shortcuts, trang external editors (P-44). Chưa nâng Done.
- Nên đưa 3 test native vào danh sách native-nightly nếu quy ước có (không kiểm tra `.github/workflows/native-nightly.yml` trong lane này).

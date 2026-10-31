# FIX-RAW-CORPUS-GATE (ST-TEST-HYGIENE F1)

Status: done (2026-10-31).

## Vấn đề
Các test RAW corpus thoát sớm (xanh giả) khi thiếu `tests/fixtures/raw-corpus` (corpus không nằm trong repo; CI không đặt `PHOTOREVIEW_RAW_CORPUS_STRICT`). Một số test thoát hoàn toàn im lặng, một số chỉ `eprintln!` (bị libtest nuốt khi test pass, không thấy nếu không có `--nocapture`).

## Thay đổi
- Helper dùng chung `crates/pr-imaging-raw/tests/common/corpus_skip.rs`: `skipped(why)` ghi `SKIPPED (no RAW corpus: <tên test>) <why>` thẳng ra stderr thật (bỏ qua cơ chế capture của libtest, nên thấy được không cần `--nocapture`). Tên test lấy từ tên thread libtest.
- Gắn vào 7 binary: `raw_exif`, `raw_reader_arw`, `readers_cr2`, `readers_rw2`, `cr3_reader`, `w5_18_integration`, `raw_nef` (mọi đường thoát sớm vì thiếu corpus/mẫu, kể cả các mẫu `optional` của golden ARW/CR3).
- Test tổng hợp `raw_corpus_presence_report` (`tests/raw_corpus_presence.rs`, không bao giờ fail): in `RAW CORPUS REPORT: ABSENT ...` hoặc `present (N files)`, trạng thái strict, số điểm skip trong 7 file.
- Không đổi code production. Không đặt STRICT trong CI.

## Test READER chạy không cần corpus
Không thêm mới: 7 binary trên đã có sẵn nhiều test container-parse dùng byte tổng hợp dựng trong Rust (ví dụ readers_cr2 33 test, readers_rw2 34, raw_nef 68, raw_exif 70, cr3_reader 57 chạy xanh không corpus; chỉ các test có tên `corpus*` / `all_*corpus*` / golden-có-mẫu là skip). Phần "chỉ khi đơn giản" nên dừng ở (2).

## Bật STRICT khi có corpus
Đặt corpus vào `tests/fixtures/raw-corpus` (hoặc `PHOTOREVIEW_RAW_CORPUS_DIR`), rồi:
- `PHOTOREVIEW_RAW_CORPUS_STRICT=1` (arw, cr2, cr3, w5_18),
- `PHOTOREVIEW_RS_RAW_CORPUS_STRICT=1` (rw2, nef).
Skip thành panic cho các test dùng cờ này; `raw_exif` hiện KHÔNG có nhánh strict (chỉ in SKIPPED) — việc nối strict cho raw_exif là follow-up.

## Kiểm chứng
`cargo fmt -p pr-imaging-raw --check` ok; `cargo clippy -p pr-imaging-raw --all-targets -- -D warnings` sạch; chạy 8 binary test đổi/mới: tất cả pass, in SKIPPED như mong đợi.

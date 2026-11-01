# FIX-RESET-PAGE (2026-11-01): nut "Reset this page" cua cua so Settings

Nguoi dung bao nut "hoat dong khong tot" (khong neu trieu chung). Lane doc ma va do tren cua so that.

## Ket luan

Khong tim thay loi trong ma Rust. `reset_active_page` khop C# baseline 7eaee2a2 (`SettingsDialog.ResetActivePage`, dong 840-854) tung buoc:
scratch presenter MOI tu settings hien tai (khong store, khong localization), `apply_defaults()`, `loading` bat, `page.load(&defaults)` CHI cho trang dang hien, roi `relayout_active_page`. Khong co code bat/tat nut Reset hay Apply theo trang o ca C# lan Rust (Reset luon enable; `_dirty` cua C# la write-only, D-W8B5-907). Duoc xac nhan bang so sanh `ApplyDefaults` (65 phep gan) va `Load` cua 5 trang (Display, Mouse, Performance, Files, Diagnostics, General): giong het.

## Bang chung chay that (cua so that, `#[ignore]` native)

`crates/pr-platform-win/tests/settings_reset_page_native.rs` (chay: `cargo test -p pr-platform-win --test settings_reset_page_native -- --ignored`, 2 pass):

- `reset_page_diff_per_page`: dat ~35 gia tri khac mac dinh (gom 2 phim tat, trinh sua ngoai, cache, hien thi, chuot, chuan RAW, logging). Voi MOI trang trong 7 trang (General, Display, Mouse, Performance, Shortcuts, Files, Diagnostics): reset trang do roi `collect_all()` tu cac control. Ket qua: khong gia tri sai (khong gia tri nao khac ca cu lan mac dinh), moi gia tri duoc dua ve mac dinh boi dung trang so huu no, trang khac giu nguyen gia tri chua luu. Khong trang nao khong reset dung.
- `clicking_the_reset_button_resets_only_the_showing_page`: BM_CLICK that vao nut Reset: nut ton tai va enable; reset General dua combo ve 0, Logging cua Diagnostics giu nguyen; chuyen sang Diagnostics va bam Reset thi Logging tat; working copy cua presenter khong doi cho toi Save (khop `ResetPage_ResetsOnlyTheShowingPage` cua C#). Rust truoc do KHONG co test nao cho reset trang.

## Lech so voi C# (khong sua vi C# y het)

- Reset Shortcuts/Mouse nap lai `actions_json` va tom tat phim tu scratch (settings da luu), khong phai tu chinh sua Action profiles chua luu: C# giong.
- Reset khong dua ngon ngu ve mac dinh (ApplyDefaults khong liet ke ngon ngu): C# giong.
- Reset khong danh dau dirty: C# khong co co dirty co tac dung.

## Gia thuyet con lai (can hoi nguoi dung trieu chung)

1. Hien thi: control custom-draw khong ve lai sau khi nap gia tri (khong do duoc o test, chi doc trang thai control). Can xem anh/mo ta.
2. Ky vong khac C#: nguoi dung mong Reset cung xoa danh sach trinh sua ngoai/phim tu Shortcuts khi dang o trang khac, hoac mong Reset khong dong vao ngon ngu / Action profiles.
3. Mong nut Apply bat/tat theo thay doi (C# khong co).
4. Reset tren trang Files/Shortcuts voi Action profiles da sua chua luu (xem muc tren).
Hoi nguoi dung: trang nao, bam Reset thi gia tri nao khong doi / doi sai, co do Defaults (nut khac) co cung loi khong.

## PARITY

Khong de xuat doi hang nao (khong sua production).

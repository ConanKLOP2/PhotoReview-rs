# FIX-INVMAP-LOW (2026-10-31): các mục thấp L1..L5 và H1-phụ của ST-INVMAP

Nguồn: [ST-INVMAP-2026-10-31.md](ST-INVMAP-2026-10-31.md), [FIX-H1-2026-10-31.md](FIX-H1-2026-10-31.md). Đối chiếu C# ở 7eaee2a2.

| Mục | Quyết định | Ghi chú |
| --- | --- | --- |
| L3 Recycle đơn không re-stat | SỬA | `service.rs` nhánh Recycle: sau `tx.begin()` và `hand_off`, stat lại; mất file -> `source_missing`, khác size/mtime -> `VERIFY_SIZE_CHANGED`, không chạm đĩa (journal ghi Failed). DIVERGENCE so với C# `FileActionService.ExecuteAsync` (không re-stat). Test mới `file_actions_service.rs::recycle_permanent_source_replaced_after_prepared_is_refused` và `recycle_source_replaced_after_prepared_is_not_recycled` (đỏ trước, xanh sau). |
| H1-phụ nhánh `send_to_recycle_bin` của member không permanent trong `recovery_retry` | SỬA | `recovery_retry.rs`: cùng re-stat + `same_fingerprint` như nhánh permanent, lỗi `core_recovery_source_changed`. Test mới `st_invmap_repro.rs::retry_group_recycle_member_replaced_between_precheck_and_mutation_is_not_recycled` (đỏ trước). `recovery_retry_golden.rs::divergence_adjusted` mở rộng có kiểm soát: chỉ bỏ op `stat P` đứng NGAY trước `bin.recycle P` (cùng cơ chế với `bin.delete-permanently P`); mọi op khác vẫn phải khớp oracle. |
| L1 `end()` nhả vô điều kiện | BỎ (tương đương C#) | C# `End()` là store 0 vô điều kiện; `FileActionGate` là trait dùng chung với undo/retry. Mọi nơi gọi `end()` trong `crates/` đi qua `GateGuard`/`GateHold` (RAII) hoặc cặp `try_begin`/`end` của chính người giữ (`undo.rs:231,379`). Không có đường dùng sai. Đổi sang token-holder là thay đổi trait lớn, ghi làm việc tiếp nếu muốn phòng thủ sâu. |
| L2 INV-4 phụ thuộc composition root | BỎ phần mã; HOÃN một test | `registry.rs` đã truyền `file_action_service()` cho retry (`:358`) và undo (`:340`); test `composition_registry.rs::recovery_retry_takes_the_file_action_gate` phủ retry. Chưa có test cho undo dùng chung cổng: nên thêm vào `composition_registry.rs` (pr-shell, ngoài phạm vi build của lane này). |
| L4 xuất settings/action list bằng `std::fs::write` | BỎ (tương đương C#, có chủ đích) | C# dùng `File.WriteAllText` (không nguyên tử) ở cả hai chỗ. `write_all_text_atomic` của repo TẠO thư mục cha, trong khi golden `action-profiles-io` và test `export_settings_to` kỳ vọng lỗi khi thư mục cha không tồn tại (đối chiếu oracle), và ghi đè bằng rename khác thông điệp lỗi khi đích là thư mục/file khoá. Đây không phải ảnh người dùng; mất tối đa một bản xuất cụt, người dùng xuất lại. Nếu muốn nguyên tử: cần ADR `Different`, kiểm tra thư mục cha tồn tại trước, giữ nguyên thông điệp lỗi, và test FileSystem giả lỗi giữa ghi. HOÃN. |
| L5 `let _ =` journal ở undo | GIỮ (xác nhận) | Best-effort có chủ đích như C#; hậu quả (thiếu dòng Committed) tự lành ở reconcile khởi động. Không đổi. |

## Kiểm chứng
- Trước sửa (production hoàn về master): `file_actions_service` 2 test mới đỏ (`assertion failed: !result.succeeded`), `st_invmap_repro` test mới đỏ.
- Sau sửa: `file_actions_service` 89/89, `file_actions_service_group` 93/93, `file_actions_service_golden` 1/1, `recovery_retry` 100/100, `recovery_retry_golden` 3/3, `file_actions_undo` 120/120, `st_invmap_repro` 2/2.
- `cargo fmt -p pr-core --check` sạch; `cargo clippy -p pr-core --all-targets -- -D warnings` sạch. pr-app không đổi nên không chạy clippy pr-app. Chưa chạy: `cargo test --workspace`, các binary pr-app/pr-shell (CI).
- Không chạm Recycle Bin thật (ScenarioBin/ScriptedBin trên đĩa giả).

## PARITY đề xuất (không sửa ở lane này)
- F-01..F-05 / Recycle đơn và F-18 retry nhóm: thêm DIVERGENCE "re-stat trước mutation Recycle (sửa lỗ hổng C#)"; bằng chứng các test trên.
- INV-4/INV-6: L3 và H1-phụ đóng.

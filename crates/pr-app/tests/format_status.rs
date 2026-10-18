//! Port of `PhotoReview.App.Shared/ViewModels/StatusFormatter.cs` tests: `tests/PhotoReview.App.Tests/ViewModels/StatusFormatterTests.cs`
//! (C# baseline d92d6aae), the number-format extras (contract traps T1, T18, T21, T22), and the golden checks over
//! `tests/golden/format-status.json` (oracle `oracle/W8-02b.cs`, command `format-status`).
//!
//! Unit W8-02b, contract `contracts/W8-02b.md`. Red stage: every stub body is `todo!()`, so each test here fails until the
//! porter implements the formatter. The C# `StatusFormatterTests` run in the default (Vietnamese) catalog, so these tests
//! use the Vietnamese localizer; `CultureInfo.CurrentCulture` is replaced by the explicit `NumberCulture`.

mod format_support;

use format_support::{
    Env, en_env, env_for_case, i32_of, invariant_numbers, opt_bool, opt_text, text_of, vi_env, vi_numbers,
};
use pr_app::viewmodels::format::context::{format_custom_fixed, format_grouped_integer};
use pr_app::viewmodels::format::status::{
    CompareStatus, action_completed, action_failed, action_invalid_operation, action_no_destination,
    all_images_processed, batch_canceled, batch_done, cache_cleared, compare, compare_hash_text, compare_hash_unknown,
    compare_size_suffix, copied_to, decoding_raw, duplicate_check_canceled, duplicate_check_canceled_folder_changed,
    duplicate_check_failed, duplicate_check_running, folder_open_failed, format_file_size, image_error, index_only,
    loading, loading_full_res, no_duplicates_found, no_images_remaining, no_supported_images,
    no_supported_images_but_subfolders, nothing_to_undo, ready, sibling_folder_boundary, with_dimensions,
};
use pr_test_support::golden;
use serde_json::Value;

// ---- StatusFormatterTests.cs ----

#[test]
fn format_file_size_formats_accurately() {
    // C#: StatusFormatterTests.FormatFileSize_FormatsAccurately (9 InlineData rows, default catalog, invariant numbers)
    let env = vi_env();
    let rows: [(i64, &str); 9] = [
        (0, "0 byte"),
        (-100, "0 byte"),
        (512, "512 byte"),
        (1024, "1 KB"),
        (1536, "1.5 KB"),
        (1_048_576, "1 MB"),
        (2_500_000, "2.38 MB"),
        (1_073_741_824, "1 GB"),
        (5_368_709_120, "5 GB"),
    ];
    for (bytes, expected) in rows {
        assert_eq!(format_file_size(bytes, &env.ctx()), expected, "bytes {bytes}");
    }
}

#[test]
fn status_image_messages_match_original_strings() {
    // C#: StatusFormatterTests.ImageStatusMessages_MatchExactOriginalStrings
    let env = vi_env();
    let ctx = env.ctx();
    assert_eq!(index_only(0, 10, &ctx), "1/10");
    assert_eq!(loading(0, 10, 1_572_864, &ctx), "1/10 · 1.5 MB · Đang tải");
    assert_eq!(
        loading_full_res(0, 10, 1_572_864, &ctx),
        "1/10 · 1.5 MB · Đang tải độ phân giải đầy đủ"
    );
    assert_eq!(ready(0, 10, 1_572_864, "photo.jpg", &ctx), "1/10 · 1.5 MB · photo.jpg");
    assert_eq!(
        with_dimensions(0, 10, 1_572_864, 1920, 1080, "photo.jpg", &ctx),
        "1/10 · 1.5 MB · 1920×1080 · photo.jpg"
    );
    let compared = CompareStatus {
        index: 0,
        count: 10,
        left_name: "left.jpg",
        left_size: " (1 MB)",
        right_name: "right.jpg",
        right_size: " (1 MB)",
        hash_text: " [same hash]",
    };
    assert_eq!(
        compare(&compared, &ctx),
        "1/10 | So sánh | left.jpg (1 MB) ↔ right.jpg (1 MB) [same hash] | nhấn để chọn"
    );
    assert_eq!(
        image_error("photo.jpg", "File hỏng", &ctx),
        "Lỗi ảnh: photo.jpg — File hỏng"
    );
}

#[test]
fn status_folder_and_action_messages_match_original_strings() {
    // C#: StatusFormatterTests.FolderAndActionMessages_MatchExactOriginalStrings
    let env = vi_env();
    let ctx = env.ctx();
    assert_eq!(
        no_supported_images(&ctx),
        "Không tìm thấy ảnh được hỗ trợ trong thư mục này."
    );
    assert_eq!(
        no_supported_images_but_subfolders(1, &ctx),
        no_supported_images_but_subfolders(1, &ctx)
    );
    assert_ne!(no_supported_images(&ctx), no_supported_images_but_subfolders(2, &ctx));
    assert_ne!(
        no_supported_images_but_subfolders(1, &ctx),
        no_supported_images_but_subfolders(2, &ctx)
    );
    assert_eq!(
        folder_open_failed("Quyền truy cập bị từ chối", &ctx),
        "Không mở được thư mục: Quyền truy cập bị từ chối"
    );
    assert_eq!(no_images_remaining(&ctx), "Không còn ảnh trong thư mục");
    assert_eq!(all_images_processed(&ctx), "Đã xử lý hết ảnh trong thư mục.");
    assert_eq!(cache_cleared(&ctx), "Đã xóa cache ảnh xem trước.");
    assert_eq!(
        duplicate_check_canceled_folder_changed(&ctx),
        "Đã hủy: thư mục đã thay đổi trong lúc kiểm tra trùng lặp."
    );
    assert_eq!(
        no_duplicates_found(&ctx),
        "Không có bản trùng lặp nào cùng hash phù hợp."
    );
    assert_eq!(batch_canceled(&ctx), "Đã hủy xử lý hàng loạt.");
    assert_eq!(batch_done(5, 0, &ctx), "Xử lý hàng loạt hoàn tất: 5 thành công, 0 lỗi.");
    assert_eq!(sibling_folder_boundary(1, &ctx), "Đã ở thư mục cuối cùng cùng cấp.");
    assert_eq!(sibling_folder_boundary(-1, &ctx), "Đã ở thư mục đầu tiên cùng cấp.");
    assert_eq!(action_completed("Move to Keep", &ctx), "Đã thực hiện: Move to Keep");
    assert_eq!(
        action_invalid_operation("Move to Keep", &ctx),
        "Không thực hiện được Move to Keep: thao tác không hợp lệ."
    );
    assert_eq!(
        action_failed("Move to Keep", Some("Disk full"), &ctx),
        "Không thực hiện được Move to Keep: Disk full"
    );
}

// ---- Hand-written extras (`// new:`): number formats, traps T1, T18, T21, T22 ----

#[test]
fn format_custom_fixed_rounds_half_away_from_zero() {
    // new: T1, custom numeric formats "0", "0.#", "0.##" on a double (golden pins 1.25, 0.96, 4294967295)
    let numbers = invariant_numbers();
    let rows: [(f64, u8, &str); 10] = [
        (1.25, 1, "1.3"),
        (-1.25, 1, "-1.3"),
        (0.96, 1, "1"),
        (0.96, 2, "0.96"),
        (1.0, 2, "1"),
        (100.0, 2, "100"),
        (0.0, 1, "0"),
        (2.5, 0, "3"),
        (-2.5, 0, "-3"),
        (4_294_967_295.0, 1, "4294967295"),
    ];
    for (value, decimals, expected) in rows {
        assert_eq!(
            format_custom_fixed(value, decimals, &numbers),
            expected,
            "{value} with {decimals} decimals"
        );
    }
}

#[test]
fn format_custom_fixed_uses_culture_separators() {
    // new: T1, the point is numbers.decimal_separator and the sign is numbers.negative_sign (no grouping in custom formats)
    let numbers = vi_numbers();
    assert_eq!(format_custom_fixed(1.25, 1, &numbers), "1,3");
    assert_eq!(format_custom_fixed(-1.5, 1, &numbers), "-1,5");
    assert_eq!(format_custom_fixed(1234.5, 1, &numbers), "1234,5");
}

#[test]
fn format_grouped_integer_groups_in_threes() {
    // new: T22, N0 grouping with the invariant group separator "," (size 3 only, contract Q1)
    let numbers = invariant_numbers();
    assert_eq!(format_grouped_integer(0, &numbers, ","), "0");
    assert_eq!(format_grouped_integer(999, &numbers, ","), "999");
    assert_eq!(format_grouped_integer(1000, &numbers, ","), "1,000");
    assert_eq!(format_grouped_integer(1_048_576, &numbers, ","), "1,048,576");
    assert_eq!(format_grouped_integer(-1_048_576, &numbers, ","), "-1,048,576");
    assert_eq!(
        format_grouped_integer(i64::MIN, &numbers, ","),
        "-9,223,372,036,854,775,808"
    );
}

#[test]
fn format_grouped_integer_uses_culture_group_and_sign() {
    // new: T22, vi-VN group "." (golden compare_size_suffix vi: " (1.048.576 byte)")
    let numbers = vi_numbers();
    assert_eq!(format_grouped_integer(1_048_576, &numbers, "."), "1.048.576");
    assert_eq!(format_grouped_integer(-5, &numbers, "."), "-5");
}

#[test]
fn index_only_wraps_like_unchecked_csharp() {
    // new: T21, C# int arithmetic is unchecked: index + 1 for i32::MAX gives i32::MIN (golden index_only)
    let env = en_env();
    assert_eq!(index_only(i32::MAX, 10, &env.ctx()), "-2147483648/10");
}

#[test]
fn compare_hash_text_off_same_and_different() {
    // new: T17, None gives compare.hash.off, Some(true) compare.hash.same, Some(false) compare.hash.different
    let env = en_env();
    let ctx = env.ctx();
    assert_eq!(compare_hash_text(None, &ctx), " | hash off");
    assert_eq!(compare_hash_text(Some(true), &ctx), " | hash SAME");
    assert_eq!(compare_hash_text(Some(false), &ctx), " | hash DIFFERENT");
    assert_eq!(compare_hash_unknown(&ctx), " | hash unknown");
}

#[test]
fn sibling_folder_boundary_zero_is_first() {
    // new: T18, direction > 0 gives the last-folder text; zero and negative give the first-folder text (golden direction 0)
    let env = en_env();
    let ctx = env.ctx();
    assert_eq!(
        sibling_folder_boundary(1, &ctx),
        "Already at the last folder on this level."
    );
    assert_eq!(
        sibling_folder_boundary(0, &ctx),
        "Already at the first folder on this level."
    );
    assert_eq!(
        sibling_folder_boundary(-5, &ctx),
        "Already at the first folder on this level."
    );
}

// ---- Golden checks over tests/golden/format-status.json ----

/// The `size` / `bytes` / `count` style integer arguments of a golden row, as `i64`.
fn i64_arg(args: &Value, key: &str) -> i64 {
    golden::i64_of(args, key)
}

/// One golden row through the formatter named by its `op`.
fn run_status_case(case: &Value, env: &Env) -> String {
    let args = &case["args"];
    let ctx = env.ctx();
    let op = golden::str_of(case, "op");
    match op {
        "file_size" => format_file_size(i64_arg(args, "bytes"), &ctx),
        "no_supported_images" => no_supported_images(&ctx),
        "no_supported_images_but_subfolders" => no_supported_images_but_subfolders(i32_of(args, "count"), &ctx),
        "folder_open_failed" => folder_open_failed(&text_of(args, "message"), &ctx),
        "index_only" => index_only(i32_of(args, "index"), i32_of(args, "count"), &ctx),
        "loading" => loading(
            i32_of(args, "index"),
            i32_of(args, "count"),
            i64_arg(args, "size"),
            &ctx,
        ),
        "loading_full_res" => loading_full_res(
            i32_of(args, "index"),
            i32_of(args, "count"),
            i64_arg(args, "size"),
            &ctx,
        ),
        "decoding_raw" => decoding_raw(&ctx),
        "ready" => ready(
            i32_of(args, "index"),
            i32_of(args, "count"),
            i64_arg(args, "size"),
            &text_of(args, "file_name"),
            &ctx,
        ),
        "with_dimensions" => with_dimensions(
            i32_of(args, "index"),
            i32_of(args, "count"),
            i64_arg(args, "size"),
            i32_of(args, "width"),
            i32_of(args, "height"),
            &text_of(args, "file_name"),
            &ctx,
        ),
        "compare" => {
            let status = CompareStatus {
                index: i32_of(args, "index"),
                count: i32_of(args, "count"),
                left_name: &text_of(args, "left_name"),
                left_size: &text_of(args, "left_size"),
                right_name: &text_of(args, "right_name"),
                right_size: &text_of(args, "right_size"),
                hash_text: &text_of(args, "hash_text"),
            };
            compare(&status, &ctx)
        }
        "compare_size_suffix" => compare_size_suffix(i64_arg(args, "bytes"), &ctx),
        "compare_hash_text" => compare_hash_text(opt_bool(args, "same"), &ctx),
        "compare_hash_unknown" => compare_hash_unknown(&ctx),
        "image_error" => image_error(&text_of(args, "file_name"), &text_of(args, "message"), &ctx),
        "no_images_remaining" => no_images_remaining(&ctx),
        "all_images_processed" => all_images_processed(&ctx),
        "cache_cleared" => cache_cleared(&ctx),
        "duplicate_check_failed" => duplicate_check_failed(&text_of(args, "message"), &ctx),
        "duplicate_check_canceled_folder_changed" => duplicate_check_canceled_folder_changed(&ctx),
        "duplicate_check_running" => duplicate_check_running(&ctx),
        "duplicate_check_canceled" => duplicate_check_canceled(&ctx),
        "no_duplicates_found" => no_duplicates_found(&ctx),
        "batch_canceled" => batch_canceled(&ctx),
        "batch_done" => batch_done(i32_of(args, "succeeded"), i32_of(args, "failures"), &ctx),
        "sibling_folder_boundary" => sibling_folder_boundary(i32_of(args, "direction"), &ctx),
        "action_completed" => action_completed(&text_of(args, "action_name"), &ctx),
        "action_invalid_operation" => action_invalid_operation(&text_of(args, "action_name"), &ctx),
        "action_no_destination" => action_no_destination(&text_of(args, "action_name"), &ctx),
        "action_failed" => action_failed(
            &text_of(args, "action_name"),
            opt_text(args, "message").as_deref(),
            &ctx,
        ),
        "copied_to" => copied_to(opt_text(args, "file_name").as_deref(), &ctx),
        "nothing_to_undo" => nothing_to_undo(&ctx),
        other => panic!("unknown format-status op {other:?}"),
    }
}

#[test]
fn golden_format_status() {
    // new: golden-driven, every case of tests/golden/format-status.json (oracle format-status, C# StatusFormatter); collects all differences
    let doc = golden::load("format-status");
    let mut mismatches = Vec::new();
    for case in &doc.cases {
        let env = env_for_case(case);
        let expected = text_of(case, "text");
        let actual = run_status_case(case, &env);
        if actual != expected {
            mismatches.push(format!(
                "{}: expected {expected:?}, got {actual:?}",
                golden::str_of(case, "name")
            ));
        }
    }
    assert!(
        mismatches.is_empty(),
        "{} of {} format-status cases differ from the C# oracle:\n{}",
        mismatches.len(),
        doc.cases.len(),
        mismatches.join("\n")
    );
}

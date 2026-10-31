# Sửa bảo mật HTTP: tắt tự đăng nhập WinHTTP và các siết chặt nhỏ

Status: đã sửa (lane `fix-http-autologon`). Nguồn: [ST-HTTP-SECURITY-2026-10-31.md](ST-HTTP-SECURITY-2026-10-31.md), mục M1, L1, L4, L8.

## M1: có thật, đã sửa

- Kiểm chứng C# tại `7eaee2a2`: `UpdateChecker.cs:33` dùng `new HttpClient()` (hoặc handler tiêm cho test); không có `Credentials`, `UseDefaultCredentials`, `DefaultProxyCredentials` hay `WebProxy` trong toàn bộ nguồn C#. Vậy C# không tự gửi thông tin đăng nhập; WinHTTP mặc định (MEDIUM) thì có thể gửi tới proxy/máy chủ vùng Intranet. Đây là khác biệt hành vi, không phải parity.
- Sửa: `crates/pr-platform-win/src/http/transport.rs` thêm hằng `REQUEST_OPTIONS` (gồm `WINHTTP_OPTION_AUTOLOGON_POLICY = WINHTTP_AUTOLOGON_SECURITY_LEVEL_HIGH`), đặt trên request handle bằng đúng vòng `WinHttpSetOption` có sẵn (đã có `// SAFETY:`), không thêm khối `unsafe` mới.
- Test: `request_options_disable_automatic_windows_logon` (thuần, khóa giá trị trong bảng option). Các test loopback hiện có vẫn pass (`WinHttpSetOption` chấp nhận option này). Chưa kiểm được bằng tay việc "không gửi NTLM" thật sự: cần rig proxy/máy chủ NTLM có đánh dấu (tự chạy trên máy lab: bật proxy đòi `Proxy-Authenticate: NTLM` ở vùng Intranet, xác nhận không có `Proxy-Authorization` được gửi).

## L1: đã sửa

`redirect_target` trim `Location`, rồi từ chối nếu có ký tự điều khiển (kể cả CR/LF/TAB/0x7f) hoặc khoảng trắng bên trong. Test: `redirect_target_rejects_control_characters_and_inner_spaces`.

## L4: đã thêm test

`get_rejects_non_loopback_http_before_opening_any_socket`: `http://example.invalid/`, `http://203.0.113.5/x`, `http://localhost/` đều trả `Other("HTTPS required…")` trước `Exchange::open` (không mở socket). Không đổi `cfg` của nhánh loopback.

## L8: đã sửa

`default_open_url` truyền chuỗi đã trim; `is_web_url` từ chối ký tự điều khiển, khoảng trắng và dấu nháy kép. Test mở rộng trong `tests/settings_dialog_state.rs` (`open_url_accepts_only_http_and_https_without_entering_the_shell`).

## Không làm

- M2 (ba hằng URL trỏ repo C#: `checker.rs:17`, `version.rs:99`, `version.rs:187`): quyết định sản phẩm của người dùng, chỉ ghi nhận, không đổi. Test "ba hằng cùng owner/repo" (mục 4 của báo cáo nguồn) không thêm vì sẽ khóa cứng giá trị chưa quyết.
- L2, L3, L5, L6, L7: ngoài phạm vi lane này. Test UNC (mục 5) chưa thêm.

## Đã chạy

`cargo test -p pr-platform-win --lib http::` (11 pass), `--test settings_dialog_state` (10 pass), `--test http_transport_loopback` (18 pass, 2 ignore sẵn), `cargo fmt --check`, `cargo clippy -p pr-platform-win --all-targets -- -D warnings` sạch, `tools/dev/unsafe-audit.sh` không vi phạm mới.

## Đề xuất PARITY (orchestrator quyết)

Hàng X-http-transport: thêm ghi chú "AUTOLOGON_POLICY=HIGH (Rust-only hardening, C# không gửi credential)". Không nâng trạng thái.

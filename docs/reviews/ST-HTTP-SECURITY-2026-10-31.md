# Review bảo mật tĩnh: mạng, kiểm tra cập nhật, pipe chuyển tiếp, mở URL

Status: báo cáo review tĩnh (lane `st-http-security`, nhánh `feat/st-http-security`, cắt từ origin/master `a61cb3aa`). Không build, không chạy test, không gọi mạng. **Mọi phát hiện là suy luận từ mã; chỗ nào phụ thuộc hành vi WinHTTP/Windows chưa chạy thử đều ghi "suy luận, chưa chạy".** So với C# tại `7eaee2a2`.

Tổng: **0 cao / 2 trung bình / 8 thấp / 4 ghi chú**. Không thấy lỗi chắc chắn khai thác được từ xa. Điểm đáng làm nhất: M1 (WinHTTP có thể tự gửi thông tin đăng nhập Windows tới proxy/máy chủ vùng Intranet, C# không làm vậy) và M2 (địa chỉ cập nhật và danh sách cho phép đang trỏ vào repo của dự án C#).

## Phạm vi đã đọc

| Khu vực | File (đọc đủ) |
|---|---|
| Transport/TLS/PAC/proxy/redirect | `crates/pr-platform-win/src/http/{mod,policy,transport}.rs` (743 dòng), test `tests/http_policy.rs`, `tests/http_transport_loopback.rs` (tên test + phần ignore) |
| Bộ kiểm tra cập nhật, JSON, URL | `crates/pr-core/src/updates/checker.rs` (phần request/đọc body/`interpret`/`Scanner`), `updates/version.rs` (`AppVersion`, `validate_update_url` và các hàm escape) |
| Lắp ráp và mở URL | `crates/pr-shell/src/composition/update_http.rs`, `crates/pr-app/src/presenters/settings/apply.rs` (`open_update_page`), `crates/pr-platform-win/src/dialogs/settings/dialog/host.rs` (`default_open_url`, `is_web_url`) |
| Pipe chuyển tiếp | `crates/pr-platform-win/src/forward_server.rs`, `forward_client.rs`, `forward_pipe.rs`, `crates/pr-core/src/instance/{protocol,handoff,forwarding}.rs`, `crates/pr-app/src/startup/forwarded_open.rs` (đầu file) |
| C# đối chiếu | `UpdateChecker.cs`, `UpdateUrlPolicy.cs`, `InstanceForwardServer.cs`, `InstanceForwardClient.cs` ở `7eaee2a2` (đọc các đoạn liên quan) |

Không đọc: `instance_scope.rs` (chỉ grep chỗ gọi), `coalescer.rs`, phần dưới của `forward_server.rs`/`forward_client.rs` ngoài đoạn FFI đã trích, test native.

## Điều KHÔNG tồn tại (nên ghi rõ để khỏi tìm)

- **Không có tải về, không ghi file, không chữ ký/hash bản cập nhật.** Luồng cập nhật chỉ GET một JSON (`LATEST_RELEASE_URL`), so phiên bản, rồi (khi người dùng bấm) mở trang phát hành trong trình duyệt. Không có `browser_download_url`, không có đường dẫn tải, nên path traversal / ghi ngoài thư mục tạm / xác thực chữ ký đều **không áp dụng**. (Đã grep `browser_download_url`, `WinVerifyTrust`, `Authenticode`: không có.) Nếu sau này thêm tải tự động thì cần thiết kế riêng, xem khuyến nghị cuối.
- Tầng http không ghi log gì cả (đã đọc cả 3 file); thông báo lỗi là chuỗi cố định, không chứa URL, proxy hay header.

## Phát hiện

### M1. WinHTTP có thể tự đăng nhập bằng thông tin Windows của người dùng (NTLM/Negotiate) tới proxy/máy chủ vùng Intranet (trung bình, suy luận, chưa chạy)

- File: `http/transport.rs:315-322` (`WinHttpOpen`, không đặt `WINHTTP_OPTION_AUTOLOGON_POLICY`), `:307` (`AUTOMATIC_PROXY`), `:349-352` (chỉ đặt `REDIRECT_POLICY` và `MAX_RESPONSE_HEADER_SIZE`).
- Kịch bản: kẻ trong LAN dựng WPAD/PAC giả (hoặc người dùng bị PAC độc) trả về một proxy mang tên thuộc vùng Intranet. Chính sách mặc định của WinHTTP là `MEDIUM` (tự gửi thông tin đăng nhập hiện tại tới máy chủ/proxy ở vùng Intranet khi bị thách thức NTLM/Negotiate), nên lần bấm "Check for updates" có thể làm lộ băm NTLMv2 của người dùng. Chưa chạy; đây là hành vi mặc định theo tài liệu WinHTTP.
- C#: `HttpClient()` mặc định không có `Credentials`/`DefaultProxyCredentials`, không tự gửi thông tin đăng nhập. Đây là **khác biệt hành vi so với C#**, không phải parity.
- Test: không có (và không thể có test không cần rig). `PARITY.md` X-http-transport chỉ nói "Authenticated/TLS proxy URIs unsupported", không nêu autologon.
- Đề xuất: ngay sau `WinHttpOpen`/mở request, `WinHttpSetOption(request, WINHTTP_OPTION_AUTOLOGON_POLICY, WINHTTP_AUTOLOGON_SECURITY_LEVEL_HIGH)` (không bao giờ tự gửi thông tin đăng nhập). Thêm một dòng vào ghi chú PARITY của X-http-transport. Kiểm chứng thủ công bằng rig proxy NTLM có đánh dấu.

### M2. Địa chỉ cập nhật và danh sách cho phép là repo của dự án C# (trung bình về tin cậy, không phải lỗ hổng mã)

- File: `pr-core/src/updates/checker.rs:17` (`.../repos/ConanKLOP2/PhotoReview/releases/latest`), `updates/version.rs:99` (`RELEASES_PAGE_URL`), `:187` (`PROJECT = "/ConanKLOP2/PhotoReview"`).
- Kịch bản/lỗi: bản Rust (PhotoReviewRs) hỏi bản phát hành của dự án C#. Mọi bản C# mới hơn sẽ báo "có bản mới" cho bản Rust và dẫn người dùng tới trang tải bản C#; nếu chủ repo đó (hoặc ai chiếm lại tài khoản/tên repo) đổi nội dung, người dùng Rust bị dẫn tới nguồn không thuộc dự án. Allow-list `validate_update_url` chỉ bảo vệ "đúng github.com và đúng đường dẫn đó", không bảo vệ "đúng dự án này".
- Đã có ghi nhận: doc comment `checker.rs:16` ("see contracts/W2-18.md open question 2"). Chưa thấy quyết định đóng.
- Test: `updates_checker.rs::foreign_url_falls_back_to_releases_page`, `updates_version.rs::golden_update_url_policy` (chỉ kiểm theo hằng đang dùng).
- Đề xuất: trước khi phát hành, đổi 3 hằng trên sang repo thật của bản Rust (cùng lúc, cập nhật golden), và thêm test khóa "ba hằng cùng một owner/repo". Ghi vào PROPOSED-DECISIONS nếu chưa có.

### L1. `Location` chuyển hướng không bị từ chối ký tự điều khiển (thấp, suy luận, chưa chạy)

- File: `http/transport.rs:198-220` (`redirect_target`: chỉ `wide()` loại NUL; `UrlCombineW` rồi `Url::parse`), `:417-466` (`headers()` tách theo `\r\n`, giá trị chỉ `trim`).
- Kịch bản: giá trị `Location` có `\r` đơn lẻ, tab hoặc ký tự điều khiển khác (WinHTTP tách dòng ở LF/CRLF nên không có CRLF trọn vẹn, nhưng CR đơn hoặc `\t`/`\x01` vẫn có thể còn trong giá trị). `WinHttpCrackUrl` có thể chấp nhận, rồi đường dẫn đó được gửi nguyên vào dòng yêu cầu của `WinHttpOpenRequest`. Chỉ ảnh hưởng yêu cầu gửi tới máy chủ mà nó đã chọn (không gửi dữ liệu người dùng, không có credential), nên tác hại nhỏ; nhưng có thể làm proxy trung gian phân tích dòng yêu cầu sai (request smuggling mức proxy). Chưa kiểm chứng.
- Test: `redirect_target_validates_uri_and_combines_relative_paths` (transport.rs:598) chỉ kiểm `""`, `file:///a`, `user:secret@`, `"\0"`.
- Đề xuất: trong `redirect_target` trả `None` nếu `location` hoặc kết quả có `char::is_control()` hoặc khoảng trắng bên trong; thêm vào test đó các mẫu `"\r"`, `"\t"`, `"a b"`, `"/x\u{7f}"`. Nhỏ, thuần hàm, không cần mạng.

### L2. Giao thức TLS, thu hồi chứng chỉ chưa được ghim, dùng mặc định hệ điều hành (thấp, tương đương C#)

- File: `http/transport.rs` không có `WINHTTP_OPTION_SECURE_PROTOCOLS`, `WINHTTP_OPTION_SECURITY_FLAGS`, `WINHTTP_OPTION_ENABLE_SSL_REVOCATION`. Đã grep toàn `pr-platform-win/src`: không có chỗ nào tắt kiểm tra chứng chỉ (tốt; test còn ghi "do not disable certificate validation": `http_transport_loopback.rs:118`).
- Ý nghĩa: kiểm tra chuỗi chứng chỉ/tên máy chủ chạy mặc định của WinHTTP (tốt). Phiên bản TLS tối thiểu theo mặc định của máy (Windows 10 cũ còn có thể thương lượng TLS 1.0/1.1; suy luận, chưa chạy). Không kiểm tra thu hồi, giống .NET `HttpClient` mặc định (NoCheck). Không ghim chứng chỉ GitHub (không cần).
- Đề xuất (tùy chọn): đặt `SECURE_PROTOCOLS` = TLS1.2 (| TLS1.3 nếu hằng có trên SDK) để không phụ thuộc cấu hình máy. Đây là siết chặt hơn C#, nên ghi vào decisions nếu làm.
- Test: chỉ có rig bị bỏ qua `native_proxy_pac_trust_and_cancel_matches_windows_defaults` (`http_transport_loopback.rs:251-255`, thân là `panic!`).

### L3. Redirect được phép tới bất kỳ máy chủ HTTPS nào (thấp, tương đương C#)

- File: `policy.rs:redirect_decision` và `transport.rs:516-535`: chỉ chặn HTTPS→HTTP, tối đa 50 lần, không giới hạn host/cổng.
- Kịch bản: nếu `api.github.com` hay bên giữa trả 301/302 tới `https://nội-bộ:cổng/…`, máy khách gửi GET tới đó (SSRF mù); phản hồi chỉ được đọc để phân tích JSON rồi bỏ. Không rò dữ liệu người dùng (chỉ gửi `Accept`, `User-Agent`, cookie phiên của chính lượt kiểm tra). Cùng hành vi .NET.
- Đề xuất: giới hạn host redirect trong `{api.github.com, github.com}` (và có thể `*.githubusercontent.com`) là siết chặt hơn C#, cần nêu trong decisions; hoặc chấp nhận rủi ro thấp này. Nếu làm, đặt trong `policy.rs` (thuần hàm, dễ test).
- Test: `redirect_decision_status_location_limit_and_downgrade_returns_expected_decision` (http_policy.rs), `get_fifty_redirects_*`. Thiếu: redirect HTTPS→HTTPS sang host lạ (hiện được phép, không có test khẳng định ý định).

### L4. Phân loại "loopback HTTP cho test" nằm trong binary phát hành (thấp)

- File: `transport.rs:178` (`loopback`), `:518-520` (`!url.https && !url.loopback` mới từ chối), `:289` (loopback bỏ qua proxy).
- Ý nghĩa: URL đầu là hằng HTTPS và HTTPS→HTTP bị chặn, nên đường HTTP loopback không tới được trong sản phẩm (suy luận từ mã: chỉ tới được nếu URL ban đầu là loopback, mà `UpdateChecker::check` luôn dùng hằng). Rủi ro là về sau ai đó cho phép URL cấu hình được. Cần giữ bất biến "URL gốc luôn là hằng".
- Test hiện có: nhiều test loopback dùng `http://127.0.0.1`. **Thiếu**: một test trực tiếp khẳng định `get("http://example.invalid/")` (không loopback) trả lỗi `Other("HTTPS required…")` mà không mở socket (hiện chỉ gián tiếp). Không cần mạng (dừng trước `Exchange::open`).
- Đề xuất: thêm test đó vào `tests/http_transport_loopback.rs` (hoặc file mới) và cân nhắc `#[cfg(any(test, feature = "loopback-http"))]` cho nhánh loopback nếu muốn loại khỏi bản phát hành (đổi nhỏ, cần build để kiểm).

### L5. Pipe chuyển tiếp: tạo pipe không có `FILE_FLAG_FIRST_PIPE_INSTANCE` (thấp, C# cũng không có)

- File: `forward_server.rs:459` (`PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED`, `max instances = 1`). C#: `NamedPipeServerStreamAcl.Create(..., 1, ..., PipeOptions.Asynchronous, ...)` (dòng 73-74), không `FirstPipeInstance`.
- Kịch bản: tên pipe là `PhotoReviewRs.Forward.<SHA-256(mutex|SID|session)>` (`forward_pipe.rs:name_for_mutex`), suy ra được từ SID công khai. Người dùng khác cùng máy tạo trước pipe tên đó với DACL rộng thì: (a) máy chủ thật không tạo được (lặp lại với backoff tới 30 s, log mỗi lũy thừa 2: `forward_server.rs` `run_server`), tức DoS chuyển tiếp; (b) nếu thuộc tính cho phép, phiên bản pipe của ta có thể nằm trong "pipe của kẻ tấn công" với DACL của họ (nghi ngờ, chưa chạy). Phía khách đã chặn việc gửi cho máy chủ lạ (xem N1), nên không rò đường dẫn.
- Đề xuất: thêm `FILE_FLAG_FIRST_PIPE_INSTANCE` cho mỗi lần tạo (vì max=1 và pipe cũ đã đóng trước khi tạo cái mới), xử lý `ERROR_ACCESS_DENIED` như lỗi cấu hình đáng log. Cần thử native.
- Test: `server_pipe_mode_rejects_remote_clients` (trong `forward_server.rs`), `tests/forward_server_native.rs`. Không có test cho "tên pipe đã bị chiếm".

### L6. Máy chủ gọi `path_exists` (std::fs::metadata) trên đường dẫn UNC trước khi gọi callback (thấp)

- File: `forward_server.rs:139-149` (mặc định `std::fs::metadata`), `protocol.rs:is_acceptable_path` (chấp nhận UNC `\\host\share\x`, chỉ cấm `\\?\`, `\\.\`, `\??\`).
- Kịch bản: bất kỳ tiến trình nào chạy cùng người dùng ghi được vào pipe có thể ép luồng nghe truy cập `\\kẻ-tấn-công\share` (rò băm NTLM, treo luồng chấp nhận tới hết timeout SMB; các pipe khác phải chờ). Vì DACL pipe chỉ cho đúng SID người dùng, kẻ đó đã chạy mã dưới quyền người dùng, nên giá trị tăng thêm thấp. Với người dùng thật mở file từ share mạng qua Explorer thì hành vi này là đúng và phải giữ (C# cũng vậy).
- Đề xuất: không đổi hành vi; ghi vào tài liệu. Nếu muốn: chạy `path_exists` có timeout riêng để không chặn vòng lặp chấp nhận.
- Test: `instance_forwarding.rs` cover cấm `\\?\`, `\\.\pipe`, `..`, điều khiển; thiếu test khẳng định ý định chấp nhận UNC.

### L7. Phía khách không đặt mức mạo danh `SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION` (thấp, không khai thác được)

- File: `forward_client.rs:374-381` (`CreateFileW(..., FILE_ATTRIBUTE_NORMAL | FILE_FLAG_OVERLAPPED, None)`), C# dùng `PipeOptions.CurrentUserOnly` (kiểm chủ sở hữu pipe).
- Ý nghĩa: máy chủ giả có thể mạo danh khách chỉ sau khi đọc dữ liệu từ khách; khách chỉ ghi sau `server_is_current_user()` (so SID tiến trình máy chủ với SID hiện tại, `:408-427`, thất bại thì đóng, tốt hơn kiểm tra chủ sở hữu pipe ở một số trường hợp), nên thực tế không khai thác được. Là hardening phòng sâu.
- Đề xuất: thêm cờ `SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION` vào `CreateFileW`. Cần thử native (`forward_client_native.rs`).

### L8. `default_open_url`: kiểm tra trên chuỗi đã `trim` nhưng truyền chuỗi chưa `trim`, không loại ký tự điều khiển/dấu nháy (thấp, phòng sâu)

- File: `dialogs/settings/dialog/host.rs:33-54` (`is_web_url` dùng `url.trim()`, nhưng `file = url.encode_utf16()` dùng `url` gốc), `:56-64`.
- Ý nghĩa: người gọi duy nhất truyền kết quả `validate_update_url` (đã escape khoảng trắng, `"`, `<`, `>`, `^`, backtick, `{|}`; tôi đọc `escape_units`, `version.rs:346-383`), nên hiện không khai thác được. Nhưng `SettingsDialogOptions::open_url` là điểm tiêm (`frame.rs:134`) và hàm này là "fallback" công khai: một người gọi khác truyền `" https://x"` hay URL có khoảng trắng/dấu nháy/`\t` sẽ qua kiểm tra rồi vào `ShellExecuteExW` với verb `open`. Dấu nháy trong URL từng là vector chèn đối số vào dòng lệnh trình duyệt.
- Test: `settings_dialog_state.rs:238-262` (rỗng, scheme, `is_web_url` dương/âm); thiếu: ký tự điều khiển, khoảng trắng, `"`.
- Đề xuất: `is_web_url` trả false nếu có `char::is_control`, khoảng trắng hay `"`; truyền chuỗi đã `trim`. Hàm thuần, thêm test không cần UI.

### N1. Ghi chú tốt (đã xác nhận bằng đọc mã)

- Khách chuyển tiếp so SID tiến trình máy chủ với SID hiện tại trước khi gửi bất kỳ byte nào (`forward_client.rs:136`, `:408`), và pipe máy chủ có DACL chỉ một SID cùng `PIPE_REJECT_REMOTE_CLIENTS` (`forward_pipe.rs`, `forward_server.rs:321`, `server_pipe_mode`). Giới hạn: tin nhắn 64 KiB, 16 đường dẫn, timeout 5 s đọc (`protocol.rs`, `forward_server.rs:handle_request`), từ chối `..`, tiền tố thiết bị, ký tự điều khiển, đường dẫn không đủ điều kiện; log không chứa đường dẫn (chỉ số byte/số đường dẫn, `handoff.rs`, `forward_server.rs`).
- Nội dung phản hồi: giới hạn 1 MiB cả khi có `Content-Length` lẫn khi chunked (`checker.rs:23`, `:277-293`), tổng thời gian 10 s một mốc cho cả chuỗi redirect + đọc body (`transport.rs` `Deadline`, `get` dòng 506-550), header tối đa 64 KiB (`HEADER_CAP`), `WinHttp` không bật tự giải nén (không bom nén), chuỗi hợp lệ khi `Content-Length` sai không qua được vì đếm theo byte đã đọc.
- JSON do máy chủ gửi: bộ quét tự viết (`Scanner`) giới hạn độ sâu 64 (`checker.rs:581`, `:767`), không đệ quy vô hạn; `tag_name` chỉ nhận dạng `số.số.số` (kèm `v`, `+`, `-`) nên không có văn bản tự do nào lên giao diện ngoài chữ số; `html_url` đi qua `validate_update_url` và được kiểm lại mỗi lần bấm (`apply.rs:638-648`).
- URL cập nhật: chỉ `https://github.com/<owner>/<repo>[/…]`, từ chối userinfo khác rỗng, cổng khác 443, host khác (kể cả lừa bằng ký tự bidi), `..` mã hóa (`%2e%2e`), dấu `\`. Test vàng `golden_update_url_policy`, `validate_matches_net_uri_escape_edge_cases`.
- Transport: `WINHTTP_OPTION_REDIRECT_POLICY_NEVER` (tự quản redirect), chặn HTTPS→HTTP ở `redirect_decision`, từ chối URL chứa userinfo (`transport.rs:167`) kể cả trong `Location`, từ chối CR/LF trong `Accept`/`User-Agent` (`transport.rs`, "invalid HTTP header"), từ chối NUL (`wide`), biến proxy môi trường có thông tin đăng nhập bị từ chối thay vì bỏ qua (không lộ qua proxy khác), cookie chỉ sống trong một lần kiểm tra.

### N2. Ghi chú: giới hạn chức năng, không phải bảo mật

- Proxy xác thực qua `HTTPS_PROXY=http://user:pass@…` bị từ chối (`environment_proxy`, `transport.rs:237-250`); .NET chấp nhận. Người dùng sau proxy xác thực sẽ thấy "bad response" thay vì gợi ý về proxy (lỗi `Other` ánh xạ thành `BadResponse`, `checker.rs:264`). Đây là khoảng trống parity đã nêu ở PARITY, đồng thời là lựa chọn an toàn (không rò thông tin đăng nhập). Đề xuất: giữ, thêm một thông điệp lỗi riêng cho người dùng khi cần.
- `WinHttpCloseHandle` trong `Handle::drop` có thể chặn khi một thao tác bất đồng bộ (dò WPAD/autoproxy) còn treo (nghi ngờ, chưa chạy; mã giả định đóng ngay và `HANDLE_CLOSING` đến sau). Công việc chạy ở luồng nền (`begin_update_check`) nên chỉ gây trễ đóng hộp thoại, không treo giao diện.
- Thăm dò hủy mỗi 10 ms (`Deadline::wait`) tốn CPU nhẹ trong tối đa 10 s: chấp nhận được.
- IPv6 loopback `[::1]`: `host.parse::<IpAddr>()` có thể thất bại nếu `WinHttpCrackUrl` trả host có ngoặc vuông; khi đó được coi là không phải loopback và HTTP bị từ chối (an toàn, chỉ làm test IPv6 không chạy). Chưa chạy.

## Đối chiếu C# (khác biệt đáng chú ý)

| Chủ đề | C# `7eaee2a2` | Rust | Nhận xét |
|---|---|---|---|
| Thông tin đăng nhập tự động | không | có thể (M1) | khác, nên chặn |
| Redirect | .NET mặc định (50, chặn HTTPS→HTTP) | tự quản, cùng chính sách | parity |
| Giới hạn body 1 MiB, timeout 10 s | có | có | parity, có test biên |
| Proxy | `HttpClient.DefaultProxy` (env rồi hệ thống, có thể có credential) | env rồi `AUTOMATIC_PROXY`, credential bị từ chối | khác an toàn hơn, thiếu tính năng |
| Pipe khách | `CurrentUserOnly` (chủ sở hữu pipe) | so SID tiến trình máy chủ | tương đương, mức khác |
| Pipe máy chủ | max 1 instance, DACL 1 SID | giống, thêm `REJECT_REMOTE_CLIENTS` | Rust siết hơn |
| `FirstPipeInstance` | không | không | cùng khoảng trống (L5) |
| Mở URL | `Process.Start(UseShellExecute)` sau `UpdateUrlPolicy` | `ShellExecuteExW` sau `validate_update_url` + `is_web_url` | Rust siết hơn một chút (L8 còn phòng sâu) |

## Test hiện có / thiếu (tổng hợp)

Hiện có: `http_policy.rs` (4), 3-4 test thuần trong `transport.rs`, `http_transport_loopback.rs` (18 chạy, 2 ignore là rig), `updates_checker.rs`, `updates_version.rs`, `instance_forwarding.rs`, `forward_client.rs`, `*_native.rs`, `settings_dialog_state.rs`.

Thiếu và làm được **không cần mạng/build nặng** (đề xuất cho lane code, mỗi cái một test thuần):
1. `redirect_target` với `\r`, `\t`, khoảng trắng, `\x7f` (L1).
2. `get("http://example.invalid/")` trả `Other("HTTPS required…")` không mở socket (L4).
3. `is_web_url`/`default_open_url` với ký tự điều khiển, khoảng trắng, `"`, URL có khoảng trắng đầu (L8).
4. Khóa "ba hằng URL cùng owner/repo" (M2).
5. Khẳng định chủ đích chấp nhận UNC ở `is_acceptable_path` (L6).
Cần rig/native (không làm ở lane tĩnh): autologon (M1), tên pipe bị chiếm (L5), mức mạo danh (L7), TLS/PAC/trust (đã là placeholder ignore).

## Đề xuất cho PARITY (orchestrator quyết, lane này không sửa PARITY)

- Hàng `X-http-transport` (PARITY.md dòng 69): thêm vào phần "NOT proved": chính sách autologon WinHTTP mặc định (M1), giao thức TLS tối thiểu không ghim (L2), `Location` có ký tự điều khiển (L1). Lý do: bằng chứng ở `transport.rs:315-352` (không có `AUTOLOGON_POLICY`/`SECURE_PROTOCOLS`) và `:198-220`.
- Hàng `X-http-composition` (dòng 70): không đổi; thêm tham chiếu M2 (hằng repo C#) nếu hàng nào đang tuyên bố "Done" cho nguồn cập nhật.
- Hàng `SEC-02` (dòng 453): vẫn đúng (encode dùng chung `is_acceptable_path`); ghi chú L5/L6/L7 là phòng sâu, không đổi trạng thái.

## Khuyến nghị nếu sau này thêm tải bản cập nhật tự động

Hiện chưa có; nếu thêm: chỉ tải về thư mục tạm riêng của người dùng với tên file sinh ra (không dùng tên từ máy chủ), từ chối `..`/ổ đĩa/ADS trong tên, tải qua cùng transport có giới hạn kích thước riêng, xác minh hash công bố **và** chữ ký Authenticode (hash lấy từ cùng máy chủ không đủ), không thực thi tự động, không theo redirect ra ngoài danh sách host, và có test đường dẫn/tên file thuần hàm trước khi viết I/O.

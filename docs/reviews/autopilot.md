# Review tĩnh: tools/autopilot

Machine C (cloud). Phạm vi: `tools/autopilot/{claim.sh,codex-gate.sh,codex-pipeline.sh,codex-pack.sh,codex-run.sh,codex-usage.sh,status.sh,unit-pr.sh,lib/codex-lib.py,lib/fmtcheck.sh,test/pipeline-selftest.sh}`. Đã đọc từng dòng các tệp trên (trừ `workflows/*.js` và docs, ngoài phạm vi). Không có tệp nào bị bỏ qua.

**Mọi phát hiện dưới đây: suy luận từ mã, chưa chạy thử.** Không chạy bất kỳ script nào (kể cả đoạn Python thuần), không chạy cargo. Các kịch bản dưới đây là lần theo mã, chưa chứng minh bằng chạy.

Tổng: 1 cao / 7 trung bình / 11 thấp.

## Đã đọc và thấy đúng
- `claim.sh claim`: tạo ref `refs/claims/<unit>` bằng commit không cha rồi `push` không `--force`, nên hai máy không thể cùng tạo một ref; phân biệt "máy khác giữ" (exit 1) và "lỗi mạng/quyền" (exit 2) dựa trên việc ref có tồn tại sau khi fetch lại.
- `codex-gate.sh`: thứ tự kiểm tra (owned-files → lint → red-test guard → fmt → check/clippy → test) và `fail` luôn xoá tệp tạm trước khi thoát; các lỗi của `$LIB owned` làm danh sách rỗng nên kiểm tra owned-files thất bại theo hướng an toàn.
- `codex-pipeline.sh`: `reset_unit` chỉ `rm -rf` thư mục nằm dưới `$REPO/.claude/worktrees/wu-*`, lưu đầu nhánh chưa merge vào `refs/pipeline-old/<unit>` trước `branch -D`; không có `git push --force`; PR chỉ bật auto-merge khi workspace gate đạt.
- `lib/codex-lib.py`: `state_save` ghi tệp tạm rồi `os.replace` (nguyên tử); `cmd_redcheck` chia khối `---- name stdout ----` đúng chỉ số; `cmd_lint` không thao tác ghi ngoài `git add -N` cho tệp sở hữu.

## Phát hiện

### H1 (cao, xác nhận bằng cách đọc): `claim.sh fetch_claims` không có `--prune`; claim đã nhả vẫn còn trong bản sao cục bộ
- File: `tools/autopilot/claim.sh:20` `g fetch -q origin '+refs/claims/*:refs/claims/*'` (không `--prune`); đọc claim cục bộ ở `claim.sh:27` (`rev-parse refs/claims/$u`), `list` (51), `gc` (58).
- Mô tả: fetch chỉ tạo/cập nhật ref, không bao giờ xoá ref cục bộ mà bên kia đã xoá trên origin (`release`, `gc`). `claim` coi sự tồn tại của ref cục bộ là "đã có người giữ".
- Kịch bản: máy A claim `W8-18o`; máy B fetch (có ref cục bộ); A chạy `release` (xoá trên origin); B chạy `claim.sh claim W8-18o` → `rev-parse` thấy ref cũ → `holder` = A ≠ B → in "is claimed by A", exit 1. `codex-pipeline.sh:554` đánh dấu unit `skipped: claimed by A` và bỏ qua; B không bao giờ nhận được unit đó (tới khi ai đó xoá tay `refs/claims/W8-18o` cục bộ). `claim.sh gc` cũng không tự sửa: nó lặp trên ref cục bộ, `g push ":refs/claims/$x"` cho ref không còn trên origin thất bại nên `&& update-ref -d` không chạy. `list` hiển thị claim ma.
- Gợi ý sửa: `git fetch --prune origin '+refs/claims/*:refs/claims/*'` (hoặc dùng `ls-remote` để quyết định).

### M1 (trung bình, xác nhận): `testguard` chỉ kiểm "dòng cũ còn là dãy con", và thất bại im lặng khi không đọc được `base`
- File: `lib/codex-lib.py:446-477` `cmd_testguard`, đặc biệt dòng 469-471 (`all(any(x == y for y in it) for x in lines)`) và 451-452 (`old = _git(...)`; `if not old: continue`).
- Mô tả: thân hàm test cũ chỉ cần là dãy con (theo thứ tự) của thân mới; thêm dòng không bị chặn. Thuộc tính trước `fn` không nằm trong thân và chỉ `#[ignore`/`#[should_panic` được đếm.
- Kịch bản (đều qua được `testguard` chế độ nghiêm ngặt): (1) chèn `return;` ngay sau `{` của test đỏ — mọi dòng `assert_*` cũ vẫn còn theo thứ tự, test thành rỗng nhưng "xanh"; (2) bọc thân trong `if false { … }` (dòng `if false {` và `}` là dòng thêm); (3) thêm `#[cfg(any())]` hoặc `#[cfg(not(test))]` lên `fn` — số `#[test]` không đổi nhưng test không còn chạy. Ngoài ra `_fn_bodies` (429-444) tô che chuỗi và `//` nhưng KHÔNG che `/* */`, chuỗi thô `r#"…"#` và ký tự `'{'`/`'}'` nên dấu ngoặc sai làm thân bị cắt sớm → phần sau không được so; và `res.setdefault(name, …)` chỉ giữ hàm đầu tiên khi trùng tên (hai `mod` cùng tên hàm).
- Fail-open: nếu `GATE_TEST_BASE` là rev không đọc được (gõ sai, shallow clone) mọi `git show` trả rỗng → `continue` → không vi phạm nào → guard đạt. Selftest (`test/pipeline-selftest.sh:396-402`) không có ca nào cho các trường hợp trên.
- Chế độ `relaxed` (dùng cho fixup) chỉ đếm tổng `assert!` toàn tệp: xoá assert ở test này, thêm ở test khác vẫn đạt.
- Gợi ý: so sánh thân theo chuỗi đầy đủ cho test cũ (cho phép chỉ thêm ở cuối), kiểm thuộc tính `#[cfg` thêm vào, và coi `git show` lỗi là vi phạm.

### M2 (trung bình, xác nhận): `cmd_lint` coi mọi thứ sau `#[cfg(test)]` đầu tiên là mã test
- File: `lib/codex-lib.py:395` `testfrom = … next(i+1 for … if "#[cfg(test)]" in x)`; dùng ở 397 (`in_test`) và 416-418 (quét `todo!()`).
- Mô tả: chỉ lấy dòng đầu tiên chứa chuỗi `#[cfg(test)]` (kể cả đặt trên một `use`/`fn` đơn lẻ hoặc trong comment). Từ đó trở đi quy tắc `unwrap`/`unsafe`-ngoài-pr-platform-win và quét `todo!()` bị bỏ qua.
- Kịch bản có thật trong repo: `crates/pr-platform-win/src/dialogs/native/dialog/tooltip.rs:51` và `…/events.rs:36` (đều có `#[cfg(test)]` đặt trên một mục ở khoảng dòng 36-51 của tệp 376/385 dòng), cũng `explorer_order_service.rs:131`, `misc/app_log.rs:57`, `display_clock.rs:114`. Với tệp như vậy, một `todo!()`/`unwrap` trên đường dữ liệu người dùng viết ở phần sau không bị lint chặn. Gate tĩnh báo đạt trong khi quy tắc nói là chặn.
- Gợi ý: xác định phạm vi khối `mod tests` thật (khớp ngoặc) thay vì dòng đầu tiên.

### M3 (trung bình, xác nhận): `extsat` coi mọi lần nhắc tên unit trong commit không phải docs là "đã merge"
- File: `lib/codex-lib.py:117-134` `cmd_extsat`: duyệt `origin/master` bỏ commit có chủ đề `docs(`, `docs:`, `chore(`, `chore:`, `ci(`, `style(`; với commit khác, nếu `uid in expand_unit_ids(subject + body)` thì coi unit đã có trên master. Ngoài ra trạng thái `done`/`partial` trong tệp state cục bộ cũng đủ.
- Kịch bản: commit `feat(W8-17c): … (partial)` (đã có trong repo, #177) có thân liệt kê "test(W8-17a): red tests"; một commit "feat(W9-E4): … needs W8-09b" làm `W8-09b` được xem là thoả. Đơn vị W9 phụ thuộc sẽ được khởi chạy khi phần phụ thuộc chỉ mới là bản một phần, chỉ có test đỏ, hoặc chỉ được nhắc tới. `unit-pr.sh:79` dùng cùng hàm để quyết định mở PR.
- Gợi ý: yêu cầu id xuất hiện trong chủ đề dạng `feat(<id>)`/`Merge <id>` hoặc kiểm tra tệp thuộc owned có trên master và không còn `todo!`.

### M4 (trung bình, xác nhận): `claim.sh gc` dùng `git log --grep "$x"` (regex, khớp chuỗi con) để quyết định xoá claim
- File: `claim.sh:60`: `g log origin/master --oneline -1 --grep "$x"` (không `-F`, không neo).
- Kịch bản: claim `W8-1` hay `W9-E4` bị xoá khi BẤT KỲ commit nào trên master nhắc chuỗi đó (ví dụ commit `feat(W8-17c): … (partial)` xoá claim `W8-17c` dù việc dở; claim `REVIEW-startup` bị xoá khi một commit chứa "REVIEW-startup"). Ký tự regex (`.`, `+`…) trong tên claim cũng khớp rộng. Cộng với giới hạn tuổi 14 ngày (`age > 1209600`, tính theo giờ tạo claim) mà không xét việc đơn vị còn chạy: claim của việc kéo dài hơn 14 ngày bị gc xoá, máy khác nhận trùng.
- Gợi ý: `--fixed-strings` và khớp theo mẫu `(<id>)`/`Merge <id>`; chỉ xoá theo tuổi khi `--stale` được yêu cầu rõ.

### M5 (trung bình, xác nhận): khoá merge không tự phục hồi nếu thiếu tệp `pid`; khoá chính có cửa sổ TOCTOU
- File: `codex-pipeline.sh:286-293` `with_merge_lock`: điều kiện gỡ khoá `{ [ -z "$op" ] || ! kill -0 … ; } && [ -d "$l" ] && [ -n "$op" ] && rm -rf "$l"` — chỉ xoá khi `op` KHÔNG rỗng và tiến trình đã chết.
- Kịch bản: tiến trình giữ khoá bị giết sau `mkdir "$l"` nhưng trước `echo "$BASHPID" > "$l/pid"` (hoặc tệp `pid` bị xoá) → `op` rỗng → không bao giờ gỡ → vòng `while ! mkdir` ngủ 2 s mãi mãi, pipeline treo. Ngược lại khoá chính (`:121-127`) coi `op` rỗng là cũ: máy B vừa `mkdir` xong, chưa kịp ghi pid, máy A thấy `op` rỗng → `rm -rf "$LOCK"; mkdir` → hai pipeline cùng chạy.
- Gợi ý: ghi pid vào tệp tạm rồi `mv`, hoặc dùng `flock`/tuổi của thư mục khoá.

### M6 (trung bình, xác nhận): ngắt (INT/TERM) giữ nguyên các tiến trình con và xoá khoá/target đang dùng
- File: `codex-pipeline.sh:133-134` `on_exit` (`cleanup_targets; rm -rf "$LOCK"`) và `trap 'say "interrupted"; exit 130' INT TERM`; các unit chạy là `( run_unit … ) &` (557).
- Kịch bản: Ctrl-C/TERM → script chính thoát, xoá `target-wave-pipe-*` và khoá, nhưng các subshell `run_unit` và `codex.exe`/`cargo` con vẫn chạy trên worktree và thư mục target vừa bị xoá; pipeline mới (khoá đã gỡ) bắt đầu và `reset_unit` gỡ worktree/nhánh mà tiến trình cũ vẫn ghi vào. `tdir_free` chỉ giảm thiểu cho khoá build của cargo.
- Gợi ý: trong trap `kill` các PID trong `SLOTPID` và đợi trước khi dọn.

### M7 (trung bình, xác nhận): `unit-pr.sh` để lại worktree bẩn sau mọi thất bại; chạy lại bị chặn
- File: `unit-pr.sh:92-98` rồi các `exit 4/5/6/7` sau khi worktree `$WT` đã được tạo (103-106) và tệp đã được sao chép (120-126); không có `trap`/dọn dẹp (chỉ nhánh `--dry-run` ở 187-190 dọn).
- Kịch bản: biên dịch lỗi (exit 5) hoặc clippy lỗi (exit 7) → worktree còn thay đổi chưa commit; lần chạy sau vào nhánh `[ -z "$(git status --porcelain -uall)" ] || { echo "worktree has changes"; exit 2; }` → mãi exit 2 cho đến khi dọn tay. Tương tự, `exit 0` "already open: $BR" (86-89) xảy ra cả khi PR đã đóng/đã merge nhưng nhánh từ xa còn lại, tức là báo thành công mà không có PR.

### L1 (thấp, xác nhận): pipeline không bao giờ nhả claim
- `grep` trong `codex-pipeline.sh` và `unit-pr.sh` không có `claim.sh release`. Claim chỉ biến mất bằng `gc` (14 ngày hoặc commit nhắc tên, xem M4) hoặc nhả tay (`docs/MULTI-MACHINE.md:28`). Unit `failed`/`skipped` trên máy A vẫn bị giữ cả 14 ngày; máy khác bị `exit 1` ở bước claim.

### L2 (thấp, xác nhận): `has_t` coi một tệp test sở hữu là đủ
- `codex-pipeline.sh:393-395`: `has_t=1` khi BẤT KỲ tệp `tests/` thuộc owned tồn tại; bước "red-tests" của Codex bị bỏ qua hoàn toàn. Nếu contract sở hữu 3 tệp test nhưng chỉ 1 tệp có sẵn, hai tệp còn lại không bao giờ được viết, unit vẫn được triển khai và merge với test thiếu.

### L3 (thấp, xác nhận): `redcheck` chấp nhận 0 test thất bại
- `lib/codex-lib.py:523-543`: nếu không có `---- … stdout ----` và không có dòng `FAILED` thì `bad` rỗng → exit 0. Các test "đỏ" đã xanh sẵn trên stub (vô nghĩa) được coi là hợp lệ. `codex-pipeline.sh:350-351` không có kiểm tra bổ sung "ít nhất một test thất bại".

### L4 (thấp, xác nhận): `state_load` nuốt mọi lỗi và trả trạng thái rỗng, rồi bị ghi đè
- `lib/codex-lib.py:190-194`: mọi ngoại lệ (JSON hỏng do sửa tay, lỗi đọc tạm thời) → `{"units": {}, "meta": {}}`; `cmd_sset`/`cmd_supd`/`cmd_sreset` (224-243, 518-521) đọc rồi `state_save` đè nên mất toàn bộ trạng thái các unit. `sget`/`sfield`/`slist`/`avgpct`/`table` truy cập `["units"]` không kiểm tra nên một tệp hợp lệ nhưng thiếu khoá gây `KeyError`.

### L5 (thấp, xác nhận): `state_lock` có TOCTOU khi gỡ khoá "cũ"
- `lib/codex-lib.py:206-211`: gỡ khoá khi `mtime` > 30 s mà không giữ khoá; hai tiến trình cùng thấy khoá cũ, tiến trình thứ nhất xoá và tạo khoá mới, tiến trình thứ hai `os.remove` khoá MỚI của tiến trình thứ nhất → hai bên cùng ghi.

### L6 (thấp, xác nhận): `testargs` thêm `--lib` toàn crate, rỗng thì chạy toàn bộ
- `lib/codex-lib.py:339-349`: `--lib` được thêm nếu BẤT KỲ tệp src sở hữu có `#[cfg(test)]` → chạy mọi test lib của crate, kể cả test của các unit khác còn stub `todo!()` (đỏ). `codex-gate.sh:115` (`if [ -z "$TA" ]; then TA=""; fi` là lệnh rỗng): khi không có tệp test nào thì chế độ `fast` chạy `cargo test -p crate` cho cả crate, trái với mô tả "ONLY the unit's own test binaries".

### L7 (thấp, xác nhận): kiểm tra owned-files miễn nhiễm với đường dẫn bắt đầu bằng `target`/`.codex-`
- `codex-gate.sh:86`: `grep -v -E '^(\.codex-|target)'` loại mọi tệp ở gốc repo có tên bắt đầu `target…` hoặc `.codex-…` khỏi danh sách thay đổi, nên sửa/ghi chúng không bị coi là "ngoài owned files". `codex-gate.sh` cũng chỉ nhìn `git status` nên một commit do Codex tự tạo trong worktree (dù prompt dặn "Do not commit") làm cây sạch và qua kiểm tra owned-files.

### L8 (thấp, xác nhận): `LIB="python $SELF/lib/codex-lib.py"` và nhiều biến không được trích dẫn
- `codex-gate.sh:76`, `codex-pipeline.sh:15`, `codex-pack.sh:10`, `unit-pr.sh:6`: đường dẫn có khoảng trắng (ví dụ thư mục người dùng Windows có dấu cách) làm `$LIB …` bị tách từ và lệnh sai. Cũng `for t in $($LIB cs …)` / `for f in $(…)`.

### L9 (thấp, xác nhận): `wait_quota` có thể vòng vô hạn khi script dùng lượng bị hỏng
- `codex-pipeline.sh:140-167`, `codex-usage.sh`: nếu `codex-usage.sh` lỗi (không phải hết hạn mức: binary hỏng, chưa đăng nhập, mạng) nó in `{"ok":false,"reason":"no response"}` và exit 1; `wait_quota` coi mọi `URC != 0` là "hết hạn mức", ngủ tối đa 1800 s rồi lặp mãi, chỉ có một dòng log mỗi lần. Tiền kiểm chỉ xác nhận tệp binary tồn tại.
- `codex-usage.sh:11` còn nội suy `$TH` (từ `QUOTA_MAX`) trực tiếp vào mã Python; giá trị môi trường có ký tự lạ chạy mã tuỳ ý (chỉ trong phạm vi môi trường của người dùng).

### L10 (thấp, chỉ nghi ngờ): `gh pr merge --squash` ngay khi thông báo "clean status"
- `unit-pr.sh:206-212`: khi `--auto` bị từ chối với thông báo chứa "clean status", script merge NGAY. Nếu lúc đó các kiểm tra bắt buộc chưa kịp xuất hiện (vừa tạo PR) trạng thái có thể bị coi là sạch và PR được merge mà chưa chờ CI. Chưa chắc: cần xem cấu hình bảo vệ nhánh.

### L11 (thấp, xác nhận): script phụ thuộc đường dẫn/nhánh cũ
- `status.sh:4-5` cố định `R=C:/MyProjects/PhotoReview-rs` và nhánh mặc định `integration/wave-8` (pipeline dùng `integration/wave8-b2`…); `codex-pipeline.sh:34` và `codex-pack.sh:15` cố định `CS_REPO=C:/MyProjects/PhotoReview`; trên máy khác chạy sai mà không báo.

## Test (`test/pipeline-selftest.sh`) yếu hoặc thiếu
- Không nằm trong CI: `.github/workflows/ci.yml` chỉ nhắc `tools/` ở bộ lọc "không phải Rust"; không có bước chạy selftest. Tệp dùng `cygpath -w` (dòng 21), chỉ chạy trên Windows/Git-Bash.
- Không có test nào cho `claim.sh` và `unit-pr.sh` (header của selftest không nhắc). Selftest chỉ sao chép `CODEX-RULES.md`, `codex-*.sh`, `lib` (dòng 81) nên `claim.sh` không có trong repo giả; lời gọi `bash claim.sh claim` của pipeline trả 127 và vì `crc != 1` nên bị bỏ qua — nhánh claim của pipeline không được thử.
- `testguard`: 7 ca (dòng 398-404) không có ca nào cho `return;`, `if false {`, `#[cfg(…)]`, ký tự `'}'`, hay base không đọc được (M1). `lint`: các ca ở dòng 381-393 dùng tệp mới có `#[cfg(test)]` ở cuối; không có ca nào cho `#[cfg(test)]` đặt sớm (M2).
- `redcheck` không có ca "0 test thất bại" (L3).
- Phần lớn `check` kiểm chuỗi trong log (`grep -q …`) do chính script ghi, nên chủ yếu xác nhận luồng điều khiển, không xác nhận kết quả đúng.

## Không kiểm tra được
- Mọi script chưa chạy; kịch bản H1, M1, M2 lần theo mã (git và Python), cần chạy trên máy có origin để chứng minh. Hành vi thật của `git fetch`/`git push` với `refs/claims/*` phụ thuộc phiên bản git và quyền origin (trong phiên cloud này `push` claim bị HTTP 403).
- Hành vi của `codex.exe` (sandbox `workspace-write`, `exec resume`, mã thoát khi timeout, tiến trình con sót lại) và `cargo-mutants`.
- Cấu hình bảo vệ nhánh/GitHub auto-merge (L10), `gh` CLI và cách `gh pr merge` báo "clean status".
- `workflows/*.js` và các tài liệu RUNBOOK/MULTI-MACHINE không thuộc phạm vi đã đọc; chỉ trích dẫn dòng 28 của MULTI-MACHINE về `release`.

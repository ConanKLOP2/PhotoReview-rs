# ST-CI-RELEASE-TOOLS (2026-10-31)

Nguon viec: [ST-GAPMAP2](ST-GAPMAP2-2026-10-31.md) muc 6, viec 10. Khong build, khong cai cong cu; chi doc va sua `.github/workflows/ci.yml`.

## 1. cargo deny (X-release-dependencies)

Job `deny` da co san trong `.github/workflows/ci.yml` (windows-latest): cai `cargo-deny` 0.19.9 bang `cargo install --locked`, chay `cargo deny --locked check --config deny.toml advisories licenses bans sources --deny unmaintained --deny unsound`. Chay tren master, dispatch, nightly, va tren PR chi khi doi `Cargo.toml`/`Cargo.lock`/`deny.toml`/`ci.yml`. `build-test` (check bat buoc) da `needs: deny`. Khong nhan doi, khong sua. Chua co ket qua audit that: can quan sat lan chay dau (cai dat tren runner, co the lo advisory/giay phep moi; `deny.toml` duoc viet tu metadata offline, chua tung chay).

## 2. actionlint (X-release-workflow)

Them job `actionlint` (ubuntu-latest, 5 phut) vao `ci.yml`: `go install github.com/rhysd/actionlint/cmd/actionlint@v1.7.7` roi chay tren moi workflow. Ghim phien ban qua Go module proxy/sumdb (tag bat bien); khong tai binary khong ghim, khong dung action ben thu ba chua kiem SHA. `continue-on-error: true` va KHONG nam trong `needs` cua `build-test` nen khong la required check (ruleset master-ci chi doi `build-test`). Nang len bat buoc chi sau mot lan chay xanh tren master. `tools/check-release-workflow.ps1` van tu chay actionlint khi co tren may.

## 3. Test attended placeholder (X-http-composition)

Test la `native_settings_close_cancels_and_open_page_uses_shell` trong `crates/pr-shell/tests/update_http_composition.rs` (L224), da co `#[ignore = "native: attended Settings close/worker/ShellExecute verification required"]`, dung quy uoc repo. Than test `panic!` co chu y vi khong co bang chung. No chi do khi chay `--include-ignored`; `ci.yml` khong bao gio truyen co do, va `native-nightly.yml` khong liet ke binary nay (chi pr-shell `ui_smoke*`/`startup_*`). Ket luan: trong CI no KHONG do; "1 attended placeholder failure" trong PARITY den tu lan chay `--include-ignored` cuc bo. Khong sua test (lam pass gia bi cam). Cung mau voi `http_transport_loopback.rs:252`.

## 4. Thay doi va quan sat khi CI chay

- Doi: chi `.github/workflows/ci.yml` (+16 dong, job `actionlint`). YAML parse duoc (PyYAML). Chua chay actionlint, chua chay CI.
- Quan sat: (a) job `actionlint` co the bao loi tren `ci.yml`/`release.yml`/`native-nightly.yml` (shellcheck cho `run:` bash, kieu `${{ }}` trong `run`); (b) `go install` can Go tren ubuntu-latest (co san); (c) vi `ci.yml` doi, job `deny` se chay tren PR nay: xem co cai duoc cargo-deny 0.19.9 va ket qua audit.

## 5. De xuat PARITY (khong sua)

- X-release-workflow: giu Partial; them con tro job `actionlint` (advisory), doi sang bang chung khi co lan chay xanh.
- X-release-dependencies: giu Partial cho den khi job `deny` tren PR nay cho ket qua that.
- X-http-composition: lam ro "1 attended placeholder failure" la `--include-ignored` cuc bo, khong phai do trong CI.

# FIX-TEST-HYGIENE 2026-10-31

> Status: lane `fix-test-hygiene`, fixes for the cheap and safe items of [ST-TEST-HYGIENE-2026-10-31.md](ST-TEST-HYGIENE-2026-10-31.md). Every item was re-checked against the code first.

## Fixed

| Item | Change | Check run |
|---|---|---|
| F5 | `session_writer.rs` `bounded_flush_one_budget_covers_lock_wait_and_save`: no fake-clock seam exists for the bounded wait (it blocks on a real condvar), so the margins were widened: budget 2000 ms, lock released after 1400 ms, limit 2700 ms (correct path ~2000 ms, the double-budget bug ~3400 ms; slack 700 ms each side, was 300 ms). `diagnostics_file_log.rs` two bounds (1500 / 1000 ms) became 1900 ms (the stall they guard is the 2000 ms `FLUSH_BUDGET_MS`, so they cannot be widened further). `slow_link_fs.rs` `golden_limiter_consume_immediate`: 500 ms became 2 s. `w6_rerun_killers.rs`: the extra `< 4 s` was implied by `recv_timeout(4 s)`; it now compares with `< 9 s` against the 10 s deadline. | session_writer 47 ok, diagnostics_file_log 48 ok, slow_link_fs 37 ok, w6_rerun_killers 7 ok |
| F7 | `w6_p01_killers.rs`: the two tests that assert facts about the real `%TEMP%` volume (`query_mount_point_returns_the_volume_root_of_a_real_directory`, `default_source_resolves_guid_drive_type_and_quota_from_the_real_volume`) print `SKIP ...` and return when `%TEMP%` is not a fixed volume. | w6_p01_killers 21 ok (on a fixed volume the assertions still run) |
| F8 | `session_store.rs` `from_environment_constructs_and_names_a_session_file` asserts that `PHOTOREVIEW_RS_DATA_ROOT` and `PHOTOREVIEW_RS_ISOLATE_CONFIG` are set before it touches the environment. `tools/dev/native-tests.sh` now exports both (a temp data root unless one is already set). Nightly already set them. | test run with `--include-ignored` and both variables: ok |
| F9 | `file_action_fakes/mod.rs`: journal root is `%TEMP%\pr-file-action-fakes-<pid>` instead of a fixed folder (nothing is written there). | `cargo check -p pr-app --tests`, clippy ok |
| F10 | `settings_shortcuts.rs`: the stale `#[ignore]` ("waits for W1-02", `serde_ext` is no longer a stub) was removed; the test passes. | settings_shortcuts 17 passed, 0 ignored |
| F3 | See below. | |
| F4 | `verify_w8_01a.rs`: the hard-coded `C:/Users/adminvti/...` scratchpad path became `PHOTOREVIEW_RS_ORACLE_DIR` (panics with a clear message if unset or a file is missing). The 10 tests stay `#[ignore]` with a reason naming the variable. `tools/dev/oracle.sh` has no command that produces these dumps (`fmt/snap/len/pan.txt`), so they cannot be moved to a golden in this cheap pass. | cargo check, 10 ignored |

## F3 result (this machine, no DLL staged, fresh target dir)

`cargo test -p pr-imaging --locked --test <bin> -- --include-ignored --test-threads=1`:

| binary | result |
|---|---|
| cache_disk_store | 58 passed, 0 failed |
| cache_key | 14 passed, 0 failed |
| preview_service_disk | 55 passed, 0 failed |
| thumbnail_cache | 46 passed, 0 failed |

None needed `libraw.dll`/`turbojpeg.dll` or a desktop, so all four were added to the `pr-imaging` entry of the nightly plan and the header comment of `.github/workflows/native-nightly.yml` was corrected (cache_preview_file and memory_headroom stay out: not run here). Not confirmed on `windows-latest` until one night has run (ACL tests use `icacls /deny`).

## Not done / noted

- F1 (PARITY), F2 (big nightly plan): out of scope by instruction.
- F18: `tools/dev/unsafe-audit.baseline` already lists the `unsafe` of pr-core (`io/physical.rs` 4, `io/journal_compaction_files.rs` 3, `text/dotnet_path.rs` 2) and pr-imaging (`cache/atomic_file.rs` 2, `pixel_memory.rs` 2, `pixels.rs` 2, `tests/pixel_memory.rs` 12) as rule `B` (outside the allowed crates), so they are grandfathered. Whether pr-core/pr-imaging should be added to the allowed list is a policy decision; nothing changed.
- F6, F11, F12, F19, F20 and the `engine.rs` part of F5: not touched.

## Checks run

`cargo fmt -p` and `cargo clippy -p <crate> --all-targets --locked -- -D warnings` for pr-core, pr-app, pr-bench, pr-platform-win: clean. No workspace test, no mutation run.

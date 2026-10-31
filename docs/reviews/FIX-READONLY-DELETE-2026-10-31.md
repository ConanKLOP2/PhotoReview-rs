# FIX-READONLY-DELETE (2026-10-31)

Source: [ST-INVMAP M2](ST-INVMAP-2026-10-31.md). User decisions in chat 2026-10-31: Q2 permanent delete must refuse a read-only file like C# `File.Delete`; Q1 `JournalDurability` stays `Fast` as in C#.

## Q2: the bug was real
- `WindowsRecycleBin::new` wired the permanent-delete seam to `std::fs::remove_file`, which on Windows deletes read-only files. C# `WindowsRecycleBin` uses `File.Delete`, which throws `UnauthorizedAccessException`; the repo already documents this (`pr-core/src/io/physical.rs` header, `journal_compaction_files.rs`).
- Test first (red): `crates/pr-platform-win/tests/recycle_bin_readonly_delete.rs::permanent_delete_refuses_a_read_only_file_and_leaves_it_intact` failed with the old code (file was deleted). Real disk but only inside a `TempRoot`; the volume type is faked as Removable; no Recycle Bin is touched; the read-only flag is cleared afterwards.
- Fix: new `recycle_bin::native_file_delete` calls the existing `PhysicalFileSystem::delete` (`DeleteFileW`, `ERROR_FILE_NOT_FOUND` = success, access denied -> `PermissionDenied`). No new `unsafe`. The now unused helper `delete_missing_file_is_success` and its two unit tests were removed (the same behaviour is covered by `PhysicalFileSystem::delete`). Green: the 2 tests of the new binary pass (refuses read-only and the file survives with its content; deletes a normal file and a missing file is success).
- Retry/recovery: `recovery_retry.rs` and `service.rs` call `delete_permanently(..)?` before `tx.commit()`, so the refusal propagates as an error; nothing is marked Committed and the Prepared entry stays for reconcile, the same as any other delete failure. The user-facing text is the OS error. No change needed.
- Other `remove_file`/`remove_dir_all` in production code: `perf_csv.rs`, `imaging/cache/atomic_file.rs`, `win32/activation_context.rs` delete the application's own temp or cache files, not user photos; `journal_compaction_files.rs` already uses `DeleteFileW`. No other user-file deletion path found.
- Not run: no workspace test, no mutation, no native stage; only the new test binary, `cargo fmt --check` and one clippy for `pr-platform-win`.

## Q1: no code change
Recorded as P-57 in `docs/PROPOSED-DECISIONS.md` (confirmed by the user) and noted on PARITY row S-21.

## PARITY
F-04 (permanent delete) got a DIVERGENCE-fixed note; S-21 got the Q1 note. No status raised.

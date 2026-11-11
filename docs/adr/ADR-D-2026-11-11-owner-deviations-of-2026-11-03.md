# ADR: intentional differences from the owner decisions of 2026-11-03 (OD-1..OD-6; P-331 is a record only)

Date: 2026-11-11

Status: Accepted. Each item is an owner decision of 2026-11-03 (option letters as in `docs/PROPOSED-DECISIONS.md`, rows `P-92`, `P-150`, `P-301`, `P-74`, `P-321`, `P-191`) and is merged on master. The owner may reverse any item with a follow-up PR.

Why one file: the six decisions came from the same review wave (BACKLOG G3/G29/G14/G9 and the C# sync of 2026-11-03), are small, and have the same shape: the owner chose a behaviour that differs from the C# baseline `e7c2fcd8`/`5e5a9ce5` and the code is already merged. AGENTS.md rule: an intentional behaviour difference needs an ADR in `docs/adr/` and a `Different` row in `docs/PARITY.md`. The rows are `D-percent-text-nls`, `D-preview-cache-read-oom`, `D-framelog-writer-thread`, `D-prefetch-lookup-pool`, `D-update-result-retry` and `D-duplicate-cleanup-keeper-precheck`. The wording was first written in `docs/reviews/PARITY-WORDING-OWNER-DECISIONS-2026-11-03.md`; every claim below was re-checked against master on 2026-11-11 (file and line named per item).

## OD-1 Percent text reads the Windows NLS pattern (P-92 option A, PR #627)

- PARITY row: `D-percent-text-nls`. Evidence: [D4-IMPL](../reviews/D4-IMPL-2026-11-03.md). Test: `crates/pr-platform-win/tests/shell_startup_locale_percent_golden.rs::percent_one_decimal_matches_csharp_for_every_golden_ratio`.
- Context: `{ratio:P1}` in C# comes from .NET `NumberFormatInfo` (ICU data). The Rust `WindowsNumberFormatting::percent_one_decimal` used a hard-coded language table for the position of the percent symbol.
- Decision: `percent_one_decimal_of` (`crates/pr-platform-win/src/shell_startup/locale.rs`) asks Windows for the number format, the symbol (`LOCALE_SPERCENT`) and the positive pattern (`LOCALE_IPOSITIVEPERCENT`: 0 `n %`, 1 `n%`, 2 `%n`, 3 `% n`); patterns 0 and 3 use a plain U+0020 as .NET does. The language table is deleted. The sign of `-0.0` is kept as in .NET (the magnitude is formatted and the minus put back).
- Consequence: Windows NLS data and .NET ICU data differ for some cultures. fa and fa-IR (separators and pattern), ar (separators, trailing U+061C) and ru (`n%` against `n %`) differ for all ratios; sv, fi, nb, he and tr differ for negative ratios (minus sign or bidi mark; for tr the negative pattern, because `LOCALE_INEGATIVEPERCENT` is not read). fr, de, es, pl, pt, it, de-CH, fr-CH and the other matching cultures have no difference for the 12 golden ratios. The per-culture list is in the evidence file.

## OD-2 An out-of-memory error while reading a preview cache entry keeps the entry (P-150 option A, PR #631)

- PARITY row: `D-preview-cache-read-oom`. Evidence: [D8-IMPL](../reviews/D8-IMPL-2026-11-03.md). Test: `pr_imaging::cache::preview_file::tests::read_failing_with_out_of_memory_is_out_of_memory_not_a_cache_entry_failure` (seam-level only).
- Context: C# `FileStream.Read` raises `IOException` for any read failure; the cache treats it as a bad entry, deletes the file and decodes the source again.
- Decision: in `crates/pr-imaging/src/cache/preview_file.rs` an `io::ErrorKind::OutOfMemory` while reading the entry is `PreviewCacheError::OutOfMemory` (the same variant as the write path and the decoder use): the entry is kept and the error propagates. `PreviewCacheError::is_cache_entry_failure` is false for it.
- Consequence: under memory pressure the Rust port does not destroy a good cache entry; C# would delete it. Only the mapping at the seam is unit-tested, not a real out-of-memory read.

## OD-3 FrameLog lines are written by a writer thread (P-301 option B, PR #632)

- PARITY row: `D-framelog-writer-thread`. Evidence: [D9-IMPL](../reviews/D9-IMPL-2026-11-03.md). Tests: `crates/pr-platform-win/tests/shell_viewing_frame_log.rs::{record_never_blocks_and_counts_dropped_lines_when_the_writer_is_stuck, close_writes_every_queued_record, a_panic_in_the_writer_does_not_reach_the_caller, a_write_error_switches_the_log_off_without_a_panic}`.
- Context: C# `FrameLog` writes on the UI path with `AutoFlush`, so switching the log on adds file-write time to the very frame it measures.
- Decision: `crates/pr-platform-win/src/shell_viewing/frame_log.rs` hands records to a `frame-log-writer` thread through a bounded channel (`QUEUE_CAPACITY` 1024). `record` never blocks and never grows memory; when the queue is full the new record is dropped and counted (`FrameLog::dropped`, reported once on stderr at close); `close` and `Drop` drain the queue and join the thread; a panic or a write error in the writer switches the log off for the session without reaching the caller.
- Consequence: a reader of the file can lag the UI by up to the queue (`flush` waits for earlier records). The line format, CRLF and `FileShare.Read` are unchanged.

## OD-4 Prefetch lookups run on a small pool of named lanes (P-74 option C, PR #625)

- PARITY row: `D-prefetch-lookup-pool`. Evidence: [D2-IMPL](../reviews/D2-IMPL-2026-11-03.md). Tests: `lookup_pool::tests::*` in `crates/pr-platform-win/src/host/main_window/lookup_pool.rs` (including `panicking_job_does_not_kill_a_lane_and_is_logged_and_counted`, `panicking_lookup_does_not_break_the_next_navigation`).
- Context: `start_lookup` started a new OS thread named `prefetch-lookup` for every navigation and a panic inside it was not caught. R0003 forbids a thread per request; a single shared lane (candidate commit `d646739`, not used) would serialise lookups during rapid switching and slow the review.
- Decision: a fixed pool of `min(max(available_parallelism / 2, 1), 4)` named lanes (`lane_count_for`); every job runs under `catch_unwind` and a panic is logged as an error and counted (`LookupPool::panic_count`). Same observable result and priority as C#, which uses the thread pool.
- Consequence: at most 4 lookup threads, no thread start per navigation. `start_lookup` itself on a live window is only reached through the native desktop tests (the seam `lookup_work` is tested).

## OD-5 The update-check answer is retried with a delay and a cap (P-321, PR #628)

- PARITY row: `D-update-result-retry`. Evidence: [D5-IMPL](../reviews/D5-IMPL-2026-11-03.md). Tests: `crates/pr-platform-win/tests/w8_settingswin4_killers.rs::{update_result_that_cannot_apply_retries_with_a_delay_and_gives_up_at_the_cap, update_result_applies_on_a_later_try_once_the_presenter_is_free}`.
- Context: the update answer is posted to the Settings UI thread. If the presenter is borrowed when it runs (a nested modal call of another page), `apply_update_result` re-posted itself at once and without a bound. C# has no such loop.
- Decision: `crates/pr-platform-win/src/dialogs/settings/pages/general.rs` waits `UPDATE_RESULT_RETRY_DELAY` (20 ms) between tries and stops after `UPDATE_RESULT_MAX_TRIES` (50, the first included); the give-up writes one warning to the application log and drops the answer. Rust-only hardening of the cross-thread post.
- Consequence: after a give-up the status keeps its "checking" text and the check button stays disabled until the window is reopened; this was not widened.

## OD-6 Duplicate cleanup skips a group whose only keepers were modified (P-191, PR #626)

- PARITY row: `D-duplicate-cleanup-keeper-precheck` (see also the X-duplicate-cleanup row, D2 text). Evidence: [D3-IMPL](../reviews/D3-IMPL-2026-11-03.md). Tests: `crates/pr-app/tests/duplicate_cleanup.rs::{keeper_replaced_while_review_open_skips_the_group_up_front_and_cancels_the_batch, keeper_write_time_moved_while_review_open_skips_the_group_up_front, modified_keeper_skips_only_its_own_group_the_other_group_is_recycled, one_unmodified_keeper_keeps_the_group_even_when_the_other_keeper_was_replaced}`.
- Context: C# #535 (D2) checks, right before each recycle, the file and at least one copy that stays against the size and write time captured at hash time, and reports `core.duplicate.fileChanged` per file. The Rust-only pre-check of DV-7 (skip a whole group when any file of it changed) was replaced by that port because it contradicts the ported C# tests (the unchanged sibling of an edited file must still be recycled).
- Decision (owner, 2026-11-03: restore it as a Rust-only extra on top of D2, narrowed): `RecycleGuard::keepers_modified` in `crates/pr-app/src/coordinators/duplicate_cleanup.rs` is true when no keeper is unchanged and at least one keeper still exists with a size or write time that differs from hash time. After the review dialog and before the first recycle every file of such a group is dropped from the batch and listed as `<name>: core.undo.sourceOrDestinationChanged`; if nothing is left the status is `BatchCanceled` and one error dialog lists the lines, otherwise the lines are added to the failures of the batch. The D2 per-file check stays in front of every recycle.
- Consequence: data safety is identical (nothing of that group is recycled); only status and text differ from C# (`BatchCanceled` and `sourceOrDestinationChanged` against `BatchDone(0, n)` and `fileChanged`). Deleted keepers, changed or deleted files to remove and a surviving keeper are not covered by the pre-check and stay with the D2 per-file check. The practical gain over D2 is small.

## Record only: P-331 (PR #630, no Different row)

The owner decided on 2026-11-03 that the startup reconcile of a single Recycle whose source is gone asks the Recycle Bin before writing Committed (in the bin: Committed; not in the bin: Failed `PendingUnconfirmed`; the bin cannot be asked: stays Prepared) and that C# is fixed too. C# #540 (`f57061ef`, baseline `5e5a9ce5`) merged on 2026-11-06 with the same rule and the Rust port matches, so the row `F-14` says Equal and no Different row or ADR is owed. Remaining differences, none of them behaviour: the Rust query is bounded by `SHELL_CONTAINS_TIMEOUT` (20 s per entry; C# joins the shell thread without a bound), the log text after `skipped {id}:` differs, and a negative size is answered without asking the bin. Proof: `crates/pr-core/tests/journal_reconcile_recycle_bin.rs`. Evidence: [D7-IMPL](../reviews/D7-IMPL-2026-11-03.md).

## Not decided here

P-300, P-312, P-313, P-340..P-343, P-170, P-320 and P-322 are Rust-only hardening with no C# counterpart and no behaviour change on the normal path; they need neither a Different row nor an ADR (see the wording file). P-310, P-330, P-160 C and P-390 have no Different row either.

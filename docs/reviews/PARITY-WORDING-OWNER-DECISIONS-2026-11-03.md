# PARITY and ADR wording for the owner decisions of 2026-11-03

Written for the lead. `docs/PARITY.md` rows with status Partial, Ported or Todo are being edited by the parity close-out workflow, so this file only holds the proposed text; nothing here was applied to `PARITY.md`. Each block is copied from the `parityAdrWording` / `deviationVsCsharp` fields of the implementing agents' reports (the evidence file named in each block has the details). Rows are proposals: pick the target row id when merging into `PARITY.md`.

Status at the time of writing: PRs #624..#626 and #629..#635 are merged; PRs #627 (P-92) and #628 (P-321) are open at the time of writing, so their evidence files (`D4-IMPL-2026-11-03.md`, `D5-IMPL-2026-11-03.md` in this folder once merged) are not linked here. Decision rows: [`PROPOSED-DECISIONS.md`](../PROPOSED-DECISIONS.md).

## Different rows (a deviation from the C# baseline)

### P-92 A: percent text (PR #627, open) — Different

Row: "Percent text (`{ratio:P1}`): Rust reads the Windows NLS percent pattern (`LOCALE_IPOSITIVEPERCENT`) and symbol with a plain U+0020; .NET reads ICU. fa, fa-IR, ar, ru differ for all ratios; sv, fi, nb, he, tr differ for negative ratios (minus sign, bidi marks, negative pattern). Test: `percent_one_decimal_matches_csharp_for_every_golden_ratio`."

ADR: "P-92 A: percent placement comes from the OS pattern with a plain U+0020 (as .NET), the language table is deleted; NLS-vs-ICU data differences for some cultures are accepted and listed in the evidence."

Detail from the report: fr, de, es, pl, pt, it, de-CH, fr-CH and the other matching cultures have no deviation for all 12 ratios including -0.000049 (the `-0.0` sign was fixed too). fa and fa-IR: NLS uses decimal "/", group ",", symbol "%", pattern 0, ICU uses Arabic separators and pattern 1. ar: NLS ".", ",", "%", ICU Arabic-Indic separators and a trailing U+061C. ru: NLS pattern 1 `n%`, ICU `n %`. sv, fi, nb, he, tr: negative ratios only (ICU U+2212 minus or U+200E mark; tr C# negative pattern `-%25,0` vs Rust `%-25,0` because `LOCALE_INEGATIVEPERCENT` is not read, outside the decision). Evidence: `D4-IMPL-2026-11-03.md` (PR #627).

### P-150 A: preview cache read (PR #631, merged) — Different

Row: "D-06 preview cache read: an OS out-of-memory error while reading a `.pv4` entry is `OutOfMemory` (entry kept, error propagates, P-150 A); C# raises `IOException` and deletes the entry."

ADR: "Reading a cache entry treats `io::ErrorKind::OutOfMemory` like the write path and the decoder: not a corrupt entry. The Rust port deliberately differs from C# `FileStream.Read` (`IOException`)."

Evidence: [D8-IMPL](D8-IMPL-2026-11-03.md). The reviewer noted that PARITY was not edited by the author and that only the seam-level mapping is unit-tested.

### P-301: FrameLog writer thread (PR #632, merged) — Different

Row: "FrameLog (P-301): the line is written by a writer thread through a bounded queue (1024) instead of synchronously on the UI path (C# `AutoFlush`); a full queue drops the new record and counts it (reported once at close); close drains and joins; a writer panic or write error switches the log off."

ADR: "Context: C# FrameLog writes on the UI path with AutoFlush, so measuring adds file-write time to the measured frame. Decision (P-301, owner 2026-11-03): hand records to a writer thread over a bounded channel; `record` never blocks or grows memory; when full drop the new record and count it; close drains and joins. Consequence: a reader of the file may lag the UI by the queue; the line format contract is unchanged."

Format, CRLF and `FileShare.Read` are unchanged. Evidence: [D9-IMPL](D9-IMPL-2026-11-03.md).

### P-331: reconcile of a single Recycle asks the bin (PR #630, merged; C# branch local) — Different until the C# branch merges

Row: "Different (P-331, D7): startup reconcile of a single Recycle whose source is gone also asks the Recycle Bin for the item (path, size, write time). In bin: Committed; not in bin: Failed `PendingUnconfirmed`; bin cannot be asked: stays Prepared. C# baseline `e7c2fcd8` (R15) writes Committed on 'source gone'; C# branch `feat/reconcile-recycle-bin-check` (`396d5ac2`, local, not pushed) has the same rule. Different until that branch merges and the baseline is synced. Proof: `pr-core/tests/journal_reconcile_recycle_bin.rs`."

ADR text: see the section "PARITY and ADR wording for the lead" of [D7-IMPL](D7-IMPL-2026-11-03.md). Detail: with no bin wired the old rule applies (production wires the bin); group Recycle is unchanged; the Windows query timeout is 20 s per entry and a timeout is an error, so the entry stays Prepared. After the C# merge and the baseline sync, flip the row to Equal and update `docs/SYNC-LEDGER.md`.

### P-191: duplicate cleanup pre-check restored (PR #626, merged) — Different

Row: "Duplicate cleanup: a group whose only remaining copies-to-keep were modified (size or write time) while the review dialog was open is skipped up front with `core.undo.sourceOrDestinationChanged` and, if nothing else is left, status `BatchCanceled` (C# #535: per-file `core.duplicate.fileChanged` and `BatchDone(0, n)`). Safety identical."

ADR: "Restore of D-07f-5 / DV-7 as a Rust-only extra on top of D2 (P-191), narrowed to modified keepers so that the ported C# #535 tests stay green; the removed whole-group pre-check for files to remove or deleted keepers stays removed."

Deleted keepers, changed or deleted files to remove, and a surviving keeper are not covered by the pre-check; the D2 per-file check is unchanged. Evidence: [D3-IMPL](D3-IMPL-2026-11-03.md).

### P-74 C: prefetch lookup pool (PR #625, merged) — Different

Row: "Prefetch lookup: C# runs cache lookups on the thread pool; Rust runs them on a fixed pool of `min(max(cpus/2,1),4)` named lanes (`lookup_pool.rs`), jobs wrapped in `catch_unwind` (error log + panic counter). Same observable result and priority. Test: `lookup_pool::tests::*`. ADR P-74."

ADR: "Replace thread-per-navigation prefetch lookup by a small pool of named lanes sized `min(max(available_parallelism/2,1),4)`; each job isolated with `catch_unwind`, logged and counted. Why: R0003 forbids thread per request, the old panic was uncaught, a single lane (`d646739`) would serialize lookups during rapid switching and hurt review speed. Consequences: at most 4 threads, no per-navigation thread start, user-visible behaviour unchanged."

Evidence: [D2-IMPL](D2-IMPL-2026-11-03.md).

## Rust-only hardening notes (no behaviour change on the normal path)

### P-321: update-result retry (PR #628, open) — Different

Row: "Settings General page: a busy presenter makes the update answer retry every 20 ms, 50 tries at most (first included), then log and drop (Rust-only hardening of the cross-thread post; C# has no loop). ADR D5 / P-321."

ADR: "The update check answer is posted to the Settings UI thread. If the presenter is borrowed when it runs, the answer is retried after 20 ms, up to 50 tries in total, then dropped with a log line. Before, it was re-posted immediately and without bound. C# has no equivalent. Consequence: after a give-up the status stays 'checking' and the button stays disabled until the window is reopened."

Evidence: `D5-IMPL-2026-11-03.md` (PR #628).

### P-300, P-312, P-313 (PR #633, merged) — note under the render rows; no Different row, no ADR

"Hardening P-300/P-312/P-313 (Rust-only edges, no C# counterpart): UI timer thread spawn failure is logged; `GpuImageBackend::upload`/`upload_rows` run with the cache state borrow released (only release still runs under it); on a released device `pop_clip` keeps the clip depth and `draw_image_oriented` pops only an applied push."

Evidence: [E1-IMPL](E1-IMPL-2026-11-03.md). Note: the P-300 log only reaches the first poster that creates the process-wide queue.

### P-340, P-341, P-342, P-343, P-170 (PR #635, merged) — note under the dialogs/startup rows; no ADR

"Rust-only hardening, no behaviour change on the happy path: `SharedDialogService` forwards `prompt_add_external_editor`, `prompt_default_external_editor` and `prompt_rename` (P-340); `LazySecondaryPort::get` answers `None` and keeps it when the port factory panics, like C# `Port()` answering null (P-341); `StartupCloak` reveals at once and logs an error when the safety-deadline thread cannot be started, a failure mode C# `Timer` does not have (P-342, P-170); `StartupTask` join failures of the early-decode plan are logged at info level, previously silent (P-343, `run.rs` only)."

Evidence: [E3-IMPL](E3-IMPL-2026-11-03.md). The default `schedule_on_thread` passes no log, so only the `StartupCloak::new` path with `options.log` logs the spawn error.

### P-320, P-322 (PR #634, merged) — note for the D-W8B5-805 entry; no change to PARITY

"Nested `EN_CHANGE` raised by a page's own `set_text` during `load_all` is swallowed by the host. Both `view.loading` and `PageRef::try_borrow_mut` each suffice, and the native test `settings_reentrancy_native` pins exactly one `refresh()` and no `BorrowMutError` (P-320)."

P-322 uses `ignore_case_key` (C# `OrdinalIgnoreCase` dictionary); no observable change. Evidence: [E2-IMPL](E2-IMPL-2026-11-03.md).

## No Different row

- P-310 (PR #624, merged): parity restored (C# `MainViewModel.cs:140-143`); no row and no ADR. Native confirmation of the HUD is still open. Evidence: [D1-IMPL](D1-IMPL-2026-11-03.md).
- P-330 (C# branch `a3dc30cc` local; Rust part waits): no new Different row; the existing M3 row notes "status stays `ActionFailed` in Rust until C# `a3dc30cc` is merged and synced". ADR text for later: "P-330: an executor throw whose source is gone (group: any image member gone) reports `status.actionOutcomeUnknown`, not `status.actionFailed`, in both implementations. The entry restore rule is unchanged (C# unconditional, Rust only when on disk, M3)." Evidence: [D6-IMPL](D6-IMPL-2026-11-03.md).
- P-160 C: no change; the row stays Partial.
- P-390: no row; the three `journal_line.rs` `expecting` mutants are recorded as observably equivalent glue ([GLUE-MUTANTS](GLUE-MUTANTS-2026-11-03.md), [F1-IMPL](F1-IMPL-2026-11-03.md)).

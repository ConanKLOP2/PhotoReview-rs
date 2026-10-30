# Audit of PARITY rows moved to Done on 2026-10-29/30 (lane cl-audit1)

Status: report. Read-only audit (no production change). Method: three read-only reviewers cross-checked every figure in the
row against the cited evidence files, counted `#[test]` per binary, grepped each named killer, and read the C# at 7eaee2a2
for missed behaviours; the lead re-ran representative tests (section "Tests re-run"). Legend: PASS = adequate; DOUBT =
evidence gap, wording or unlisted limit, state not provably wrong; WRONG = provably contradicted. [V] = verified by grep or
reading code; [S] = inference, nothing run.

Result: 26 rows, 0 WRONG, 11 PASS, 15 DOUBT. No row was downgraded: no status is contradicted by a reproducible fact. The
two strongest doubts (INV-6 journal, F-16) rest on a mutation figure whose evidence report was never updated.

## Findings per row

| Row (PARITY line) | Verdict | Reason (file:line) | Action |
|---|---|---|---|
| X-physical-fs (L31) | DOUBT | Totals add up (411+3+13=427) [V]. The 3 timeouts counted as detected are not listed anywhere (raw `mutants.out` is gone); 8 mutants only native tests kill, and CI (`.github/workflows/ci.yml`) never runs `--include-ignored` [V]. | List the 3 timeouts; add a native CI job or put the limit in the Gaps column. |
| X-counting-fs (L32) | PASS | 30+7=37 [V]; killer `w2_fs_mutation_killers.rs:306`, `io_counting` 7 tests pass (re-run). | none |
| RV-C05 (L41) | PASS | Three tests exist (`io_physical.rs:1154`, `:1182`, ops golden). Manual mutation is not reproducible by cargo-mutants but the 427-mutant run covers it. | none |
| AR16 (L43) | PASS (light doubt) | `fast_listing_does_not_probe_locked_file_*` at `io_physical.rs:1428` [V]; depends on native tests like X-physical-fs. | as X-physical-fs |
| AR16 RemovePaths (L61) | DOUBT | Done (W2-10) with no figures and no evidence file; only `remove_paths_*` at `catalog_review_catalog.rs:467/483/504` found. Not part of W2-FS evidence. | Confirm it really changed in the 29-30/10 window or add a one-line evidence pointer. |
| SEC-01 / F-05 (L45) | DOUBT (strongest FS doubt) | The row says the `MAX_FOLLOWED_LINKS` cap survivors (P-03) are "caught with native tests", but `tests/io_physical.rs:1369-1371` states the 64-junction test "pins the OS answer, not the MAX_FOLLOWED_LINKS check" (the OS stops at 63 with 1921 before the manual walk). The cap is `physical.rs:455`, loop `physical.rs:995-996` (`hops += 1; if hops > MAX`). A mutant `hops += 1 -> *= 1` looks unkillable by any junction test [S]. P-03 is still PROPOSED. `> -> ==` is truly equivalent [V]. | Run cargo-mutants on `final_link_target_slow` with `--include-ignored`; if it survives, record it as the accepted P-03 survivor and fix the row wording. |
| D-05 (L40) | DOUBT (light) | 9 killers in `w2_fs_flush_killers.rs` exist, 17 = 15 caught + 2 unviable matches CL-FSFLUSH [V]. The 7 hand-made mutants are not reproducible; mutation ran while the tests lived in pr-core and was not repeated after the move to pr-platform-win (CL-FSFLUSH says so); all 9 tests are `#[ignore="native"]`, no CI. C# `Flush(flushToDisk)` and `WriteThrough` have Rust counterparts [V]. | Re-run mutation on the moved tests; native CI job. |
| INV-8 file-system (L44) | DOUBT (light) | `open_read_shared_sets_the_sequential_scan_mode` exists (`w2_fs_flush_killers.rs:137`). The row names one unspied site (`PhysicalSourceReader`/`open_shared_sequential`); CL-FSFLUSH also lists `journal_compaction_files.rs` and `atomic_file.rs` as unchecked, which the row omits. | Add those two sites to the Gaps column. |
| F-23 (L74) | DOUBT | 50+11+29+1=91 [V]. 11 timeouts (12%) are not listed, and the two pass tables of the evidence give 6+7=13 timeouts against a union of 11 (unexplained) [V arithmetic]. Timeouts are "detected by hang", not by assertion. The one equivalent (`ThreadTimerRun::wait`, `writer.rs:123`) is convincing [V]. | List the 11 timeouts with line and mutant; explain 13 -> 11. |
| F-22 (L75) | PASS | "47 passed + 1 slow ignored" = 48 tests [V]; re-run: 47 passed, 1 ignored. | none |
| F-22 store (L76) | DOUBT (scope) | W2-FS evidence says `session/store.rs` was NOT mutated there; the killer `depth_guard_ignores_brackets_after_an_escaped_quote_inside_a_string` exists (`session_store.rs:1081`) but the 30/10 evidence does not cover this row. | Check that W2-12 evidence stands; no change otherwise. |
| X-perf-diagnostics (L48) | DOUBT (light) | Totals add up for the three files [V]. "Buffer constant" equivalent is `FILE_BUFFER` 64 KiB (`perf_csv.rs:39`), distinct from the killed `WRITER_BUFFER` 8 KiB (`perf_csv.rs:37`), so no contradiction, but the wording invites one; the 8 KiB killer accepts 4096 as well (`w2_fs_mutation_killers.rs:378`). The evidence lists 11 of the 12 perf_csv equivalents. The long-path-prefix equivalence was probed with a drive path of 309 characters only, the `\\?\UNC\` branch was not [S]. | Reword, list all 12, probe the UNC case or record the limit. |
| S-64 (L33) | DOUBT (wording) | 134+3+13=150 [V]; `settings_store` has 119 tests [V] (re-run 119 pass); equivalents 467:42, 739:46, 834:36 are convincing. The cell still ends "no full-row mutation claim", contradicting "whole store.rs mutated"; `normalizer.rs` ran only 49 of 67 (`validate_shortcuts` 14 not run) and `parse_text` depends on it. | Delete the stale sentence; state the normalizer limit. |
| S-65 (L34) | PASS | Functions and killers exist; C# `CurrentConfigVersion = 4` at 7eaee2a2, so no stamp difference exists [V]. | none |
| INV-11 (L35) | PASS | Killers exist, behaviour matches C# `SettingsStore` (retries 50/100/200/400, backups) [V]. | none |
| INV-6 journal (L80) | DOUBT (strong) | The cell says `journal.rs` "179 of 179 covered and clean", but `docs/reviews/W3-01-JOURNAL-MUTATION.md:28,81,117` records 94 of 179 run, 85 NOT run, and ends "PARITY rows stay Partial" [V]. The 179/179 figure comes from the `journal_a` pass of commit 5946f0f6 (#331), whose figures are in the PARITY diff only, never in the evidence file [V]; the killers comment `journal_mutation_killers_3.rs:72` points to a report section that does not exist. Also "all 64 green" is stale (83 tests, 20 ignored; 63 run by default). The `#[ignore]`d native twins are outside mutation [V]. | Restore the `journal_a` table in the evidence file (or re-run `journal.rs`), fix "64". Downgrade to Partial if it cannot be restored. |
| F-16 / D-14 (L81) | DOUBT (strong) | Same 179/179 root as INV-6 [V]. Counts 27/30/15 pass (re-run). Stale sweep rule matches C# `OperationJournal.cs:823-838` [V]. `file_actions_compaction.rs:9` still says "red against todo!() stubs" (stale). Native Windows primitives are not measured by mutation. | As INV-6. |
| F-10 / INV-4 / INV-6 undo (L86) | DOUBT (procedure) | All figures reconcile: 177+3+19=199, 180+19=199, 46+176-23=199; 120 tests = 105+13+2 [V]; `file_actions_undo` re-run 120 pass. The single-Recycle post-restore stat exception (C# `UndoService.cs:373` escapes; Rust returns `CoreUndoFailed`) is recorded as "accepted as is" by the lane itself (CL-UNDO2), while CX-UNDOSTAT earlier said "not an accepted intentional difference"; there is no ADR and no entry in `docs/PROPOSED-DECISIONS.md` [V]. The equivalents listed belong to the older run, not the 199 re-run. | Add the decision to `docs/PROPOSED-DECISIONS.md`. |
| W6-01b (L59) | DOUBT (wording) | 23+3+2+2=30, round 2 9+1=10 [V]; 18 tests = 18 C# facts; 3 killers at `w6_mutation_killers.rs:70,78,90`. The 2 "hangs" are real infinite loops in `percent_decode` (`explorer_window_selector.rs:204-218`). The row says live COM is "covered by the P-01 pump smoke", but the evidence says that smoke proves only that the STA pump starts and joins; P-01 is Partial. `%zz`/bad UTF-8 and `file://C:/x` return None where .NET `Uri` keeps the text [S]. | Reword; optional tests plus a Different note. |
| P-04 (L392) | DOUBT | `instance_keys` 22 caught + 11 survivors = 33 against "34 tested": the missing mutant is a timeout (probably `instance_keys.rs:160`) [S]. The 11 survivors are not itemised in the row. `dot_and_dotdot_segments_resolve_lexically_in_the_key` is credited with `:84/:85` but those arms are listed as dead/equivalent (`std::path::absolute` already normalises), so it kills nothing. `current_session_id -> 1` is environmental (machine in session 1). The cell claims no second OS process was run, yet `pr-shell/tests/exe_process.rs:39` already forwards between two real exes; only sessions other than 1 are untested. | Rewrite counts; itemise survivors; relabel the killer; cite `exe_process`. |
| X-exe-manifest (L16) | PASS | 9/3/3 test counts match [V]; `exe_resources.rs:75` compares the whole RT_MANIFEST payload; the 3 `exe_process` tests are native. Note the env override `PHOTOREVIEW_RS_RESOURCE_EXE` (`exe_resources.rs:19`). | none |
| I-11 order/pace/backoff/policy (L95) | PASS | 15+68+41+111=235, 221+14=235, per-file sums correct [V]; 18/12 test counts and goldens match; C# PreloadOrder/Window/Pace test twins present. | none |
| I-05 (L94) | PASS | `adaptive_preview.rs` matches C# operation order; the underflow 0x0 quirk is pinned by oracle goldens (`both-sides-underflow-to-zero-then-nan`) and `imaging_mutation_killers.rs`; 33+7=40, 0 missed [V]. | none |
| W4-16a pixels (L484) | PASS | 174+10+1+15=200 [V]; test counts 37/7/26/5 match; the equivalent `pixels.rs:468:75` is right (`step_y` is +/-1, `y>=0`); killers2 has a 20 s guard thread turning hangs into assertions. | none |
| X-viewing-pure (L550) | DOUBT (wording) | 25+1+3=29, 85+4+3=92, 121-92=29 [V]; the 4 equivalents match C#. "33 of 33" reads as 33 caught; it is 29 caught + 2 equivalent + 2 unviable (figures in W8-PURE-MUTATION). The cell still carries the superseded "92/121 ... 29 not run". | Reword. |
| X-host-ui-dispatcher (L600) | DOUBT (wording) | 79+11=90, 9 and 22 test counts, 14+7=21 C# tests [V]. Stale text: "MessageLoop was still a stub", `dequeue_background 3/3` repeated; the header of `tests/host_ui_dispatcher_native.rs:1-8` still says the real loop is not used. Only 2 native tests drive the real `MessageLoop` (`w8_host_mutation_killers.rs:34,76`); the 22 native tests still use the test-local pump. The 90-mutant run did not include the killers binary. | Reword; update the file header. |
| X-overlay-button (L545) | DOUBT (light) | 56+9=65 caught + 6 unviable = 71 [V]; native counts 35/32/25/50/5/14/18/6 match. The row says `overlay_button` has 17 tests; the file has 24 [V]. | 17 -> 24. |
| X-w9r7 (L690) | PASS | 26=24+2, 34=28+6, 80=79+1, 68=59+9 [V]; tests 10/7/9/14 match. Side finding: W9-RENAME-LANE calls `zoom_detail.rs:306:13` equivalent but W8-PURE-MUTATION says a killer kills it. | Fix that evidence contradiction. |

Totals: 26 table rows: 11 PASS (X-counting-fs, RV-C05, AR16, F-22, S-65, INV-11, X-exe-manifest, I-11, I-05, W4-16a, X-w9r7),
15 DOUBT, 0 WRONG.

## Cross-cutting findings

1. Native tests are not gated. CI runs `cargo test -p pr-core` and `-p pr-platform-win` without `--include-ignored`; no
   nightly native job exists in `.github/` although ADR-INV Q7 promises one. Rows relying on native killers (X-physical-fs,
   AR16, SEC-01, D-05, INV-8, F-16) have no regression gate.
2. Timeouts are counted as detected in several rows (X-physical-fs 3, F-23 11, perf_csv 7, W6-01b 2, P-04 2) without
   itemised lists; most are genuine infinite loops, but the lists are missing.
3. Stale wording survives status changes (S-64, INV-6 "64", X-host-ui-dispatcher, X-viewing-pure).

## Tests re-run by the lead (CARGO_TARGET_DIR=<worktree>/target)

pr-core: `io_counting` 7 pass; `settings_store` 119 pass; `session_writer` 47 pass, 1 ignored; `file_actions_undo` 120 pass;
`file_actions_journal_compaction` 15 pass, 15 ignored (default run). Native (stage-native PASS, `--include-ignored --test-threads=1`): `io_physical` 64 pass; `file_actions_destination_policy` 20 pass;
`file_actions_journal_compaction` 30 pass; pr-platform-win `w2_fs_flush_killers` 9 pass, `w6_mutation_killers` 37 pass; pr-app
`overlay_button` 24 pass. No test failed.

## To-do list

1. INV-6 journal / F-16: restore the `journal_a` mutation table into `W3-01-JOURNAL-MUTATION.md` or re-run `journal.rs`;
   otherwise downgrade both to Partial (owner decision).
2. SEC-01: run cargo-mutants on `final_link_target_slow` with native tests; settle the `hops += 1` mutant and P-03.
3. Add a native test job (or an explicit Gaps note) for rows depending on `#[ignore="native"]` killers.
4. List the timeouts and equivalents in evidence (X-physical-fs, F-23, X-perf-diagnostics, P-04).
5. Record the single-Recycle undo difference in `docs/PROPOSED-DECISIONS.md`.
6. Wording fixes: S-64, W6-01b, P-04 counts and killer label, X-viewing-pure, X-host-ui-dispatcher, X-overlay-button 17 -> 24.
7. Re-run mutation on D-05/INV-8 tests after their move to pr-platform-win; list the two other SEQUENTIAL_SCAN sites.
8. Fix the `zoom_detail.rs:306:13` equivalent/killed contradiction between W9-RENAME-LANE and W8-PURE-MUTATION.

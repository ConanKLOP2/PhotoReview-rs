# ST-PARITY-SYNC5 (2026-10-31)

Status: report (lane `st-parity-sync5`, docs and the native-nightly workflow only; nothing was built or tested). Syncs `docs/PARITY.md`, the ADRs and the decision list with PRs #489..#495 (all merged: checked with `git log origin/master`) and reads their reports `FIX-RAW-CORPUS-GATE`, `FIX-BENCH-HOST`, `FIX-FFI-M4`, `FIX-SHELL-LOW`, `FIX-RECYCLE-C2`, `FIX-RAW-SHARE-DELETE`, `FIX-UPDATE-REPO`.

## PARITY rows noted (no status change; `parity.py stats` before and after: Done 183, Partial 102, Ported 38, Superseded 17, Different 14, Todo 4, Implementation 3)

| PR | Rows | What is recorded |
|---|---|---|
| #490 | X-benchmark-compare, X-bench-cli-batch | host decided (P-60): `pr-bench-win` binary has `WindowsEngineHost`; `pr-bench` stays unwired; no profile run, W10-B2 open |
| #495 | P-09 (status row and spec row) | endpoint follows `ConanKLOP2/PhotoReview-rs` (Different, DV-22); 404 with no release gives `Failed(BadResponse)`; no decision to read it as up to date |
| #493 | P-03 | C2 sink and `ShellRecycleOutcome` done against a fake only; real Shell calling the sink NOT confirmed; risk and fallback B written out; not Done |
| #494 | I-15b / INV-8, I-15 (LibRaw decoder) | Different, DV-24: shared read into memory; 14 of 14 pixels equal on a synthetic DNG; real corpus not run; `read_info` and the thumbnail path still `open_wfile`; not Done |
| #491 | I-02b (TurboJPEG), I-15b (LibRaw) | pointer helpers behind feature `test-support`; M2 and M3 deferred |
| #489 | five RAW reader rows (I-13 x2, X-cr3-reader, I-14, RawDecoder routing) | skips are loud (SKIPPED on stderr, presence report); STRICT env names; `raw_exif` has no strict branch; CI sets neither |
| #492 | X-startup-composition-root, X-startup-overlay-viewmodel, X-startup-options-run | L1, L3, L4, L5, L6, L8 fixed; L2 deferred; L7 recorded |

## ADR and decision ids

- Fixed: P-62 said DV-22 (now DV-23, matches the ADR file), P-63 said DV-22 (now DV-24), the bench-host ADR said P-61 (now P-60). `docs/adr/README.md`: DV-22 (update repository, in the merged ADR file) added to the range, rows added for the raw-share-delete and unsafe-policy ADRs. No duplicate P-nn or DV-nn remains: P-60 bench, P-61 update-repo, P-62 recycle-c2, P-63 raw-share-delete, P-64 unsafe policy; DV-22 update-repo (merged file), DV-23 recycle-c2 (own file), DV-24 raw-share-delete (own file).
- New: `docs/adr/ADR-D-2026-10-31-unsafe-policy.md` and P-64 (`PROPOSED, acted on, no user confirmation yet`). Recommended option of owner question 4, minimum form: the 9 production unsafe lines of `pr-core` and 6 of `pr-imaging` (checked in `unsafe-audit.baseline`) are grandfathered and may only shrink; no new unsafe there without an ADR. `AGENTS.md` (one sentence) and `tools/dev/docs/unsafe-audit.md` (one sentence) say so. `unsafe-audit.py` and the baseline were NOT edited, so the audit still reports these sites as rule B.

## native-nightly (`.github/workflows/native-nightly.yml`, additions only, YAML parsed)

Added, all unconfirmed until one night has run and all need a desktop session: `pr-platform-win` `host_main_window_lifecycle` (16 native tests, M1 pending_opens), new `pr-shell` entry `startup_overlay_vm` (3 native tests, L6) and `composition_graph` (2 native tests, dispose flush). A night that is red only for lack of a desktop means removing the binary.
Not added: `pr-libraw` `raw_share_delete` (needs `libraw.dll` and python; the workflow has no staging, P-55 blocks fetching DLLs; noted in the header comment); `recycle_delete_sink` (no `#[ignore]` test, already runs in `ci.yml`).

## Việc còn lại

- Run the nightly once and confirm or prune the three binaries.
- Wire `raw_exif` strict mode; run the RAW corpus with both STRICT variables.
- W10-B2 attended benchmark with `pr-bench-win`.
- Follow-up code lane: add `pr-core` and `pr-imaging` to the allowed list of `unsafe-audit.py` and drop their rule B baseline entries after adding `SAFETY:` comments.
- Mutation testing for the lanes #489..#495 was not done.
- L2 of the shell review (lone surrogate in a file name), `start_viewer_preview` panic, M2 and M3 of the FFI review stay deferred.

## Cần người

1. Run once, attended, with a self-made temp file: `native_attended_recycle_of_own_temp_file_reports_moved_to_recycle_bin` (`crates/pr-platform-win/tests/recycle_bin_facade.rs`). If the Shell never calls the sink every recycle reports `Unknown` and fails; then choose fallback B of P-62.
2. Decide whether "no release in the Rust repository" (HTTP 404) should read as up to date instead of `Failed(BadResponse)`.
3. The host for the bench binary was done as option (a) (P-60); confirm or ask for option (b).
4. Confirm or reverse P-60, P-62, P-63 and P-64 (all acted on without confirmation); RAW corpus for DV-24.

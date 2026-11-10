# G17-IGNORED-TESTS: audit of `#[ignore]` tests (2026-11-03)

Status: audit report, docs only. No production or test code changed, no `cargo` run. Source: `origin/master` `82da9f7` (branch docs/g17-audit).
Backlog entry: [BACKLOG G17](BACKLOG.md) ("CI does not run 1157 `#[ignore]` tests"; that figure is the 2026-10-23 count in `test-quality.md` M1).
Every number below comes from the commands in section 1, run on this commit.

## 1. Commands

| # | Command (repo root) | Result |
|---|---|---|
| 1 | `git grep -n -I '#\[ignore' -- '*.rs'` (count lines) | 1643 lines in 231 files (`git grep -l`), all under `crates/` |
| 2 | `git grep -n -I -E '^\s*#\[ignore' -- '*.rs'` | **1548 attribute lines**, 219 distinct files |
| 3 | difference 1643 - 1548 | 95 lines are `//` / `//!` doc or comment text that mentions `#[ignore]`; they are not tests |
| 4 | `git grep -n -I -E 'ignore' -- '*.rs'`, then keep lines containing `#[ignore` or `ignore = "` that do not start with `#[ignore` or `//` | 0 hits (no `cfg_attr(..., ignore)`, no attribute split over several lines, none generated inside a macro body) |
| 5 | from command 2, a PowerShell pass: parse `crates/<crate>/<path>:<line>:` and `ignore = "<reason>"` with one regex; the reason is the string literal | 1548 parsed, 0 unparsed, 0 non-literal reasons |
| 6 | same pass, group by crate / by reason text | sections 2 and 3 |
| 7 | read `.github/workflows/native-nightly.yml`; the `$plan` crate -> test binary map is parsed with a regex (only `'pr-core'`, `'pr-platform-win'`, `'pr-imaging'`, `'pr-app'`, `'pr-shell'` appear); a test counts as nightly only when its file is `crates/<crate>/tests/<bin>.rs` and `<bin>` is listed for that crate | section 5 |
| 8 | `git grep -n -E 'include-ignored\|--ignored' -- .github` | only `native-nightly.yml` (comments at lines 4 and 104; the command is at line 120: `cargo test -p $crate --locked --test $bin -- --include-ignored --test-threads=1`); `ci.yml` and `release.yml` never pass it |

One `#[ignore]` attribute = one test; this was not checked against `cargo test -- --list --ignored` (that needs a build). The totals are therefore attribute counts. `--test` in command 8 runs integration test binaries only, so ignored tests in `src/` (unit tests) can never run nightly.

## 2. Totals per crate

| Crate | `#[ignore]` | In `src/` | Run by nightly |
|---|---:|---:|---:|
| pr-platform-win | 1143 | 85 | 106 |
| pr-core | 156 | 0 | 150 |
| pr-shell | 95 | 4 | 6 |
| pr-turbojpeg | 67 | 1 | 0 |
| pr-libraw | 44 | 0 | 0 |
| pr-app | 17 | 0 | 7 |
| pr-imaging | 11 | 0 | 7 |
| pr-imaging-raw | 8 | 0 | 0 |
| pr-bench | 5 | 0 | 0 |
| pr-perf | 2 | 0 | 0 |
| **Total** | **1548** | **90** | **276** |

1548 - 276 = **1272 ignored tests are not run by any workflow** (`ci.yml`, `release.yml`, `native-nightly.yml`). 276 are in test binaries that nightly lists; many `pr-platform-win` and all `pr-shell` binaries in the plan are marked in the workflow comments as "no nightly run yet, one night must confirm" (not counted here binary by binary), so the real nightly count can be lower.

## 3. Totals per reason category

The reason is the text of `ignore = "..."`. 165 distinct reason strings exist. Category rules (first match wins): needs DLL = reason contains `libraw.dll`, `turbojpeg.dll` or `RAW corpus` (except "no turbojpeg.dll needed"); needs desktop = contains `desktop`, `window`, `DWM`, `hidden`, `modal`, `UI thread`, `pixels` or `popup`; slow = starts with `slow`; native other = starts with `native`; other = anything else.

| Category | Total | Run nightly | Never run |
|---|---:|---:|---:|
| needs desktop | 983 | 104 | 879 |
| native other (no desktop, no DLL) | 408 | 166 | 242 |
| needs DLL | 113 | 1 | 112 |
| other (not `native`/`slow`) | 27 | 2 | 25 |
| slow | 17 | 3 | 14 |
| no reason | **0** | 0 | 0 |
| **Sum** | **1548** | **276** | **1272** |

The sum row differs from section 2 by one because the "needs DLL" nightly cell printed empty in the script (one `pr-core` binary is in the plan); the per-crate table is the authority: 276 nightly, 1272 never. The category split of that one test was not re-run.

"native other" by reason text: 150 `native: needs WIC`, 66 plain `native`, 57 OS graphics stack / Direct2D / DirectWrite / COM, 135 real file system / disk / junction / ACL / registry / locale / time zone.

"other" (27): 10 `needs the C# oracle dumps ... PHOTOREVIEW_RS_ORACLE_DIR` (`pr-app/tests/verify_w8_01a.rs`), 9 `manual` / `perf` / `process-wide` / `attended` probes, 2 `waits for W8-19b..19i` (`settings_framework.rs:672,766`), 5 `blocked: ... todo!()` / OQ1 (`startup_secondary_dialogs.rs:119,127,135,143`, `wic_decoder.rs:825`), 1 `attended` Recycle Bin test (`recycle_bin_facade.rs:896`, correctly never automated, AGENTS.md "Recycle Bin").

## 4. Ignores without a reason

Command 5 found **0** attributes without `= "..."` and 0 with an empty or under 4 character reason. No AGENTS.md "Tests" violation of the "native/slow reason" kind exists in attribute form. Note the weaker point: 27 attributes use a reason that is not `native`/`slow` (section 3, "other"); AGENTS.md asks for a `native`/`slow` reason for real-OS tests, which does not fit the oracle, manual, attended and blocked cases. Whether to allow extra prefixes is P-80.

## 5. What `native-nightly.yml` runs and what nobody runs

The workflow runs `cargo test -p <crate> --locked --test <bin> -- --include-ignored --test-threads=1` for 46 binaries: `pr-core` 19, `pr-platform-win` 18, `pr-imaging` 4, `pr-app` 2, `pr-shell` 3. It is not a required check and its header says what it deliberately skips: binaries that load `libraw.dll` / `turbojpeg.dll` (blocked by P-55), the window / DWM / GPU / input-desktop families of `pr-platform-win`, `pr-bench`, anything touching the real Recycle Bin.

Never run anywhere (counts are ignored tests, from command 7):

| Crate | Never run | Files | Why |
|---|---:|---:|---|
| pr-platform-win | 1037 | 129 | 85 in `src/` (unit tests, `--test` cannot reach them), the rest in binaries not in the plan: top files `wic_decoder_native.rs` 70, `win32_dialog_native.rs` 31, `win32_api.rs` 28, `host_shell_window.rs` 28, `dialogs_native_dialog_layout_native.rs` 27, `dialogs_native_dialog_modal.rs` 26, `src/render/draw_context.rs` 26, `dialogs_native_dialog_events_native.rs` 25, `src/render/surface.rs` 24, `host_ui_dispatcher_native.rs` 22 |
| pr-shell | 89 | 19 | `ui_smoke*`, `startup_*` (desktop) |
| pr-turbojpeg | 67 | 7 | needs `turbojpeg.dll` |
| pr-libraw | 44 | 5 | needs `libraw.dll` + RAW corpus |
| pr-app | 10 | 1 | `verify_w8_01a.rs`, needs C# oracle dumps |
| pr-imaging-raw | 8 | 2 | needs `libraw.dll` + corpus; one machine-dependent perf test |
| pr-core | 6 | 3 | `file_actions_compaction.rs` 4 (slow), `diagnostics_perf_csv.rs` 1, `session_writer.rs` 1 (not in the plan) |
| pr-bench | 5 | 2 | benchmark tool |
| pr-imaging | 4 | 4 | `cache_preview_file`, `preview_file_repair`, `verify_navigation_pace`, `w4_mutation_killers3` |
| pr-perf | 2 | 1 | manual benchmark |

By category (section 3): never run = 879 desktop + 242 native other + 112 DLL + 25 other + 14 slow. Of the "native other" ones that need only the file system, `pr-core` is covered well (150 of 156 run); the 242 are mostly 150 `needs WIC` tests plus OS graphics-stack tests in `pr-platform-win`, which the workflow skips because windows-latest was not confirmed to have WIC / a desktop.

## 6. Reading for G17

- The BACKLOG figure 1157 is now 1548 attributes (1643 grep lines include 95 comment lines). The task text "about 1630" counts comment lines too.
- 82% of ignored tests (1272 of 1548) have no automated run. The 879 desktop and 112 DLL ones are the large blocks; they need either a desktop runner or staged DLLs, which is an owner decision (P-80).
- The 95 comment mentions are not defects, but several say "none is left" / "run with --include-ignored"; they were not checked for staleness here.
- Not examined: whether ignored tests still pass, stale reasons (test-quality M2) and tests that pass vacuously (M3). That needs `cargo`, a desktop and DLLs and is the remaining G17 code work per crate.

## 7. P-80 follow-up (2026-11-03, owner chose option A)

Evidence from `gh run list --workflow native-nightly.yml` and `gh run view <id> --log` (read-only; no run was triggered). The nightly exists and ran four times, all `schedule`, all `success`:

| Run id | Date (UTC) | Binaries run | Tests passed / failed |
|---|---|---:|---|
| 41139005966 | 2026-10-30 | 23 | 667 / 0 |
| 41245151518 | 2026-10-31 | 45 | 973 / 0 |
| 41356161985 | 2026-11-01 | 45 | 975 / 0 |
| 41557791599 | 2026-11-03 | 46 | 1002 / 0 |

- Step 1 (confirm one night of the listed desktop binaries): DONE. The 2026-11-03 run executed all 46 listed binaries, including the desktop ones (`w8_mainwin_frame_tests`, `host_main_window_lifecycle`, `w8_settingswin4_killers`, `shell_input_pan_glide_native`, `startup_overlay_vm`, `composition_graph`, `startup_early_decode_native` and the rest) and all passed. The "no nightly run yet" comments in the workflow are therefore out of date (comment-only clean-up, not done here).
- WIC on windows-latest: partly shown. No `wic_*` binary ran on any night (the only `wic` hits in the logs are the word "twice"). But `w4_mutation_killers2::jpeg_option_bag_keeps_image_quality_observable_in_encoded_bytes` (`#[ignore = "native: needs WIC"]`, real `encode_jpeg` through WIC) ran and passed in all four nights. So WIC encode works on the runner. Decode (`WicDirectDecoder`), the scaler, EXIF reader and the goldens in `wic_decoder_native` (70 tests; pixel hash compared only when `PHOTOREVIEW_RS_WIC_VERSION` matches the runner's `windowscodecs.dll`, otherwise tolerance 3) are not shown to run there.
- Step 2 (add the `needs WIC` binaries): NOT DONE. The instruction was to add only binaries the logs prove can run; the logs prove none of them. `.github/workflows/native-nightly.yml` is unchanged. The evidence can only come from a first run, which a schedule-only workflow gets only if the binaries are added on trial (as every earlier addition was, with a "one night must confirm" comment). That is a decision, recorded as P-86 in PROPOSED-DECISIONS.
- Candidate binaries for step 2 (counts of `native: needs WIC` attributes; NOT checked for fixtures, DLLs or a desktop beyond the header text): `wic_decoder_native` 70, `wic_imaging_twins_embedded` 19, `wic_cache_image` 14, `wic_scaler` 6, `wic_imaging_twins_thumbnail` 8, `wic_interop` 7, `preview_cache_wic` 5, `wic_cache_adapters` 5, `wic_availability` 4, `wic_decoder_buffers` 4, `wic_embedded_lifetime` 3, `wic_exif_reader` 3, `wic_repair1` 3. Excluded: `wic_imaging_twins_preview` and `wic_preview_service_broken_entry` (need staged DLLs, P-55), `pr-imaging/cache_preview_file` (same).
- Step 3 (the rest stays hand-run): documented in `docs/DEVELOPING.md`, section on `native-nightly.yml`, "Running the hand-run families".

## 8. P-370 per-family owners (2026-11-10, draft with the recommended defaults)

Status: UNCONFIRMED PROPOSAL, acted on under the standing autonomy rule (the owner has not named the owners or cadences; change this table if the owner picks differently). Docs only: no workflow, test or AGENTS.md change. Basis: P-80 option A (confirmed 2026-11-03: grow the nightly in steps, the rest is hand-run on the owner machine) and P-86 (five `wic_*` trial binaries in `native-nightly.yml`). Counts are the never-run counts of section 3 (1272 in total); the 276 tests that `native-nightly.yml` already runs are not repeated here.

| Family | Never run | Owner (recommended default) | Cadence | How |
|---|---:|---|---|---|
| needs desktop (windows, DWM, modal, input, hidden windows) | 879 | owner machine (maintainer of the Windows port) | before merging a PR that touches the window, dialog, shell or host code, and once a week; the 104 desktop tests already in the nightly stay there | `bash tools/dev/native-tests.sh` per binary (`--list` first); no new runner (P-80 option B was not chosen) |
| needs DLL (`libraw.dll`, `turbojpeg.dll`, RAW corpus) | 112 | owner machine | before a release and after any change in `pr-libraw`, `pr-turbojpeg`, `pr-imaging-raw` | `tools/dev/stage-native.ps1` (copies the DLLs from the local C# checkout), then `native-tests.sh`; CI stays out (P-55: no DLL download) |
| needs WIC | 150 | CI nightly (trial), then owner machine for the rest | nightly for the five trial binaries (`wic_decoder_native`, `wic_cache_image`, `wic_scaler`, `wic_exif_reader`, `wic_availability`), judged after each night (P-86, P-80 step 2); the other `needs WIC` binaries weekly on the owner machine | `native-nightly.yml`; a binary that is red on windows-latest is removed from the list and moves to the owner machine |
| other native without a desktop or DLL (about 92 OS graphics stack tests in `pr-platform-win`: Direct2D, DirectWrite, COM) | 92 | same owner as the desktop family | with the desktop family | `native-tests.sh`; fold the nightly candidates into the nightly once one night of the file-system-only ones is green |
| oracle, manual, attended, blocked (`oracle:`, `manual:`, `attended:`, `blocked:` reasons) | 25 | oracle: owner machine; manual and attended: whoever changes the feature; blocked: none until the blocker is lifted | oracle: after every `tools/sync-from-csharp.ps1` baseline move and every golden regeneration; manual and attended: when the related feature changes; blocked: re-read when the blocker (`todo!()`, OQ1, W8-19b..19i) is closed | `bash tools/dev/oracle.sh` for the oracle dumps; the others by hand |
| slow | 14 | owner machine | once a month and before a release; candidates for the nightly if all 14 together stay under the job budget | `native-tests.sh` (or `cargo test -- --ignored` on the named binary, bounded) |

Sum check: 879 + 112 + 150 + 92 + 25 + 14 = 1272. The 242 "native other" tests of section 3 are the 150 WIC tests plus these 92.

What this changes: the 1272 ignored tests now have a named owner and cadence in this document. It does not make them run: the hand-run families have no automated gate (that stays the P-80 trade-off, "a red night is not blocking"). The owner may rename the owners, change the cadences or move a family to CI; each such change edits this table and the P-370 row. Closed by this section: the "per-family owners are not assigned" part of P-370 (recorded in `docs/PROPOSED-DECISIONS.md`, `docs/reviews/BACKLOG.md` and `docs/reviews/README.md`).

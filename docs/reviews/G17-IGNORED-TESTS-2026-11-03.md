# G17-IGNORED-TESTS: audit of `#[ignore]` tests (2026-11-03)

Status: audit report, docs only. No production or test code changed, no `cargo` run. Source: `origin/master` `82da9f7` (worktree `docs/g17-audit`).
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

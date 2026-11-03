# Q6 PARITY close, startup and exe cluster (2026-11-03)

Rows: `X-exe-entry`, `X-exe-versioninfo`, `X-startup-instance`, `X-startup-early-decode`, `U-initial-viewport`, `X-composition-factories`.
Branch `parity/startup-exe` from `origin/master` 84d2c151 (C# baseline `7eaee2a2`). `docs/PARITY.md` and the other lead-owned files were not edited; the proposed status text per row is at the end of each section.

## How this run was set up (read first)

- Desktop: Windows 10, `native-tests.sh pr-shell --list` reports "interactive input desktop: yes"; two monitors, AppliedDPI 96. No person watched the screen; the native tests drive the real exe and read its dialogs themselves.
- Pinned DLLs: `tools/dev/stage-native.ps1` failed (`Missing source DLL: C:\Users\PC\Projects\PhotoReview\native\x64\libraw.dll`: the main C# checkout has no `native\x64`). The same two DLLs exist in a C# worktree (`PhotoReview\.claude\worktrees\executor-throw-unverified\native\x64`); their SHA-256 equal the pins in `native/libraw.sha256` (6a459c22...) and `native/turbojpeg.sha256` (e9bdec69...). They were copied by hand to `<target>\debug` and `<target>\debug\deps` (P-50). Nothing was copied into the repo.
- `AGENTS.md` (this branch) contains no explicit "promotion rule" for a PARITY row. `docs/PARITY.md` line 5 says "done when its ID appears in the table with the Rust test that proves it", `docs/PARITY-CLOSEOUT.md` asks for "a proving test and record Done" while keeping native, GUI, corpus, mutation and benchmark evidence distinct. I applied the strict reading: Done only when every gap named in the row text is closed by evidence produced in this run and nothing needs a person, a corpus, an owner decision or a C# change.
- Build: `CARGO_TARGET_DIR=C:\Users\PC\Projects\target-pc-startup-exe` (plus `-rel`, `-bs`, `-clippy`), `CARGO_BUILD_JOBS=2`, `CARGO_INCREMENTAL=0`. `cargo fmt -p` fails with os error 206 on these long paths: `rustfmt --edition 2024 --check` on every changed file instead (clean).
- Mutation was run with `cargo mutants --baseline skip --timeout 90 -j 1` directly (not `mutants.sh`, whose own target dir has no staged DLLs) with `-- --include-ignored --test-threads=1` in the correct `--cargo-test-arg` form, `PHOTOREVIEW_GOLDEN_ROOT` set and the sources touched. Every run had an unmutated preflight (`running N tests`, exit 0): 24, 23, 26, 6, 3 (pr-app), 4 (pr-app), 17 tests. Total mutation wall time about 45 minutes. A TIMEOUT is never counted as caught.
- Trap met: a first batch ordered slow exe tests before the fast killers, so one mutant ran into the 90 s timeout; it was rerun with only the fast binaries (see X-startup-instance). A reused target after cargo-mutants also keeps the last mutant's artifacts (needs `touch`), as AGENTS.md warns.

## Native runs of this run (all passed, `--include-ignored --test-threads=1`, `PHOTOREVIEW_RS_ISOLATE_CONFIG=1`, throw-away data root)

| Crate / binary | Result |
| --- | --- |
| pr-shell `exe_process` | 3/3 (real exe: smoke, JPEG first frame + icons + close, second exe forwards) |
| pr-shell `exe_resources` | 3/3 on the ordinary debug exe (no VERSIONINFO) and 3/3 again with `PHOTOREVIEW_RS_RELEASE=1 PHOTOREVIEW_RS_VERSION=0.1.0` (separate target; the test took the release branch) |
| pr-shell `build_version_info` 2, `build_res_writer` 9 (12 after this run), `entry_args` 7, `w8_entry_killers` 3 | all pass |
| pr-shell `startup_instance` 1, `startup_forward_target` 3, `startup_phase_one` 2, `startup_native` 2, `startup_early_decode_native` 1 | all pass |
| pr-shell `startup_run_native` 23, `ui_smoke_startup` 4, `startup_wiring_native` 13 | all pass (includes `startup_without_saved_placement_opens_at_the_wpf_outer_size`, `startup_saved_placement_*`, the second-instance-while-starting scenario) |
| pr-shell `composition_factories` 10, `composition_decoders` 13 (pinned DLLs staged; the ORF corpus test returns early, no sample) | pass |
| pr-platform-win `native_initial_viewport` 2, `shell_startup_local_time` 1 | pass |
| pr-app `startup_instance` 3, `startup_early_decode` 3 (4 after this run) | pass |

Windows API read-back of the release-env exe (`(Get-Item PhotoReviewRs.exe).VersionInfo`): FileVersion 0.1.0, ProductVersion 0.1.0, FileDescription PhotoReviewRs, ProductName PhotoReviewRs, OriginalFilename PhotoReviewRs.exe, FileMajor/Minor/Build 0/1/0, Language English (United States), CompanyName and LegalCopyright empty (by design, P-54). The ordinary debug exe has none of these fields.

## New tests (10, all tests-only except one test-support helper)

| File | Tests | Kills / proves |
| --- | --- | --- |
| `crates/pr-shell/tests/build_res_writer.rs` | +3 (`parse_ico_returns_every_directory_field_of_every_entry`, `parse_ico_distinguishes_a_short_header_from_a_truncated_directory`, `parse_ico_directory_size_is_six_plus_sixteen_per_entry`) | 11 surviving `res_writer::parse_ico` mutants |
| `crates/pr-shell/tests/exe_startup_failure_native.rs` | 1 native | the real exe shows the error TaskDialog and returns exit code 1 (blank `PHOTOREVIEW_RS_INSTANCE_PREFIX`); a second, behavioural proof for `production_run`/`main_entry` |
| `crates/pr-shell/tests/startup_unanswered_owner_native.rs` | 2 native | the real exe against an owner that holds the mutex but no pipe: TaskDialog shown, its text read back with UI Automation (SingleWindow: "already running but did not respond"; PerFolder: "folder is already open"), exit code 0, no image opened |
| `crates/pr-shell/tests/q6_composition_killers.rs` | 3 | `create_decoder_providers` (production overload), `AppLogCompositionLog::{info, error_forced}`, three `Debug` impls |
| `crates/pr-app/tests/startup_early_decode.rs` | +1 | `Debug for EarlyDecodePlan` |
| helper | `ui_driver::main_window_of_process` (pub, `test-support` module of `pr-platform-win`, 3 lines) and `ShellProcess::start_with_config` in `tests/common/w8_20_process_fixture.rs` | support for the native tests above |

Manual sanity mutants (reverted): making `phase_one` skip the message box fails `startup_unanswered_owner_native` ("child exited before the already-running box"); making it pass `FolderAlreadyOpen` for the single-window case fails the text assertion.

## X-exe-entry

Gaps before: `production_run` and `main_entry` need an attended GUI smoke/forwarding run; 3 `production_run` mutants (0/1/-1) were recorded as unobservable without a window.

Run: `exe_process` 3/3 and `exe_resources` as above. Mutation of `entry.rs` with `entry_args, w8_entry_killers, exe_process, exe_startup_failure_native` (plus the startup binaries): 9 of 9 caught, 0 missed, 0 unviable, 0 timeout. The `production_run` mutants are in fact caught by `exe_process` (0: the first-image test sees the child exit before a frame; 1/-1: smoke expects exit 0). The earlier "unobservable" statement was true only without native tests. Build-script modules are not visible to cargo-mutants, so the 88 mutants of `build_support/res_writer.rs` and `version_info.rs` were run in a scratch crate holding byte-identical copies and the same test files: 73 caught, 12 missed (11 `parse_ico`, killed by the 3 new tests: `-F parse_ico` rerun 49 tested, 48 caught, 1 unviable; 1 equivalent, see versioninfo), 2 timeouts, 1 unviable.

Gaps after: `StaApartment::enter` failure branch of `main_entry` cannot be provoked (no mutant either; `main_entry` returns `!`). Nothing needs a person if "attended" only means an interactive desktop.

Proposed status: **Done** (2026-11-03 Q6). Note: "exe_process 3/3 and exe_startup_failure_native 1/1 on an interactive desktop in this run; entry.rs 9/9 mutants caught with native tests (production_run included); build_support res_writer 88-mutant scratch run: all non-equivalent survivors killed except 2 unkillable timeouts in the shared `align` loop; linked manifest/icon/version read back (exe_resources 3/3 ordinary and release-env). Not covered: the `StaApartment::enter` error branch." If the lead reads "attended" as "a person watching", keep Partial and use the human step below.

Human (optional): double-click `PhotoReviewRs.exe` from the release artifact (P-55) once, open a photo, close it.

## X-exe-versioninfo

Gaps before: legal/company strings; Windows Properties/SmartScreen review; no mutation of `version_info.rs`.

Run: mutation of `version_info.rs` (in the scratch crate, see above): of its mutants, all caught except `version_info.rs:54:39 | -> ^` (equivalent: `major << 16` and `minor <= 0xFFFF` share no bits) and two TIMEOUTs in `align` (`%` -> `/` and `%` -> `+`; the loop never ends and pushes zeros until memory or the 90 s limit; not caught, not killable without changing production code). `build_version_info` 2/2, `exe_resources` 3/3 with the release env, Windows API read-back (above).

Gaps after: Company/copyright values are an owner decision (P-54 chose no unverified claim); the Properties dialog look and SmartScreen reputation need a person on a signed/published release (P-55). Stays Partial.

Proposed status text: "Partial (Q6 2026-11-03): release resource built with `PHOTOREVIEW_RS_RELEASE=1 PHOTOREVIEW_RS_VERSION=0.1.0` read back by `exe_resources` 3/3 and by the Windows version API (FileVersion/ProductVersion 0.1.0, FileDescription and ProductName PhotoReviewRs, OriginalFilename PhotoReviewRs.exe, Language en-US, Company and Copyright empty by P-54); `version_info.rs` mutation clean except 1 equivalent (`|` vs `^`) and 2 unkillable `align` timeouts. Open: company/copyright strings (owner), Properties/SmartScreen review (person, P-55)."

Human: owner decides whether CompanyName/LegalCopyright are set; after the first published release a person opens file Properties > Details and records SmartScreen behaviour.

## X-startup-instance

Gaps before: production mode-specific unanswered-owner message UI not exercised; `instance.rs` killers already in evidence.

Run: `startup_unanswered_owner_native` 2/2 (both modes, text and exit code). `instance.rs` plus `entry.rs` batch: 7 `instance.rs` mutants = 5 caught, 1 unviable (`create_scope` -> `Default`), 1 TIMEOUT (`try_acquire` -> `Ok(false)`, because slow exe tests ran first); rerun with `w8_startrerun_killers, startup_instance` only: 2 of 2 `try_acquire` mutants caught. So 6 caught, 1 unviable, 0 missed, 0 timeout after the rerun. Existing native binaries (`startup_instance`, `startup_forward_target`, `startup_phase_one`, `startup_native`, `startup_run_native` second-instance scenario, `exe_process` forwarding) pass.

Gaps after: none that a machine can close; Win32 scope/pipe glue (`pr_platform_win::instance_scope`) belongs to the W2-19 rows and was not mutated here.

Proposed status: **Done** (2026-11-03 Q6): "unanswered-owner message UI proven natively in both modes (`startup_unanswered_owner_native` 2/2: TaskDialog text read back, exit 0, no image); `instance.rs` mutation 6 caught, 1 unviable, 0 missed; all startup native binaries green in this run."

## X-startup-early-decode

Gaps before: stays Partial until one green native-nightly run of `startup_early_decode_native`.

Run: green nightly verified with `gh`: run 41557791599 (schedule, 2026-11-03 01:08Z, head 1496425f, conclusion success) log shows `pr-shell startup_early_decode_native ... test startup_early_decode_starts_only_after_the_instance_lock_is_held ... ok`. The test also passed here (1/1 on origin/master 84d2c151), with `pr-app startup_early_decode` 4/4 and `startup_phase_one` 2/2. Mutation of `pr-app/src/startup/early_decode.rs`: first run 5 tested, 2 caught, 1 missed (`Debug for EarlyDecodePlan`), 2 unviable; with the new Debug killer: 5 tested, 3 caught, 2 unviable, 0 missed. The nightly head differs from current master by 64 lines in `adapters.rs`, `registry.rs`, `run.rs`, `window.rs` (checked with `git diff --stat 1496425f origin/master`); the next nightly will re-confirm.

Gaps after: none.

Proposed status: **Done** (2026-11-03 Q6): "process scenario in `startup_early_decode_native`, green in native-nightly run 41557791599 and locally 1/1; `early_decode.rs` mutation 3 caught, 2 unviable, 0 missed."

## U-initial-viewport

Gaps before: expectations computed from this machine's monitor/DPI only; a full startup desktop run is not part of the binaries.

Run: `native_initial_viewport` 2/2, `shell_startup_local_time` 1/1, plus the full startup chain in `startup_run_native` (23/23, default-size and saved-placement scenarios) and `ui_smoke_startup` 4/4. Mutation of `monitor_probe.rs` (`native_initial_viewport, w8_rest1_killers`, 17 tested): 13 caught, 2 unviable, 2 MISSED: `monitor_probe.rs:62:22` and `:67:22` (`dpi_for`, `awareness == PER_MONITOR` and `== UNAWARE` flipped). With every display at 96 DPI (AppliedDPI 96, both monitors) the per-monitor, system and unaware answers are all 96, so no test can tell the branches apart on this machine; the existing test mirrors the same awareness logic and fails the mutants on a display with DPI other than 96. Pure predictor mutation (41/41) was not rerun.

Gaps after: DPI-dependent branches (2 mutants) and a second monitor/DPI configuration. Stays Partial.

Proposed status text: "Partial (Q6 2026-11-03): native_initial_viewport 2/2 and the full startup chain (`startup_run_native` 23/23, `ui_smoke_startup` 4/4) pass on a 2-monitor 96 DPI desktop; `monitor_probe.rs` mutation 17 tested = 13 caught, 2 unviable, 2 missed (`dpi_for` awareness comparisons, indistinguishable at 96 DPI). Open: run `native_initial_viewport` on a display scaled to 125% or 150% (ideally mixed DPI) and rerun the 2 mutants."

Human: set one display to 125% or 150%, then `cargo test -p pr-platform-win --test native_initial_viewport -- --include-ignored --test-threads=1`, and `cargo mutants -p pr-platform-win --baseline skip --timeout 90 -j 1 --file crates/pr-platform-win/src/shell_startup/monitor_probe.rs -F dpi_for --cargo-test-arg=--test --cargo-test-arg=native_initial_viewport --cargo-test-arg=-- --cargo-test-arg=--include-ignored --cargo-test-arg=--test-threads=1`; both `dpi_for` `==` -> `!=` mutants must be caught.

## X-composition-factories

Gaps before: ORF/camera corpus; decoder policy mutation not rerun; 13 native/FFI composition candidates excluded.

Run: `composition_factories` 10/10, `composition_decoders` 13/13 (DLLs staged). Mutation of `composition/decoders.rs` + `factories.rs` (26 mutants): first run 7 caught, 13 unviable, 6 missed (all in `decoders.rs`: `create_decoder_providers` -> `vec![]`, `AppLogCompositionLog::{error_forced, info}` -> `()`, and the `Debug` impls of `DecoderProbes`, `DecoderDeps`, `LibRawPreviewFallback`). The 3 killers in `q6_composition_killers.rs` kill all six: rerun of `decoders.rs` 19 tested = 12 caught, 7 unviable, 0 missed. `factories.rs` 7 tested = 1 caught, 6 unviable, 0 missed. Final for both files: 13 caught, 13 unviable, 0 missed, 0 timeout. The production overload test redirects `AppLog::global()` to a temp file, so it never writes to the user's data folder.

Gaps after: no ORF/camera file on this machine (`PHOTOREVIEW_RS_RAW_CORPUS_ORF` unset; no `*.orf` under Projects, Pictures, Documents, Downloads, Desktop), so `read_jpeg_thumbnail_orf_corpus_file_returns_complete_jpeg` still returns early; the 13 excluded native/FFI composition candidates (other `composition/*.rs` glue) were not touched. Stays Partial.

Proposed status text: "Partial (Q6 2026-11-03): composition_factories 10/10, composition_decoders 13/13 with the pinned DLLs; decoder policy mutation rerun: decoders.rs + factories.rs 26 mutants = 13 caught, 13 unviable, 0 missed after 3 killers. Open: real ORF/camera corpus; the 13 excluded native/FFI composition candidates."

Human: put one real Olympus ORF file on disk, `set PHOTOREVIEW_RS_RAW_CORPUS_ORF=<path>`, stage the pinned DLLs, run `cargo test -p pr-shell --test composition_decoders -- --include-ignored --test-threads=1` (the corpus test must print no early return and pass); decide whether the 13 glue candidates need mutation at all.

## Not run

- Mutation of `phase_one.rs`, `run.rs`, `window.rs` and the other `composition/*.rs` glue (13 native/FFI candidates), of `pr_app::services::initial_viewport` (41/41 on 2026-10-31, not rerun), of `pr_platform_win::instance_scope` (W2-19).
- Release profile build and publication (P-55), SmartScreen, a Properties dialog screenshot.
- `cargo test --workspace`, `pre-pr.sh --test`, `native-tests.sh` over whole crates (only the binaries named above were run). Clippy `-D warnings` for `pr-shell`, `pr-platform-win` and `pr-app` with `--all-targets --locked` passed on the final tree; `rustfmt --edition 2024 --check` on the 7 changed Rust files is clean.

## Proposals (no production change made)

1. `AGENTS.md` has no written promotion rule for a PARITY row, although the workflow text refers to one: the lead may add the exact conditions (native in the same run, mutation, corpus, person) so the next close does not depend on a reading.
2. `res_writer::align`/`version_info::align` loops `while out.len() % 4 != 0 { out.push(0) }`; two mutants run forever. If the lead wants them caught, extract the padding into `fn padding(len) -> usize` (production change, tests then compare the byte count). Not done.
3. `tools/dev/stage-native.ps1` fails when the main C# checkout has no `native\x64` although another C# worktree has the pinned files: either let the script try `git worktree list` of the C# repo or document the manual copy used here.
4. `tools/dev/mutants.sh` cannot mutate `crates/pr-shell/build_support/*.rs` (cargo-mutants does not discover build-script modules); the scratch-crate method used here could become a documented recipe.

# Wave 8 pure-logic postmerge mutation verification

Completed bounded verification, 2026-10-29. Source head: `83da367f`, branch
`feat/claude-mut-w8`. Production source remains unchanged. No commits, pushes,
native/window tests, real user data or Recycle Bin operations were performed.

## Scope and baseline

The five requested PARITY rows map to the following files and integration test
binaries. The platform frame modules are pure policies/formatting, despite their
crate name. Window hosts, GPU/COM/FFI implementation and background/thread glue
are excluded. There are no `#[cfg(test)]` unit modules in these thirteen source
files; no additional `--cargo-arg=--lib` mutation run is required.

| Row | Source (relative to `crates/`) | Existing test binaries |
|---|---|---|
| X-viewing-pure | `pr-app/src/viewing/scaling_policy.rs` | `shell_viewing_scaling`, `verify_w8_16a_scaling` |
| X-viewing-pure | `pr-app/src/viewing/image_layer.rs` | `shell_viewing_image_layer` |
| X-viewing-pure | `pr-platform-win/src/shell_viewing/frame_stats.rs` | `shell_viewing_frame_stats`, `shell_viewing_golden`, `verify_w8_16a_diff` |
| X-viewing-pure | `pr-platform-win/src/shell_viewing/pacer.rs` | `shell_viewing_pacer`, `shell_viewing_golden`, `verify_w8_16a_diff` |
| X-viewing-pure | `pr-platform-win/src/shell_viewing/frame_log.rs` | `shell_viewing_frame_log`, `shell_viewing_golden`, `verify_w8_16a_diff` |
| X-pointer-click-pan | `pr-app/src/input/pointer/click_zoom.rs` | `pointer_click_zoom` |
| X-pointer-click-pan | `pr-app/src/input/pointer/pan.rs` | `pointer_pan`, `pointer_click_zoom` |
| X-overlay-button | `pr-app/src/overlay/button.rs` | `overlay_button` |
| I-11 (order, pace, backoff, policy) | `pr-imaging/src/preload_order.rs` | `preload_order` |
| I-11 (order, pace, backoff, policy) | `pr-imaging/src/navigation_pace.rs` | `navigation_pace`, `verify_navigation_pace` |
| I-11 (order, pace, backoff, policy) | `pr-imaging/src/preload_backoff.rs` | `preload_backoff` |
| I-11 (order, pace, backoff, policy) | `pr-imaging/src/preload/policy.rs` | `preload_policy` |
| X-zoom-detail-loader | `pr-app/src/coordinators/zoom_detail.rs` | `zoom_detail` |

Baseline runs use the original source in `<worktree>/target`, never `target-mut`.
All seven listed pr-app binaries passed; pr-imaging: 70 passed, 1 ignored;
pr-platform-win: 29 passed, 0 ignored. Per-run output is retained locally in
`.codex-w8-baseline-*.log` where captured.

## Method and command template

Environment: `PATH=%USERPROFILE%/.cargo/bin;%PATH%`,
`CARGO_INCREMENTAL=0`, `CARGO_PROFILE_DEV_DEBUG=line-tables-only`.
Normal tests and gates use `CARGO_TARGET_DIR=<worktree>/target`.
Mutation uses `CARGO_TARGET_DIR=<worktree>/target-mut` and
`TEMP=TMP=<worktree>/.codex-mut-tmp`. Intended budget: 1200 seconds per file,
one owned cargo-mutants process at a time. The timeout incident below violated
that intended serialization; affected results are rechecked separately.

```text
cargo mutants -p <crate> --gitignore true --baseline skip --timeout 90 -j 1 \
  --file "**/<source>.rs" [-F "<regex>"] \
  --output <worktree>/mut-out-<group> \
  --cargo-arg=--test=<binary> [--cargo-arg=--test=<additional-binary>]
```

`--gitignore true` is necessary here: the initial attempt used the installed
default (false), recursively copied the in-worktree scratch folder, and was
interrupted through its owning execution session before any build or verdict.
It is not counted as a mutation run. Temporary ignore entries for `target-mut`
and `mut-out-*` prevent copying artifacts. The repo's `tools/dev/mutants.sh`
still uses an outside-worktree target and `-- --test`; the explicit task
instructions override those defaults. No normal cargo test runs in target-mut.

Raw output: `mut-out-<group>/mutants.out/{outcomes.json,mutants.json,caught.txt,missed.txt,unviable.txt,timeout.txt}`.
These local artifacts and `.codex-*` files must not be staged.

### Interrupted sessions and timeout incident

The second user continuation interrupted zoom after three field mutants. The
resume filter enumerated the other names, but cargo-mutants generated the four
struct-update field deletions again despite the regex. Zoom's completed
`zoom-rest` set therefore supersedes those three duplicate outcomes; never add
the two sets together.

The zoom wrapper reached 1200 seconds and attempted `taskkill /PID <its owned
Cargo PID> /T /F`. The sandbox returned `ERROR: Access denied`. The wrapper
incorrectly proceeded to order while the zoom child still ran, and zoom later
printed `45 mutants tested in 22m: 8 missed, 36 caught, 1 unviable`.
This is **not** a compliant 20-minute serialized run. The final two zoom
outcomes and the first order run are not used as clean serialization proof.
The remaining owned PIDs were confirmed absent on the next continuation.
Subsequent runs use one execution session per file (no automatic batch), with
`--build-timeout 90` as well as `--timeout 90`, and are monitored by the owner.
Order is rechecked from scratch; the final timer mutants are rechecked using
the new killers binary. The report retains the incident instead of relabeling
the original run as clean.

Removing the initial aborted recursive source copy was blocked by the tool's
policy. It remains in `.codex-mut-tmp`; no alternate deletion was attempted.

## Completed selected runs (initial outcomes)

Counts distinguish full-file inventory, selected inventory and completed
outcomes. Missed includes equivalent mutants; it is never silently counted as
caught. Partial selections cannot establish a full-file verdict.

| Group | File inventory | Selected | Tested | Caught | Missed | Unviable | Timeout |
|---|---:|---:|---:|---:|---:|---:|---:|
| scaling | 23 | 23 | 23 | 20 | 2 | 1 | 0 |
| image-layer | 10 | 10 | 10 | 9 | 0 | 1 | 0 |
| click | 9 | 9 | 9 | 6 | 0 | 3 | 0 |
| pan (initial + resumed, disjoint) | 66 | 56 | 56 | 53 | 1 | 2 | 0 |
| button | 71 | 56 | 56 | 56 | 0 | 0 | 0 |
| zoom (superseding interrupted run; incident above) | 89 | 45 | 45 | 36 | 8 | 1 | 0 |
| order (clean `order-verify` rerun only) | 15 | 15 | 15 | 14 | 0 | 1 | 0 |
| pace | 68 | 58 | 58 | 58 | 0 | 0 | 0 |
| backoff | 41 | 30 | 30 | 30 | 0 | 0 | 0 |
| policy | 111 | 82 | 82 | 79 | 2 | 1 | 0 |
| pacer | 23 | 18 | 18 | 17 | 0 | 1 | 0 |
| stats | 55 | 36 | 36 | 34 | 2 | 0 | 0 |
| frame-log | 10 | 5 | 5 | 5 | 0 | 0 | 0 |
| **Total before killers** | **591** | **443** | **443** | **417** | **15** | **11** | **0** |

After applying only the isolated new-binary reruns to matching mutant names:

| PARITY row | Tested / full inventory | Caught | Missed | Unviable | Timeouts | Not run |
|---|---:|---:|---:|---:|---:|---:|
| X-viewing-pure | 92 / 121 | 85 | 4 equivalent | 3 | 0 | 29 |
| X-pointer-click-pan | 65 / 75 | 60 | 0 | 5 | 0 | 10 |
| X-overlay-button | 56 / 71 | 56 | 0 | 0 | 0 | 15 |
| I-11 (order, pace, backoff, policy) | 185 / 235 | 181 | 2 equivalent | 2 | 0 | 50 |
| X-zoom-detail-loader | 45 / 89 | 42 | 1 equivalent + 1 diagnostic-only | 1 | 0 | 44 |
| **Total after killers** | **443 / 591** | **424** | **8 (7 equivalent, 1 diagnostic-only)** | **11** | **0** | **148** |

Seven observable initial survivors were killed (one pan, six zoom). The ten
mutants in the final isolated reruns include three already-caught controls;
they are not ten additional mutants in the source inventory. All five named
PARITY rows remain **Partial**: the full inventory was not exhausted, zoom has
an unverified elapsed-log diagnostic, and the existing native/presenter gaps
were not verified by this task. No row was promoted from a partial mutation
selection to a complete/full-file verdict.

### Exact selection and commands

Use the source/test mapping above with the command template. Group names are
the output suffixes. The initial selection regexes were:

- `scaling`, `image-layer`, `click`, `order-verify`: no `-F` (all listed mutants of each small source file).
- `pan`: `scroll_bounds|on_image_press|on_image_move|on_image_release|try_pan_by_arrow|on_kinetic_frame`.
- `button`: `::click|::set_content|::set_style|::ensure_layout|::measure_core|::render_core`.
- `zoom`: `::reset|::set_zoom|::refresh|::on_preview_presented|::update|::on_original_decoded|::cancel_pending|::on_indicator_elapsed` (plus the four automatically included struct-update field mutations; see incident above).
- `pace`: `::record|::get_lead|::get_viewer_start_delay|::active_interval_ms|ticks_to_ms`.
- `backoff`: `::begin_pass|::note_busy|::record_success|::clear|::should_skip|::record|::mean_bytes`.
- `policy`: `forget_unless_failed|parse_diag_workers|in_preload_window|is_still_wanted|viewer_busy_worker_limit|worker_limit|resume_check_delay|resume_max_load|fill_limit_reached|evaluate_resume|has_headroom_cached`.
- `pacer`: `::mode_for|::should_defer|::observe_present`.
- `stats`: `ticks_to_ms|::record|::reset|::is_slow|percentile_of`.
- `frame-log`: `::format|::record`.

The pan continuation enumerated the remaining 27 mutant names with an escaped
regex. The full original selected inventory remains in `mut-out-pan/mutants.out/mutants.json`.
Zoom's superseding inventory is `mut-out-zoom-rest/mutants.out/mutants.json`.
All 148 generated mutants outside these actual inventories were **NOT RUN**.
They include getters/constructors/formatters, lock/clock/thread helpers, window
composition/lifetime glue and additional W9 retarget rules; some are pure code,
so this is explicitly not a full pure-source verdict. Native/window glue and
real Recycle Bin operations were excluded independently of these counts.

The later single-file runs added `--build-timeout 90`; platform runs used
`--build-timeout 300` for the cold Windows build. Test timeout remained 90
seconds and mutation parallelism remained `-j 1`.

New-binary-only commands (same environment, output paths under this worktree):

```text
cargo mutants -p pr-app --gitignore true --baseline skip --timeout 90 --build-timeout 90 -j 1 --file "**/input/pointer/pan.rs" -F "pan.rs:149:28:" --output <worktree>/mut-out-pan-kill-final --cargo-arg=--test=w8_mutation_killers
cargo mutants -p pr-app --gitignore true --baseline skip --timeout 90 --build-timeout 90 -j 1 --file "**/coordinators/zoom_detail.rs" -F "zoom_detail.rs:632:57:|zoom_detail.rs:705:" --output <worktree>/mut-out-zoom-kill --cargo-arg=--test=w8_mutation_killers
```

The latter regex also causes the four retarget fields to be generated by
cargo-mutants 27.1.0; all eight mutants were caught by the new binary alone.

### Last summary lines from mutation commands

| Command / group | Last summary line |
|---|---|
| scaling | `23 mutants tested in 5m: 2 missed, 20 caught, 1 unviable` |
| image-layer | `10 mutants tested in 2m: 9 caught, 1 unviable` |
| click | `9 mutants tested in 2m: 6 caught, 3 unviable` |
| pan initial | Interrupted after 29/56; no terminal summary. |
| pan-rest | `27 mutants tested in 4m: 27 caught` |
| button | `56 mutants tested in 7m: 56 caught` |
| zoom-rest | `45 mutants tested in 22m: 8 missed, 36 caught, 1 unviable` (timeout/overlap incident; affected outcomes rechecked). |
| order-verify | `15 mutants tested in 2m: 14 caught, 1 unviable` |
| pace | `58 mutants tested in 7m: 58 caught` |
| backoff | `30 mutants tested in 4m: 30 caught` |
| policy | `82 mutants tested in 8m: 2 missed, 79 caught, 1 unviable` |
| pacer | `18 mutants tested in 6m: 17 caught, 1 unviable` |
| stats | `36 mutants tested in 10m: 2 missed, 34 caught` |
| frame-log | `5 mutants tested in 85s: 5 caught` |
| pan-kill-final | `2 mutants tested in 34s: 2 caught` |
| zoom-kill | `8 mutants tested in 2m: 8 caught` |

The original pan execution session was interrupted after 29/56 outcomes.
The recorded Cargo PID was absent on resumption. `pan-rest` ran precisely the
27 names in `mutants.json` without an outcome in the first run: all 27 caught.
These counts are combined only because the name sets are disjoint. The ten
unselected pan mutants are not covered by this verdict.

`outcomes.json.total_mutants` is the number of completed outcomes while a run
is active in cargo-mutants 27.1.0. The selected denominator above is taken from
the initial `mutants.json` inventory, not that moving counter.

## Survivor dispositions

Source lines below refer to head `83da367f`; production source is unchanged.

- `scaling_policy.rs:47:32`, `||` to `&&`: equivalent. Both nonpositive sides
  still return false. Exactly one nonpositive source side produces a nonpositive
  numerator with valid positive destination sides, so `ratio > 4` is false;
  invalid destination sides are rejected by the next guard. Source products are
  computed in f64 and cannot overflow for i32 inputs.
- `scaling_policy.rs:52:9`, `||` to `&&`: equivalent. This combines the two NaN
  tests with `&&`, while leaving the nonpositive destination guards intact. An
  otherwise unguarded single NaN propagates into the ratio; comparison with 4 is
  false, matching the original early return. Both NaNs still return early.
- `scaling_policy.rs:24:5`, `choose` to `Default::default()`: unviable, E0277:
  `ImageInterpolation` does not implement `Default`. No test-kill claim.

The scaling guard behavior was cross-checked read-only with C#
`7eaee2a2:src/PhotoReview.Shell.Win32/Viewing/ScalingPolicy.cs`, lines 34-46.

- `pan.rs:149:28`, `-` to `/`: observable survivor. A purely vertical drag from
  `(200, 200)` to `(200, 220)` or `(200, 180)` must cross the drag threshold.
  Dividing the y coordinates instead of subtracting them reports about one DIP
  and incorrectly leaves click-zoom armed. Added
  `vertical_drag_crosses_threshold_from_a_nonzero_origin` in
  `crates/pr-app/tests/w8_mutation_killers.rs`. The first survivor rerun, using
  only `--cargo-arg=--test=w8_mutation_killers -F 'pan.rs:149:28:'`, caught it.
  That line filter also selected `-` to `+`, which survived the drag-only test.
  Added `vertical_jitter_below_threshold_is_not_a_drag` to guard the opposite
  boundary: y displacement of one DIP at a large origin is not a drag.
  Both tests pass on the unchanged production code. Final rerun
  `pan-kill-final`, using only `w8_mutation_killers`, tested 2/2: both caught,
  zero missed/unviable/timeouts.

The image-layer `&&` to `||` at line 53 is unviable: Rust's let-chain syntax
cannot use `||` there. The three click-zoom `Default::default()` replacements
are unviable because `PointerStep` has no `Default` implementation.
All eleven unviable outcomes were checked against their compiler logs: the
remaining `Default` errors name `ScrollBounds`, `PointerRelease`,
`ZoomDetailRefreshResult`, `PreloadWindow`, `ResumeVerdict` and `PresentMode`.
None of the eleven is an environment/build-cache failure.

- `zoom_detail.rs:306:13`, `307:13`, `308:13`, `309:13`: deleting `id`,
  `token`, `path`, or `key` from the `Target { ..previous.clone() }` update
  preserves the old value. These are observable rename survivors. They were
  generated despite a function regex that does not name `retarget_path` and
  are included in the actual selected inventory, not discarded as out of scope.
  Added `retarget_drops_the_old_completion_and_decodes_the_renamed_file` to
  the new pr-app killers binary. It completes the old decode without pumping,
  renames a TempRoot-owned file, advances the navigation, and verifies the old
  completion is discarded before running the newly queued decode. It then
  checks a held original and `OriginalShown` for the new path. The fake decoder
  and queued runner use no sleeps. Original-source test passes. `zoom-kill`,
  running only `w8_mutation_killers`, caught all eight selected mutants: these
  four fields, the error sentinel, and three indicator operators. Zero missed,
  unviable or timeouts in this isolated rerun.
- `zoom_detail.rs:442:30`, `||` to `&&`: equivalent under the loader's
  single-UI-thread state invariant. `refresh` returns early if already showing
  original, sets `refresh_hold = true`, captures whether original is held, then
  calls `update`. With a current target and a held original, `update` shows it;
  without one it cannot set `showing_original` (decode completions are applied
  only by `pump`). Thus both operands agree at this branch. Navigation cannot
  change in the middle of this UI-thread operation.
- `zoom_detail.rs:632:57`, deleting unary `-` from the absent-target token
  `-1`: killed by `stale_decode_failure_logs_the_absent_target_sentinel`.
  A fake decoder fails, its completion is queued, reset removes the target,
  and pump logs the exact sentinel into an in-memory `AppLogTarget`. The test
  serializes access to the process-global logger and restores its old target
  with a drop guard; it never enables or writes the real log.
- `zoom_detail.rs:705:57`, `&&` to `||`: killed by
  `elapsed_indicator_from_an_old_slot_does_not_show_the_new_indicator`. The fake
  timer returns immediately but its old message remains unpumped until a new
  slot exists. The old message must not show the new indicator. The test then
  runs the current timer and requires a visible effect, also catching `==` to
  `!=` and deleting `!` at the same line (the final two original outcomes
  affected by the overlap incident).
- `zoom_detail.rs:604:36`, `==` to `!=` in the diagnostic `elapsed_ms` lookup:
  **unobserved diagnostic-only survivor, not equivalent**. State/effects still
  use the independent `slot_owner` check at line 600. The mutation changes the
  ready log's duration, often to zero; the loader's `Instant` is not injectable.
  The fake logger makes text observable, but cannot make a nonzero elapsed
  duration deterministic without a real-time delay/performance assumption.
  No sleep, polling assertion or production clock-seam change was introduced.
  This remains an explicit verification gap, so zoom cannot be called clean.

The five new pr-app tests pass on the original source. New-test-only reruns:
`pan-kill-final` 2/2 caught; `zoom-kill` 8/8 caught. These reruns are not added
to source inventory totals. Six observable original zoom survivors are killed;
one equivalent and one diagnostic-only survivor remain.

The first order run and its five-mutant continuation are superseded entirely
by `order-verify`: 15/15 tested, 14 caught, 1 unviable, zero missed/timeouts.
The earlier run reported extra unviable outcomes during the overlap; those are
not counted or explained as legitimate compiler-equivalent mutations.

- `preload/policy.rs:144:30`, `>` to `>=` in `is_still_wanted`: equivalent.
  `ahead == 0` has already returned false at lines 140-142, so the predicates
  agree on every value reaching line 144 (including wrapping i32 values).
- `preload/policy.rs:179:14`, `<` to `<=` in `resume_check_delay`: equivalent.
  At equality both branches return the same maximum duration; otherwise the
  predicates agree. The powers-of-two delay sequence does not hit exactly
  30 seconds anyway. These two policy survivors are not counted as caught.
- `frame_stats.rs:157:52`, `-` to `+` or `/` in `min(count - 1)`:
  equivalent. At this point `count > 0` and `count <= CAPACITY` (2048).
  Clamped percentile gives a finite rank between zero and `count - 1` after
  the saturating subtraction; NaN uses the earlier rank-zero branch. Raising
  the upper bound to `count + 1` or `count / 1` cannot alter that rank, and the
  addition cannot overflow at this bounded count.

Preload scope total: 185/235 listed mutants tested (181 caught, 2 equivalent
missed, 2 unviable, 0 timeouts); 50 not selected. Existing test binaries caught
all observable selected mutants, so no pr-imaging killers file was necessary.

## Gates, command tails and limits

Every normal build/test uses `CARGO_TARGET_DIR=<worktree>/target` with
`CARGO_INCREMENTAL=0` and `CARGO_PROFILE_DEV_DEBUG=line-tables-only`.
The new binary was rerun after mutation finished; its five tests still pass.

| Command | Result / last nonempty output line |
|---|---|
| `cargo fmt -p pr-app --check` | PASS, exit 0, no formatter output (`FMT_pr-app_EXIT=0` in the wrapper). |
| `cargo fmt -p pr-imaging --check` | PASS, exit 0, no formatter output (`FMT_pr-imaging_EXIT=0`). |
| `cargo fmt -p pr-platform-win --check` | PASS, exit 0, no formatter output (`cargo fmt -p pr-platform-win --check: exit=0`). |
| `cargo clippy -p pr-app --all-targets --locked -j 2 -- -D warnings` | PASS, `Finished dev profile [unoptimized + debuginfo] target(s) in 58.45s`. |
| `cargo clippy -p pr-imaging --all-targets --locked -j 2 -- -D warnings` | PASS, `Finished dev profile [unoptimized + debuginfo] target(s) in 28.58s`. |
| `cargo clippy -p pr-platform-win --all-targets --locked -j 2 -- -D warnings` | PASS, `Finished dev profile [unoptimized + debuginfo] target(s) in 3m 07s`. |
| `cargo test -p pr-app --locked --test=w8_mutation_killers` | PASS, `test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s`. |
| `python -I tools/dev/doc-links.py --quiet` | PASS, exit 0: `doc-links: files=450 errors=0 proposed=171 archive_unchecked=14 csharp_refs=503 runtime_paths=13 placeholders=104`. |

The baseline command for pr-app selected all seven existing binaries listed in
the scope table; all passed (its last binary, `zoom_detail`, reported 43 passed,
0 failed, 0 ignored). The pr-imaging baseline selected `preload_order`,
`navigation_pace`, `verify_navigation_pace`, `preload_backoff`, `preload_policy`:
70 passed, 0 failed, 1 ignored in aggregate (last binary: 3 passed, 1 ignored).
The platform baseline selected `shell_viewing_frame_stats`,
`shell_viewing_pacer`, `shell_viewing_frame_log`, `shell_viewing_golden`,
`verify_w8_16a_diff`: 29 passed, 0 failed, 0 ignored (last binary: 3 passed).
Each baseline was guarded by a 1200-second command timeout.

**NOT RUN:** the 148 unselected mutants; additional whole-file/workspace
mutation; workspace fmt/Clippy/tests; ignored/native/window/COM/GPU tests;
RAW corpus; C# oracle regeneration; GUI/attended, soak, performance or real
user-data/Recycle Bin operations. No `--lib` mutation run was needed because
the thirteen selected files have no `#[cfg(test)]` unit modules. The elapsed
diagnostic clock gap remains unverified. No production code, C# files, assets,
dependencies, commits, pushes or PRs were changed/created.

Files for review: the new pr-app killers binary, this report, exactly five
PARITY rows, one current-state line in `task_on_progress.md`, and two ignore
patterns keeping `target-mut` / `mut-out-*` artifacts out of Git staging.
The overlay row's stale renderer-address note was corrected to
`text_renderer_id`, as proved by its existing fake-renderer test; native
integration was not inferred from that correction.

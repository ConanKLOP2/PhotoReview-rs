# CL-MUT-STATE2: killers for the `state.rs` survivors of X-w9r6-rename-view-retarget

Status: report, 2026-10-31 (lane cl-mut-state2). Source list: `CL-MUT-MISC-2026-10-31.md` (PR #512), section X-w9r6-rename-view-retarget (26 survivors of `crates/pr-app/src/coordinators/presenter/state.rs`).

## Method

- New test binary `crates/pr-app/tests/mut_state2_killers.rs`: 12 tests, fakes only (`PresenterFixture`, `ScriptedFs`), no fixed sleep. Hang-prone paths run in a thread with `mpsc::recv_timeout` (`guarded`), so an endless `pump` loop fails the test instead of timing out the run.
- Preflight: `cargo test -p pr-app --test mut_state2_killers -- --include-ignored --test-threads=1`: `running 12 tests`, 12 passed.
- Rerun of exactly the survivor positions: `cargo mutants -p pr-app -f .../presenter/state.rs -F 'state\.rs:(91:21|131:9|...|840:9):' -j 1 --timeout 300 --baseline skip --cargo-test-arg=--test --cargo-test-arg=mut_state2_killers --cargo-test-arg=-- --cargo-test-arg=--include-ignored --cargo-test-arg=--test-threads=1` (CARGO_PROFILE_DEV_DEBUG=0, own target dir).
- Only `mut_state2_killers` runs in the mutated build, so "caught" here means this binary alone detects it (stronger than the previous pass, which let other binaries catch). cargo-mutants does not copy `tests/golden`, none is used.

## Result (real numbers)

33 mutants tested in 22 min (all variants at the 25 survivor positions): 18 caught, 14 missed, 1 unviable (`+ -> *` on `Instant`), 0 timeout. Of the 26 original survivors: 15 killed, 11 not killable by a black-box test (equivalent or unreachable), 0 need a person.

Earlier attempt (first version of the tests) left `pump` 724:65 `>=` and `==` as TIMEOUT because other tests of the binary hung inside `pump`; fixed by running every pumping test through `guarded`, then all three `724:65` variants are caught.

### Killed (15)

| Mutant | Killer test |
|---|---|
| 91:21 `bitor` `\|` -> `^` (and `&`) | `bitor_unions_the_flags_and_is_idempotent` |
| 131:9 `Display for PresentError` | `present_error_display_is_the_csharp_argument_exception_text` |
| 194:9 `Debug for ImagePresenterDeps` | `deps_debug_names_the_struct` |
| 506:9, 509:56 `Debug for ImagePresenter` (`busy` `\|\|` -> `&&`) | `presenter_debug_reports_the_path_the_status_and_busy` |
| 604:9 `clock()` | `clock_returns_the_injected_generation_clock` |
| 634:9 `is_same_source_swap -> true` | `pump_applies_a_zoom_only_completion_and_clears_the_swap_flag` |
| 696:12 `clear_presentation` `delete !` | `clear_presentation_reports_the_compare_changes` |
| 724:65 `pump` `>` -> `>=`, `==`, `<` | `pump_on_an_idle_presenter_returns_empty_changes` (`>=`, `==`: endless loop), `pump_applies_a_zoom_only_completion_...` (`<`) |
| 751:39 `wait_for_event` `+ -> -`; 759:31 `>= -> <`; 753:75 `> -> ==`, `>=` | `wait_for_event_without_events_waits_the_whole_timeout_then_reports_false`, `wait_for_event_wakes_on_a_worker_completion_...` |
| 803:58 `== -> !=`, 803:67 `&& -> \|\|` (dispatch, `StatAbandoned` of `NextForMissing`) | `abandoned_next_stat_ends_the_pending_missing_run`, `abandonment_of_an_older_missing_run_leaves_the_newer_run_pending` (a stat that panics in the worker, older run abandoned while a newer run is pending) |

### Not killable (11 survivors, with reason)

- 634:9 `is_same_source_swap -> false`: the flag is true only inside the synchronous sink callback of a zoom swap and the sink cannot reach the presenter, so no code outside can read `true` from the getter. The sink observes it through `ImageChange` (asserted). Equivalent for any caller. Could be killed only with a unit test inside `zoom.rs` that reads the field.
- 753:39 `|| -> &&`, 753:75 `> -> <`, 756:45 `|| -> &&` (`wait_for_event`): the first `if` is a fast path; both inboxes' `wait_ready` return true whenever their queue is non-empty, and the loop re-checks the fast path on the next pass, so the result is the same. They differ only by one 2 ms slice in a race at the exact deadline. Equivalent.
- 821:12 `delete !`, 827:9 (`true`, `false`), 827:62, 828:13, 831:46, 831:55, 832:13 (`dispatch` / `has_wait_for`), 836:9 `retire_child_waiters -> ()`, 840:9 `track_child_present -> ()`: they only matter when a parent token is in `retired_waits` AND has a child link in `retired_parent_by_child`. A token enters `retired_waits` only through `track_superseded_navigation`, which takes `navigation` while it is still `Some`; a child link is created (`present_b.rs` stale-NotFound retry, `missing.rs` next present) only after `navigation` was taken for that parent. So the two sets never intersect through the public API and the chain cleanup (also unit-tested in `retired_wait_tests` for `retire_wait_chain`) is unreachable with the current flows. Equivalent for black-box tests; if the owner wants them killed, the options are an in-crate test that seeds both maps directly (`pub(super)` access), or removing the dead link tracking. Proposed decision: leave as is, record as "dead infrastructure, no behaviour".

## Suggested PARITY changes (PARITY.md and task_on_progress.md NOT edited)

- X-w9r6-rename-view-retarget: keep Partial in the sense of "mutation": `state.rs` survivors 26 -> 11 (all 11 argued equivalent / unreachable, none need a person). No "Done" upgrade from this lane alone.

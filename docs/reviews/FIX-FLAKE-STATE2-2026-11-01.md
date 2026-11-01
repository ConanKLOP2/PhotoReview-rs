# FIX-FLAKE-STATE2 (2026-11-01)

Test: `wait_for_event_wakes_on_a_worker_completion_and_keeps_reporting_it_until_pumped` (`crates/pr-app/tests/mut_state2_killers.rs`, PR #514). Flaky on CI (PR #548, job test (app)): panic at `assert!(!f.presenter.wait_for_event(Duration::ZERO), "consumed by the pump")`.

## Root cause (test, not production)
With the default `LoadingMode::Preview` a present starts two workers: the viewer preview and the thumbnail (`present_a.rs`, THUMBNAIL_WAIT_THREAD). The present ends on whichever result wins; `is_idle()` is then true and `pump_until_idle` returns. The losing worker posts its `Thumbnail`/`Preview` event to the inbox afterwards (stale token), so `wait_for_event(ZERO)` can legitimately be true. Not a production race: late stale events are expected and discarded on the next pump.

## Fix
The test sets `loading_mode = Fast` (one job: the preview worker only, see `presenter_present_a.rs` line 671, Preview = 2 jobs, Fast = 1). No other worker exists, so after `pump_until_idle` the inbox is deterministically empty. Assertions unchanged, so the killed mutants (constant true/false, queued-events fast path) are still killed; no sleep or retry added.

## Evidence
`cargo test -p pr-app --test mut_state2_killers -j 2` x30 (every third iteration with 4 extra parallel cargo test runs as load): 30/30 pass. Baseline flake rate before the fix was not measured (CI-only failure).

PARITY: no row change.

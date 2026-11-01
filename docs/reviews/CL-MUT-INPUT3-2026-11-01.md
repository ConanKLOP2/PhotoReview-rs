Status: Partial - surface.rs shard 0/6 done (21/21, 20 caught, 1 unviable, 0 missed); shards 2/6..5/6 (84) NOT run.

# CL-MUT-INPUT3 2026-11-01

Lane `feat/cl-mut-input3`. Continues [CX-INPUTMUT2](CX-INPUTMUT2-2026-10-31.md). No production or test code changed.

## Setup

Local script (mutants.sh has no `-F`/`--shard` pass-through; not modified), same flags as the common rules:
`cargo mutants -p pr-platform-win --baseline skip --timeout 300 -j 1 --no-shuffle --file **/shell_input/surface.rs --shard 0/6`
with `--cargo-test-arg=--test --cargo-test-arg=shell_input_surface --cargo-test-arg=-- --cargo-test-arg=--include-ignored --cargo-test-arg=--test-threads=1`,
`CARGO_PROFILE_{DEV,TEST}_DEBUG=0`, `PHOTOREVIEW_GOLDEN_ROOT`, isolated config/data root, one cargo-mutants at a time.
Preflight (unmutated): `shell_input_surface` `running 29 tests`, 29 passed, exit 0. Shards are numbered from zero.

## Results (real numbers)

| File / selection | Listed | Tested | Caught | Missed | Unviable | Timeout | Not run |
|---|---:|---:|---:|---:|---:|---:|---:|
| surface.rs shard 0/6 | 21 | 21 | 20 | 0 | 1 | 0 | 0 |
| surface.rs shards 2/6, 3/6, 4/6, 5/6 | 84 | 0 | 0 | 0 | 0 | 0 | 84 |
| handlers.rs shard 1/3 | 14 | 0 (earlier CL-INPUTDONE run, not repeated) | - | - | - | - | - |
| native.rs shards 1/3, 2/3 recheck | 34 | 0 (not rechecked) | - | - | - | - | - |

- The 20 caught logs show `running 29 tests` and assertion failures; no log contains `no tests/golden above`; golden-only catches: 0.
- The one unviable mutant: surface.rs:118:9 `extent_width -> f64 with 0.0` (compile failure, no test ran). It is neither caught nor equivalent.
- No survivor, so no killer file was needed (`w8_inputmut3_killers.rs` not created; nothing added to native-nightly.yml).
- Cost: 21 mutants took about 2 hours (about 5 min each under 8 parallel lanes). Shard 2/6 was started at 13:00 and stopped by killing only my own PIDs (no verdict). The time-box was exceeded by shard 0 alone.

## PARITY proposal

X-input-viewport-surface stays Partial: 126 listed, 21 (old shard 1/6) + 21 (shard 0/6) tested, 84 not run. No PARITY raise.

## Artifacts for the orchestrator to delete

Worktree `C:\MyProjects\PhotoReview-rs\.claude\worktrees\cl-mut-input3`: `target-mut`, `mut-out-surf0`, `mut-out-surf2` (partial), `mut-surf0.log`, `mut-data`.

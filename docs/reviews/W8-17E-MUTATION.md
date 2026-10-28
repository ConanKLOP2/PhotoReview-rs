# W8-17e postmerge mutation evidence (pure startup logic)

Status: complete for the five files below. Run 2026-10-29 at master `cc3e183c` (unit W8-17e was ported in PR #294 without a mutation run). Machine A, claim W8-17e-mutation. No production or test code changed.

## Result

Every cargo-mutants candidate of the five files was run (no filter, no time-box expiry). **23 tested / 23 total: 18 caught, 0 missed, 5 unviable, 0 timeouts.**

| File | Test binary | Total | Caught | Missed | Unviable | Timeouts | Time |
|---|---|---:|---:|---:|---:|---:|---:|
| `startup/crash.rs` | `startup_crash` | 8 | 6 | 0 | 2 | 0 | 86 s |
| `startup/recovery.rs` | `startup_recovery` | 5 | 4 | 0 | 1 | 0 | 45 s |
| `startup/shutdown.rs` | `startup_shutdown` | 4 | 4 | 0 | 0 | 0 | 58 s |
| `startup/smoke.rs` | `startup_smoke` | 4 | 3 | 0 | 1 | 0 | 41 s |
| `startup/placement_read.rs` | `startup_placement_read` | 2 | 1 | 0 | 1 | 0 | 18 s |
| **Total** | | **23** | **18** | **0** | **5** | **0** | |

Unviable (the replacement does not compile because the return type has no `Default`): `LoopFaultCounter::record` and `record_default` -> `LoopVerdict` with `Default::default()`; `prompt_for_failed_entries` -> `RecoveryPromptOutcome` with `Default::default()`; `smoke_trigger` -> `SmokeTrigger` with `Default::default()`; `read_saved_placement` -> `Some(Default::default())` (`WindowPlacementData` has no `Default`). These cannot be observed by any test.

## Survivors

None, so no `startup_mutation_killers.rs` was added (a file with no purpose would only be noise). Production behaviour was not changed and no C# cross-check was needed.

## Not run

Only these five files. Not covered: the other `startup/*` modules (`context`, `early_decode`, `error_log`, `forwarded_open`, `instance`, `launch`, `options`, `settings_load`, `ui_hop`), `pr_shell::startup::*` window glue and every native/FFI path. This is not a verdict for the whole W8-17e unit. Each file ran only against its own `startup_*` test binary; mutants that other binaries would catch count as caught anyway, so the numbers are not inflated.

## Command

```
cargo mutants -p pr-app --baseline skip --timeout 90 -j 1 --file '**/startup/<file>.rs' --cargo-arg=--test=startup_<file> -o <out>
```

`-j 1`; env `CARGO_INCREMENTAL=0`, `CARGO_PROFILE_DEV_DEBUG=line-tables-only`; two sequential streams (crash, recovery, placement_read / shutdown, smoke), each with its own `CARGO_TARGET_DIR` outside the worktree.

Status: evidence report (2026-11-03), lane stj-mut. Complete for the 8 `+= 1` -> `*= 1` mutants of `mod json`; PARITY status NOT raised.

# X-stj-reader: cargo-mutants rerun of the 8 former timeouts

File: `crates/pr-core/src/settings/app_settings.rs` (`mod json`). The 8 mutants are the ones listed as "unresolved timeouts" in `CX-JSON4-2026-10-30.md` (section "Remaining gaps") and `CX-SURV-2026-10-30.md`; `ST-EARLY-STJ-2026-10-31.md` applied them by hand against `w3_stj_timeout_killers`. This report is the first real cargo-mutants run of them. Tool: cargo-mutants 27.1.0.

## Result (from `mutants.out/outcomes.json` via `tools/dev/mutants.sh`)

| Tested / total | Caught | Missed | Unviable | Timeouts | Not run |
|---|---|---|---|---|---|
| 8 / 8 | 8 | 0 | 0 | 0 | 0 |

Preflight (unmutated test command): `running 4 tests`, exit 0. Whole run 4 minutes, `-j 1`, `--baseline skip`, `--timeout 180`, budget 1200 s. The 180 s harness timeout was never reached.

## Per-mutant outcome

| Mutant | Function | Outcome | Build | Test |
|---|---|---|---|---|
| `:1146:26` `+= -> *=` | `Parser::skip_plain_ws` | caught | 23.6 s | 10.2 s |
| `:1179:53` `+= -> *=` | `Parser::skip_ws` (block comment body) | caught | 23.5 s | 10.5 s |
| `:1228:34` `+= -> *=` | `Parser::parse_number` | caught | 19.6 s | 10.8 s |
| `:1239:30` `+= -> *=` | `Parser::parse_number` | caught | 19.8 s | 10.3 s |
| `:1251:30` `+= -> *=` | `Parser::parse_number` | caught | 19.5 s | 10.7 s |
| `:1494:15` `+= -> *=` | `is_decimal_literal` | caught | 18.4 s | 10.3 s |
| `:1501:19` `+= -> *=` | `is_decimal_literal` | caught | 20.8 s | 10.2 s |
| `:1516:19` `+= -> *=` | `is_decimal_literal` | caught | 20.1 s | 10.6 s |

## What "caught" means here (honest reading)

Each mutant leaves the cursor stationary, so the mutated parser loops forever. With the earlier test binaries only, cargo-mutants reported these as timeouts. With `w3_stj_timeout_killers` (4 tests) each input runs on a worker thread and the test waits with `mpsc::recv_timeout(10 s)`; a miss panics ("parser did not finish"). cargo-mutants therefore saw a failing test (exit non-zero after about 10 s), not a harness kill, and recorded `CaughtMutant`. This is a bounded-wait (timing) guard: deterministic and never fires on correct code (a correct parse takes microseconds), but it is not an assertion on a value. No production hook was added. The stuck worker thread is leaked until the test process exits.

Only `w3_stj_timeout_killers` was run (the other json test binaries `settings_app_settings`, `w2_json_killers*` were not rerun here); the claim is limited to these 8 mutants. The other mutants of the file (312 total, counts in CX-JSON4) are not rerun and this report says nothing new about them. Could a loop-guard test convert them further? Not needed: they are already converted; no test was added or changed (test-only change count: 0).

## Commands (from the worktree root, `PY_BIN` = Python 3.12, `CARGO_BUILD_JOBS=3`)

Selection check (exactly 8 names listed):

```
cargo mutants -p pr-core --list --file '**/settings/app_settings.rs' \
  -F 'app_settings.rs:(1146:26|1179:53|1228:34|1239:30|1251:30|1494:15|1501:19|1516:19):.*\*= '
```

Run:

```
TEMP=<repo>/.claude/target-mut-stj MUT_OUT=$TEMP/out MUT_TIMEOUT=180 bash tools/dev/mutants.sh pr-core \
  crates/pr-core/src/settings/app_settings.rs --tests w3_stj_timeout_killers --budget 1200 --jobs 1 \
  -F 'app_settings.rs:(1146:26|1179:53|1228:34|1239:30|1251:30|1494:15|1501:19|1516:19):.*\*= '
```

The target dir (`mut-target-pr-core` under that TEMP) was outside the worktree and deleted afterwards.

## Verdict

The 8 former timeouts are now 8 caught by cargo-mutants through a bounded-wait guard. The X-stj-reader row stays Partial: the 8 compile-unviable return replacements and the proven-equivalent survivors are unchanged, and the guard is timing-based.

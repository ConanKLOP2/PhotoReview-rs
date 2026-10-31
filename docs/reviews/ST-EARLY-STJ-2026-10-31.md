# ST-EARLY-STJ (2026-10-31)

Status: test-only lane (no production change). Rows covered: X-startup-early-decode, X-stj-reader.

## 1. X-startup-early-decode

C# source (`7eaee2a2:tests/PhotoReview.Shell.Integration.Tests/Startup/ShellStartupTests.cs`, `Startup_EarlyDecode_StartsOnlyAfterTheInstanceLockIsHeld`):
start the shell, wait for the first image, wait for the `earlyDecodeStarted` mark, assert order `instanceLock` < `earlyDecodeStarted`, close cleanly.

The Rust port already existed: `startup_early_decode_starts_only_after_the_instance_lock_is_held`, `#[ignore = "native: ... needs a desktop"]`.
It already runs through `rig::isolated_test`, which re-executes that one test in a child process with a unique instance prefix
(`PhotoReviewRsTest<temp name>`), `PHOTOREVIEW_RS_DATA_ROOT` + `PHOTOREVIEW_RS_ISOLATE_CONFIG=1` in a `TempRoot`, WARP/NOWARM, 300 s channel
bound, kill by exact PID only, no fixed sleep. So nothing needed porting; the gap was that it was never run on CI: it sat in `startup_native`,
which is not in `native-nightly.yml`, and that binary also holds a test that fails (below).

Real runs (this machine, desktop session, `-j 2`, `--include-ignored --test-threads=1`):

- `startup_native` whole binary: 2 passed, 1 FAILED. The failure is `startup_with_logging_off_writes_no_log_file`
  (`assertion failed: !process.root.path().join("data/logs/app.log").exists()`): the real exe writes `data/logs/app.log` although the C# test
  (INV-10, log off) forbids it. Not touched here; it is a separate finding (possible real bug or fixture env difference), not investigated.
- Change: the early-decode test moved to its own binary `crates/pr-shell/tests/startup_early_decode_native.rs` (same body, same rig) and the binary was
  added to the `pr-shell` list in `.github/workflows/native-nightly.yml`.
  `cargo test -p pr-shell --test startup_early_decode_native -- --include-ignored --test-threads=1`: 1 passed, 0.50 s.
- Nightly confirmation pending (windows-latest desktop/WARP not verified): remove it from the list if red only because of no desktop.

## 2. X-stj-reader (8 `*= 1` timeouts)

The eight mutants (CX-SURV / CX-JSON4): `app_settings.rs` 1146:26, 1179:53, 1228:34, 1239:30, 1251:30, 1494:15, 1501:19, 1516:19 (`+= 1` to `*= 1`,
a stationary cursor, infinite loop).

New `crates/pr-core/tests/w3_stj_timeout_killers.rs` (4 tests): each input reaches one loop and is parsed on its own thread; the result is awaited with
`mpsc::recv_timeout(10 s)` and a miss panics ("parser did not finish"). No sleep, no production hook. A stuck worker is leaked until the test process exits.
Normal run: 4 passed, 0.00 s.

Hand-applied mutants (one at a time, source restored from a copy after each; `git status` shows no change in `app_settings.rs`):

| Mutant | Input | Failing test | Time |
|---|---|---|---|
| 1146:26 | ` true` | leading_whitespace_is_skipped_to_the_value | 10.0 s |
| 1179:53 | `/*x*/true` | block_comment_body_is_scanned_to_its_end | 10.0 s |
| 1228:34 | `1` | number_digit_runs_are_consumed_to_the_delimiter | 10.0 s |
| 1239:30 | `0.1` | same | 10.0 s |
| 1251:30 | `0e1` | same | 10.0 s |
| 1494:15 | string `"1"` | decimal_string_digit_runs_are_consumed_to_the_end | 10.0 s |
| 1501:19 | string `".1"` | same | 10.0 s |
| 1516:19 | string `"0e1"` | same | 10.0 s |

All 8 fail (3 other tests pass in each run). Caveat, stated honestly: this is still a timing guard (bounded wait), so a mutant is caught after about 10 s
instead of the 90 s cargo-mutants timeout; it is deterministic and never fires on correct code. cargo-mutants was NOT run, so no mutation-score claim.

## Checks run

`cargo fmt -p pr-core -p pr-shell --check` clean; `cargo clippy -p pr-core --test w3_stj_timeout_killers -- -D warnings` clean;
`cargo clippy -p pr-shell --test startup_early_decode_native --test startup_native -- -D warnings` clean.

## PARITY rows (not edited here)

- X-startup-early-decode: keep Partial; evidence: desktop run 2026-10-31 passes the process scenario (1/1); becomes Done only after one green native-nightly run.
  Note the red `startup_with_logging_off_writes_no_log_file` (separate row/lane).
- X-stj-reader: the "8 unresolved timeouts" can be restated as "8 killed by hand-applied mutants through a bounded-wait test (w3_stj_timeout_killers)";
  keep Partial until a cargo-mutants rerun confirms (not run).

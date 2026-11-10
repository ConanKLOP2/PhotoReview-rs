Status: evidence report (2026-11-03). Complete for the 27 mutants of the SEC-01 link-walk functions; PARITY status NOT raised.

# SEC-01 / F-05: mutation run of the manual link walk in `physical.rs`

Functions: `crates/pr-core/src/io/physical.rs` `final_link_target`, `final_link_target_with`, `final_link_target_for_tests`, `follow_link_chain`, `follow_link_chain_for_tests`, `immediate_link_target`, `strip_nt_prefix`, `is_path_unreachable` (lines about 970-1100, after PR #574). Tool: cargo-mutants 27.1.0. Pure tests: `w2_sec01_guard_killers`, `w2_sec_killers2`, `w2_sec_killers3`, `io_physical_seams` (17 tests that run without `--include-ignored`).

## Result 1: the pure tests (default `cargo test`)

| Tested / total | Caught | Missed | Unviable | Timeouts | Not run |
|---|---|---|---|---|---|
| 27 / 27 | 17 | 10 | 0 | 0 | 0 |

Preflight (unmutated): `running` 17 tests, exit 0. 8 minutes, about 15 s build per mutant, `-j 1`, budget 1800 s. Caught: the guard `is_path_unreachable(&err)` -> `true` (the old open survivor), every `is_path_unreachable` mutant, every `follow_link_chain` mutant (cap, hops, NotFound), the `final_link_target_with` body mutants.

## The 10 survivors

All are in code that only runs on a real reparse point, and every real-disk test of this repo is `#[ignore = "native"]` by convention (`io_physical_seams.rs` header); `immediate_link_target` and `strip_nt_prefix` are private and have no injection seam, so a pure test cannot reach them. No test was added (a seam would change `src`, outside this test-only task).

- `final_link_target` -> `Ok(String::new())` / `Ok("xyzzy")` (971:5), `immediate_link_target` -> `None` / `Some("")` / `Some("xyzzy")` (1042:5), `==` -> `!=` (1043:50), `strip_nt_prefix` -> `String::new()` / `"xyzzy"` (1066:5): 8 mutants, NOT killable by a pure test, KILLED by the hand-run native tests (Result 2).
- `&` -> `|` and `&` -> `^` (1043:19) in `if attributes & FILE_ATTRIBUTE_REPARSE_POINT == 0 { return None }`: equivalent in practice. The check is a shortcut: for a non-reparse entry `fs::read_link(path).ok()?` on the next line also gives `None`. With `|` the shortcut never fires (value is never 0), same result through `read_link`. With `^` it fires only when the attributes are exactly `0x400`: that is a file symlink with no archive bit; a junction always has `DIRECTORY` (0x10) set. Reaching it needs a file symlink (privilege or Developer Mode), not available here. Not killed.

## Result 2: hand-run native tests on the 16 mutants of `final_link_target|immediate_link_target|strip_nt_prefix`

Only `io_physical` tests named `resolve_real_path*`, `--include-ignored --test-threads=1`, skipping `dangling_chain_of_63` (9 tests, preflight ok). 16 / 16 tested, 11 caught, 5 missed, 0 unviable, 0 timeouts. The 8 survivors above ARE caught here. The 5 misses are the two `&` mutants (above) and three mutants (guard `true`, the two `final_link_target_for_tests` bodies) that this narrow test set does not exercise; the first two are caught by the pure set (Result 1), the `_for_tests` wrappers are caught in Result 1 as well (they are only called by the pure tests). Net: no mutant is missed by both runs except the two equivalent `&` ones.

Side finding: `io_physical::resolve_real_path_dangling_chain_of_63_junctions_resolves_to_the_missing_target` FAILS on this machine (Windows 10 19045) when run by hand, unmutated: `resolve_real_path` returns `Os { code: 1921 }` for a 63-junction dangling chain. Not investigated here (not a mutant effect; the test comment says the OS stops a chain at 63 reparse points, so the real limit on this build may be lower). It is why that test was skipped in Result 2.

## Commands (worktree root, PowerShell env of the repo: cargo-mutants and Python 3.12 on PATH, `CARGO_INCREMENTAL=0`, `CARGO_BUILD_JOBS=3`; `TEMP` pointed at a private dir because `mutants.sh` puts its target dir under `$TEMP`)

```
bash tools/dev/mutants.sh pr-core crates/pr-core/src/io/physical.rs \
  --tests w2_sec01_guard_killers,w2_sec_killers2,w2_sec_killers3,io_physical_seams \
  --budget 1800 --jobs 1 -F 'final_link_target|follow_link_chain|immediate_link_target|strip_nt_prefix|is_path_unreachable'

bash tools/dev/mutants.sh pr-core crates/pr-core/src/io/physical.rs --tests io_physical \
  --cargo-test-arg resolve_real_path --cargo-test-arg -- --cargo-test-arg --include-ignored --cargo-test-arg --test-threads=1 \
  --cargo-test-arg --skip --cargo-test-arg dangling_chain_of_63 --budget 1200 --jobs 1 -F 'final_link_target|immediate_link_target|strip_nt_prefix'
```

(Also run, informational: the 5-file pure set plus `io_physical` without ignored tests, 16 mutants, same 10 survivors: `io_physical` has no non-ignored link test.) Each runs `cargo mutants -p pr-core --baseline skip --timeout 90 -j 1 ...` after the script's own preflight.

## Verdict for SEC-01

The fail-closed guard, `is_path_unreachable` and the hop cap are mutation-clean. The remaining gap is structural: `immediate_link_target` / `strip_nt_prefix` / the `final_link_target` wrapper are only covered by hand-run native tests, and one of those (63 junctions) fails here. The SEC-01 row stays Partial.

## Update 2026-11-10 (task T4): the two `&` survivors were NOT equivalent; all 33 mutants of the resolver closed

Correction of the "equivalent in practice" claim above: `&` -> `^` fires for attributes exactly `0x400` (a file symlink without the archive bit) and then returns `None` where the real code follows `read_link`; `&` -> `|` makes a non-reparse entry with a readable target look like a link. Both change the result, so they were killable, not equivalent. No attribute enumeration can prove them equal because the function reads the disk.

Change: behaviour-neutral seam. `immediate_link_target` is now a wrapper over `immediate_link_target_with(path, attributes, read_link)` (same code, the two OS calls injected) plus the `#[doc(hidden)]` `immediate_link_target_for_tests`. New pure tests: `crates/pr-core/tests/w2_sec01_immediate_step.rs` (6 tests: exactly 0x400 followed, junction and archive symlink followed, entries without the reparse bit never reach `read_link`, missing entry or unreadable link is `None`, NT/UNC prefix stripping, relative targets). No privilege or real disk needed.

Runs (cargo-mutants, `-j 1 --baseline skip`, own target dir, unmutated preflight shown with `running N tests`, N > 0):

| Run | Tests | Tested / total | Caught | Missed | Unviable | Timeouts |
|---|---|---|---|---|---|---|
| A: `-F 'final_link_target\|follow_link_chain\|immediate_link_target\|strip_nt_prefix\|is_path_unreachable'`, pure tests (23 preflight tests, 17 min) | the 3 w2_sec tests + `w2_sec01_immediate_step` + `io_physical_seams` | 33 / 33 | 27 | 5 | 1 | 0 |
| C: the 5 A survivors, native `io_physical resolve_real_path*` with `--include-ignored --test-threads=1 --skip dangling_chain_of_63` (9 preflight tests) | `io_physical` | 5 / 5 | 5 | 0 | 0 | 0 |
| B: `file_exists_full` + `win32_file_exists` probe, native `io_physical exists` (4 preflight tests) | `io_physical` | 7 / 7 | 7 | 0 | 0 | 0 |

- The 5 run-A survivors are the thin wrappers `final_link_target` -> `Ok("")`/`Ok("xyzzy")` and `immediate_link_target` -> `None`/`Some("")`/`Some("xyzzy")`: they only act on a real reparse point, so only native tests reach them; run C catches all 5 (cargo-mutants does count the native binary once `--include-ignored` is passed in the correct form).
- The 1 unviable mutant is `==` -> `!=` at the reparse-bit test (cargo-mutants reports it unviable). Checked by hand: with `!= 0` applied, 5 of the 6 new tests fail, so the test set guards it; this is not a cargo-mutants verdict.
- The 8 earlier "native-only" survivors and the 2 `&` survivors are therefore all closed: no resolver mutant is missed by every run.
- Not done: the `dangling_chain_of_63_junctions` native test still fails here (1921, see above); skipped, not investigated. PARITY.md wording is unchanged (docs agent).

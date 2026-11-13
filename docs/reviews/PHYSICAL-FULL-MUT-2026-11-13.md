Status: evidence report (2026-11-13). Complete native mutation run of `crates/pr-core/src/io/physical.rs`, killers for the real survivors, and a confirmation rerun. Measured on one machine (Windows 11, NTFS).

# `physical.rs` complete mutation run (X-physical-fs)

Earlier lanes covered the file group by group (CL-PHYS3/4/5, CX-PHYSREST/2, PHYSICAL-LISTING, PHYSICAL-TAIL, SEC01-MUTATION). This is the first single run over every identity of the file with the correct native flag form.

## Result

Source after this change: 455 identities (`cargo mutants --list`). Master 8eed99ef had 446; the nine added ones belong to the new test seams and to the split of `resolve_if_reparse_point` (see "Production change").

| Outcome | Count | Notes |
|---|---|---|
| Caught | 405 | 373 in the full run, 8 former timeouts, 15 former survivors, 9 new identities (the last three groups by the confirmation rerun) |
| Unviable | 14 | do not compile |
| Not compiled on Windows | 15 | `#[cfg(not(windows))]` twins; cargo-mutants lists them, the build never contains them |
| Equivalent by code argument | 21 | no test can fail; argument per mutant below, no run proof |
| Timeout | 0 | |
| Missed with a real gap | 0 | |
| NOT RUN | 0 | |

A caught mutant has at least one failing test that is not a `golden_*` test (P-65); checked per log for the 41 mutants of the confirmation rerun.

## How it ran

1. Full run (master 8eed99ef, `tools/dev/mutants.sh pr-core crates/pr-core/src/io/physical.rs`, `--jobs 1`, own TEMP): test binaries `io_physical`, `io_physical_seams`, `w2_phys2/3/4/5_killers`, `w2_physrest_killers`, `w2_physrest2_killers`, `q3_physical_mkdir_killers`, `w2_sec01_guard_killers`, `w2_sec01_immediate_step`, with `-- --include-ignored --test-threads=1 --skip chain_of_6`. Preflight 105 tests. 446 tested: 373 caught, 51 missed, 14 unviable, 8 timeout.
2. Rerun of the mutants thought equivalent against the WHOLE `pr-core` suite without ignored tests (preflight 2526 tests, 40 mutants at 22 positions): 27 missed, 13 caught. The 13 are siblings at the same positions that the full run had already caught. No other `pr-core` test catches any of the 21 equivalents. (A first attempt with `--include-ignored` over the whole crate failed its preflight: `session_store::from_environment_constructs_and_names_a_session_file` fails by design without its environment. That attempt and a later stopped run of 65 mutants are not used.)
3. Confirmation rerun on this branch (the 12 binaries: the 11 above plus `w2_phys6_killers`, same native arguments, preflight 112 tests): 42 mutants, 41 caught, 1 missed, 0 timeout, 66 min. The missed one is `open_directory_handle` `|` -> `^` (equivalent, below).

`--skip chain_of_6` leaves out the two 63/64-junction tests (about 50 s each per mutant; the 63-junction one fails on this machine with OS error 1921, see SEC-01 / F-05).

## The eight timeouts

`replace_with_retry` 113:25 (guard -> `true`), 113:52; `match_pattern` 267:25, 297:36, 308:36 (`+=` -> `*=`); `create_directory_impl` 694:19 (`/=`), 696:15 (`+=`, `/=`). Each makes a loop spin for ever. Bounded killers existed, but `io_physical` runs first and hung, so cargo-mutants reported a timeout.

Fix in `tests/io_physical.rs`: a `bounded` helper runs the code under test on a worker thread and fails the test after 20 s (60 s for the two whole-suite wrappers `file_system_contract_physical` and `golden_physical_fs_ops`, which take 0.2 s and 2.3 s unmutated). The fake rename closures of the three retry tests assert `calls <= REPLACE_ATTEMPTS`. No assertion was removed. A timed-out worker is leaked and keeps spinning until the binary exits; this only happens under a mutant. All eight are now caught as ordinary test failures.

## Former survivors now caught (15)

| Mutant (line at 8eed99ef) | Killer in `w2_phys6_killers.rs` |
|---|---|
| `is_device` 491:39, 492:16 (`==`, `<=`), 495:16, 495:29, 495:63, 496:27, 496:59 | `is_device_recognises_exactly_the_device_prefixes`, `ensure_extended_prefix_if_needed_adds_the_unc_form_only_to_non_device_paths` (pure, through seams) |
| `create_directory_impl` 721:19 `&&` -> `\|\|` | `create_directory_with_an_invalid_name_directly_under_an_existing_root_reports_the_failure` (native) |
| `open_directory_handle` 880:56 `\|` -> `&` | `enumerate_works_while_another_handle_holds_the_folder_with_delete_or_write_access` (native) |
| `resolve_if_reparse_point` 957:19 `&` -> `\|`, `^` | `resolve_if_reparse_point_resolves_exactly_the_entries_with_the_reparse_attribute` (pure, injected probes) |
| `open_read_shared` 1364:24 `>` -> `==`, `<`, `>=` | `open_read_shared_buffers_only_above_one_byte`, `open_read_shared_with_buffer_size_one_is_an_unbuffered_file` (native) |

Corrections to earlier notes: `is_device` 492:16 `<=` and `resolve_if_reparse_point` 957:19 `&` -> `|` were recorded as equivalent. Through the seams they are observable (a four-character device path; the number of `read_link` probes), so they are now killed instead of argued. 492:16 `==` panics with an index out of bounds on a three-character input and reports the four-character devices as non-devices.

Limits of the new native tests:

- The 721:19 test asks the real `create_directory` for `X:\a|b` at the root of the drive that holds the temp folder. NTFS rejects the name with error 123, so nothing is written; the test asserts that, and a drop guard removes the (empty) folder if a file system that accepts `|` created it. It is the only test that names a path outside its private `TempRoot`. When the temp folder is not on a plain `X:\` path the test prints `SKIPPED` and does not check the mutant.
- The two `open_read_shared` tests rely on how std's `BufReader` fills its buffer (a first small read fills it; a zero-length read fills a one-byte buffer). A std change could make them fail on correct code or let `>=` survive.
- The four native tests are `#[ignore]` and do not run in default `cargo test`.

## Equivalent by code argument (21)

Lines at 8eed99ef. None is caught by any `pr-core` test (run 2).

| Mutant | Argument |
|---|---|
| 161:85 `\|\|` -> `&&` (`matches_win32_expression_units`) | Only `*` and `*.*` change route; both translate to an expression that matches the same names as the shortcut `*`. |
| 197:13 delete arm, 198:43 `!=` -> `==` (`units_eq_ignore_case`) | The arm needs an unpaired surrogate on both sides; the expression side comes from a `&str`, so it is unreachable. |
| 288:39 `&&` -> `\|\|` (`match_pattern`) | The extra evaluations of `not_last_period` cannot change `zero_or_more`. |
| 421:5 `now_nanos` -> 0, 1 | Only a seed hashed with a fresh `RandomState`, the pid and a counter; uniqueness does not depend on it. |
| 496:43 `&&` -> `\|\|` (`is_device`) | The extra true inputs (`\?x..`, `a??\..`) are not outputs of `GetFullPath`; reachable through the new seam only with such unreachable text. |
| 513:36 `>=` -> `<` (`ensure_extended_prefix_if_needed`) | std adds the verbatim prefix itself for 260 units or more; for short full paths the explicit prefix changes nothing. |
| 677:15 `>=` -> `<`, 678:16 `-=` -> `/=` (`create_directory_impl`) | The trailing separator stays; one extra `CreateDirectory` returns 183, which is swallowed. Same result and folders. |
| 683:15, 693:21 `>` -> `>=` | Differ only at `i == root_length`, where nothing is read or pushed. |
| 730:22 guard `!last_ok` -> `true` | Needs a level that fails while the wanted child is still created: a race, or an ACL that denies stat but allows creating children. Not reachable in a single-threaded test without ACLs. Not strictly equivalent. |
| 801:43, 880:37, 880:56, 1181:29, 1181:48, 1442:49, 1442:68 `\|` -> `^` | Disjoint flag bits: `a \| b == a ^ b`. |
| 1336:29 `\|\|` -> `&&` (`try_copy_new_with_proof`) | With one blank path, `copy_file` repeats the same check and returns the same error. |

730:22 is the weakest entry: it is "not observable here", not a proof. It stays listed as an open exception.

## Not compiled on Windows (15)

`os_units` 533:5 (3), `attributes_of` 567:5 (2), 569:20, 572:20, `last_write_of` 586:5, `win32::wide` 838:9 (3), 842:9, 846:9, 850:9, 854:9. All inside `#[cfg(not(windows))]`. The product targets Windows only; a verdict needs a non-Windows build and is not planned.

## Production change (P-406)

`physical.rs`, behaviour-neutral:

- `#[doc(hidden)] pub fn is_device_for_tests`, `ensure_extended_prefix_if_needed_for_tests`: call the private pure functions.
- `resolve_if_reparse_point` delegates to the private `resolve_if_reparse_point_with`, which takes the attribute query, the "`read_link` succeeds" probe and the final-target walk as closures (same convention as `immediate_link_target_with`). `#[doc(hidden)] pub fn resolve_if_reparse_point_for_tests` exposes it. The production wiring (`entry_attributes`, `fs::read_link(..).is_ok()`, `final_link_target`) is covered by the existing native junction tests: the wrapper mutants at 967:5 and 984:5 fail several of them.

Two read-only reviewers checked the change (kill claims; data safety and neutrality of the seams). Their two should-fix items are applied: the drop guard and explicit skip in the 721:19 test, and the 60 s bound for the whole-suite wrappers.

## Open

- 730:22 (above) and the 15 non-Windows twins have no run proof.
- The 373 catches of run 1 were measured before the `io_physical.rs` changes of this branch. Those changes only wrap existing calls; the moved `resolve_if_reparse_point` identities were rerun (run 3).
- The attended and OS-limit items of X-physical-fs and SEC-01 / F-05 are unchanged (63-junction test, error 1921).

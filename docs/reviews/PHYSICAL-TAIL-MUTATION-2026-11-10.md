Status: evidence report (2026-11-10), task T3 (X-physical-fs tail). All 76 identities of the tail groups were run once; PARITY.md NOT edited (the row stays Partial until the owner decides about the other identities, see "Open").

# `physical.rs` mutation tail (the groups Q3 section 3 listed as never run)

Source: `crates/pr-core/src/io/physical.rs` at master f804cedf. Q3 said "at most about 36 identities"; the exact `cargo mutants --list` (440 identities in the file) gives **76** for these functions (cargo-mutants counts `win32_directory_exists` 11 and `win32_file_exists` 5 return-value variants, and the non-Windows copies of `attributes_of`, `win32::wide`, `win32::move_file`).

## How it ran

- Selection: `-F '(replace (<16 names>) -> )|( in (<16 names>)$)'` on `--file crates/pr-core/src/io/physical.rs`, `-j 1`, `--baseline skip`, own target dir, `--timeout 120`, machine shared with other agents (3-5 min per mutant at times).
- Test command: `io_physical`, `io_physical_seams`, `w2_phys2/3/4/5_killers`, `w2_physrest_killers`, `w2_physrest2_killers`, `q3_physical_mkdir_killers`, `-- --include-ignored --test-threads=1`. Preflight (unmutated): `running` 97 tests, exit 0 (95 with the filter below).
- Run 1 (`tools/dev/mutants.sh --native`, whole suite): 41 of 76 tested before its 3900 s budget ended (`BUDGET EXHAUSTED`, partial); it covered the mutants from line 124 to the first ones of `open_like_file_stream`.
- Run 2 (direct `cargo mutants`, same tests plus `--skip chain_of_6`, which leaves out the two 63/64-junction tests that take about 50 s each each mutant): 63 of 63 tested (59 min), 49 caught, 12 missed, 2 unviable. Preflight of that command: 95 tests, exit 0. Its 63 mutants are every tail function except `is_transient_replace_failure`, `new_guid_n` and `attributes_of`, which run 1 did.
- Union by identity: every one of the 76 has an outcome. Where both runs saw a mutant, the results agree except `win32::wide` 782:29 (`>=` -> `<`): unviable in run 1, caught in run 2; counted as caught.
- Re-run of the one survivor that has a killer outside this list: `final_link_target_with` 993:21 guard -> `true`, with `w2_sec01_guard_killers` (4 tests, preflight ok): 1 of 1 caught.
- Not run: none of the 76. Not done: the whole-file remainder (see Open).

## Result per group (union, 76 identities)

| Group | Identities | Caught | Missed | Unviable | Timeout |
|---|---|---|---|---|---|
| `is_transient_replace_failure` | 3 | 3 | 0 | 0 | 0 |
| `new_guid_n` | 4 | 3 | 0 | 0 | 1 (`String::new()`, test hangs, counts as detected) |
| `attributes_of` | 6 | 2 | 4 | 0 | 0 |
| `open_like_file_stream` | 6 | 5 | 0 | 1 | 0 |
| `win32_directory_exists` | 11 | 11 | 0 | 0 | 0 |
| `win32_file_exists` | 5 | 5 | 0 | 0 | 0 |
| `move_impl` | 2 | 2 | 0 | 0 | 0 |
| `win32::wide` | 7 | 4 | 3 | 0 | 0 |
| `win32::check` | 2 | 2 | 0 | 0 | 0 |
| `win32::move_file` | 4 | 2 | 2 | 0 | 0 |
| `resolve_if_reparse_point` | 5 | 3 | 2 | 0 | 0 |
| `final_link_target_with` | 4 | 4 (993:21 after the SEC01 killers) | 0 | 0 | 0 |
| `immediate_link_target` | 6 | 4 | 2 | 0 | 0 |
| `is_stat_missing` | 3 | 3 | 0 | 0 | 0 |
| `flush_temp_file` | 3 | 3 | 0 | 0 | 0 |
| `PhysicalFileSystem::open_read_shared_file` | 5 | 2 | 2 | 1 | 0 |
| **Total** | **76** | **58** | **15** | **2** | **1** |

## The 15 survivors: all classified equivalent by analysis (not proven by a mutant)

- 8 in code that is not compiled on Windows (`#[cfg(not(windows))]`): `attributes_of` 567:5 (`-> 0`, `-> 1`), 569:20 and 572:20 (`|=` -> `&=`); `win32::wide` 838:9 (`vec![]`, `vec![0]`, `vec![1]`); `win32::move_file` 846:9 (`Ok(())`). The Windows test run cannot reach them.
- 3 `|` -> `^` on flag constants with distinct bits, where `a | b == a ^ b`: `win32::move_file` 801:43 (`MOVEFILE_COPY_ALLOWED | 0/MOVEFILE_REPLACE_EXISTING`), `open_read_shared_file` 1161:29 and 1161:48 (`FILE_SHARE_READ | WRITE | DELETE`).
- 4 on the reparse-point attribute test: `resolve_if_reparse_point` 957:19 and `immediate_link_target` 1043:19, `&` -> `|` and `&` -> `^`. With `|` the early return never happens, and the next line (`fs::read_link(..)` fails for a non-link) gives the same result. With `^` the early return happens only when the attributes are exactly `FILE_ATTRIBUTE_REPARSE_POINT` (0x400) with no other bit; a junction always has DIRECTORY and a file symlink normally ARCHIVE, so the observable result is the same unless a file symlink has its archive bit cleared. That case is not tested (it needs symlink privilege); it is the one residual risk of these 4.

No test was added: none of the survivors is killable through behaviour on this OS.

## Already documented, not new work

- CL-PHYS5 line 678 survivor: `create_directory_impl` 678:16 is one of the 17 equivalent survivors Q3 decided (section 3, "`-=` -> `/=`").
- The 63-junction test failure (OS error 1921 on this Windows 10 build) is documented in Q3 section 1, line 23 (OS-dependent). Here that test and the 64-junction one were left out of run 2 on purpose (time), and ran in run 1.

## Open

- The remaining `physical.rs` identities (440 minus these 76, minus Q3's 63 + 32): their status is still only known from the CL-PHYS3/4/5, CX-PHYS2 and CL-PHYSRERUN reports. This note does not decide whether they count as covered.
- Caught results of run 2 were obtained without the two 63/64-junction tests; a mutant caught only by those two would be listed as missed there. No survivor fits that case (all 15 are in the classes above), so this changes no number.

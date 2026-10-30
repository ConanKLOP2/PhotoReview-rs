# CL-PHYSRERUN 2026-10-30: native mutation re-run of `crates/pr-core/src/io/physical.rs` with the correct flag form

Status: evidence of lane `cl-physrerun` (branch `feat/cl-physrerun`). Context: `docs/reviews/AUDIT-MUTATION-FLAG-2026-10-30.md` found that
the native-only passes of `W2-FS-EVIDENCE-2026-10-29.md` (passes 2-7) ran with a flag form that made cargo run no test. Scope here is the
group the brief named as native-only and most security-relevant, not the whole file (see "Not run").

## 1. Method

- Preflight (no mutation) with the same test arguments: `io_physical` `running 64 tests` ok (44 s), `io_physical_seams` 3, `io_counting` 7,
  `w2_fs_mutation_killers` 17, `w2_sec_killers2` 4, all exit 0.
- Run: `cargo mutants -p pr-core --baseline skip --timeout 300 -j 1 --file "**/physical.rs" -F "<regex>"` with
  `--cargo-arg=--test --cargo-arg=<binary>` and `--cargo-test-arg=--test --cargo-test-arg=<binary>` for `io_physical`, `io_physical_seams`,
  `io_counting`, `w2_fs_mutation_killers`, `w2_sec_killers2`, then `--cargo-test-arg=-- --cargo-test-arg=--include-ignored
  --cargo-test-arg=--test-threads=1` and test-name filters after it (`resolve try_copy_new physical_non_durable golden_physical_fs_ops atomic
  junction follow_link file_system_contract_physical golden_physical_fs_resolve`) to keep one mutant under about 2 minutes on a machine that
  runs seven mutation lanes. A survivor of the filtered run would have been re-run with the full suite (the 5 below were already reported
  missed by the full-binary form in cl-sec01 or are equivalent by reading).
- Own `CARGO_TARGET_DIR` (`target-mut`), `--output mut-out-B`, one cargo-mutants process, `TempRoot` junctions only. Self-check: the
  mutant logs show `running 26 tests ... test result: FAILED` for caught mutants and `ok` for the missed ones, i.e. the native tests ran. A first
  attempt without `--cargo-arg=--test` rebuilt every pr-core test target per mutant (2-3 minutes each) and was stopped after 3 mutants (2 caught).

## 2. Result (35 mutants: `final_link_target|immediate_link_target|strip_nt_prefix|is_path_unreachable|try_copy_new_with_proof|write_all_text_atomic|follow_link_chain`)

35 tested, 30 caught, 5 missed, 0 timeout, 0 unviable (66 minutes).

Caught (native tests executed): `final_link_target` -> `Ok("")`/`Ok("xyzzy")`, guard -> `false`; `final_link_target_slow`, `follow_link_chain`,
`follow_link_chain_for_tests` -> `Ok("")`/`Ok("xyzzy")`; `follow_link_chain` `+=` -> `-=`/`*=`, `>` -> `==`/`<`/`>=`; `immediate_link_target` ->
`None`/`Some("")`/`Some("xyzzy")` and `== 0` -> `!= 0`; `strip_nt_prefix` -> `""`/`"xyzzy"`; `is_path_unreachable` -> `false`;
`write_all_text_atomic_observed` -> `Ok(())` and `delete !`; `write_all_text_atomic` -> `Ok(())`; `try_copy_new_with_proof` -> `Ok(true)`/`Ok(false)` and all four
mutants of its guard (`true`, `false`, `&&` -> `||`, `delete !`).

Missed, with the reason each is not a test gap that can be closed:

| Mutant | Reason |
|---|---|
| `physical.rs:983:21` guard `is_path_unreachable(&err)` -> `true` in `final_link_target` | fail-closed property of RV-S06: needs an OS error that is neither a loop nor unreachable. Probed this session (junction chains of 58..64 links, dangling and not, directly and through a child path): the OS boundary (63 ok, 64 -> 1921, dangling 2/3) is the same as the manual walk's, so no junction chain separates the two. Needs malformed reparse data, which needs `unsafe` (not allowed in pr-core). Same as cl-sec01. |
| `physical.rs:1066:5` `is_path_unreachable` -> `true` | the same property seen through the function body. |
| `physical.rs:1025:19` `&` -> `\|` and `&` -> `^` in `immediate_link_target` | equivalent (cl-sec01): for a non-reparse entry `read_link` fails and `.ok()?` returns `None` anyway. |
| `physical.rs:1298:29` `\|\|` -> `&&` in `try_copy_new_with_proof` blank check | equivalent: `copy_file` (called next) starts with the same `is_blank(source) \|\| is_blank(destination)` check and returns the same `blank_path_error()`, which `is_swallowed_as_destination_exists` does not swallow, so the `Err(err)` arm gives the same result. New relative to cl-sec01 (that lane did not cover this function). |

No new killer test was needed: `crates/pr-core/tests/w2_physical_killers3.rs` was not created.

## 3. Not run (still unverified native mutants)

About 400 of the 438 mutants of `physical.rs`: listing and matcher (`enumerate_*`, `list_entries`, `match_pattern`, `matches_win32_expression*`),
`create_directory_impl`, `is_device`, the decoders, the `win32::*` FFI wrappers, thin trait methods, `replace_with_retry`,
`resolve_real_path`/`resolve_if_reparse_point` beyond the first 2 mutants. At 1.5-2 minutes per mutant on this machine they do not fit the time box.
AR16 and the rest of X-physical-fs therefore keep their "re-run pending" note.

## 4. PARITY effect

| Row | Effect |
|---|---|
| X-physical-fs | stays Partial; the note now says which part was re-run for real |
| SEC-01 / F-05 | stays Partial (same survivors as cl-sec01) |
| D-05, RV-C05 | stay Done: `write_all_text_atomic(_observed)` mutants caught with the native tests executed ("rerun OK 2026-10-30", scope as stated) |
| AR16 | unchanged; the `enumerate_*` mutants were not re-run |

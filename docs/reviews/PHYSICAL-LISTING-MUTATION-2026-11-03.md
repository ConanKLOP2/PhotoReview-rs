Status: evidence report (2026-11-03), lane mut-physical-listing. Complete for the mutants of the listed listing functions only; PARITY status NOT raised. No test and no production code changed.

# X-physical-fs / AR16: mutation run of the physical listing functions

Functions in `crates/pr-core/src/io/physical.rs`: `listing_root` (864), `list_entries` (904), `open_physical_listing` (923), and the trait methods `PhysicalFileSystem::enumerate_files` (1389), `enumerate_files_with_stat` (1414), `enumerate_directories` (1431); as a bonus the seam `enumerate_files_with_stat_from` (58-91). Tool: cargo-mutants 27.1.0, via `tools/dev/mutants.sh`, `-j 1`, `--baseline skip`, own target dir under `.claude/tmp-mut-physlisting`. The link-walk functions (`final_link_target` and friends) and `remove_paths` are NOT part of this report.

## Result (real numbers from `mutants.out/outcomes.json`, three runs)

| Run | Tests run | Mutants | Tested / total | Caught | Missed | Unviable | Timeouts |
|---|---|---|---|---|---|---|---|
| 1: listing_root, list_entries, open_physical_listing, trait `enumerate_files` (return values), `enumerate_files_with_stat`, `enumerate_directories` | `w2_physrest_killers`, `w2_physrest2_killers`, `w2_phys4_killers`, `w2_phys5_killers` with `--native` (preflight `running 19 tests`, exit 0, the 2 `#[ignore]` tests included) | 19 | 19 / 19 | 17 | 0 | 2 | 0 |
| 2: trait `enumerate_files` `||`->`&&` (2 mutants, lines 1396) plus `enumerate_files_with_stat_from` (11) | same four binaries, `--native` | 13 | 13 / 13 | 6 | 6 | 1 | 0 |
| 3: the 6 run-2 survivors' function again, all 11 mutants of `enumerate_files_with_stat_from` | `io_physical` (preflight `running 64 tests`, exit 0) | 11 | 11 / 11 | 10 | 0 | 1 | 0 |

Combined, 32 distinct mutants: the 21 listing/trait mutants (run 1 plus the 2 `||` mutants of run 2) gave 19 caught, 0 missed, 2 unviable, 0 timeouts; the 11 mutants of the seam `enumerate_files_with_stat_from` gave 10 caught (run 3), 1 unviable, 0 missed (run 2 alone: 6 caught, 6 missed, 1 unviable).

The 6 survivors of run 2 (`:75:28` `+=` -> `*=`, `:81:35` guard `true`/`false`, `:81:42` `>` -> `==`/`<`/`>=`) are survivors only of the four killer binaries, which do not exercise the injected-listing seam. They are NOT survivors of the suite: the existing seam tests in `crates/pr-core/tests/io_physical.rs` (C# `PhysicalFileSystemListingInterruptedTests` ports, lines 432-575) caught all of them in run 3. No test was added.

Unviable (do not compile): `open_physical_listing` `Ok(Box::new(Default::default()))` and `enumerate_files_with_stat` `Ok(vec![(Default::default(), Some(Default::default()))])`.

## Caught list, run 1 and the `||` pair

`listing_root` -> `Ok(String::new())` and `Ok("xyzzy")`; `list_entries` -> `Ok(vec![])`, `Ok(vec![Default])`, `!=`->`==` (:910:51 and :910:57), `&`->`|`, `&`->`^`; `open_physical_listing` `!=`->`==` (:935:64), `&`->`|`, `&`->`^`; trait `enumerate_files` -> `Ok(vec![])`, `Ok(vec![Default])`, `||`->`&&` at :1396:37 and :1396:63; `enumerate_files_with_stat` -> `Ok(vec![])`, `Ok(vec![(Default, None)])`; `enumerate_directories` -> `Ok(vec![])`, `Ok(vec![Default])`.

## NOT run

All other mutants of `physical.rs` (438 in the whole-file inventory at CX-PHYSREST; about 408 of them not run by this lane): device/prefix helpers, `create_directory_impl`, `resolve_real_path`, link walk, decoder and `replace_with_retry`. `open_directory_handle` (called by `listing_root`, native glue) has no arithmetic mutants selected here. The listing mutants were run on Windows only, with the four killer binaries and `io_physical` (its 2 `#[ignore]` native tests not included in run 3). This report makes no statement about mutants outside the six listed functions and the seam.

## Commands (from the worktree root; `PY_BIN` = Python 3.12, `CARGO_BUILD_JOBS=3`)

```
TEMP=<repo>/.claude/tmp-mut-physlisting MUT_OUT=$TEMP/out bash tools/dev/mutants.sh pr-core crates/pr-core/src/io/physical.rs \
  --tests w2_physrest_killers,w2_physrest2_killers,w2_phys4_killers,w2_phys5_killers --native --budget 1200 --jobs 1 \
  -F 'listing_root|list_entries|open_physical_listing|PhysicalFileSystem>::enumerate_(files|files_with_stat|directories).(->|$)'
... --budget 900 -F 'PhysicalFileSystem>::enumerate_files$' -F 'enumerate_files_with_stat_from'      (same tests, out2)
... --tests io_physical --budget 600 -F 'enumerate_files_with_stat_from'                            (out3)
```

The first run's regex missed the two `||` mutants of `enumerate_files` (their name ends without ` ->`); run 2 covers them. Run 1 took about 4 minutes of mutants; runs 2 and 3 about 3 and 2 minutes.

## Verdict

Every listing mutant in scope is caught or unviable; zero survivors, no killer needed, no open question (P-108 and P-109 not used). This closes the "13 listing / list_entries / trait enumeration mutants not rerun" gap of the AR16 and X-physical-fs notes. Both rows stay Partial: the rest of `physical.rs` is not rerun and the AR16 change in the 29-30/10 window is unconfirmed against the C# oracle.

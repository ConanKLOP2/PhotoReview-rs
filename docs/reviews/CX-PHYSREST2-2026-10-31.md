Status: Partial - selected groups complete; cleanup blocked by policy; parity gaps remain.

# CX-PHYSREST2 evidence (2026-10-31)

Lane `feat/cx-physrest2`, continuing [CX-PHYSREST](CX-PHYSREST-2026-10-31.md).
The predecessor recorded 21/543 evaluated (physical 12/438, settings 9/105).
Those historical results are not counted as new executions here.

Only the two new integration test files and this report are owned by this lane.
No production source, unsafe baseline, PARITY or task status changes. No Git
add/commit/push: integration belongs to the orchestrator.

## Isolation and commands

All disk operations use `pr_test_support::TempRoot`; settings adapters are fakes.
Junctions, when tested, link only paths inside the same TempRoot. No Recycle Bin,
registry, real configuration or C# app data operations are performed.

Environment: `CARGO_INCREMENTAL=0`, `CARGO_PROFILE_DEV_DEBUG=0`,
`CARGO_PROFILE_TEST_DEBUG=0`, `PHOTOREVIEW_GOLDEN_ROOT=<worktree>`,
`PHOTOREVIEW_RS_ISOLATE_CONFIG=1`, and
`PHOTOREVIEW_RS_DATA_ROOT=<worktree>/target/test-data`.
Mutation uses `<worktree>/target-mut`; ordinary independent preflights use
`<worktree>/target`. One mutation process at a time. Every crate source and test
Rust file is touched before each mutation run to invalidate stale artifacts.

Preflight command (substitute the appropriate crate and binary):

```text
cargo test -p pr-core --locked -j 2 --test w2_physrest2_killers
  -- --include-ignored --test-threads=1
```

Mutation command:

```text
cargo mutants -p pr-core --baseline skip --timeout 300 -j 1
  --file '**/physical.rs'
  -F 'physical.rs:(1378|1396):|follow_link_chain|replace_with_retry|physical.rs:(678|694|696):'
  --output <worktree>/mut-out-core
  --cargo-test-arg=--test --cargo-test-arg=w2_physrest2_killers
  --cargo-test-arg=-- --cargo-test-arg=--include-ignored
  --cargo-test-arg=--test-threads=1
```

This selects 31 mutants. Initial preflight: 5 tests passed, zero ignored, exit 0.
The first completed mutant log explicitly shows `running 5 tests` and
`test result: FAILED`, with a killer assertion failure. Expanded preflights and
final results are recorded below after execution completes.

Raw inventories, preflight logs and retained outcome evidence are under
`target/cx-physrest2-evidence/` (local artifacts, not committed).

## Proposed PARITY changes for the orchestrator

- X-physical-fs: keep **Partial**. Add this report and the new core killers as
  focused evidence only; unexecuted device/prefix/resolution/read mutants remain.
- AR16: keep **Partial**. Record exact trait-body and pattern-guard results when
  available; this does not verify all listing, skipped-entry and native behavior.
- P-03: keep **Partial**. Fake adapter checks do not establish raw registry,
  Win32/COM, attended Shell or real-bin behavior.

## Not run and scope limits

No whole-file mutation run and no workspace test or workspace Clippy claim.
Raw Win32/COM/registry glue is excluded. No real-bin test is permitted.
Every mutant outside the exact executed ledger remains NOT RUN by this lane.
Timeouts are not detection. No golden tests are selected, so no result can be
credited solely to a golden test. Missing-golden logs must still be checked.

## Survivor analysis

`physical.rs:678:16`, `length -= 1` to `length /= 1`, is MISSED even with
the new test creating a previously absent nested path ending in a backslash and
asserting that the requested last directory exists. Division by one leaves the
trailing separator in the first candidate. The walk then adds the same candidate
without its trailing separator. Creation runs root-first: the plain candidate
succeeds, then the duplicate trailing candidate reports AlreadyExists; the
directory-existence check suppresses that duplicate error. A stable filesystem
therefore produces the same final directory and result. This is unobservable
through this public API under the controlled, non-racing fixture. Concurrent
filesystem races are not verified, so no universal equivalence claim is made.
It remains a missed mutant in the numerical ledger, not detection or a fixed bug.

## Completed core result

**31 / 438 tested: 29 caught, 1 missed, 1 unviable, 0 timeout; 407 NOT RUN by this lane.**
Mutation elapsed 24m34s; cargo-mutants exited 0 (its exit status alone does not
mean zero survivors). All 29 caught logs execute `running 5 tests` and
`test result: FAILED`. Every failure is from this killers binary alone. The
mutation copy contains the five-test preflight version; two further native tests
for long/device paths and real junction resolution passed an expanded seven-test
preflight but are not mutation evidence for those additional groups.

| Core selection | Tested | Caught | Missed | Unviable | Timeout |
|---|---:|---:|---:|---:|---:|
| replace_with_retry | 11 | 11 | 0 | 0 | 0 |
| create_directory_impl :678/:694/:696 | 6 | 5 | 1 | 0 | 0 |
| follow_link_chain and test seam | 9 | 9 | 0 | 0 | 0 |
| enumerate_files pattern guards :1378 | 2 | 2 | 0 | 0 | 0 |
| enumerate_files_with_stat trait body :1396 | 3 | 2 | 0 | 1 | 0 |

The :1396 replacement with `Some(Default::default())` is **unviable**:
rustc E0277, `FileStat: Default` is not satisfied. It is not test detection.
The remaining two trait replacements are caught. The full identity ledger is
included below, with raw logs retained locally before mutation-output cleanup.

Both focused Clippy commands passed with `--locked -j 2 -- -D warnings`:
`cargo clippy -p pr-core --test w2_physrest2_killers` and
`cargo clippy -p pr-platform-win --test w2_recyclesettings2_killers`.
No all-target or workspace Clippy claim is made.

## Settings selection

Same mutation options and environment as core; crate `pr-platform-win`, file
`**/recycle_bin_settings.rs`, filter
`drive_type_from_win32|get_volume_guid|get_volume_drive_type`, binary
`w2_recyclesettings2_killers`, output `<worktree>/mut-out-settings`.
The inventory selects 12 mutants: GUID adapter 3, drive-type adapter 2, pure
drive-type mapping 7. The immediate preflight runs 4 tests, all passed, exit 0.
The original 32-mutant adapter/normalization scope is not a complete selection
here: its seven drive-code mapping mutants are selected; raw queries and private
normalization paths needing the OS adapter are excluded. Additional selected
trait adapters are observed through fakes. Quota, GUID validation and the fake
final-path normalization also have ordinary test coverage without a new mutation
verdict for those groups.

## Final mutation totals and remaining scope

| Source file | Tested / whole file | Caught | Missed | Unviable | Timeout | NOT RUN by this lane |
|---|---:|---:|---:|---:|---:|---:|
| physical.rs | 31/438 | 29 | 1 | 1 | 0 | 407 |
| recycle_bin_settings.rs | 12/105 | 10 | 0 | 2 | 0 | 93 |
| New execution total | 43/543 | 39 | 1 | 3 | 0 | 500 |

Settings elapsed 17m09s, exit 0. Core plus settings mutation elapsed about
42 minutes; preparation, ordinary gates and handoff bring this lane close to
the requested one-hour budget. Both owned mutation processes have exited.
The settings split is GUID adapter 3 caught; drive-type adapter 1 caught and
1 unviable; drive-code mapping 6 caught and 1 unviable. Both settings unviable
replacements require `DriveType: Default` and fail with rustc E0277.

All **29/29 core caught logs** execute five tests; all **10/10 settings caught
logs** execute four tests. Each contains `test result: FAILED` from the selected
killers binary alone. Every completed mutant log was scanned: **zero** contains
`no tests/golden above`. No golden tests or golden-only catches are credited.
All 43 logs, outcomes, diffs and inventories are retained under the local
evidence directory recorded above before mutation-directory cleanup.

The only MISSED mutant is the trim operation analyzed above; no new killable
survivor or production bug was found. No production source was changed and no
separate survivor-confirmation round is claimed. The caught results already
come from the new killers binaries alone.

The predecessor's disjoint selection was physical listing helpers (12) and
`resolve_final_path_with` (9). Adding those historical executions explicitly,
these **two reports together evaluate 64/543: 59 caught, 1 missed, 4 unviable,
0 timeout; 479 NOT RUN across these two reports**. Physical cumulative:
43/438, 395 NOT RUN; settings cumulative: 21/105, 84 NOT RUN. These numbers do
not represent all other historical reports or whole-project coverage.

Remaining assigned physical groups NOT RUN by this lane: `is_device` (22),
`ensure_extended_prefix` (4), `ensure_extended_prefix_if_needed` (4), the other
27 `create_directory_impl` mutants, `resolve_if_reparse_point` (5),
`resolve_real_path` (2), `final_link_target` (4), `final_link_target_slow` (2),
`open_read_shared_file` (5), `open_read_shared` (4), `file_exists_full` (2),
`win32_file_exists` (5), and trait `file_exists` (4). New ordinary tests exercise
long/device paths, reads and live/dangling private junctions; those passes do
not replace a mutation verdict. Other source-file mutants outside this lane's
exact ledger are also NOT RUN here.

Of the original 32 settings adapter/normalization mutants, seven pure drive-code
mapping mutants were evaluated here; the other 25 were NOT RUN. Raw Win32 query
glue and private OS-coupled normalization were excluded. The five additional
trait-adapter mutants use fakes. Quota and GUID-validation mutants were NOT RUN;
the resolver's earlier nine-mutant verdict belongs to the predecessor.
X-physical-fs, AR16 and P-03 all remain **Partial**.

## Final ordinary gates

- `cargo fmt -p pr-core --check`: passed.
- `cargo fmt -p pr-platform-win --check`: passed.
- Final core killers: **7 passed, 0 failed, 0 ignored**, including all five
  native TempRoot tests; final settings killers: **4 passed, 0 failed, 0 ignored**.
  Both commands use `--locked -j 2` and
  `-- --include-ignored --test-threads=1`; both exited 0.
- Focused Clippy for each new binary passed, as recorded above.
- Final `python -I tools/dev/doc-links.py --quiet`: exit 0, 624 files,
  **0 errors**, 181 proposed items and 42 archive-unchecked items (separate
  informational categories, not verified links).
- Workspace tests, workspace Clippy, raw Win32/COM mutation and attended desktop
  behavior were NOT RUN.

Janitor dry-run exited 0 and reported zero candidates, with process enumeration
unavailable and target/worktree candidates suppressed. No broad cleanup is
applied. Only the explicitly authorized lane-owned mutation directories are
scheduled for cleanup after the owned mutation process exits and logs are saved.

## Cleanup result and handoff

**CLEANUP BLOCKED.** After both owned mutation processes exited with code 0
and all 43 raw logs/outcomes were saved, automatic policy review rejected the
PowerShell command containing `Remove-Item -LiteralPath $ownedPath -Recurse
-Force -ErrorAction Stop` with **blocked by policy**. The command restricted
targets to direct children of this worktree, rejected reparse-point targets,
and checked completed saved outcomes before deletion. The command was rejected
before execution. No deletion was performed and no alternate deletion was
attempted.

The worktree-local `target-mut`, `mut-out-core`, and `mut-out-settings`
directories remain for the orchestrator. Retained raw evidence is also under
`target/cx-physrest2-evidence/core-raw` and
`target/cx-physrest2-evidence/settings-raw`. No other worktree, branch, claim,
process or build directory was removed. No Git add/commit/push was run.

## Core exact ledger

- **CaughtMutant**: `crates/pr-core/src/io/physical.rs:109:5: replace replace_with_retry -> io::Result<()> with Ok(())`
- **CaughtMutant**: `crates/pr-core/src/io/physical.rs:113:25: replace match guard attempt < REPLACE_ATTEMPTS && is_transient_replace_failure(&err) with true in replace_with_retry`
- **CaughtMutant**: `crates/pr-core/src/io/physical.rs:113:25: replace match guard attempt < REPLACE_ATTEMPTS && is_transient_replace_failure(&err) with false in replace_with_retry`
- **CaughtMutant**: `crates/pr-core/src/io/physical.rs:113:52: replace && with || in replace_with_retry`
- **CaughtMutant**: `crates/pr-core/src/io/physical.rs:113:33: replace < with == in replace_with_retry`
- **CaughtMutant**: `crates/pr-core/src/io/physical.rs:113:33: replace < with > in replace_with_retry`
- **CaughtMutant**: `crates/pr-core/src/io/physical.rs:113:33: replace < with <= in replace_with_retry`
- **CaughtMutant**: `crates/pr-core/src/io/physical.rs:113:107: replace * with + in replace_with_retry`
- **CaughtMutant**: `crates/pr-core/src/io/physical.rs:113:107: replace * with / in replace_with_retry`
- **CaughtMutant**: `crates/pr-core/src/io/physical.rs:116:17: replace += with -= in replace_with_retry`
- **CaughtMutant**: `crates/pr-core/src/io/physical.rs:116:17: replace += with *= in replace_with_retry`
- **CaughtMutant**: `crates/pr-core/src/io/physical.rs:678:16: replace -= with += in create_directory_impl`
- **MissedMutant**: `crates/pr-core/src/io/physical.rs:678:16: replace -= with /= in create_directory_impl`
- **CaughtMutant**: `crates/pr-core/src/io/physical.rs:694:19: replace -= with += in create_directory_impl`
- **CaughtMutant**: `crates/pr-core/src/io/physical.rs:694:19: replace -= with /= in create_directory_impl`
- **CaughtMutant**: `crates/pr-core/src/io/physical.rs:696:15: replace -= with += in create_directory_impl`
- **CaughtMutant**: `crates/pr-core/src/io/physical.rs:696:15: replace -= with /= in create_directory_impl`
- **CaughtMutant**: `crates/pr-core/src/io/physical.rs:998:5: replace follow_link_chain -> io::Result<String> with Ok(String::new())`
- **CaughtMutant**: `crates/pr-core/src/io/physical.rs:998:5: replace follow_link_chain -> io::Result<String> with Ok("xyzzy".into())`
- **CaughtMutant**: `crates/pr-core/src/io/physical.rs:1002:14: replace += with -= in follow_link_chain`
- **CaughtMutant**: `crates/pr-core/src/io/physical.rs:1002:14: replace += with *= in follow_link_chain`
- **CaughtMutant**: `crates/pr-core/src/io/physical.rs:1003:17: replace > with == in follow_link_chain`
- **CaughtMutant**: `crates/pr-core/src/io/physical.rs:1003:17: replace > with < in follow_link_chain`
- **CaughtMutant**: `crates/pr-core/src/io/physical.rs:1003:17: replace > with >= in follow_link_chain`
- **CaughtMutant**: `crates/pr-core/src/io/physical.rs:1017:5: replace follow_link_chain_for_tests -> io::Result<String> with Ok(String::new())`
- **CaughtMutant**: `crates/pr-core/src/io/physical.rs:1017:5: replace follow_link_chain_for_tests -> io::Result<String> with Ok("xyzzy".into())`
- **CaughtMutant**: `crates/pr-core/src/io/physical.rs:1378:63: replace || with && in <impl FileSystem for PhysicalFileSystem>::enumerate_files`
- **CaughtMutant**: `crates/pr-core/src/io/physical.rs:1378:37: replace || with && in <impl FileSystem for PhysicalFileSystem>::enumerate_files`
- **CaughtMutant**: `crates/pr-core/src/io/physical.rs:1396:9: replace <impl FileSystem for PhysicalFileSystem>::enumerate_files_with_stat -> io::Result<Vec<(PathBuf, Option<FileStat>)>> with Ok(vec![])`
- **CaughtMutant**: `crates/pr-core/src/io/physical.rs:1396:9: replace <impl FileSystem for PhysicalFileSystem>::enumerate_files_with_stat -> io::Result<Vec<(PathBuf, Option<FileStat>)>> with Ok(vec![(Default::default(), None)])`
- **Unviable**: `crates/pr-core/src/io/physical.rs:1396:9: replace <impl FileSystem for PhysicalFileSystem>::enumerate_files_with_stat -> io::Result<Vec<(PathBuf, Option<FileStat>)>> with Ok(vec![(Default::default(), Some(Default::default()))])`

## Settings exact ledger

- **CaughtMutant**: `crates/pr-platform-win/src/recycle_bin_settings.rs:182:9: replace <impl RecycleBinSettingsSource for WindowsRecycleBinSettingsSource>::get_volume_guid -> Option<String> with None`
- **CaughtMutant**: `crates/pr-platform-win/src/recycle_bin_settings.rs:182:9: replace <impl RecycleBinSettingsSource for WindowsRecycleBinSettingsSource>::get_volume_guid -> Option<String> with Some(String::new())`
- **CaughtMutant**: `crates/pr-platform-win/src/recycle_bin_settings.rs:182:9: replace <impl RecycleBinSettingsSource for WindowsRecycleBinSettingsSource>::get_volume_guid -> Option<String> with Some("xyzzy".into())`
- **CaughtMutant**: `crates/pr-platform-win/src/recycle_bin_settings.rs:188:9: replace <impl RecycleBinSettingsSource for WindowsRecycleBinSettingsSource>::get_volume_drive_type -> Option<DriveType> with None`
- **Unviable**: `crates/pr-platform-win/src/recycle_bin_settings.rs:188:9: replace <impl RecycleBinSettingsSource for WindowsRecycleBinSettingsSource>::get_volume_drive_type -> Option<DriveType> with Some(Default::default())`
- **Unviable**: `crates/pr-platform-win/src/recycle_bin_settings.rs:356:5: replace drive_type_from_win32 -> DriveType with Default::default()`
- **CaughtMutant**: `crates/pr-platform-win/src/recycle_bin_settings.rs:357:9: delete match arm 1 in drive_type_from_win32`
- **CaughtMutant**: `crates/pr-platform-win/src/recycle_bin_settings.rs:358:9: delete match arm 2 in drive_type_from_win32`
- **CaughtMutant**: `crates/pr-platform-win/src/recycle_bin_settings.rs:359:9: delete match arm 3 in drive_type_from_win32`
- **CaughtMutant**: `crates/pr-platform-win/src/recycle_bin_settings.rs:360:9: delete match arm 4 in drive_type_from_win32`
- **CaughtMutant**: `crates/pr-platform-win/src/recycle_bin_settings.rs:361:9: delete match arm 5 in drive_type_from_win32`
- **CaughtMutant**: `crates/pr-platform-win/src/recycle_bin_settings.rs:362:9: delete match arm 6 in drive_type_from_win32`

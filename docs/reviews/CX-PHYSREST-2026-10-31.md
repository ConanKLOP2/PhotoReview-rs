# CX-PHYSREST mutation evidence (2026-10-31)

Branch `feat/cx-physrest`; all work remains uncommitted for the orchestrator.
Scope: `physical.rs` (X-physical-fs, AR16) and `recycle_bin_settings.rs` (P-03).
Production code, unsafe baseline, PARITY and task_on_progress are unchanged.

## Prior evidence and selection

Read [CL-PHYSRERUN](CL-PHYSRERUN-2026-10-30.md),
[CL-W6RERUN](CL-W6RERUN-2026-10-30.md), [CL-P01](CL-P01-2026-10-30.md),
[CL-PHYS5](CL-PHYS5-2026-10-31.md) and [CX-PHYS2](CX-PHYS2-2026-10-31.md).
Decoder/retry and listing seam mutants already evaluated by CX-PHYS2 are not repeated.
The historical adapter claim was corrected by CL-W6RERUN; this lane does not credit
those old results as new executions. Current whole-file inventories from
`cargo mutants --list --json`: physical **438**, settings **105**.
Neither source file contains in-module tests; no extra `--lib` pass applies.

The settings file has gained `resolve_final_path_with` since the older reports.
Its nine pure-logic mutants are selected using a fake canonicalizer. Raw Win32/COM
adapters and registry handle Drop are excluded from mutation in this lane.
The new listing killers use only private TempRoot files and directories.
No test calls Recycle Bin, Shell deletion, user config or C# data operations.

## Environment and preflight

`CARGO_INCREMENTAL=0`, `CARGO_PROFILE_DEV_DEBUG=0`, `CARGO_PROFILE_TEST_DEBUG=0`;
normal builds use `<worktree>/target`, mutations use `<worktree>/target-mut`.
Config isolation is `PHOTOREVIEW_RS_ISOLATE_CONFIG=1` and a worktree-local
`PHOTOREVIEW_RS_DATA_ROOT`. One cargo-mutants process at a time.
`stage-native.ps1` passed after process-local safe.directory exceptions for this
worktree, the main checkout and the read-only C# checkout; both pinned decoder DLLs
verified in all three worktree destinations. Global Git config was not changed.
Two earlier stage attempts failed ownership checks and are not passes.

Preflight: `cargo test -p pr-core --locked -j 2 --test w2_physrest_killers
-- --include-ignored --test-threads=1`: **2 passed, 0 ignored**.
Settings preflight with the corresponding crate and `w2_recyclesettings_killers`:
**3 passed, 0 ignored**. New tests use no sleeps, mtime changes or wall-clock assertions.
The listing test compares a stable file's entry stat to its stat lookup; no clock
boundary is involved. A 20-repeat timing gate does not apply.

## Commands and mutation ledger

Settings command:

```text
cargo mutants -p pr-platform-win --baseline skip --timeout 90 -j 1
  --file '**/recycle_bin_settings.rs' -F 'resolve_final_path_with'
  --output <worktree>/mut-out-settings --cargo-arg=--test=w2_recyclesettings_killers
```

The binary has three ordinary tests; no ignored-test flags are necessary for this
pure fake selection. The `--list` selection and run both identify nine mutants.
Results and per-mutant ledger are appended below after completion.

## Proposed row changes for orchestrator

- X-physical-fs: **keep Partial**. Device/prefix survivors and create-directory
  timeouts in CL-PHYS5 were not resolved here; most of physical.rs was not rerun.
- AR16: **keep Partial**. A listing test pass alone is not exhaustive mutation proof.
- P-03: **keep Partial**. A pure resolver verdict cannot establish the remaining
  adapters, Shell/COM behavior or the row's attended native requirements.

## Not run

No new execution of is_device :490-496, ensure_extended_prefix :501/:513,
create_directory_impl :678/:694/:696, resolve_real_path/resolve_if_reparse_point,
final_link_target, follow_link_chain, open_read_shared, file_exists, decoder or
replace_with_retry mutants. Existing CX-PHYS2 and CL-PHYSRERUN evidence is referenced,
not added to this lane's totals. Raw Win32/COM glue and real-bin tests are deliberately
excluded. The original 32 settings adapter/normalization mutants are not rerun as a
group: older corrected evidence exists, raw adapter mutation is outside the chosen
pure-logic budget, and no complete-file verdict is claimed.

## Gates, final results and cleanup

Final results are recorded below. Raw logs and inventories live under
`target/cx-physrest-evidence/`; mutation output and target-mut were scheduled for removal only
after the owned mutation process has exited and its ledger has been copied here.

### Completed settings result

**9 / 9 selected tested, 9 caught, 0 missed, 0 unviable, 0 timeout** (15 minutes).
Whole-file denominator: 9 / 105 evaluated by this lane; **96 NOT RUN**.
All nine mutant logs contain both running 3 tests and test result: FAILED;
the command exited 0. Killers binary alone detects every selected mutant.

Exact caught identities:

- crates/pr-platform-win/src/recycle_bin_settings.rs:258:5: replace resolve_final_path_with -> Option<String> with None
- crates/pr-platform-win/src/recycle_bin_settings.rs:258:5: replace resolve_final_path_with -> Option<String> with Some(String::new())
- crates/pr-platform-win/src/recycle_bin_settings.rs:258:5: replace resolve_final_path_with -> Option<String> with Some("xyzzy".into())
- crates/pr-platform-win/src/recycle_bin_settings.rs:269:27: replace match guard error.kind() == io::ErrorKind::NotFound with true in resolve_final_path_with
- crates/pr-platform-win/src/recycle_bin_settings.rs:269:27: replace match guard error.kind() == io::ErrorKind::NotFound with false in resolve_final_path_with
- crates/pr-platform-win/src/recycle_bin_settings.rs:269:40: replace == with != in resolve_final_path_with
- crates/pr-platform-win/src/recycle_bin_settings.rs:286:27: replace match guard rest.as_bytes().get(1) == Some(&b':') with true in resolve_final_path_with
- crates/pr-platform-win/src/recycle_bin_settings.rs:286:27: replace match guard rest.as_bytes().get(1) == Some(&b':') with false in resolve_final_path_with
- crates/pr-platform-win/src/recycle_bin_settings.rs:286:50: replace == with != in resolve_final_path_with

### Final ordinary gates

- fmt -p pr-core --check: clean.
- fmt -p pr-platform-win --check: clean.
- clippy -p pr-core --all-targets --locked -j 2 -- -D warnings: clean.
- clippy -p pr-platform-win --all-targets --locked -j 2 -- -D warnings: clean.
- Final killers reruns: core 2/2, settings 3/3 passed, 0 ignored.
- doc-links: 610 files, 0 errors, 179 proposed, 42 archive-unchecked.
- Initial core Clippy rejected two unnecessary clones in the new test; fixed and
  full crate all-target Clippy rerun clean. No production edits.


### Listing selection and remaining AR16 gap

The exact listing command used the common settings above plus:

```text
cargo mutants -p pr-core --baseline skip --timeout 90 -j 1
  --file '**/physical.rs'
  -F 'listing_root|list_entries|open_physical_listing|PhysicalFileSystem>::enumerate_files_with_stat$'
  --output <worktree>/mut-out-listing --cargo-arg=--test=w2_physrest_killers
  --cargo-test-arg=-- --cargo-test-arg=--include-ignored --cargo-test-arg=--test-threads=1
```

The trailing anchored alternative did not match the mutation display with its
return type. Both list inventory and execution therefore select **12** mutants:
listing_root 2, list_entries 6, open_physical_listing 4. The trait method's three
body replacements at physical.rs:1396 were **NOT RUN**. No exhaustive AR16 verdict
is claimed. The 11 listing seam mutants already evaluated by CX-PHYS2 are not
rerun or counted here. Pattern guard mutants at :1378 also remain NOT RUN even
though a new unmutated test covers each input rejection independently.

### Final settings ledger

Tested 9; caught 9; missed 0; unviable 0; timeout 0.

- CaughtMutant: crates/pr-platform-win/src/recycle_bin_settings.rs:258:5: replace resolve_final_path_with -> Option<String> with None
- CaughtMutant: crates/pr-platform-win/src/recycle_bin_settings.rs:258:5: replace resolve_final_path_with -> Option<String> with Some(String::new())
- CaughtMutant: crates/pr-platform-win/src/recycle_bin_settings.rs:258:5: replace resolve_final_path_with -> Option<String> with Some("xyzzy".into())
- CaughtMutant: crates/pr-platform-win/src/recycle_bin_settings.rs:269:27: replace match guard error.kind() == io::ErrorKind::NotFound with true in resolve_final_path_with
- CaughtMutant: crates/pr-platform-win/src/recycle_bin_settings.rs:269:27: replace match guard error.kind() == io::ErrorKind::NotFound with false in resolve_final_path_with
- CaughtMutant: crates/pr-platform-win/src/recycle_bin_settings.rs:269:40: replace == with != in resolve_final_path_with
- CaughtMutant: crates/pr-platform-win/src/recycle_bin_settings.rs:286:27: replace match guard rest.as_bytes().get(1) == Some(&b':') with true in resolve_final_path_with
- CaughtMutant: crates/pr-platform-win/src/recycle_bin_settings.rs:286:27: replace match guard rest.as_bytes().get(1) == Some(&b':') with false in resolve_final_path_with
- CaughtMutant: crates/pr-platform-win/src/recycle_bin_settings.rs:286:50: replace == with != in resolve_final_path_with

Logs showing expected test count and test-result FAILED: 9 / 9.


### Final listing ledger

Tested 12; caught 11; missed 0; unviable 1; timeout 0.

- CaughtMutant: crates/pr-core/src/io/physical.rs:865:5: replace listing_root -> io::Result<String> with Ok(String::new())
- CaughtMutant: crates/pr-core/src/io/physical.rs:865:5: replace listing_root -> io::Result<String> with Ok("xyzzy".into())
- CaughtMutant: crates/pr-core/src/io/physical.rs:905:5: replace list_entries -> io::Result<Vec<PathBuf>> with Ok(vec![])
- CaughtMutant: crates/pr-core/src/io/physical.rs:905:5: replace list_entries -> io::Result<Vec<PathBuf>> with Ok(vec![Default::default()])
- CaughtMutant: crates/pr-core/src/io/physical.rs:910:57: replace != with == in list_entries
- CaughtMutant: crates/pr-core/src/io/physical.rs:910:51: replace != with == in list_entries
- CaughtMutant: crates/pr-core/src/io/physical.rs:910:24: replace & with | in list_entries
- CaughtMutant: crates/pr-core/src/io/physical.rs:910:24: replace & with ^ in list_entries
- Unviable: crates/pr-core/src/io/physical.rs:924:5: replace open_physical_listing -> io::Result<Box<dyn Iterator<Item = io::Result<ListedFile>>>> with Ok(Box::new(Default::default()))
- CaughtMutant: crates/pr-core/src/io/physical.rs:935:64: replace != with == in open_physical_listing
- CaughtMutant: crates/pr-core/src/io/physical.rs:935:37: replace & with | in open_physical_listing
- CaughtMutant: crates/pr-core/src/io/physical.rs:935:37: replace & with ^ in open_physical_listing

Logs showing expected test count and test-result FAILED: 11 / 12.


### Final totals and cleanup limitation

**21 / 543 whole-file mutants evaluated in this lane: 20 caught, 0 missed,
1 unviable, 0 timeout; 522 NOT RUN** (physical 12/438, settings 9/105).
The selected groups are complete; the files and PARITY rows are not.
No equivalent mutant is claimed in this lane.
Both cargo-mutants commands exited 0. Mutation elapsed approximately 25 minutes;
total lane time approximately 50 minutes including staging, builds and gates.

The unviable mutant is physical.rs:924:5, open_physical_listing replaced by
Ok(Box::new(Default::default())); rustc E0790: Default::default has no concrete impl type. This is a compile failure, not test detection.
The 11 caught listing logs execute 2 tests; all 9 caught settings logs execute 3.

CLEANUP BLOCKED: after both owned mutation processes exited, automatic policy
review rejected the command containing removal of target-mut, mut-out-settings
and mut-out-listing with 'blocked by policy'. No deletion was performed and no
alternate deletion command was attempted. These three worktree-local directories
remain for the orchestrator. Raw outcomes/logs are also copied under
target/cx-physrest-evidence/{settings,listing}-raw for review.

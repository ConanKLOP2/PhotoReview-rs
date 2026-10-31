# FIX-RESTORE-M3 (2026-10-31): the catalog is asked about the disk after an executor panic

Source: ST-INVMAP-2026-10-31 item M3. Data-safety fix, `crates/pr-app/src/coordinators/file_actions/actions.rs` (`executor_threw`).

## Finding (confirmed)

- Rust: when the file-action executor panicked, `executor_threw` called `catalog.restore` / `restore_members` without asking the disk and registered no Undo. A panic after the file reached the Recycle Bin (journal Prepared/Committed) showed a photo that was gone, with no Ctrl+Z.
- C# (`FileActionController.cs` at 7eaee2a2, the `catch (Exception ex) when (ex is not OperationCanceledException)` of the core, ~lines 475-520): identical. It restores unconditionally ("most likely still on disk; if the file really is gone the presenter's RemoveOrDegrade heals the entry on the next present"), registers no Undo, does not stat. So this is C# parity, with a latent risk: the healing happens only when that entry is presented.

## Fix (minimal, intentional DIVERGENCE)

After a panic (not the post-success bookkeeping path, not the non-panic failure path), each path is asked through the existing `deps.file_system.file_exists` seam. A single file that is gone is not restored (error log "threw and the file is gone"); a group restores only the members still on disk. Without a `file_system` seam the old unconditional restore stays. Behaviour of every non-panic path is unchanged. The status `ActionFailed` is unchanged.

## Not done (open work)

- No Undo is registered after a panic: the `FileActionResult` is lost with the panic. Recovering it means reconciling the Prepared/Committed journal line (`reconcile`) into the single Undo slot, a design change; not attempted.
- A file that is merely unreadable at that instant (`file_exists` false for a transient reason) is not put back until the next folder load; accepted, the same stat is what the presenter does.

## Tests

- Red before, green after (`file_action_core`): `executor_throws_after_the_file_is_gone_entry_does_not_come_back`, `group_executor_throws_after_some_members_are_gone_only_the_existing_ones_come_back`.
- Fakes: `HidingFs::virtual_disk()` (all files exist until `remove`); `Env.disk` is the default file system of the test controller. The 20 test binaries that include `file_action_fakes` all pass.
- `cargo fmt -p pr-app --check`, `cargo clippy -p pr-app --all-targets -- -D warnings`: clean. Not run: workspace tests, mutation (CI).

# FIX-RECYCLE-C1C2 - kiem chung C1, danh gia C2 (2026-10-31)

Status: done for C1 (verified by a run, hardened); C2 deferred with a design. Source findings:
[ST-COM-RECYCLE-2026-10-31](ST-COM-RECYCLE-2026-10-31.md) C1 and C2 (both were inference from code, not run).

## C1 - ket luan: KHONG tai hien duoc voi junction thu muc

Claim: `query_mount_point` (`recycle_bin_settings.rs`) asks `GetVolumePathNameW` about a string path without resolving
links, so `C:\link` (a junction to a USB/network folder) would read as Fixed and `IFileOperation` would delete for good
while the journal says Recycle.

Experiment (run on this machine, safe: only `GetVolumePathNameW`, `GetDriveTypeW`, `GetFinalPathNameByHandle`, links
created in a fresh folder under `%TEMP%` and removed with `rmdir`, no Shell call, no Recycle Bin, no real file touched):

- Machine volumes: `C:` (NTFS Fixed) and one unlettered 953 MB NTFS volume (`\\?\Volume{4be6bdac-...}\`, a recovery
  partition). So the only second volume is reachable only by its volume GUID.
- A real volume mount point (reparse data created with `FSCTL_SET_REPARSE_POINT`, substitute name `\??\Volume{guid}\`):
  `GetVolumePathNameW(<temp>\jvol\f.jpg)` returned `<temp>\jvol\` (documented mount-point behaviour).
- A plain directory junction to a folder on the other volume (substitute name `\??\Volume{guid}\Recovery`, the C1
  shape): `GetVolumePathNameW(<temp>\jdir\f.jpg)` returned `\\?\Volume{4be6bdac-...}\`, that is the TARGET volume, not
  `C:\`. So `GetVolumePathNameW` DOES follow a directory junction, even for a leaf that does not exist.
- Consequence for the eligibility chain: the mount root is then not `C:\`; `GetDriveTypeW` of the link folder returned 1
  (no root directory) and `can_recycle_with` requires `DriveType::Fixed`, so such a path is refused (fail closed), not
  recycled-as-permanent-delete.
- Not testable here: symbolic links to a folder or to a UNC share (`mklink /D` -> "You do not have sufficient
  privilege"), and a junction to a drive that is a real USB/network letter (the machine has no such drive; a mapped
  network drive was not created because it would change system state). For those the answer is inference: Microsoft
  documents that `GetVolumePathName` returns the root of the volume where the END POINT of the path lies, which agrees
  with the junction result, but a symlink was not run.

Verdict: C1 as stated (junction hides a non-Fixed volume) is NOT a real bug on this machine for directory junctions.
Symlinks remain unverified, hence the defence below.

### Hardening (kept small, fail closed)

`query_mount_point` now resolves the links of the containing folders first via the new pure
`resolve_final_path_with(full, canonicalize)` (production: `std::fs::canonicalize`, i.e. `GetFinalPathNameByHandle`):

- only parent folders are resolved, never the leaf (a link is recycled as the link);
- missing folders are walked up to the deepest existing one (`NotFound`), any other error (loop 1921, access denied)
  or nothing existing returns `None` (the callers already treat `None` as "no positively known volume": recycle
  refused, permanent delete refused);
- a verbatim (`\\?\`) input keeps the verbatim spelling (existing mutation killer `w6_rerun_killers`); otherwise
  `\\?\C:\` shows as `C:\` and `\\?\UNC\` as `\\`;
- a volume root (`C:\`) is returned unchanged.

Effect: the same function also feeds the settings queries (volume GUID, quota), now consistent with the volume that
really holds the file. Side effect to know: a file reached through a link to a Removable/Network volume now yields
`can_recycle == false` and `is_permanent_delete_allowed == true` (the target decides), as a direct path would.

Tests: `crates/pr-platform-win/tests/recycle_final_path.rs`: 6 fakes (link to another volume refuses recycle and
allows permanent delete of the target; missing folders; leaf not resolved; fail closed on loop, denied, nothing
existing; drive/UNC/verbatim/Volume-GUID spelling and volume root) and 1 `#[ignore]` native test (a real junction in a
`TempRoot`, read-only volume query, `through == direct`; it passed when run with `--include-ignored`). The tests were
written together with the function, so no separate red run of the production change was kept (stated, not claimed).
Re-run and green: `w6_p01_killers` 21, `w6_rerun_killers` 6, `recycle_bin_facade` 20, `recycle_bin_settings_capacity` 26.

## C2 - ket luan: hoan, thiet ke chi tiet

Why deferred: a correct fix needs a hand-written COM object (`IFileOperationProgressSink`, 12 methods) handed to
`DeleteItem`, plus a result enum through the `shell_recycle` seam. The only way to prove the sink distinguishes
"recycled" from "deleted permanently" is a real Shell delete, which the Recycle Bin rule forbids in this lane (and a
fake cannot prove the Shell's real `psiNewlyCreated` behaviour). Shipping an untested COM sink on the delete path would
add risk to the data-safety path it is meant to protect.

Design for a later lane (needs an owner-attended native run with a self-made temp file):

1. In `recycle_bin.rs::shell`, `#[windows::core::implement(IFileOperationProgressSink)] struct DeleteSink` holding a
   `Cell`/`RefCell` result `{Recycled, DeletedPermanently, Failed(hr)}`; all other 11 callbacks return `S_OK`.
2. `PostDeleteItem(dwflags, psiitem, hrdelete, psinewlycreated)`: `hrdelete` failed -> `Failed(hr)`;
   `psinewlycreated` non-null -> `Recycled`; null with success -> `DeletedPermanently`. Also check
   `TSF_DELETE_RECYCLE_IF_POSSIBLE` is honoured (flag reported in `dwflags`).
3. `DeleteItem(&item, &sink_interface)` with the sink obtained `.cast()`ed from the implementing object; it lives on the
   same STA thread, so no cross-thread access. `// SAFETY:` comments only in `pr-platform-win`.
4. Seam: change `shell_recycle: Fn(&Path) -> io::Result<()>` to `-> io::Result<ShellRecycleOutcome>` with
   `{Recycled, DeletedPermanently, Cancelled}`; the facade maps `DeletedPermanently` to an error carrying a distinct
   message (the file is already gone, so the journal must record a failed/permanent outcome, not Recycle), `Cancelled`
   to the existing `not_removed` path. Fakes cover the three outcomes in `recycle_bin_facade`.
5. Same sink also returns the per-item HRESULT, which fixes L6 (cause of a failure) and lets `FOF_NOERRORUI` be
   considered (owner decision).
6. Cheap interim guard, not done here (behaviour change, owner call): refuse paths longer than 259 characters before the
   Shell call.
7. Native check (manual, attended): create own temp file, recycle it, confirm `Recycled`, then restore/delete only that
   item; never run in CI.

## PARITY

Row `P-03` (WindowsRecycleBin / recycle eligibility): note appended, stays Partial, nothing raised to Done. The note
says C1 was not reproduced for directory junctions (run) and symlinks are inference; C2 open.

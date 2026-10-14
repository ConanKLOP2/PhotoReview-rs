# W2-10: ReviewCatalog (ordered entries + current position, lazy path index, undo/restore, Explorer order, unreadable removal)
- Color: 🟡 | Porter: **sonnet** (index arithmetic with stale stored positions, capture-group fold/degrade logic, current-index bookkeeping; a wrong
  position here moves/recycles the wrong photo, INV-3/5/7/9) | Test porter: haiku | Verifier: sonnet
- C# source (baseline d92d6aae): `src/PhotoReview.Core/Catalog/ReviewCatalog.cs` (532 lines)
  (read with `git -C C:\MyProjects\PhotoReview show d92d6aae:src/PhotoReview.Core/Catalog/ReviewCatalog.cs`)
- C# tests to port (all at d92d6aae, `tests/PhotoReview.Core.Tests/Catalog/` unless noted):
  - `ReviewCatalogTests.cs` (587 lines): all `[Fact]`/`[Theory]` (plain-catalog boundaries, Remove/Restore/RestoreMembers/ReplaceOrder/RemovePaths,
    UpdateMetadata, `Paths_TakenBeforeAChange_IsNotMutatedByIt` => an `Arc<[String]>` taken earlier is unchanged). `CatalogEntry_ValidPath_InitializesCorrectly`
    is a W1-07 duplicate: port it only if it is cheap, do not re-implement the entry.
  - `ReviewCatalogMutationGapTests.cs` (183), `ReviewCatalogRemoveIndexTests.cs` (103; uses `index_rebuild_count_for_tests`: `Remove_Burst_DoesNotRebuildIndexPerRemoval`,
    `Remove_PastThreshold_StillCorrect`, `Index_StaysCorrect_AgainstListModel`).
  - `ReviewCatalogRobustnessTests.cs` (234): port `Reset_SkipsNullAndBlank` (only the blank-path half: a `CatalogEntry` cannot hold a blank path, so build the
    entries through `CatalogEntry::new` and assert the helper below drops blank STRINGS before constructing; null entries do not exist in Rust),
    `ReplaceOrder_*` (non-permutation theory, null element => not applicable, drop; case-variant duplicates never panic), `Restore_BlankPath_Rejected`,
    `RandomOperations_MatchReferenceModel` (8 seeds; write a deterministic xorshift RNG in the test, no `rand` crate; the list model is in the C# file).
    DROP `Reset_PathsEnumerationThrows_KeepsPreviousState` and `Reset_EntriesEnumerationThrows_KeepsPreviousState` (lazy-enumerable exception safety; the Rust
    `reset` takes a `Vec`, so there is nothing to throw): say so in the report.
  - `ReviewCatalogThreadGuardTests.cs` (84): `Unbound_*` and `Bound_ReadFromOtherThread_*` (use `std::thread::spawn` / `std::thread::scope`, the catalog is `Send`),
    `Bound_MutatedOnOwnerThread_*`, and `Bound_MutatedFromOtherThread_ThrowsOnlyInDebug` (`#[cfg(debug_assertions)]`: the spawned thread panics, `join()` is `Err`, the
    payload string contains `"ReviewCatalog.SetCurrent called on thread"`, `current_index` is unchanged; `#[cfg(not(debug_assertions))]`: no panic, index is 1).
  - Test-support overloads of `Reset(paths)` / `Reset(entries)` (`tests/PhotoReview.TestSupport/ReviewCatalogTestExtensions.cs`): write a local `fn reset_paths(c: &mut
    ReviewCatalog, paths: &[&str])` helper in the test file (skip blank strings, `RawPairMode::Separate`, `sidecar_paths = None`). Do NOT add it to the library.
  - `CaptureGroupBuilderTests.cs` (the three catalog-using tests W2-06 left for this wave): `GroupEntries_UsesSelectedRepresentativeAndKeepsBothMembersAddressable`,
    `GroupEntries_PreferRawUsesRawMetadata_AndExplorerOrderCollapsesMembers`, `Reset_DefaultModeKeepsPairMembersSeparate`.
  - `Services/InterleavedFileActionSequenceTests.cs`, `BenchmarkScenarioTests.cs`, `ExplorerSnapshotValidatorRobustnessTests.cs`: NOT here (other units use the catalog as a
    collaborator; they port in their own waves). `tests/PhotoReview.App.Tests/**` belongs to App waves.
  - Golden test (new, required): `tests/golden/review-catalog.json` (101 cases, 1877 recorded steps; ~6 MB reference, so load it once and compare step by step).
- PARITY rows: I-17 (capture groups and pair modes in the catalog), INV-3/INV-5/INV-7/INV-9 (catalog mutations: structural version, current-position rules), AR16 (RemovePaths),
  RV-C09 (Remove keeps the same photo current), CORE-01 (ReplaceOrder is a true permutation), ADR 0005 (owner-thread guard).
- Owned files (the ONLY files you may create/modify):
    crates/pr-core/src/catalog/review_catalog.rs   (stub: contracts-w2/W2-10/review_catalog.rs, compiles clippy-clean)
    crates/pr-core/tests/catalog_review_catalog.rs
- Read-only inputs: `tests/golden/review-catalog.json`. Lead-owned: `catalog/mod.rs` gets `pub mod review_catalog;`.
- Public API (stub, do NOT change signatures; struct fields are private and may be reorganised only if behaviour is identical): `MAX_REMOVED_BEFORE_REBUILD`,
  `ReviewCatalog::{new, default, bind_to_current_thread, structural_version, count, current_index, current, paths, path_at, index_of, reset, find, update_metadata,
  set_current, remove, restore, restore_members, replace_order, remove_paths, entries_snapshot, index_rebuild_count_for_tests}`; private `assert_owner_thread`,
  `ensure_index`, `invalidate_index`, `try_reform_group`, `is_restorable_image`, `try_collapse_member_order`.
- Semantics notes (all verified against C# d92d6aae and the golden; the per-method doc comments in the stub are the line-by-line spec and match the C# source):
  1. **Index.** `index_by_path` keys are `crate::text::ignore_case_key(path)` (the OrdinalIgnoreCase key; never `to_lowercase`). `ensure_index` is lazy; first occurrence
     wins (`entry(key).or_insert`-style, set `index_has_duplicate_keys` when the key already existed). A capture-group entry also registers each `image_paths()` member that is
     not its own path. `index_of` blank check is `path.trim().is_empty()`. `index_rebuild_count` increments on every rebuild, not on every lookup.
  2. **Incremental remove.** The fast path keeps the dictionary stale and records the stored position in the SORTED `removed_positions` (`binary_search` / `insert`); live index =
     `stored - (removed positions below stored)`, `-1` when the stored position itself was removed. It is only valid while `!index_has_duplicate_keys` and fewer than 64
     removals are recorded; otherwise `remove` invalidates (rebuild on the next lookup). The 65th removal therefore costs one rebuild (`Remove_AfterTheMaximumNumber...`).
  3. **StructuralVersion** is bumped by `invalidate_index` AND by the fast-path remove, never by `set_current`/`update_metadata`/rejected calls. `restore_members` can bump
     TWICE (once in `try_reform_group`, once at the insert) and `reset` always bumps (even with an empty list): the golden records the exact numbers.
  4. **`current_index`** is `i32` with `-1` sentinel; all arithmetic stays `i32`/`usize` with explicit casts and never underflows (`removed < current` => `current - 1`).
     `restore*` clamp the insert index with `index.clamp(0, count as i32)` BEFORE casting to `usize` (negative inputs are legal).
  5. **ReplaceOrder** rejects (returns false, nothing changes, no version bump) unless the order is a permutation (count, known, no repeats, all OrdinalIgnoreCase). With
     case-variant duplicate entries (`a.jpg`, `A.JPG`) it must return false and never panic: the C# `ToDictionary` is never reached because the `seen` check fails first;
     build the entry map only AFTER the permutation check and with first-wins. The kept current is found by `index_of(old current path)` AFTER the rebuild.
  6. **RestoreMembers** order of operations matters: `group = None` when `!raw_enabled`; `reformed` fold runs BEFORE the "already listed" filter; when folded the group's
     image members are removed from `all` (OrdinalIgnoreCase); `unique` filters by `is_restorable_image` then `index_of < 0` (so `.xmp`, `.avif`, `.txt` never become entries;
     `.webp/.heic/.heif` always do; RAW only when `raw_enabled`). Pairing: `rest` = `unique` minus the group's image members, but ONLY when the explicit group was actually added (`group` is `Some` and both image members are in `unique`); otherwise `rest` = all of `unique`. `capture_groups::build(&rest, Some(&xmps))`. Use `catalog::types::{SUPPORTED_EXTENSIONS, WEBP_HEIC_EXTENSIONS, RAW_EXTENSIONS, extension_in}`
     and the existing `catalog::types::get_extension` (W1-07; NOT W2-00's `dotnet_path`, which deliberately does not duplicate it), not a local extension list. The xmp check is `eq_ignore_case(get_extension(p), ".xmp")`. The xmp paths passed to `capture_groups::build` come from `all` (not `unique`).
     `CaptureGroup` equality/hash is by the three paths OrdinalIgnoreCase, so `emitted` may be a `HashSet<CaptureGroup>`; it is only used for "emit the group once".
     `get_representative_path` returns `Result` (Separate is `Err`): the mode is never `Separate` at that point, so `expect` with a message is acceptable only there
     (guarded by `_raw_pair_mode != Separate`); never `unwrap()` elsewhere.
  7. **TryReformGroup** (data-safety critical): the lowest standalone index is the slot; the displayed (current) standalone entry is kept as-is plus `capture_group = Some(group)`,
     else the entry is replaced by `CatalogEntry::new(representative)` with the group unless its path already equals the representative (then `capture_group` is set on the
     existing entry, keeping length/date). The loop removes the OTHER standalone index from the highest down, adjusting `current_index` (`== removed` => `slot`, `> removed` => `-1`).
  8. **RemovePaths** reports in catalog order, not input order; group entry fully doomed: own path first, then the other doomed members; one doomed member: degrade in place
     (survivor == own path => same entry with `capture_group = None`, metadata kept; else `CatalogEntry::new(survivor)`, metadata LOST). `next_current` is the kept-list position of
     the first non-removed entry at or after the OLD current index; `old_current < 0` => `0`; none after => last. A `.xmp` path never matches (sidecars are not entries).
  9. **UpdateMetadata** through a member path returns false (golden `update-metadata-member-path-is-a-no-op-representative-stamps`); case-variant representative path succeeds.
  10. **Owner guard**: `std::thread::ThreadId`; only mutators call `assert_owner_thread` with the C# method name as first statement (`"Reset"`, `"UpdateMetadata"`, `"SetCurrent"`,
      `"Remove"`, `"Restore"`, `"RestoreMembers"`, `"ReplaceOrder"`, `"RemovePaths"`); `bind_to_current_thread` takes `&mut self`. The message uses Rust `{:?}` thread ids
      (the C# managed ids are not portable); tests match the prefix `ReviewCatalog.{member} called on thread` only.
  11. **`paths()`** returns an `Arc<[String]>` cached until the next structural change; mutation never writes into a previously returned `Arc`.
  12. **Complexity**: `index_of` is O(1)/O(log removed) and `remove` is O(n) only for the `Vec::remove` memmove; do not use a linear scan (`Remove_Burst_DoesNotRebuildIndexPerRemoval`).
- Traps checklist (docs/MULTI-AGENT-PLAN.md §7): 1 (OrdinalIgnoreCase via `ignore_case_key`/`eq_ignore_case`; the golden mixes `a.jpg`/`A.JPG`), 3 (`length` is `i64`, sentinel `-1`
  indices are `i32`), 8 (`Option<&[String]>` sidecars `None` vs empty are the same for the builder), 10 (stable ordering: `Vec::sort` never needed here; do not iterate a
  `HashMap`/`HashSet` to emit results: `removed`, `restored` and `collapsed` order come from the entry/path lists), plus interior mutability: `index_of(&self)` mutates
  `RefCell`/`Cell` only; no `borrow_mut` held across a call that re-enters `ensure_index`.
- Depends on (called by the implementation or its tests):
  - **W2-06 `catalog::capture_groups::{build, group_entries}`: MUST be merged first** (`reset` and `restore_members` call them; the golden's pairing cases fail without it). Until
    then the porter may only run the `Separate`-mode golden cases.
  - W2-00 is NOT a dependency (`get_extension` lives in `catalog::types`, W1-07).
  - W1-07 `catalog::types::{CatalogEntry (`new` returns `Result<_, ArgumentError>`: never `unwrap()` outside tests, skip/ignore the impossible `Err` for non-blank paths; `capture_group` is a pub field so C# `entry with { CaptureGroup = g }` is clone + assign; `with_metadata`, `path`), CaptureGroup (new, image_paths, get_representative_path, Eq/Hash), get_extension, extension lists}`,
    W1-01 `model::RawPairMode`, `time::DateTimeUtc` (merged).
  - W0 `text::{ignore_case_key, eq_ignore_case}`, pr-test-support `golden` loader. Std only: no new crate.
- Done when: every test above has a Rust twin with `// C#: <Class>.<Method>` (dropped ones listed in the report with the reason); `golden/review-catalog.json` passes for all
  101 cases and all 1877 steps (compare result, count, current_index, current_path, structural_version, paths, entries incl. length/width/last_write ticks/group, and the
  `index_of` probe map); clippy clean; stub's `#![allow(unused_variables, dead_code)]` line removed with the last `todo!()`.

## Golden package
- `oracle/W2-10.cs`: command `review-catalog` (`Commands.ReviewCatalogTrace`). Needs `W2-00.cs` (`W2Util`). Dispatch line is in the file header. Scripted traces: after every
  op (`reset`, `setCurrent`, `remove`, `restore`, `restoreMembers`, `replaceOrder`, `removePaths`, `updateMetadata`) it records op result + full state, plus an initial `new`
  step; `probes` are `IndexOf`'d after every step.
- `golden-inputs/gen-review-catalog.js` generates `golden-inputs/review-catalog.json` deterministically (node, no deps); `golden-reference/review-catalog.json` is the C# output
  (`generator`, `csharp_commit`, `cases[]{name, ops, probes, steps[]}`).
- Case design: 101 cases = plain boundaries, every `RemovePaths`/`Remove`/`Restore`/`ReplaceOrder` current-index branch, case-variant duplicates, 70 removals past the rebuild
  threshold, burst remove/restore, PreferJpeg/PreferRaw/Separate pairing with sidecars, member-order collapse, restoreMembers (fold, reform, RAW off, WebP/HEIC, clamping), plus
  seeded random traces of 100-120 ops (seeds 1-4 plain; 11-13 PreferJpeg, 21-23 PreferRaw, 31-32 Separate with groups).
- Verified at review time: reference has 101 cases and 1877 steps; inputs/reference agree on case names; `new` step 0 of every case is the empty state.

## Open questions
- Stub doc comment on `restore_members` says "see contracts/W2-10.md": the real file is this one (`contracts-w2/W2-10/contract.md`); cosmetic, fix when the stub is copied. Same for the stub's `CatalogEntry::new(...)` mentions: they return `Result` (see Depends on).
- Oracle sets `width` but never `height`/`lastWrite` for `reset` entries; `updateMetadata` covers `height`. Good enough: `CatalogEntry::with_metadata` itself is W1-07.
- `reset` takes `Vec<CatalogEntry>` (no iterator overload). If a later App unit wants `impl IntoIterator`, the lead decides; the helper in tests stays local.

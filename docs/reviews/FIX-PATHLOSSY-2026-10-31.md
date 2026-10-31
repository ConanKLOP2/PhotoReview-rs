# FIX-PATHLOSSY (2026-10-31): unpaired-surrogate names no longer reach the operation layer

Status: fix (data safety, low probability). Source: `ST-PATHLOSSY-2026-10-31.md` (PR #450, merged into this branch for its test file).

## 1. What was wrong

`folder_load/load.rs` built `CatalogEntry` text with `to_string_lossy()`. A file name with an unpaired UTF-16 surrogate (legal on NTFS) became a name containing U+FFFD. Every later Move/Copy/Recycle/Undo ran on that other name, and when a sibling file really named with U+FFFD existed, the operation hit the sibling (test `lossy_source_with_a_decoy_acts_on_the_wrong_file`). C# refuses with `UnrepresentablePath` (`OperationJournal.ThrowIfUnrepresentable`) before changing anything. Catalog, request and journal types are `String` in Rust, so the gate has to sit where a `Path` becomes text.

## 2. Change

- `pr_core::file_actions::errors::path_to_journal_string(&Path) -> Result<String, JournalCodedError>`: `to_str()`, else `JournalCodedError` with code `UnrepresentablePath` and the English text of `core.journal.unrepresentablePath` (key already in en/vi catalogs and the journal text table). Catalog types unchanged.
- `folder_load/load.rs` (scan job): an entry whose path is not representable is NOT loaded (neither as an image nor as an `.xmp` sidecar) and, when it would have been listed (supported image or, with RAW on, `.xmp`), is pushed into the existing `skipped` list (`SkippedKind::FileUnreadable`, reason = the English refusal text). Why this and not "log only": the skipped list is the existing, user-visible channel (`on_files_skipped`, warn log with path and reason, skipped-files window) for "this file in the folder is not reviewable", and it carries the exact lossless `PathBuf`, so the user sees it instead of a silently shorter folder. C# refuses at action time because its catalog can hold the name; here the catalog cannot, so the refusal moves to load time (documented difference; PARITY F-13 stays open, see 5).
- `list_visible_directories` (`catalog/sorting.rs`): directories with a non-representable name are left out (sibling navigation cannot open them by `String`). Intentional difference from C#, documented in the doc comment.
- `viewmodels/main/commands.rs` `pick_and_open_folder`: a picked folder with a non-representable name is refused with an error log instead of opening another folder.

## 3. Other lossy sites, decided one by one

| Site | Decision |
|---|---|
| `load.rs:86` include predicate | Keep: only the extension is tested, lossy text is fine. |
| `catalog/sorting.rs:215` | Fixed (skip). |
| `commands.rs:698` (was 668) | Fixed (refuse). |
| `duplicate_cleanup.rs:195`, `file_actions/recycle.rs:218` `file_name_of(&str)` | Keep: input is already a `String`, display name only, lossless. |
| `duplicate_cleanup.rs:761` | Keep: parent of an already-`String` path, lossless. |
| `folder_load/skipped.rs:215,225` | Keep: paths come from catalog `String`s, which by this fix are always representable; only keys the removal set. |
| `presenters/secondary.rs:369,444` | Keep: display of skipped/batch entries (the skipped entry for an unrepresentable name now shows the U+FFFD form, which is the intended display). |
| `destination_policy.rs:109`, `journal.rs:627,968` | Keep (see ST-PATHLOSSY: comparison only / app-owned paths). |

Not done, follow-up design: a destination chosen through a picker or typed rename that contains a lone surrogate cannot arise (input arrives as UTF-8/UTF-16 text from the OS dialog; only existing names can hold lone surrogates). Carrying `PathBuf`/`OsString` end to end in the catalog (so such a file can be reviewed, not just skipped) is a larger redesign and is not proposed unless the owner wants parity for this rarity.

## 4. Tests

Before: `st_pathlossy` had 4 pass (hazard demonstrations) and 1 red (ignored). New `folder_load_pathlossy` tests run against the pre-fix `load.rs`: 2 of 3 FAIL (`unrepresentable_name_is_not_loaded_and_is_reported_skipped`, `unrepresentable_name_never_aliases_a_real_replacement_character_sibling`); `unrepresentable_sidecar_is_not_loaded` passes before and after (the lossy sidecar name did not pair with the image anyway; kept as a guard).

After (run on the final head, `CARGO_TARGET_DIR` inside the worktree):
- `cargo test -p pr-core --test st_pathlossy`: 8 passed, 0 ignored (`#[ignore]` removed from `listed_directory_text_must_name_the_listed_directory`, whose assertion now expects the lone-surrogate folder to be absent; new: helper refusal, helper pass-through, `list_visible_directories_skips_only_the_unrepresentable_folder`).
- `cargo test -p pr-app --test folder_load_pathlossy`: 3 passed (real files in `TempRoot`, including the decoy scenario: the catalog holds only the real U+FFFD file with its own length; the lone-surrogate file is in the skipped call with its exact path).
- Also run and green: `pr-app --test folder_load_load` (56 passed, 3 ignored), `pr-app --test viewmodels_main_commands` (114 passed), `pr-core --test catalog_sorting` (28 passed, 4 ignored), `pr-core --test file_action_errors` (32 passed).
- `cargo fmt -p pr-core -p pr-app --check` clean; one `cargo clippy -p pr-core -p pr-app --all-targets --locked -- -D warnings` clean. Not run: workspace-wide clippy/tests, cargo-mutants (left to CI / owner).

`pick_and_open_folder` refusal has no dedicated test (the dialog fake returns a `PathBuf`; building a lone-surrogate one is possible but the branch is a 4-line guard); noted as a gap.

## 5. PARITY (not edited)

Suggest F-13 / `UnrepresentablePath` stays Partial: the code constant, key and text exist and a boundary helper now emits it, but the file-action layer still cannot refuse at action time because the entry is excluded at load time (different, safer-by-exclusion behaviour; needs a `Different` row/ADR if kept). Do not mark Done.

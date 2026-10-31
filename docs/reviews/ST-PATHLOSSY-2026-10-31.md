# ST-PATHLOSSY (2026-10-31): unpaired-surrogate paths and `UnrepresentablePath`

Status: review report with tests. No production change. Source: ST-CSPARITY (PR #442) finding that `UNREPRESENTABLE_PATH` is never emitted.

## 1. Verdict

Real defect (confirmed by running tests on real files under a temp root): a Windows file or folder name with an unpaired UTF-16 surrogate is silently converted to a name containing U+FFFD at the `OsStr -> String` boundary, and the file-action stack then operates on that other name. C# refuses with `JournalErrors.UnrepresentablePath` before journaling or mutating anything (`OperationJournal.ThrowIfUnrepresentable`, called first in `AppendLines`; test `JournalAppendRobustnessTests.Append_PathWithUnpairedSurrogate_ThrowsCodedErrorAndWritesNothing`). Rust has no equivalent gate anywhere.

Severity: low likelihood, wrong-file risk only when a second file whose name really contains U+FFFD sits in the same folder (or two names differ only by the lone surrogate). Otherwise it fails safe (NotFound, source untouched), but with a wrong error and a journal line naming a non-existent file.

## 2. Why the journal gate cannot fire (and where the gate must be)

`FileActionRequest.source`, `JournalEntry.source/destination`, `CatalogEntry.path` are all `String`; a Rust `String` cannot hold a lone surrogate (`journal.rs` `append_lines` comment, trap T19). So by the time `pr-core` sees a path the information is already gone. The only place the loss can be detected is the producer of those Strings.

## 3. Conversion points (file:line at origin/master 0d97ff34)

Path -> String that feeds catalog entries, requests, journal lines:

| Site | Behaviour on a lone surrogate |
|---|---|
| `crates/pr-app/src/coordinators/folder_load/load.rs:99` (`path.to_string_lossy().into_owned()` then `CatalogEntry::new(text)`); `:86` filter | silent U+FFFD; the entry (and every later move/copy/recycle/undo request) carries the wrong name. Main hazard. |
| `crates/pr-core/src/catalog/sorting.rs:215` (`list_visible_directories`) | silent U+FFFD in the sibling-folder path (red test below). |
| `crates/pr-app/src/coordinators/duplicate_cleanup.rs:195, :761`, `file_actions/recycle.rs:211` (`file_name_of`), `viewmodels/main/commands.rs:668` (picked file/folder), `folder_load/skipped.rs:215,225`, `presenters/secondary.rs:369,444` | silent U+FFFD (display names, picker results, skipped list keyed by lossy text). |
| `crates/pr-core/src/file_actions/destination_policy.rs:109` | `real.to_string_lossy()` for the symlink-escape containment check; lossy text only compared, not used to act. |
| `crates/pr-core/src/file_actions/journal.rs:627` (`path_text` of the journal file itself), `:968` (compaction suffix) | journal location is an app path; lossy harmless in practice. |
| `crates/pr-core/src/io/physical.rs:472` (`path_str`) | correct: non-Unicode -> `InvalidInput`, not lossy. |

Contrast: `pr-core/src/io/physical.rs` listing keeps `PathBuf` (lossless; control test below), `pr-libraw` documents a deliberate no-retry divergence for non-Unicode paths (T7). No site emits `UnrepresentablePath`; `rg UnrepresentablePath crates/*/src` finds only the constant, its key and the "cannot happen" comment.

## 4. C# comparison

C# strings hold the lone surrogate, so the file is addressable; the journal's JSON serializer would write U+FFFD, hence the guard: refuse, write nothing, mutate nothing, show `core.journal.unrepresentablePath`. Rust loses the information earlier, so the guard degenerates to "act on a different name".

## 5. Tests (`crates/pr-core/tests/st_pathlossy.rs`, Windows only, real files in `TempRoot`, bin = deleting temp bin)

Ran `cargo test -p pr-core --test st_pathlossy -j 2 -- --include-ignored`: 4 passed, 1 red (ignored by default).

- `listing_returns_the_lone_surrogate_path_losslessly` (pass): `PhysicalFileSystem.enumerate_files` returns the exact `PathBuf`; loss happens later.
- `lossy_string_of_a_lone_surrogate_path_names_another_file` (pass): lossy text has U+FFFD and does not exist.
- `lossy_source_without_a_decoy_fails_safe_but_is_not_refused_as_unrepresentable` (pass): Recycle fails, file untouched, no `UnrepresentablePath` code/journal text.
- `lossy_source_with_a_decoy_acts_on_the_wrong_file` (pass = hazard present): with a sibling file really named `a<U+FFFD>.jpg`, a Recycle of the lone-surrogate file's lossy name succeeds, deletes the decoy, leaves the selected file. C# would touch neither.
- `listed_directory_text_must_name_the_listed_directory` (RED, `#[ignore]`): `list_visible_directories` returns `...\d<U+FFFD>.jpg` which does not exist.

The "decoy" test conversion is done in test code mirroring `load.rs:99` (pr-core cannot reach pr-app); the hazard is in the production conversion at that line.

## 6. Proposed fix (not done: touches pr-app/catalog types, not a small change)

1. Add in pr-core one `path_to_journal_text(&Path) -> Result<String, JournalCodedError(UNREPRESENTABLE_PATH)>` (`to_str()`, else the code) and use it at every producer that builds a `CatalogEntry`/request source: folder load (skip the entry into the `skipped` list with a reason, or load it as non-actionable), duplicate cleanup, picker results, `list_visible_directories` (skip).
2. In the service, still never act on text containing U+FFFD that did not come from a verified `to_str()` path (not possible to know) - so the guard must be at the producers, hence the entry should be refused at load time.
3. Show `core.journal.unrepresentablePath` when the user acts on such an entry; keep PARITY F-13 honest until then.

## 7. PARITY

Not edited. Suggest: the F-13 / `UnrepresentablePath` row stays Partial/Open with this report as evidence ("cannot fire by type; boundary guard missing; wrong-file risk with a U+FFFD sibling"); do not mark Done.

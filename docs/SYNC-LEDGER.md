# Sync ledger (C# → Rust)

**Current spec baseline:** C# `master` at `e7c2fcd8` (full `e7c2fcd86900d3406e1a0e643c43d5899b7d7a56`, after #538), version line 2.0.N.
Assets synced from that commit: see `assets/SYNC-MANIFEST.txt`. Goldens: see "Oracle diff, b6847767 to e7c2fcd8" (only `l10n-generator`, `l10n-localizer` and `translation-export` were regenerated; every other golden keeps its old `csharp_commit`, its cases are unchanged; `recovery-retry` was regenerated afterwards in the D1 port PR, see the row for #534).

## How to advance the baseline

1. `git -C ../PhotoReview fetch` then `git -C ../PhotoReview log --oneline <baseline>..origin/master`.
2. Add one row per merged C# PR (or per group of PRs with one effect) below. "Behaviour" = does it change what the
   user sees or what is written to disk.
3. Port behaviour-changing rows (or mark them N/A if the feature is not ported yet; the PARITY row then
   points at the newer C# code). Re-run `tools/sync-from-csharp.ps1` if languages/fixtures changed.
4. Regenerate goldens (`tools/oracle.ps1 -Commit <new>`): a golden whose cases change is a behaviour change.
5. Move the baseline line above to the new commit.

| C# PR / commit | Summary | Behaviour? | Rust status | Rust PR |
|---|---|---|---|---|
| `e441d67a` (#327) | Baseline: whole-project review docs | No | Baseline | — |
| `7eaee2a2` (icon only) | `src/PhotoReview.App/Assets/PhotoReview.ico` synced to `assets/PhotoReviewRs.ico` for the exe icon (W8-20a); no other asset re-synced | No (asset) | Done | — |
| `e441d67a..d92d6aae` (193 commits, #328-#523) | NO-WPF: Win32 shell + Direct2D (ADR 0010, two exes, `PhotoReview.App.Shared`), Native AOT, pixel goldens, CI split | UI only (not ported yet) | N/A until Wave 8; informs R0001 (UI framework) | — |
| same range | `FitWidthAnchor.BottomThird`, new `MiddleClickAction`, shortcuts `FitWidth2` (D4) / `Refresh` (R), WebP/HEIC listing (`WicImageFormat`, `IsSupported(path, raw, webpHeic)`) | Yes | Ported (W1-01/02/04/07), goldens extended | sync PR |
| same range | `JournalLineParser` moved to source-generated JSON (AOT) | No (golden `journal-lines` identical) | Nothing to port | sync PR |
| same range | Abstractions: `IShellIntegration` (Explorer "Browse with"), `IUiDispatcher` priorities, `SettingsTarget`, `IFileSystem.TryDeleteEmptyDirectory` | Yes | Ported (Rust verb `PhotoReviewRs`, R0002) | sync PR |
| same range | Settings (`AppSettings` +72, `SettingsStore` +112, `SettingsNormalizer`, new `ContextMenuLayout`), `ComparePairService`, `SiblingFolderService`, `ReviewCatalog`, `SessionStore`, `PhysicalFileSystem`, `DragDropInputService` | Yes | Not ported yet: Wave 2 contracts written against `d92d6aae` | Wave 2 |
| same range | `FileActionService` +117, `UndoService`, `RecoveryRetryService`, `OperationJournal`, `JournalTransaction`, `PartialDestinationCleanup` | Yes | Not ported yet: Wave 3 contracts must use `d92d6aae` or later | Wave 3 |
| same range | Languages (en/vi +113 lines, notes) and test fixtures (pixel goldens, viewport layout, PreviewCache) | Yes (text) | Synced | sync PR |
| `d92d6aae` (no baseline change) | P-15: `tools/sync-from-csharp.ps1` also maps `tests/PhotoReview.Core.Tests/Fixtures/` files: `operations-legacy.jsonl`, `journal-older-build/*.jsonl` (5, Wave 3 W3-01/W3-03) and `config-v1.json`/`config-v2.json` (W2-03, copied by hand before, now hash-checked; bytes unchanged). Re-run at `d92d6aae`: 81 files, only additions to `assets/SYNC-MANIFEST.txt` | No (test inputs) | Synced | Wave 3 integration |
| `57b04243` (#524) | NOWPF WARP edge documentation | No | Reference only | — |
| `46d33fa1`, `55a37986`, `9800058e`, `170a2d19` (#525, #527, #529, #530) | FEAT-RENAME design, Core rename actions, catalog/session/presenter/cache state, Win32 UI and F2 shortcut | Yes | Deferred; the baseline lane only synced golden-driven settings and shortcut data. Not ported: contracts W9-R1..R11 and `docs/WAVE-9-PLAN-ren.md` exist (contracts PR #176) | W9-R1..R11 (contracts #176; units not ported) |
| `d9623945` (#526) | External editors list, settings migration/repair, default editor shortcut and submenu | Yes | Golden-driven settings/shortcut data synced with the baseline move; editor rules and UI deferred to W9-E1..E9 (contracts PR #165, units not ported; `docs/WAVE-9-PLAN-edt.md`) | W9-E1..E9 |
| `f6662778` (#528) | STA test host timing fix | No | Test infrastructure only | — |
| `a6e530f2` (#531) | Shell-only release workflow | Packaging | Deferred to release tooling owner | — |
| `02aaee0a` (#532) | Language convention documentation | No | Reference only | — |
| `7eaee2a2` (#533) | Unknown `Operation` in typed/imported action list is a JSON error; config loader remains lenient | Yes | Rust action-list parser checked against the regenerated golden; config loader parity tracked separately (`D-actions-unknown-operation`). Qualification: C# `LenientEnumConverter.TryReadStrict` also accepts a defined number encoded as a string, while the Rust action-list reader accepts defined JSON numbers but not numeric strings; matching that input is still open | baseline lane (#182) |
| `1cd6e375` (#536) | Rust-port decision fragments RP-GATE-END-01, RP-JOURNAL-NOTICE-01, RP-SETTINGS-BACKUP-01 and entries in `OPEN-DECISIONS.md` (docs only) | No | Docs only, N/A. The Rust side of the same questions is tracked in `docs/PROPOSED-DECISIONS.md` (P-71, P-72, P-84) | — |
| `b6847767` (#537) | `FileActionController` logs and shows the status text `status.actionJournalNotSaved` when a successful Move/Copy/Recycle was not journaled (`JournalPersisted` false); new key in `en.json`, `en.notes.json`, `vi.json`; new C# test file `FileActionControllerJournalNoticeTests.cs` | Yes (status bar text; nothing new on disk) | Catalog synced; Rust log + notice ported (P-72 C / P-84 A) | #591 (catalog); #592 (behaviour) |
| `6bb0d706` (#534, D1) | `RecoveryRetryService` re-stats each group Recycle member right before the mutation on retry; a changed or missing file fails as `core.recovery.sourceChanged`. Oracle `recovery-retry` gains `stat` calls in its call log (several cases) | Yes (retry of a group Recycle) | Ported (branch `feat/d1-recovery-restat`, owner decision "port both" resolves P-190): the group Recycle re-stat already existed in `pr-core` since #448 and #463 (DV-1, DV-2) as a Rust-only fix; this port aligns the order with C# (re-stat first, then the permanent / `can_recycle` check), ports C# `Retry_MemberReplacedAfterPreparedWasJournaled_TouchesNothingAndFailsAsSourceChanged` (2 rows) and commits the regenerated golden (`csharp_commit` `e7c2fcd8`), so `recovery_retry_golden` no longer strips the extra `stat` op | ported |
| `770f355d` (#535, D2) | `DuplicateCleanupController` / `DuplicateFinder.FindAsync` re-verify the kept and removed copies right before each recycle; new key `core.duplicate.fileChanged` in the three catalogs | Yes (batch duplicate cleanup) | Catalog synced; behaviour NOT ported in `pr_app::coordinators::duplicate_cleanup` (the `duplicates` golden is unchanged, `FindAsync` results are the same); tracked as P-191 | catalog: this sync PR; behaviour: follow-up PR |
| `e7c2fcd8` (#538) | `FileActionController`: a late SINGLE result whose journal line was not saved calls `_undoService?.Register(singleResult)` before showing `status.actionJournalNotSaved` (it skipped Undo before). New C# test `SingleMove_JournalNotPersistedAfterFolderSwitch_IsStillRegisteredForUndoAndTheNoticeIsShown` | Yes (Ctrl+Z after a folder switch) | Ported in this PR (`file_action_journal_notice`, 10 fake tests); the Rust port of #537 had mirrored the old skip (PR #592), now removed | this PR |

## G-INPUT input-scripts (note, 2026-10-29)

`input-scripts` is not an `oracle-cs` golden: it is the C# recording `tests/Fixtures/golden/input-scripts.v1.json` (WP-10, WPF `MainWindow`), synced by `tools/sync-from-csharp.ps1` to `tests/fixtures/golden/input-scripts.v1.json` (listed in `assets/SYNC-MANIFEST.txt`) and replayed by `crates/pr-platform-win/tests/shell_input_dispatch/g_input.rs` (91 scripts, 767 checkpoints, all match). There is nothing to regenerate with `tools/oracle.ps1`; a re-recording happens in the C# repo and arrives through a sync.

## Oracle diff, d92d6aae to 7eaee2a2

The driver ran every oracle command twice at `7eaee2a2`, with `turbojpeg.dll` and `libraw.dll` present. Consecutive outputs were byte-identical, including `turbojpeg-native-decode`, `ordinal-casing-table`, and `libraw-decoder-native`. The final goldens are preserved as generated. The 288 goldens outside this table differ from master only in `csharp_commit`, which is part of the oracle format; their cases are unchanged.

| Golden | Semantic change and C# attribution | Rust status |
|---|---|---|
| `context-menu` | `RenameFile` ID 18 and shell-only item row in group 5 (#530 `170a2d19`, `ContextMenuLayout.cs`); WPF computed-menu cases exclude it. | Shell-only visibility ported; all 27 context-menu tests pass. |
| `l10n-generator` | 53 new constants and members for editor and rename text (#526 `d9623945`, #530 `170a2d19`). | Catalog assets synced; generator suite passed. |
| `l10n-localizer` | 53 new catalog keys for the same features (#526, #530). | Catalog assets synced; localizer suite passed. |
| `menu-model` | Rename command/menu rows and external editor submenu (#526, #530). | Golden cases and `shell_menus_controller` dispatch coverage pass with `RenameFile = 31`; the rename operation and fuller external submenu W9-E6 remain deferred. |
| `preview-service-sequence` | `evict-keeps-original-dims` now returns `known:false` after path eviction (#529 `9800058e`, rename cache work; exported `PreviewImageService.cs` removes that path's dimensions). | Pure eviction change ported; all 96 preview-service-decode tests pass. |
| `secondary-window-supports` | Enum adds `ExternalEditors` and `RenamePrompt` (#526, #530). | Deferred W9-E8 and rename dialog (rename lane W9-R, `docs/WAVE-9-PLAN-ren.md`). |
| `settings-dialog-frame` | Files, shortcuts, and mouse page item counts increase (#526, #530). | Deferred W9-E4 and rename UI (rename lane W9-R, `docs/WAVE-9-PLAN-ren.md`). |
| `settings-dialog-state` | Group/control IDs shift and shortcut boxes increase 27 to 29 (#526, #530). | Shortcut page now declares all 29 boxes; remaining dialog-state changes deferred W9-E4 and rename UI (rename lane W9-R, `docs/WAVE-9-PLAN-ren.md`). |
| `settings-field-map` | Three editor fields, two shortcut boxes, and `RenameFile` menu tick (#526, #530). | Presenter data and native shortcut boxes ported; other W9-E4 native pages deferred. |
| `settings-json` | Config v4, three editor fields, `OpenInDefaultEditor`, `RenameFile` (#526, #530). | JSON/default data ported; editor list rules W9-E1 deferred. |
| `settings-normalize` | v4 repairs, shortcut defaults, and hidden `RenameFile` parsing (#526, #530). | Exercised golden suite passed; editor reconciliation W9-E1 deferred. |
| `settings-page-files` | Manage editors control and RAW-original option (#526). | Deferred W9-E4. |
| `settings-page-mouse` | `RenameFile` context-menu tick (#530). | Data ported; native page deferred (rename lane W9-R). |
| `settings-page-shortcuts` | 29 cells; default editor and rename boxes (#526, #530). | Native page now has 29 boxes and 16 optional clear buttons; full `pr-platform-win` test suite passes. Presenter data and F2 rename routing ported. Default-editor routing W9-E7 and rename operation deferred. |
| `settings-parse-text` | v4 and new settings/shortcut serialization (#526, #530). | Golden suite passed; editor reconciliation W9-E1 deferred. |
| `settings-store` | v4 persisted payload and fields (#526, #530). | Golden suite passed; editor migration/repair W9-E1 deferred. |
| `settings-validate` | New `RenameFile` shortcut validation messages (#530). | Golden suite passed. |
| `shortcut-canonical` | 30 properties, 16 optional names, default editor and F2 rename (#526, #530). | Ported; `shortcut_defaults_match_golden` enabled and passed. |
| `translation-export` | Exported catalog content grows with editor/rename strings (#526, #530). | Catalog synced; export suite passed. |
| `win32-constants` | Dialog constant `EnChange = 768` (#526, dialog editor UI). | Constant ported; golden test passes. Native editor dialog remains W9-E2/E3. |

### Unexplained

None. The exported `ContextMenuLayout.cs` resolves the apparent context-menu discrepancy: `RenameFile` is marked `ShellOnly`, while the old computed-menu cases use the default WPF context (`IncludeShellOnly = false`). The Win32 menu-model cases include it.

### Deferred

External editor list reconciliation/migration is W9-E1; dialogs and the remaining native settings pages are W9-E2/E3/E4; launcher is W9-E5; submenu/model is W9-E6; input routing is W9-E7; secondary-window wiring is W9-E8; shell menu wiring is W9-E9. The rename feature's file actions, catalog/session state, and UI belong to the W9-R lane (`contracts/W9-R1.md`..`W9-R11.md`, `docs/WAVE-9-PLAN-ren.md`, contracts PR #176) and were outside the baseline lane. The `shell_menus_controller` suite passes after routing `RenameFile` through `ReviewCommandType::RenameFile = 31`; shell input accepts this command without starting the unported operation. The shortcut page now has all 29 field-map cells, including `OpenInDefaultEditor` and `RenameFile`. No failing test was ignored or removed.

`settings_shortcuts::shortcut_defaults_match_golden` is enabled and passes. Its separate `serialize_review_action_uses_pascal_case_names` remains ignored as requested; its W1-02 `serde_ext` dependency exists now, so the ignore reason is stale and should be reviewed by that test's owner.

## W9-E8 supports replay extension (2026-10-25, machine A)

At unchanged spec baseline7eaee2a2, added six ExternalEditors supports/router cases for settings_enabled true/false/default and regenerated `secondary-window-supports` with `tools/oracle.ps1 -Command secondary-window-supports -Commit 7eaee2a2`. Output50cases; all44previous case objects remain exactly equal. Existing enum row already lists10kinds. No assets or C# checkout changed. RenamePrompt support cases await W9-R8. Cached C# binaries were verified complete before generation; build0errors/89existing duplicate-type warnings.

## Oracle diff, b6847767 to e7c2fcd8

Range `b6847767..e7c2fcd8` is three commits: #534 (D1, `RecoveryRetryService`), #535 (D2, `DuplicateFinder` + `DuplicateCleanupController` + one catalog key) and #538 (`FileActionController`, no catalog or oracle effect). `tools/sync-from-csharp.ps1` at `e7c2fcd8` changes only `en.json`, `en.notes.json`, `vi.json` (key `core.duplicate.fileChanged`) and `assets/SYNC-MANIFEST.txt`; no fixture changed (the script's emptying of `tests/fixtures/` was undone for the Rust-generated image fixtures). The oracle was run at `e7c2fcd8` only for the commands the range can touch (`duplicates`, `recovery-retry`, the five `l10n-*` and the two `translation-*` commands), not for all of them.

| Golden | Semantic change | Rust status |
|---|---|---|
| `l10n-generator` | New constant `CoreDuplicateFileChanged` for `core.duplicate.fileChanged`. | Regenerated and committed. |
| `l10n-localizer` | The new key appears in the key lists of the shipped-catalog cases. | Regenerated and committed. |
| `translation-export` | The exported `vi` template and the `xx.todo.json` outputs carry the new key. | Regenerated and committed; `golden_translation_export` now expects `e7c2fcd8`. |
| `recovery-retry` | D1: extra `stat` call per group Recycle member before `bin.recycle` / `bin.delete-permanently` in the call log. | Committed in the D1 port PR (`csharp_commit` `e7c2fcd8`, LF-normalised; P-190 resolved by "port both"). |
| `duplicates`, `l10n-catalog`, `l10n-flat-json`, `l10n-loader`, `translation-problems` | Only `csharp_commit` differs. | Not committed (cases unchanged). |

## Oracle diff, 7eaee2a2 to b6847767

Range `7eaee2a2..b6847767` is two commits: #536 (docs only) and #537 (code plus one catalog key in three files). The oracle was run for all commands at `7eaee2a2` and then at `b6847767` on the same Windows 10 machine (the first run already used the newly synced `assets/languages`, which some oracle commands read, so the catalog goldens were also compared with the committed files). Compared without the `csharp_commit` field, the goldens that changed because of the range are:

| Golden | Semantic change | Rust status |
|---|---|---|
| `l10n-generator` | New constant `StatusActionJournalNotSaved` for `status.actionJournalNotSaved` (plan entry and simple member). | Regenerated; `generator_plan_matches_the_csharp_oracle` passes. |
| `l10n-localizer` | The new key appears in the key lists of the shipped-catalog cases (4 lines). | Regenerated; `localizer_matches_the_csharp_oracle` passes. |
| `translation-export` | The exported `vi` template and the `xx.todo.json` outputs carry the new key (8 output strings; 39 cases unchanged in count). | Regenerated; `golden_translation_export` now expects `b6847767`. |

These three are the only goldens committed with the new `csharp_commit`. Every other golden keeps `csharp_commit` `7eaee2a2` (tests assert that value; their cases are unchanged). Goldens that differ on this machine regardless of the range (known Windows 10 drift: `cli-tool-path-guard`, `embedded-thumbnail-*`, `recovery-dialog-details`, `session-key`, `turbojpeg-*`, `wic-*`) were not committed; the oracle also rewrote tracked fixtures and `crates/pr-core/src/text/ordinal_casing_table.rs`, which were restored with `git checkout`. Neither commit touches imaging code. Unexplained: none.

# Sync ledger (C# → Rust)

**Current spec baseline:** C# `master` at `7eaee2a2` (2026-10-21, after #533), version line 2.0.N.
Assets synced from that commit: see `assets/SYNC-MANIFEST.txt`. Goldens regenerated at that commit.

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
| `e441d67a..d92d6aae` (193 commits, #328-#523) | NO-WPF: Win32 shell + Direct2D (ADR 0010, two exes, `PhotoReview.App.Shared`), Native AOT, pixel goldens, CI split | UI only (not ported yet) | N/A until Wave 8; informs R0001 (UI framework) | — |
| same range | `FitWidthAnchor.BottomThird`, new `MiddleClickAction`, shortcuts `FitWidth2` (D4) / `Refresh` (R), WebP/HEIC listing (`WicImageFormat`, `IsSupported(path, raw, webpHeic)`) | Yes | Ported (W1-01/02/04/07), goldens extended | sync PR |
| same range | `JournalLineParser` moved to source-generated JSON (AOT) | No (golden `journal-lines` identical) | Nothing to port | sync PR |
| same range | Abstractions: `IShellIntegration` (Explorer "Browse with"), `IUiDispatcher` priorities, `SettingsTarget`, `IFileSystem.TryDeleteEmptyDirectory` | Yes | Ported (Rust verb `PhotoReviewRs`, R0002) | sync PR |
| same range | Settings (`AppSettings` +72, `SettingsStore` +112, `SettingsNormalizer`, new `ContextMenuLayout`), `ComparePairService`, `SiblingFolderService`, `ReviewCatalog`, `SessionStore`, `PhysicalFileSystem`, `DragDropInputService` | Yes | Not ported yet: Wave 2 contracts written against `d92d6aae` | Wave 2 |
| same range | `FileActionService` +117, `UndoService`, `RecoveryRetryService`, `OperationJournal`, `JournalTransaction`, `PartialDestinationCleanup` | Yes | Not ported yet: Wave 3 contracts must use `d92d6aae` or later | Wave 3 |
| same range | Languages (en/vi +113 lines, notes) and test fixtures (pixel goldens, viewport layout, PreviewCache) | Yes (text) | Synced | sync PR |
| `d92d6aae` (no baseline change) | P-15: `tools/sync-from-csharp.ps1` also maps `tests/PhotoReview.Core.Tests/Fixtures/` files: `operations-legacy.jsonl`, `journal-older-build/*.jsonl` (5, Wave 3 W3-01/W3-03) and `config-v1.json`/`config-v2.json` (W2-03, copied by hand before, now hash-checked; bytes unchanged). Re-run at `d92d6aae`: 81 files, only additions to `assets/SYNC-MANIFEST.txt` | No (test inputs) | Synced | Wave 3 integration |
| `57b04243` (#524) | NOWPF WARP edge documentation | No | Reference only | — |
| `46d33fa1`, `55a37986`, `9800058e`, `170a2d19` (#525, #527, #529, #530) | FEAT-RENAME design, Core rename actions, catalog/session/presenter/cache state, Win32 UI and F2 shortcut | Yes | Deferred; baseline lane only syncs golden-driven settings and shortcut data. No W9-R owner found in contracts or wave plans | Unassigned |
| `d9623945` (#526) | External editors list, settings migration/repair, default editor shortcut and submenu | Yes | Golden-driven settings/shortcut data in this lane; editor rules and UI deferred to W9-E1..E9 | W9-E1..E9 |
| `f6662778` (#528) | STA test host timing fix | No | Test infrastructure only | — |
| `a6e530f2` (#531) | Shell-only release workflow | Packaging | Deferred to release tooling owner | — |
| `02aaee0a` (#532) | Language convention documentation | No | Reference only | — |
| `7eaee2a2` (#533) | Unknown `Operation` in typed/imported action list is a JSON error; config loader remains lenient | Yes | Rust action-list parser checked against regenerated golden; config loader parity tracked separately | baseline lane |

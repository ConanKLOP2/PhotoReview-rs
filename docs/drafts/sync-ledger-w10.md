# Proposed C# sync ledger rows

The range `d92d6aae..7eaee2a2` contains the following proposed rows for commits not adequately described by the current ledger. The existing external-editors row describes #526 in outline; its proposed row below supplies the commit and the narrower Rust status. The existing `7eaee2a2` (#533) row should be checked as noted after the table, not duplicated.

| C# PR / commit | Summary | Behaviour? | Rust status | Rust PR |
|---|---|---|---|---|
| `57b04243` (#524) | Documents the WARP pixel-golden edge and its test tolerance. | No (documentation only; no product or test code changed) | N/A | — |
| `46d33fa1` (#525) | Documents the proposed rename design before implementation. | No (documentation only) | N/A | — |
| `d9623945` (#526) | Multiple named external editors, a selected default, argument templates and optional RAW original; adds editor management, menu, shortcut and settings UI, with legacy single-path migration. | Yes | Partial: Rust has only `external_editor_path` and the single-editor command in `crates/pr-app/src/viewmodels/main/commands.rs`; `contracts/README.md` records the new editor types and dialogs as unported (P-44). | Not ported (P-44 follow-up proposed in `contracts/README.md`) |
| `55a37986` (#527) | Adds `RenameNamePolicy`, journaled same-folder `RenameAsync` and capture-group rename, including case-only handling and undo/recovery changes. | Yes | Not ported: no rename API or name policy in `crates/pr-core/src/file_actions/`; `contracts/README.md` identifies rename as post-baseline work. | — |
| `f6662778` (#528) | Fixes `StaTestHost.DrainAsync` to read elapsed time once per iteration before setting a timer interval. | No (test infrastructure only) | N/A | — |
| `9800058e` (#529) | Re-keys the catalog, session, presenter and cache after rename; connects rename and undo to the file-action controller and view model. | Yes | Not ported: no rename path in `crates/pr-core/src/catalog/` or `crates/pr-app/src/coordinators/`; the Rust view-model command remains the baseline set. | — |
| `170a2d19` (#530) | Adds the rename prompt, context-menu command and F2 shortcut to the Win32 UI, plus settings and localization text. | Yes | Not ported: `crates/pr-platform-win/src/dialogs/secondary_window_port.rs` documents `RenamePrompt` as outside the baseline and its support method remains `todo!()`; `contracts/README.md` records the prompt as unported (P-44). | — |
| `a6e530f2` (#531) | Changes release publishing and verification to package the Win32 `PhotoReview.exe` by default, with WPF opt-in. | No (release packaging and CI only; application logic unchanged) | N/A (release packaging only) | — |
| `02aaee0a` (#532) | Adds the C# repository language convention to `AGENTS.md`. | No (instructions only) | N/A | — |

## Existing-row checks

- The current external-editors row covers #526 but leaves its commit unspecified and says only “NOT ported.” The proposed #526 row distinguishes the Rust baseline single-editor command from the new multi-editor feature. `docs/PARITY.md` still describes C-14 through `ExternalEditorPath`; `contracts/README.md` calls the newer dialogs and settings pages unported.
- The current `7eaee2a2` (#533) row already covers the strict typed/imported action-list change. Rust rejects unknown operation names and undefined numeric values in `crates/pr-app/src/presenters/action_list_json.rs`, with tests in `crates/pr-app/tests/action_list_json.rs`; `docs/PARITY.md` records the separate `config.json` difference under `D-actions-unknown-operation`. The row's “already rejects” wording needs a qualification: C# `LenientEnumConverter.TryReadStrict` also accepts a *defined number encoded as a string*, while Rust's current action-list reader accepts defined JSON numbers but not numeric strings. Matching that accepted input is still partial; no updated golden run is verified.

## Proposed baseline move

Move the spec baseline from `d92d6aae` to `7eaee2a2` after the behaviour rows are ported or explicitly deferred. The range changes `src/PhotoReview.Core/Localization/Languages/en.json`, `en.notes.json` and `vi.json` in #526, #527, #529 and #530. These map to `assets/languages/` through `tools/sync-from-csharp.ps1`; rerun that script at `7eaee2a2` and refresh `assets/SYNC-MANIFEST.txt`. No mapped test fixture changed in this range. Regenerate and review the goldens with `tools/oracle.ps1 -Commit 7eaee2a2`, especially action-list JSON after #533. Neither sync nor oracle regeneration was run for this proposal.

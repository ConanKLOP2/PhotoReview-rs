# Oracle baseline diff: d92d6aae to 7eaee2a2

The driver ran every oracle command twice at `7eaee2a2`, with `turbojpeg.dll` and `libraw.dll` present. Consecutive outputs were byte-identical, including `turbojpeg-native-decode`, `ordinal-casing-table`, and `libraw-decoder-native`. The final goldens are preserved as generated. The 288 goldens outside this table differ from master only in `csharp_commit`, which is part of the oracle format; their cases are unchanged.

| Golden | Semantic change and C# attribution | Rust status |
|---|---|---|
| `context-menu` | `RenameFile` ID 18 and shell-only item row in group 5 (#530 `170a2d19`, `ContextMenuLayout.cs`); WPF computed-menu cases exclude it. | Shell-only visibility ported; all 27 context-menu tests pass. |
| `l10n-generator` | 53 new constants and members for editor and rename text (#526 `d9623945`, #530 `170a2d19`). | Catalog assets synced; generator suite passed. |
| `l10n-localizer` | 53 new catalog keys for the same features (#526, #530). | Catalog assets synced; localizer suite passed. |
| `menu-model` | Rename command/menu rows and external editor submenu (#526, #530). | Golden cases and `shell_menus_controller` dispatch coverage pass with `RenameFile = 31`; the rename operation and fuller external submenu W9-E6 remain deferred. |
| `preview-service-sequence` | `evict-keeps-original-dims` now returns `known:false` after path eviction (#529 `9800058e`, rename cache work; exported `PreviewImageService.cs` removes that path's dimensions). | Pure eviction change ported; all 96 preview-service-decode tests pass. |
| `secondary-window-supports` | Enum adds `ExternalEditors` and `RenamePrompt` (#526, #530). | Deferred W9-E8 and rename dialog (no W9-R owner). |
| `settings-dialog-frame` | Files, shortcuts, and mouse page item counts increase (#526, #530). | Deferred W9-E4 and rename UI (no W9-R owner). |
| `settings-dialog-state` | Group/control IDs shift and shortcut boxes increase 27 to 29 (#526, #530). | Shortcut page now declares all 29 boxes; remaining dialog-state changes deferred W9-E4 and rename UI (no W9-R owner). |
| `settings-field-map` | Three editor fields, two shortcut boxes, and `RenameFile` menu tick (#526, #530). | Presenter data and native shortcut boxes ported; other W9-E4 native pages deferred. |
| `settings-json` | Config v4, three editor fields, `OpenInDefaultEditor`, `RenameFile` (#526, #530). | JSON/default data ported; editor list rules W9-E1 deferred. |
| `settings-normalize` | v4 repairs, shortcut defaults, and hidden `RenameFile` parsing (#526, #530). | Exercised golden suite passed; editor reconciliation W9-E1 deferred. |
| `settings-page-files` | Manage editors control and RAW-original option (#526). | Deferred W9-E4. |
| `settings-page-mouse` | `RenameFile` context-menu tick (#530). | Data ported; native page deferred, no W9-R owner. |
| `settings-page-shortcuts` | 29 cells; default editor and rename boxes (#526, #530). | Native page now has 29 boxes and 16 optional clear buttons; full `pr-platform-win` test suite passes. Presenter data and F2 rename routing ported. Default-editor routing W9-E7 and rename operation deferred. |
| `settings-parse-text` | v4 and new settings/shortcut serialization (#526, #530). | Golden suite passed; editor reconciliation W9-E1 deferred. |
| `settings-store` | v4 persisted payload and fields (#526, #530). | Golden suite passed; editor migration/repair W9-E1 deferred. |
| `settings-validate` | New `RenameFile` shortcut validation messages (#530). | Golden suite passed. |
| `shortcut-canonical` | 30 properties, 16 optional names, default editor and F2 rename (#526, #530). | Ported; `shortcut_defaults_match_golden` enabled and passed. |
| `translation-export` | Exported catalog content grows with editor/rename strings (#526, #530). | Catalog synced; export suite passed. |
| `win32-constants` | Dialog constant `EnChange = 768` (#526, dialog editor UI). | Constant ported; golden test passes. Native editor dialog remains W9-E2/E3. |

## Unexplained

None. The exported `ContextMenuLayout.cs` resolves the apparent context-menu discrepancy: `RenameFile` is marked `ShellOnly`, while the old computed-menu cases use the default WPF context (`IncludeShellOnly = false`). The Win32 menu-model cases include it.

## Deferred

External editor list reconciliation/migration is W9-E1; dialogs and the remaining native settings pages are W9-E2/E3/E4; launcher is W9-E5; submenu/model is W9-E6; input routing is W9-E7; secondary-window wiring is W9-E8; shell menu wiring is W9-E9. The rename feature's file actions, catalog/session state, and UI have no W9-R owner in `contracts/` or `docs/WAVE-*.md` and remain outside this baseline lane. The `shell_menus_controller` suite passes after routing `RenameFile` through `ReviewCommandType::RenameFile = 31`; shell input accepts this command without starting the unported operation. The shortcut page now has all 29 field-map cells, including `OpenInDefaultEditor` and `RenameFile`. No failing test was ignored or removed.

`settings_shortcuts::shortcut_defaults_match_golden` is enabled and passes. Its separate `serialize_review_action_uses_pascal_case_names` remains ignored as requested; its W1-02 `serde_ext` dependency exists now, so the ignore reason is stale and should be reviewed by that test's owner.

Status: Partial. general.rs and shortcuts.rs fully evaluated; nav.rs evaluated once, with 10 survivors and only 1 of them re-proven against the new killers. About 280 of the 360 settings mutants listed by CX-SETTINGSWIN2/3 are NOT run. No production changes. PARITY not raised.

# CL-MUT-SETTINGS3: settings pages general, shortcuts and nav (2026-11-01)

Lane `feat/cl-mut-settings3`. Continues [CX-SETTINGSWIN2](CX-SETTINGSWIN2-2026-10-31.md) and [CX-SETTINGSWIN3](CX-SETTINGSWIN3-2026-10-31.md).
New test binary `crates/pr-platform-win/tests/w8_settingswin4_killers.rs`: 27 tests (22 without a desktop; 5 native `#[ignore]` tests that open a hidden Settings window and call `SetKeyboardState` on their own thread only). Registered in `.github/workflows/native-nightly.yml` (no nightly run yet). No golden files; inline fakes (recording `SettingsPageHost`, fake shell, fake localization over `FakeFileSystem`).

## Method

`cargo mutants -p pr-platform-win --baseline skip --timeout 300 -j 1 --file <file>` with `--cargo-test-arg=--test <bin> ... -- --include-ignored --test-threads=1`, via `tools/dev/mutants.sh` for the first runs (preflight `running N tests`, N > 0, exit 0) and the same flags by hand for `-F`/`-E` shards. One cargo-mutants at a time. Timeout would count as NOT detected. `grep "no tests/golden above"` over all logs: 0 hits.

Defect found in the method: `mutants.sh` puts the target dir in `$TEMP/mut-target-<crate>`, shared by every lane that mutates the same crate. The first general/shortcuts runs hit "Blocking waiting for file lock" (2 shortcuts timeouts) and are NOT used for the final numbers. All final runs use a private `TEMP` (`%TEMP%\clms3`). One run also produced 2 false "unviable" results ("only metadata stub found for core", toolchain error); they were rerun and are counted caught.

## Results (final runs, private target)

| File | Mutants | Caught | Missed | Unviable | Timeout | Test binaries |
|---|---:|---:|---:|---:|---:|---|
| pages/general.rs | 38 | 35 | 0 | 3 | 0 | settings_page_general + w8_settingswin4_killers |
| pages/shortcuts.rs | 65 | 46 | 14 | 5 | 0 | settings_page_shortcuts + settings_shortcuts_l10n + killers |
| dialog/nav.rs | 40 | 29 | 10 | 1 | 0 | settings_dialog_frame(+e2e native) + killers (run BEFORE the native killers existed) |

general.rs before the killers (existing `settings_page_general` only): 19 caught, 16 missed, 3 unviable. The 16 are killed now (12 pure killer tests). Unviable are E0277 (no `Default` for the return type).

shortcuts.rs: 45 non-Ambient mutants 40 caught, 5 unviable (E0277/E0271), 0 missed. The 20 `AmbientSettingsTexts` mutants: 6 caught (`format`, `action_default_name`, `action_default_backup`, through the default-action names of the editor), 14 missed. Those 14 (7 trait methods x 2: shortcut_invalid, action_invalid, action_binding_name, shortcut_duplicate, settings_load_repaired, dialog_import_settings_repaired, settings_load_shortcut_disabled) are unreachable: the private type is only passed to `actions_for_editor`, which calls only `action_default_name` and `action_default_backup`. They are equivalent for every caller (dead trait methods); no test can observe them. Candidate for removing the dead methods in a later production change.

nav.rs (first run, 40 mutants): 29 caught, 1 unviable, 10 missed: 73:21 `&&`->`||`, 89:16 `!=`->`==`, 130:61, 136:52, 153:21, 153:13 (delete `!`), 166 `on_nav_selection_changed` -> (), 175 `key_down` -> false, 175:76 `<`->`==`, `<`->`<=`. Killers written for all of them: pure ones (`page_of`, `classify_dialog_key`, VK constants) and 5 native tests (row selection follows `activate_page`, LBN_SELCHANGE opens the page, SYSKEYDOWN/KEYDOWN reach the key box without asking the Control option, a foreign window with a key box id is not a key box, Ctrl/Shift state through `SetKeyboardState`). The killers binary passes 27/27 with `--include-ignored` (12.5 s).

Re-run of nav survivors against the killers alone was stopped by the orchestrator after 7 of 21 mutants: caught 4 (66 activate_page -> (), 89:16 `!=`->`==`, handle_dialog_key -> true/false), missed 3 (73:21, 73:16, 73:28: those are caught only by the older e2e binary, not by the killers alone, so no verdict against the union). The other 8 target survivors (130:61, 136:52, 153:21, 153:13, 166, 175 x3) were NOT re-run: the killers are written for them and pass unmutated, but they are NOT proven by mutation. 73:21 (`old >= 0 && old != index` -> `||`) is believed equivalent for end state: it only hides then re-shows the same viewport (viewport_id(-1)=109 is no control).

## NOT run

- All other settings files: pages/mouse, display, files, performance, diagnostics, dialog/ (apply, close, create, dirty, footer, host, keybox, messages, page_layout, state, surface, validate, frame), builder/*, catalog, context, page_view: about 280 of the listed mutants. Win32/COM glue skipped.
- nav.rs: mutation proof of the 8 survivors above; no full union run of nav with killers + existing e2e.
- Workspace tests, Clippy, nightly confirmation of the 5 native tests (a desktop session is needed).

## Proposed PARITY note

Keep `X-settings-ui-window` Partial. Add: general.rs 35/38 caught (3 source-unviable), shortcuts.rs 46/65 caught with 14 dead-code equivalents and 5 unviable, nav.rs 29/40 caught before the native killers, 10 killers pending mutation proof. No promotion.

## Cleanup for the orchestrator

Build artifacts left (agent may not delete): `C:\Users\adminvti\AppData\Local\Temp\clms3` (private target `mut-target-pr-platform-win`, all `out-*` mutants.out dirs and logs), `C:\Users\adminvti\AppData\Local\Temp\mut-out-clmutsettings3-general`, `...\mut-out-clmutsettings3-shortcuts`, `...\out-general-final`, `...\mut-out-pr-platform-win-*` from the first script runs, and in the worktree `target\` (test build) and `target\clms3-data`. The shared `Temp\mut-target-pr-platform-win` belongs to other lanes too.

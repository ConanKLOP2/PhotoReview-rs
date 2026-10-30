# CL-SETTINGSUI 2026-10-30: mutation evidence for the Settings UI framework, pages and window rows

Status: partial. Lane `cl-settingsui` (branch `feat/cl-settingsui`). Rows `X-settings-ui-framework`, `X-settings-ui-pages`, `X-settings-ui-window` stay Partial. No production code changed.

Killers file: `crates/pr-platform-win/tests/w8_settingsui_killers.rs` (11 tests, fake surface with dx = 2n and dy = 3n so no operator or constant is degenerate; no window, no sleeps). It pins 38 placements of `GroupElement`, `CheckElement`, `RadioElement`, `CheckFlowElement`, `RowElement`, `SliderElement`, `TextElement` and `ButtonsElement` (values read back against the C# arithmetic by hand for group, check and slider) plus `label_width`, accessors, Debug and control ids. Builds on [CL-DLG](CL-DLG-2026-10-30.md) (680 mutants, 0 missed).

## Method

`cargo mutants -p pr-platform-win --baseline skip --timeout 90 -j 1`, `CARGO_TARGET_DIR=<worktree>/target-mut`, outputs `mut-out-sui1` and `mut-out-sui2` (not committed).

| Run | Files | Binaries | Result |
|---|---|---|---|
| sui1 (before killers) | builder/{groups,rows,inputs}.rs (352 listed) | `--lib` + 13 `settings_*` binaries | stopped after 21 tested: 11 caught, 10 MISSED, all in `groups.rs` (`add`/`sub`/`mul` -> 0/1/-1, `<Debug for GroupElement>::fmt`, `children`): the existing layout tests never assert a group placement |
| sui2 (with killers) | builder/{groups,rows}.rs (153 listed) | `w8_settingsui_killers` + `settings_builder_layout` | stopped after 21 tested (the first 21 `groups.rs` mutants, same ones): 19 caught, 0 missed, 2 unviable (`children`/`children_mut` -> `Vec::leak(vec![Default::default()])`), 0 timeout |

Throughput on the shared 12-core machine was about 2 mutants per minute, so the 90-minute time-box ended the run early. The 10 sui1 survivors are the same mutants killed in sui2.

## NOT run (still unmutated)

- groups.rs after line 70 and all of rows.rs (about 132 mutants): killers exist for them (`check_family` pins every placement) but the mutation run did not reach them.
- builder/inputs.rs (199), context.rs (94), dialog/host.rs (68), dialog/frame.rs (190), dialog/surface.rs (130): not mutated. Win32 glue in host/frame/surface (window creation, `ScrollInfo`, GDI) is out of scope by policy; the pure part still needs a run.
- Native `--include-ignored` tests, DPI/GDI/language scenarios, C# runtime page type labels, goldens `settings-layout` and `settings-builder` (not loaded by any `settings_*` test file), Shortcuts English action names (D-W8B5-806), and an attended walkthrough: human-only.

## Rows

| Row | Evidence | Verdict |
|---|---|---|
| X-settings-ui-framework | `catalog`, `page_view`, `builder/layout` mutation-clean (CL-DLG: 3 groups, 0 missed); `groups.rs` first 21 mutants clean now | Partial: rest of groups/rows, inputs, context unmutated; two goldens not loaded |
| X-settings-ui-pages | seven pages mutation-clean (CL-DLG dlg3: 244 tested, 226 caught, 18 unviable, 0 missed) | Partial: per-page goldens not loaded by name, English Shortcuts names, post-baseline external editors (P-44). The mutation gap is closed; the non-mutation gaps are not human-only except the attended check |
| X-settings-ui-window | `apply, close, validate, state, dirty, nav, keybox, messages, create, footer, page_layout` clean (CL-DLG) | Partial: host, frame, surface not mutated; DPI/GDI/language and C# runtime type labels need a person at the machine |

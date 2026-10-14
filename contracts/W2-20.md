# W2-20: ContextMenuLayout (context-menu item ids, item table, hidden-set parsing and the pure visibility rule)
- Color: 🟡 | Porter: **sonnet** (one real .NET quirk: `Enum.TryParse` on a non-`[Flags]` enum accepts numbers, signs, whitespace and
  `,`-joined names whose values are OR-ed; everything else is small, table-driven logic) | Test porter: haiku | Verifier: sonnet
- C# source (baseline d92d6aae): `src/PhotoReview.Core/Settings/ContextMenuLayout.cs` (182 lines: enum `ContextMenuItemId`, records
  `ContextMenuItemInfo`, `ContextMenuContext`, `ContextMenuEntry`, class `ContextMenuLayout`, static `ContextMenuItems`).
  Read with `git -C C:/MyProjects/PhotoReview show d92d6aae:src/PhotoReview.Core/Settings/ContextMenuLayout.cs`.
- C# tests to port (all at d92d6aae): `tests/PhotoReview.Core.Tests/Settings/ContextMenuLayoutTests.cs`, the 13 NON-store tests
  (lines 25-177): `Nothing_hidden_shows_every_item_with_a_separator_between_each_group`,
  `Item_ids_are_stable_names_and_the_table_has_no_duplicates` (names come from `ContextMenuItemId::ALL[..].name()`; also assert
  `ITEMS.len() == ALL.len()` and no duplicate id), `Copy_items_need_an_open_photo_on_top_of_the_user_flag`, `An_emptied_group_takes_its_separator_with_it`,
  `First_group_never_starts_with_a_separator`, `Hiding_everything_leaves_only_Settings_without_any_separator`,
  `Settings_cannot_be_hidden_even_when_listed`, `Menu_always_has_at_least_one_item_for_every_combination_of_flags`,
  `Zoom_submenu_collapses_when_the_user_hides_it_or_all_of_its_children`,
  `Zoom_submenu_children_are_toggled_individually_and_separators_follow_the_groups`, `Legacy_flags_map_to_the_hidden_set`,
  `EffectiveHidden_prefers_the_stored_list_and_treats_empty_as_show_everything`,
  `ApplyHidden_stores_names_in_menu_order_and_mirrors_the_legacy_flags`.
  - `Menu_always_has_at_least_one_item...`: the C# comment says 2^11, but there are **12** unlocked top-level items (Undo, MoveToRecycleBin,
    Fit, ZoomToLevel, ZoomSubmenu, Refresh, OpenFolder, NextFolder, PreviousFolder, ExternalEditor, CopyFileName, CopyFullPath), so the loop is
    2^12 = 4096 masks x 2 contexts. Port it in full (cheap); the golden samples 174 of them.
  - NOT here (they need `SettingsStore`/`AppSettings` JSON, owned by W2-01/W2-03): `Load_old_config_without_the_list_migrates...`,
    `Load_config_with_no_menu_settings_at_all_keeps_todays_defaults`, `Load_an_explicit_list_wins_over_the_legacy_flags...`,
    `Load_drops_unknown_duplicate_and_locked_names_and_reports_the_repair`, `Load_a_list_of_the_wrong_json_type_is_salvaged...`,
    `Save_then_Load_round_trips_the_list`, `Clone_and_json_export_keep_the_list_and_an_empty_list_stays_empty`. Other consumers
    (`SettingsRobustnessTests` line 374, `ConfigKeyContractTests`) belong to W2-01/W2-03; the App-layer users (`ContextMenuModelBuilder`,
    `SettingsPresenter`, `SettingsFieldMap`, `MenuGoldenRecorder`/`tests/Fixtures/golden/context-menu.v1.json`) belong to the App waves.
  - Golden test (new, required): `tests/golden/context-menu.json` (510 cases, see Golden package).
- PARITY rows: C-07, C-08, C-09, C-11, C-13, C-15, C-16 (hidden set, grouping and separator rules; the gesture text, the external editor's
  "only when `ExternalEditorPath` is set" rule of C-14 and the checked/enabled states are App-layer, NOT here: `compute` knows only
  `has_photo`), K-38. Suggested new row: `S-xx` "Context menu items hidden set / legacy flags migration".
- Owned files (the ONLY files you may create/modify):
    crates/pr-core/src/settings/context_menu.rs      (stub: contracts-w2/W2-20/context_menu.rs, compiles clippy-clean)
    crates/pr-core/tests/settings_context_menu.rs
- Lead-owned edit: `crates/pr-core/src/settings/mod.rs` gets `pub mod context_menu;`. Read-only input: the golden above.
- Stub note: `item_info` returns `&'static ContextMenuItemInfo` from a `const` `ITEMS`; index it (`&ITEMS[pos]`, rvalue-promoted) or make `ITEMS` a `static` of the same type; do not return a reference into a local copy.
- Public API (stub, do NOT change signatures): `ContextMenuItemId` (+ `ALL`, `name`, `from_value`), `ContextMenuItemInfo`,
  `ContextMenuContext`, `ContextMenuEntry`, `ContextMenuLayout` (+ `is_visible`, `has_separator_before`), `ITEMS`, `DEFAULT_HIDDEN`, `item_info`,
  `legacy_hidden`, `effective_hidden`, `parse_hidden`, `to_stored`, `apply_hidden`, `compute`.
- Semantics notes (verified against the C# source and the 510-case golden produced by the oracle at d92d6aae):
  1. **Enum values and names are persistent.** `HiddenContextMenuItems` in `config.json` stores the NAME (`id.ToString()`); numeric values
     are declaration order 0..=17 and `Refresh` is last (17) although it sits in group 2 of the table. The stub's `#[repr(i32)]` values and
     `name()` match; do not reorder.
  2. **Item table** (`ITEMS`, order matters): top level `Undo`(g1), `MoveToRecycleBin`(g1), `Fit`(g2), `ZoomToLevel`(g2), `ZoomSubmenu`(g2),
     `Refresh`(g2), `OpenFolder`/`NextFolder`/`PreviousFolder`(g3), `ExternalEditor`(g4), `CopyFileName`/`CopyFullPath`(g5, needs_photo),
     `Settings`(g6, locked); children of `ZoomSubmenu`: `ZoomFitWidth`/`ZoomFitWidth2`/`ZoomFitHeight`(g1), `ZoomPresets`(g2),
     `ZoomLevelOptions`(g3). `to_stored` iterates this table, so stored order is Undo, MoveToRecycleBin, Fit, ZoomToLevel, ZoomSubmenu,
     **Refresh**, OpenFolder, ... (Refresh before OpenFolder; golden `to_stored` cases 4 and 5). `DEFAULT_HIDDEN` = {MoveToRecycleBin, Fit,
     ZoomToLevel, ZoomSubmenu} (golden `items.default_hidden`); it equals `legacy_hidden(AppSettings::default())` because the C# defaults are
     zoom=false, folder=true, recycle=false.
  3. **`parse_hidden`** (the only non-trivial part). For each element: `None` => skip. Otherwise emulate .NET 10
     `Enum.TryParse<ContextMenuItemId>(s, ignoreCase: false)` followed by `Enum.IsDefined`, then drop `Settings` (locked). Algorithm, pinned
     by the 77 `single_name` golden rows:
     a. Trim leading and trailing Unicode white space (`char.IsWhiteSpace`: includes U+00A0, U+2003, U+3000, `\t`, `\n`); empty => skip.
        Do NOT trim `\0`. Do not use ASCII-only trimming.
     b. If the first char is an ASCII digit, `+` or `-`: parse the whole text as `i32` with an optional sign and ASCII digits only
        (`Int32.TryParse(AllowLeadingSign, Invariant)`): leading zeros ok (`003` => 3), `-0` => 0, `+1` => 1, trailing `\0` characters are
        accepted (`"3\0"` => 3, golden) but a leading `\0` is not; overflow (`2147483648`), `0x3`, `3.0`, `1e0`, `1_0`, `3 4`, `+`, `-`,
        `--1`, fullwidth/Arabic-Indic digits (`１`, `٣`) all fail. If it parses, the value is that integer, DONE (no name fallback needed;
        `3,4` and `3, Fit` are not numbers and also fail as names).
     c. Otherwise split on `,`; every part is trimmed (same whitespace rule) and must be non-empty and EXACTLY (case-sensitive, ordinal) one
        of the 18 names; numbers inside a list fail (`Fit,3`), `Fit,,Undo`, `Fit,` and `,Fit` fail, `FIT`/`undo` fail. The result value
        is the bitwise OR of the parts' values: `Fit,Undo` => 2|0 = Fit, `ZoomFitWidth,ZoomFitWidth2` => 12|13 = 13 = ZoomFitWidth2,
        `Fit,ZoomToLevel` => 3 = ZoomToLevel, `Undo,MoveToRecycleBin` => 1, `ZoomPresets,ZoomLevelOptions` => 31 (undefined => dropped),
        `Settings,Fit` => 11 (Settings => dropped). A trailing/embedded `\0` after a name fails (`Undo\0` => nothing).
     d. The value must be one of 0..=17 (`ContextMenuItemId::from_value`), then not `Settings`. Negative, 18, 99, `i32::MAX` => dropped.
     Result is a set (duplicates collapse). Do not add a Rust enum-parsing crate; this is ~40 lines on `&str`.
  4. **`effective_hidden`**: `hidden_context_menu_items` is `Option<Vec<Option<String>>>` (W2-01): `None` => `legacy_hidden`; `Some(list)` =>
     `parse_hidden(list)` (`Some(vec![])` = show everything, even when the legacy flags say hide). `Settings` never appears.
  5. **`legacy_hidden`**: zoom flag false hides {Fit, ZoomToLevel, ZoomSubmenu} (NOT Refresh, not the children), folder flag false hides
     {OpenFolder, NextFolder, PreviousFolder}, recycle flag false hides {MoveToRecycleBin}.
  6. **`apply_hidden`**: drop locked ids first, `hidden_context_menu_items = Some(to_stored(set).into_iter().map(|s| Some(s)).collect())`
     (always `Some`, empty list stays `Some(vec![])`), then mirror: `show_zoom_menu_items = !(Fit && ZoomToLevel && ZoomSubmenu all hidden)`,
     `show_folder_menu_items = !(OpenFolder && NextFolder && PreviousFolder all hidden)`, `show_recycle_menu_item = !MoveToRecycleBin hidden`.
     So hiding only two of the three zoom items leaves the flag `true` (golden). The mirror is lossy by design.
  7. **`compute(hidden, context, parent)`**: take table rows whose `parent == parent` (`None` = top level; ids of the other level in `hidden` are
     simply never consulted: golden `compute_misc`), group by `group` in order of first appearance (groups are 1..6 and already ascending, but
     iterate by first appearance, not by sorting, so a future table change stays faithful), keep the rows for which `item_shows` is true;
     skip empty groups; emit a separator `ContextMenuEntry { item: None, group }` (the NEW group's number) only when an earlier group was
     emitted; then the items with `item: Some(id), group`. `item_shows`: false when `!locked && hidden.contains(id)`; false when `needs_photo
     && !has_photo`; if the item has children (rows with `parent == Some(id)`) the item shows iff at least one child shows (children are
     evaluated with the same `hidden`/`context`, recursively; the child's own group does not matter); else true. Consequences pinned by the
     golden: hiding all five Zoom children removes `ZoomSubmenu` from the top level (`submenu_visible_on_top_level` false) and the submenu
     level computes to empty; `Settings` always shows, even with `has_photo == false`; the top level is never empty and never starts, ends
     or doubles a separator.
  8. **`is_visible(id)`**: any entry with `item == Some(id)`. **`has_separator_before(group)`**: any entry with `item == None && group == g`.
     On the default layout the separators are before groups 2, 3, 4, 5, 6 (golden `layout_queries`; group 1 never has one).
- Traps checklist (docs/MULTI-AGENT-PLAN.md §7): 1 (names are compared ordinal and case-sensitive: `undo`/`FIT` are not accepted; do not
  `to_lowercase`), 2 (white space is Unicode `char::is_whitespace`, whose set equals .NET `char.IsWhiteSpace` (Zs/Zl/Zp, U+0009-000D, U+0085, U+00A0; golden uses U+00A0, U+2003, U+3000);
  digits are ASCII only: use `is_ascii_digit`, never `char::is_numeric`/`is_alphanumeric`; fullwidth and Arabic-Indic digits must fail), 10 (use `Vec` for entries and `to_stored`; a
  `HashSet` only for membership, never iterate it to produce output: `to_stored` iterates `ITEMS`), and:
  - `Enum.TryParse` quirk above: do NOT implement `parse_hidden` as `ALL.iter().find(|i| i.name() == s)`; it would fail the 29 golden `single_name` rows (non-empty result, not an exact name) with
    numbers, whitespace and comma lists (`"0"` => Undo, `" Undo "` => Undo, `"Fit,Undo"` => Fit).
  - `Refresh` has value 17 but sorts between `ZoomSubmenu` and `OpenFolder` in `ITEMS`; never sort by the enum value (the stub derives `Ord`
    only for use in sets, do not use it for output order).
  - `ContextMenuItemInfo` defaults `Locked = false, NeedsPhoto = false`; only `Settings` is locked, only `CopyFileName`/`CopyFullPath` need a photo.
- Depends on: **W2-01** `settings::app_settings::AppSettings` (fields `hidden_context_menu_items: Option<Vec<Option<String>>>`,
  `show_zoom_menu_items`, `show_folder_menu_items`, `show_recycle_menu_item`, `Default`; the stub imports it, so it will not compile on its
  own until W2-01 is merged or its stub is in the tree). Nothing else (std only). **Reverse dependency:** W2-03 (`SettingsStore`
  migration `hidden_context_menu_items` from legacy flags, `legacy_hidden`/`to_stored`) and W2-02 (normalizer) call this module; merge W2-20
  right after W2-01.
- Done when: every listed test has a Rust twin tagged `// C#: ContextMenuLayoutTests.<Method>`; every one of the 510 golden cases matches
  (`items` incl. `names`/`values`/`default_hidden`, 10 `parse_hidden`, 77 `single_name`, 8 `legacy` incl. `effective_with_null_list`, 12
  `effective`, 5 `to_stored`, 11 `apply_hidden` incl. the three flags, `top_level_hideable`, 174x2 `compute` with `render` and `entries`,
  `submenu_children`, 32 `compute_submenu` incl. `submenu_visible_on_top_level`, 3 `compute_misc`, `layout_queries`); clippy clean; the stub's
  `#![allow(unused_variables, dead_code)]` line removed with the last `todo!()`.

## Golden package
- `oracle/W2-20.cs`: command `context-menu` (dispatch line in the file header; copy to `tools/oracle-cs/Commands/W2-20.cs`). Uses only
  `PhotoReview.Core.Settings` (no W2-00 helper). Output kinds as listed above; `render` is the C# test helper's string (`,`-joined names,
  `|` for a separator) and `entries` is `[[name|null, group], ...]`.
- `golden-inputs/context-menu.json`: 10 `parse_hidden` lists (null elements, duplicates, locked, empty strings), 77 `single_names`
  (case, every whitespace flavour, decimal spellings, signs, overflow, comma lists, NULs, look-alike digits), 8 `settings_flags` (all
  combinations), 12 `effective`, 5 `to_stored`, 11 `apply_hidden`, 174 `masks` (all single bits, all-but-one, pairs, boundaries 0 and 4095)
  and all 32 `submenu_masks`.
- `golden-reference/context-menu.json` (294 KB) produced at C# commit d92d6aae; I re-checked the `single_name`, `legacy`, `effective`,
  `to_stored` and `apply_hidden` rows against the C# source by hand and they agree.

## Open questions
- None blocking. Stub observations (no change made): (1) the stub's `parse_hidden` takes `&[Option<String>]`, which fits W2-01's
  `Option<Vec<Option<String>>>` via `.as_deref()`/`&vec`; (2) `apply_hidden` stores `Some(Some(name))` elements, consistent with that type;
  (3) the stub module doc cites `contracts/W2-20/contract.md`, which is where the lead moves this file on merge.

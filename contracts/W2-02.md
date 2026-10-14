# W2-02: SettingsNormalizer + SettingsValidator + SettingsLoadRepairText + SettingsParseResult
- Color: 🟢 | Porter: **sonnet** (see "Porter model") | Test porter: haiku | Verifier: sonnet
- C# sources (baseline d92d6aae), under `src/PhotoReview.Core/Settings/`:
  - `SettingsNormalizer.cs` (297 lines): `Normalize`, `DisableConflictingOptionalShortcuts`
  - `SettingsValidator.cs` (41 lines): `ValidateShortcuts`
  - `SettingsLoadRepairText.cs` (23 lines): `Build`
  - `SettingsParseResult.cs` (17 lines): record + `Repairs`
  (read with `git -C C:/MyProjects/PhotoReview show d92d6aae:src/PhotoReview.Core/Settings/<file>`)
- C# tests to port (all at d92d6aae; `tests/PhotoReview.Core.Tests/Settings/` unless noted):
  - `SettingsNormalizerMutationGapTests.cs` (139 lines, 11 tests): `Normalize_DefaultSettings_ReportsNothing`,
    `Normalize_SourceBytesCapacity_MustBePositive`, `Normalize_MemoryReserve_MayBeZeroButNotNegative`,
    `Normalize_PreloadMemoryLoadLimit_MustBeInZeroExclusiveToOneInclusive`, `Normalize_UndefinedJournalDurability_IsResetToFast`,
    `Normalize_UndefinedRawFullDecode_IsResetToItsDefault`, `Normalize_UndefinedRawPairMode_IsResetToItsDefault`,
    `Normalize_ActionWithUndefinedOperation_BecomesMoveAndIsReported`, `Normalize_ValidActions_AreNotReported`,
    `DisableConflictingOptionalShortcuts_NoActionsList_DoesNotThrow`, `CanonicalizeAll_NoActionsList_StillCanonicalisesTheShortcuts`.
    The "undefined enum" tests assign `(T)99` (`JournalDurability`, `RawFullDecode`, `RawPairMode`) and `(FileOperationType)99`: Rust cannot
    hold that. Port each through the golden-style path (`AppSettings::from_json` of `{"JournalDurability": 99}`) and expect the lenient read (value 0, no report) as the golden `... garbage is silently zero` cases do; for the
    `Option<..>` fields (`loading_mode`, `image_sort_mode`, ...) set `None` directly and expect the reset. `NoActionsList` tests: Rust `actions` is a
    `Vec` (never null): use an empty Vec. `CanonicalizeAll_NoActionsList...` belongs to W2-01 (`AppSettings::canonicalize_all`); port it here only if
    W2-01 did not.
  - `SettingsNormalizerMandatoryShortcutConflictTests.cs` (59 lines): `MandatoryShortcutSet_IsTheThirteenValidatedBindings`,
    `OptionalShortcut_SharingAMandatoryKey_IsDisabledAndSettingsStayValid` (Theory: 13 mandatory x 14 optional; build the grid from
    `ShortcutMappings::PROPERTY_NAMES` / `OPTIONAL_NAMES` minus `MoveToFolder2`).
  - `SettingsValidatorTests.cs` (121 lines): `DefaultSettings_AreValid`, `InvalidShortcutKey_ReturnsError`, `InvalidAction_ReturnsError`,
    `DuplicateKeys_ReturnsConflictMessage`, `ActionAndShortcutConflict_ReturnsConflictMessage`. NOT ported (no null in Rust):
    `NullSettings_ThrowsArgumentNullException`, `NullValidator_ThrowsArgumentNullException`.
  - `SettingsValidatorRobustnessTests.cs` (123 lines): `BlankMandatory_NamesTheProperty`, `Optional_BlankOk_DuplicateStillConflicts`,
    `Duplicate_IgnoresCaseAndPadding`, `Fuzz_ConflictOracle` (seeds 1-3, 500 rounds: use a deterministic xorshift written in the test, no `rand`;
    the oracle compares `to_ascii_uppercase`, which is enough for the ASCII pool). NOT ported: `NullShortcuts_ReportsInvalid`,
    `NullActionEntry_ReportsInvalidAction`, `NullActions_IsFine` (the Rust types cannot hold null; the reader drops null actions, see W2-01
    `ReadNotes`).
  - Tests elsewhere that call `SettingsNormalizer.Normalize` directly (22 files: `ToolbarAutoHideSettingsTests`, `TouchpadRefreshSettingsTests`,
    `ViewerQuickSettingsTests`, `PreloadWindowSettingsTests`, `MouseSettingsTests`, `DecoderBackendNormalizationTests` ...) are ported with their own
    settings unit (W2-01 / W2-03 / W2-04). The golden `settings-normalize` already pins every range and enum rule of this unit, so they are not
    a requirement here; port one of them here only when it needs nothing but `normalize`.
  - NOT here: `ShortcutKeyCanonicalTests` / `ShortcutKeyCanonicalPropertyTests` (W1-xx `shortcuts.rs`), `SettingsStoreLoadRepairPersistenceTests`
    and `SettingsParsePropertyTests` (W2-03, they drive `SettingsStore.ParseText`).
  - Golden tests (new, required): `tests/golden/settings-normalize.json` (294 cases), `tests/golden/settings-validate.json` (84),
    `tests/golden/settings-repair-text.json` (22).
  - Hand-written (no C# twin): `SettingsParseResult::repairs()` (reset names first, then `"Shortcuts." + name`; a `disabled` list of two keeps order).
- PARITY rows: K-32 (shortcut validation, duplicate detection), C-28 (settings repair notice text), S-64 (load and repair pipeline: normalize,
  disable conflicting optional shortcuts; the pipeline itself is W2-03), plus the S-* range/clamp rows of `docs/PARITY.md` section 3 that
  `SettingsNormalizer.cs` enforces (section 3 header names it as the repair source).
- Owned files (the ONLY files you may create/modify):
    crates/pr-core/src/settings/normalizer.rs      (stub: contracts-w2/W2-02/normalizer.rs, compiles clippy-clean)
    crates/pr-core/tests/settings_normalizer.rs
- Read-only inputs: the three goldens above. Lead-owned: `settings/mod.rs` gets `pub mod normalizer;`.
- Public API (stub, do NOT change signatures): `SHORTCUT_REPAIR_PREFIX`, `normalize`, `disable_conflicting_optional_shortcuts`,
  `build_repair_text`, `SettingsValidator::{new, validate_shortcuts}`, `SettingsParseResult` (6 pub fields) + `repairs`.
- Semantics notes (checked against the C# at d92d6aae and the golden-reference):
  1. **`normalize` order of reports** (the repair list is observable: the dialog text and the golden pin it). Each check appends its property name at most
     once: `ImageCacheCapacityBytes` (<= 0 -> `PerformanceOptions.ImageCacheCapacityBytes`), `ImageCacheRamPercent` (CLAMP to
     [`MinImageCacheRamPercent`, `MaxImageCacheRamPercent`]), `SourceBytesCapacityBytes` (<= 0 reset), `MemoryReserveBytes` (< 0 reset; 0 valid),
     `PreviewDiskCacheCapacityBytes` (< 0 reset; 0 valid), `PreloadWorkerCount` (<= 0 reset to default; > `MaxPreloadWorkerCount` clamp),
     `PreloadMemoryLoadLimit` (reset unless `0 < x <= 1`; NaN, +-Infinity, -0.0, 5e-324 -> see golden), `PreloadForwardCount` and
     `PreloadBackwardCount` (clamp), `InitialViewMode` (`Percent400` -> `Percent200`, reported), `LoadingMode`, `ImageSortMode`, `ScalingQuality`
     (`None` -> `Preview` / `Default` / `HighQuality`), `DecoderBackend` (`None` OR `LibRaw` -> `WicDirect`), `JournalDurability`, `InstanceMode`,
     `FitWidthAnchor`, `KeyboardZoomAnchor` (`None` -> `ViewportCentre`), `FitWidthAnchor2` (`None` -> `BottomThird`), `MiddleClickAction`
     (`None` -> `ActualSize`), then `Actions`, then `Shortcuts`, then (after `canonicalize_all`) any `Actions[<i>].Shortcut`, then `MouseWheelAction`,
     `KineticGlideSmoothing`, `ClickZoomPercent`, `ArrowPanStepPercent`, `KeyboardZoomStepPercent`, `ToolbarAutoHideDelayMs`,
     `InfoOverlayAutoHideDelayMs`, `ToolbarOpacityPercent`, `InfoOverlayFontSize`, `ImageTransition`, `ImageTransitionMs`, `RawFullDecode`,
     `RawPairMode`, `TouchpadSwipeDistancePerImage`, `HiddenContextMenuItems`. The golden cases `every settings-enum garbage at once` and
     `every numeric setting out of range at once` pin this list; do not reorder.
  2. **Which "undefined" enums can still occur.** C# `Enum.IsDefined` fails only for the `(T)int.MinValue` sentinel that the lenient reader writes
     for the eight converter-backed properties. In Rust those are the `Option<..>` fields (`None` = sentinel) and are the only enum reports. The plain
     enum fields (`journal_durability`, `instance_mode`, `fit_width_anchor`, `mouse_wheel_action`, `image_transition`, `raw_full_decode`,
     `raw_pair_mode`, `initial_view_mode` except the Percent400 migration) cannot be undefined: golden `... garbage is silently zero` = no report.
     `KineticGlideSmoothing` (an `Option`, `None` -> `Some(Predict)`) is the only one of the "reset to the `AppSettings` default" checks that can fire; the C# `ImageTransition` check (`AppSettings().ImageTransition` = `None`) can never fire in Rust. `normalize` has no `texts` argument: use literal defaults (`Predict`, `RawFullDecode::Never`, `RawPairMode::Separate`), never `AppSettings::new(texts)`.
  3. **Actions.** Drop null entries (Rust: `settings.read_notes.null_actions > 0` stands for `RemoveAll(a => a is null)`), null strings become `""`
     (nothing to do in Rust); an undefined `Operation` becomes `Move` (Rust `FileOperationType` is always defined: the golden `actions: odd
     operations are silently Move` is a no-report case). `Actions` is reported once for "removed > 0 or invalid operation". Finish by resetting
     `settings.read_notes` to default (it is not a setting; a second `normalize` must report nothing: the oracle runs Normalize twice and requires
     `fixed_again == []`).
  4. **Mandatory shortcuts.** The 13 names = `PROPERTY_NAMES` minus `OPTIONAL_NAMES` minus `MoveToFolder2`. A value that is blank
     (`trim().is_empty()`, same set of white space as .NET) is replaced by `ShortcutMappings::default()`'s value; `Shortcuts` is reported ONCE for any
     number of resets. A merely padded value (`" Right "`) is NOT reset and (by `canonicalize`) only trimmed/aliased. `MoveToFolder2` is never reset.
     Optional ones: `trim()` (null already `""`).
  5. **Reset-key clash (RV-S03).** For each reset mandatory shortcut remember `canonicalize(default)`. After `settings.canonicalize_all()` loop the
     actions by index: an action whose canonical shortcut (non-empty) is in that set (OrdinalIgnoreCase: use `pr_core::text::ignore_case_key`) gets
     `shortcut = ""` and reports `Actions[<i>].Shortcut` with `<i>` the index AFTER null removal, decimal, no padding. An action with a blank shortcut
     is untouched.
  6. **Clamps.** `InfoOverlayFontSize`: non-finite -> `DEFAULT_INFO_OVERLAY_FONT_SIZE`, else clamp to [min, max]; report when the result differs
     (`NaN != NaN` is true in both languages, so NaN is always reported). All other ranges are `i32::clamp` against the module-level `settings::app_settings::{MIN_,MAX_}*` constants (W2-01; not associated consts of
     `AppSettings`) and the `settings::perf_options` constants (W1, e.g. `MIN_IMAGE_CACHE_RAM_PERCENT`, `MAX_PRELOAD_COUNT`).
  7. **`HiddenContextMenuItems`.** When `Some(list)`: `canonical = context_menu::to_stored(context_menu::parse_hidden(&list))`; compare element-wise
     ordinally with the list (a `None` element never equals, so a list that only differs by a null element is repaired and reported); when different
     store `Some(canonical.into_iter().map(Some).collect())` and report. `None` stays `None`. Depends on W2-20.
  8. **`disable_conflicting_optional_shortcuts`.** `taken` = a set keyed on `ignore_case_key(canonicalize(..))`, seeded (non-blank only) with the 13
     mandatory shortcuts and every action shortcut (NOT `MoveToFolder2`). Then walk `OPTIONAL_NAMES` in order: `key = canonicalize(value)`; `""` stays
     `""` (not disabled, not reported); a key already taken is set to `""` and its NAME is pushed; otherwise the canonical key is stored (so the optional
     ones are trimmed/aliased here as well) and added to `taken`. The returned names are therefore in `OPTIONAL_NAMES` order, and for equal keys the
     earlier optional wins (golden `all optional shortcuts on the same free key`).
  9. **`validate_shortcuts`.** Iterate `PROPERTY_NAMES` in declaration order, skip `MoveToFolder2`; `value = get(name).trim()`; blank optional is
     skipped; blank mandatory OR `!key_validator.is_valid_key_name(value)` returns `core_settings_shortcut_invalid(name)` immediately (the validator
     receives the TRIMMED text); otherwise push `(name, canonicalize(value))`. Then each action in order: blank `name`, blank `shortcut`, or a trimmed
     shortcut rejected by the validator returns `core_settings_action_invalid()`; otherwise push
     (`core_settings_action_binding_name(name)`, `canonicalize(shortcut)`). Finally group by `ignore_case_key(key)` keeping first-occurrence order of
     groups; the FIRST group with more than one member gives `core_settings_shortcut_duplicate(first_member_key, names.join(", "))` where `first_member_key`
     is the canonical spelling of that group's first binding (C# `group.Key`) and `names` are in binding order. Everything is an `Option<String>` result;
     no exceptions. Order matters: an invalid shortcut is reported before an invalid action, which is reported before any duplicate (golden).
  10. **`build_repair_text`.** `reset` = repairs that do not start with `"Shortcuts."` (ordinal, case sensitive; `"Shortcuts"` with no dot and
      `"shortcuts.X"` are resets and `"Actions[0].Shortcut"` is a reset). One reset line first (if any reset), then one line per prefixed repair
      with the name after the prefix (an empty name is allowed). Lines joined by `"\r\n"` (`Environment.NewLine` of the Windows baseline: write the literal,
      not `cfg!`). The texts come from `SettingsTexts` (English catalog in the goldens).
  11. **`SettingsParseResult::repairs`** = `reset_settings` followed by `"Shortcuts." + name` for each `disabled_shortcuts`. The struct is produced by
      `SettingsStore::parse_text` (W2-03), not here.
- Traps checklist (docs/MULTI-AGENT-PLAN.md §7): 1 (OrdinalIgnoreCase: `text::ignore_case_key`, never `to_lowercase`/`eq_ignore_ascii_case` for the
  duplicate and `taken` sets: goldens `Kelvin sign equals K`, `sharp s is not SS`, `dotless i is not I`, `case-insensitive non-ASCII duplicate`),
  2 (`canonicalize` alias match is ASCII-only: use `shortcuts::canonicalize`, do not re-implement), 3 (`i64` capacities, `i32` counts: no `as` casts;
  `-9223372036854775808` golden), 6 (NaN/-0.0 comparisons: `PreloadMemoryLoadLimit`, `InfoOverlayFontSize`), 8 (null -> `Option`/blank mapping above),
  10 (order of lists: use `Vec`, never a `HashSet` iteration for output; `taken` is a set only for membership).
- Depends on (functions called by the implementation or its tests):
  - **W2-01** (MUST be merged first): `AppSettings` (all fields, `new(texts)`, `from_json`, `to_json`, `canonicalize_all`, `ReadNotes`, MIN/MAX
    constants), `SettingsTexts` + `EnglishSettingsTexts`, `SettingsJsonError`. The golden runner needs `from_json` and `to_json` (it ports the oracle's
    `W202Segments` to diff the indented text against the defaults).
  - **W2-20** `settings::context_menu::{parse_hidden, to_stored}` (MUST be merged first; until then `normalize` cannot be completed).
  - W1 `settings::shortcuts::{ShortcutMappings (PROPERTY_NAMES, OPTIONAL_NAMES, is_optional, get, get_mut), canonicalize, ReviewAction}`,
    `settings::perf_options`, `abstractions::KeyNameValidator`, `text::{ignore_case_key}`, `model::enums`.
  - Consumers (do not block this unit): W2-03 `SettingsStore` (builds `SettingsParseResult`, calls both normalizer functions and `build_repair_text`).
- Done when: every listed test has a Rust twin with `// C#: <Class>.<Method>`; the three goldens pass (all cases: `fixed`, `disabled`, `fixed_again == []`,
  `disabled_again == []`, `changed` for normalize; `message` for validate; `text` for repair-text); clippy clean; the stub's
  `#![allow(unused_variables, dead_code)]` line removed with the last `todo!()`.

## Golden package
- `oracle/W2-02.cs`: commands `settings-normalize` (deserialize -> fill null Actions/Shortcuts -> Normalize -> DisableConflicting -> Normalize again ->
  DisableConflicting again; reports the four lists and the settings segments that differ from the defaults), `settings-validate` (nulls replaced by `""`
  so Rust can hold them; four validator kinds `non_blank`, `letters_digits`, `reject_all`, `known{names}`; must be re-implemented in the Rust test as
  `KeyNameValidator`s), `settings-repair-text` (for_import both ways). All texts are the built-in English catalog. Dispatch lines are in the file header.
  Needs no other oracle file (`W202Segments` is duplicated on purpose).
- `golden-inputs/{settings-normalize (294 cases), settings-validate (84), settings-repair-text (22)}.json`, outputs in `golden-reference/`.
- Case design: every numeric bound on both sides (+-1, `int.MinValue/MaxValue`, NaN/Infinity as strings), every enum with garbage/null/999, each of the
  13 mandatory shortcuts blank and null, each of the 14 optional ones null/padded/equal to `Next`/equal to an action key, combined cases for
  report order, hidden-menu lists (reorder, drop `Settings`, duplicates, null element, numeric and padded names), the v1 legacy document;
  validator: blank / unknown per property, first-error order, duplicate group order, case and non-ASCII equality.
- Reviewed: the oracle code matches the C# at d92d6aae (control flow, order, messages). The reference files were not regenerated.

## Porter model
**sonnet.** The logic is small (about 330 C# lines) and fully specified by the goldens, but the observable report order, the first-occurrence duplicate
grouping, the OrdinalIgnoreCase set and the `Option<enum>` sentinel mapping are easy to get subtly wrong, and the golden runner has to port
`W202Segments` and four tiny validators. Haiku is enough for the repair-text and validator halves; keep one porter for coherence. No opus: no
concurrency or data-safety risk.

## Open questions
- `settings.read_notes` after `normalize`: the stub says it is cleared. W2-01 should confirm `ReadNotes` has no other consumer (e.g. W2-03 reading
  `null_actions` AFTER `normalize` to decide whether to rewrite the file); if it does, `normalize` should leave it and W2-03 clears it.
- `Normalize_Undefined*` tests (`(T)999`): see the first test bullet; lead decides whether a `from_json` based twin satisfies parity or whether
  those four C# facts count as covered by the goldens only.

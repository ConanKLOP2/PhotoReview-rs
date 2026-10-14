# W2-01: AppSettings + System.Text.Json-compatible config.json reader/writer (`AppSettings.cs`, `AppSettingsJsonContext.cs`)
- Color: 🟡 | Porter: **sonnet** (hand-written lenient JSON parser with System.Text.Json quirks, .NET double formatting, 785 golden cases that
  pin every edge; no design freedom left, so not opus) | Test porter: haiku | Verifier: sonnet
- C# sources (baseline d92d6aae), under `src/PhotoReview.Core/`:
  - `Settings/AppSettings.cs` (460 lines: 70 properties, 28 `const`s = `CurrentConfigVersion` + 27 range/default consts, `Clone`)
  - `Settings/AppSettingsJsonContext.cs` (18 lines: `WriteIndented`, `NumberHandling = AllowReadingFromString`, `ReadCommentHandling = Skip`,
    `AllowTrailingCommas`)
  - read for behaviour only (already ported): `Model/SettingsEnumConverter.cs`, `Model/LenientEnumConverter.cs` (W1-01/W1-02),
    `Settings/ReviewAction.cs`, `Settings/ShortcutKeyCanonical.cs` (W1-04; the deserialization half is OWNED here, lead decision D10)
  (read with `git -C C:/MyProjects/PhotoReview show d92d6aae:src/PhotoReview.Core/<file>`)
- C# tests to port (all at d92d6aae, `tests/PhotoReview.Core.Tests/`):
  - `Settings/AppSettingsPocoTests.cs`: `AppSettingsDefaults`, `AppSettingsDeserialization` (use `AppSettings::from_json`; `"fast"` -> Fast,
    `"Size"` -> SizeDescending, `"100%"` -> Percent100, RAW and WebP/HEIC defaults survive).
  - `Settings/ConfigKeyContractTests.cs`: the three tests (the frozen list equals `JSON_KEYS` AND the written default document's key order).
  - `Settings/SettingsDefaultsLiteralTests.cs`: both tests (literals: 16 GiB, 50, 2 GiB, 8, 0.9, 4 GiB, false, 16 GiB, 32, 8 ...).
  - `Settings/ContextMenuLayoutTests.cs`: `Clone_and_json_export_keep_the_list_and_an_empty_list_stays_empty` (replace `SettingsStore.ParseText` by
    `from_json`; `Some(vec![])` survives `to_json` -> `from_json` and `deep_clone`).
  - `Settings/SettingsRobustnessTests.cs`: `EveryPublicSettingRoundTripsThroughClone` (line 557; C# uses `Random(99)` + reflection over the
    properties) only in spirit: assert `deep_clone() == original` and `from_json(to_json(x)) == x` for a hand-built non-default value (no `Random`;
    the fuzz tests, incl. `Fuzz_NormalizeIdempotent_CloneEqual`, belong to W2-02/W2-03). Note `AppSettings` derives `PartialEq` over ALL fields
    including `read_notes`: the hand-built value must keep `read_notes` at its default (or compare with notes zeroed), and must not hold NaN.
  - NOT here: `ReviewActionDefaultsTests` (W1-04), everything that calls `SettingsStore`/`Normalize`/`Validator` (W2-02, W2-03), the
    reflection `JsonSerializer.Serialize(settings)` comparisons in `SettingsStoreTests`/`SettingsStoreFailureTests` (reflection options; W2-03 decides),
    all `tests/PhotoReview.App.Tests/**` and `Integration.Tests/**` settings tests (App waves).
  - UNCLAIMED (see Open questions): the per-property `*SettingsTests.cs` (`ExifInfoSettingsTests`, `TitleBarFieldsSettingsTests`,
    `MoveCopyToSettingsTests`, `MouseSettingsTests`, ...). Verified at d92d6aae: every one goes through `SettingsStore.Load`/`Parse` and most also
    through `SettingsNormalizer`, so they cannot be ported on `from_json` alone; W2-03's contract assumes they "stay with W2-01", W2-01 does not
    claim them. The golden `settings-json` already pins their reader half.
  - Golden test (new, required): `tests/golden/settings-json.json` (785 cases: 1 `defaults`, 104 `read`, 475 `scalar`, 13 `roundtrip`, 42 `double`,
    150 `escape`). Harness rules are in "Golden package".
- PARITY rows (docs/PARITY.md): `S-01..S-63` (defaults of the persisted settings; the clamp ranges are exercised by W2-02), `S-18`/`S-19`
  (Actions/Shortcuts shape), `S-20` (`UiLanguage` default `auto`), and `D-01` (the `config.json` format: indented, PascalCase names in order,
  comments, trailing commas, numbers as strings, aliases, case-insensitive and numeric enums). There are no separate "comments"/"string numbers"
  rows. Add a new row `X-stj-reader`
  ("Done | `pr_core::settings::app_settings::json` | `settings_app_settings::golden_settings_json` | hand-written System.Text.Json subset").
- Owned files (the ONLY files you may create/modify):
    crates/pr-core/src/settings/app_settings.rs   (stub: contracts-w2/W2-01/app_settings.rs, compiles clippy-clean)
    crates/pr-core/tests/settings_app_settings.rs
- Lead-owned edits: `crates/pr-core/src/settings/mod.rs` gets `pub mod app_settings;` (W2-02 and W2-03 already import
  `crate::settings::app_settings::{AppSettings, SettingsJsonError, SettingsTexts, json}`).
- Read-only inputs: `tests/golden/settings-json.json`, `tests/golden-inputs/settings-json.json`.
- Public API (stub, do NOT change signatures): consts `CURRENT_CONFIG_VERSION`, `AUTO_LANGUAGE_CODE`, the `MIN_/MAX_/DEFAULT_*` consts,
  `JSON_KEYS: [&str; 70]`; `trait SettingsTexts`, `EnglishSettingsTexts`; `SettingsJsonError`; `ReadNotes`; `AppSettings` (+ `new`, `*_or_default`
  getters, `from_json`, `from_json_value`, `apply_json_property`, `to_json`, `canonicalize_all`, `deep_clone`, `Default`); module `json`
  (`MAX_DEPTH`, `JsonText`, `JsonValue` + `property`, `parse`, `to_i32`, `to_i64`, `to_f64`, `to_bool`, `to_string_or_null`, `to_serde_value`,
  `escape_string`, `format_f64`). Already complete in the stub (no porter work): constants, `JSON_KEYS`, `SettingsTexts` + English impl, `new`,
  getters, `canonicalize_all`, `deep_clone`, `Default`. `todo!()` bodies: the `json` module, `from_json`, `from_json_value`, `apply_json_property`,
  `to_json`.
- Semantics notes:
  1. **Why hand-written.** serde cannot give: comments, trailing commas, number-as-string, duplicate names (last wins but an unreadable earlier
     duplicate already fails), depth 64, per-property salvage (W2-03 calls `apply_json_property` once per property). Do not use `serde_json` to
     parse the file; `serde_json::Value` only appears in `to_serde_value` to feed the W1-02 helpers.
  2. **Reading = `JsonSerializer.Deserialize(text, ctx)`.** Whole text: skip whitespace/comments, one root value, then only
     whitespace/comments. Root `null` -> `Ok(None)`; root not an object -> `Err`; empty or whitespace-only text -> `Err`. The properties are applied
     in file order to `AppSettings::new(texts)`; the first unreadable one aborts. Unknown names are skipped (their value was syntax-checked only).
     Names are matched case-sensitively and ordinally AFTER unescaping (`"Click\u005AoomPercent"` is `ClickZoomPercent`; `"clickzoompercent"` is
     unknown). The empty name `""` is unknown.
  3. **Syntax (verified by the 104 `read` cases).** Standard JSON plus:
     | Rule | Behaviour |
     |---|---|
     | whitespace | only space, `\t`, `\r`, `\n` (BOM, NBSP, `\f`, `\v` are errors; a leading BOM is an error here, W2-03 strips it before calling) |
     | `//` comment | ends at `\n`; `\r` alone also ends it; U+2028/U+2029 inside it is an error |
     | `/* */` comment | does not nest; `/*/` is unterminated; unterminated is an error |
     | where comments are allowed | wherever whitespace is, EXCEPT between a property name and its `:` (`{"A"/*x*/:7}` is an error). After `:` , before/after `,`, in empty `{}`/`[]`, before/after the root are fine |
     | trailing comma | one, in objects and arrays (also followed by a comment); `,,`, leading `,`, lone `[,]` are errors |
     | numbers | JSON grammar only (no `+`, `.5`, `5.`, `00`, `0x`, `NaN`, `Infinity`, bare `-`); `1e` is an error; a number glued to a letter (`7x`) is an error; literals are lowercase only and `truex` is an error |
     | strings | raw control chars `< 0x20` are an error (DEL 0x7F is fine); escapes `\" \\ \/ \b \f \n \r \t \uXXXX` (hex any case; `\u12` and `\q` are errors); an escaped control char (`\u001f`) is fine |
     | surrogates | an unpaired `\uD800`..`\uDFFF` escape parses (the value is `JsonText = None`); it becomes an error only when the string is READ as a typed value, or is a property name that is not inside a skipped unknown value (top-level name, `Shortcuts`/`Actions[]` object names: error; name nested in an unknown property's value: fine). A high surrogate escape followed by anything that is not a low-surrogate escape (a plain char such as `\ud83dA`, or a non-surrogate escape) counts as unpaired, and so does a low surrogate before a high one |
     | depth | `MAX_DEPTH = 64`: root object + 63 nested objects/arrays OK; one more is an error |
     | trailing text | any non-comment token after the root is an error (`{} x`, `{}{}`) |
  4. **Per-property conversion** (`apply_json_property`; `Err` = `JsonException`):
     | C# type | Properties | Accepted | Everything else |
     |---|---|---|---|
     | `int` | ConfigVersion, ImageCacheRamPercent, PreloadWorkerCount, ClickZoomPercent, PreloadForwardCount, PreloadBackwardCount, ToolbarAutoHideDelayMs, KeyboardZoomStepPercent, InfoOverlayAutoHideDelayMs, ToolbarOpacityPercent, ArrowPanStepPercent, ImageTransitionMs, TouchpadSwipeDistancePerImage | number token `-?digits` in `i32`; string `[+-]?[0-9]+` (ASCII digits only, leading zeros fine, no spaces) in `i32` (`"-0"`, `-0` give 0) | error: `250.0`, `2.5e2`, `1E0`, `null`, bool, array, object, `" 250"`, `""`, `"+"`, overflow (2147483648), non-ASCII digits |
     | `long` | ImageCacheCapacityBytes, MemoryReserveBytes, PreviewDiskCacheCapacityBytes, SourceBytesCapacityBytes | as `int` with `i64` | same errors; `99999999999999999999` is an error |
     | `double` | PreloadMemoryLoadLimit, InfoOverlayFontSize | any number token (overflow gives +/-infinity, underflow 0, stored); string: `NaN`, `Infinity`, `-Infinity` (exact case, no `+Infinity`), or `[+-]?(digits[.digits*]|.digits)([eE][+-]?digits)?` that is finite (`".5"`, `"5."`, `"5.e3"`, `"1e0000003"`, `"-.5"`, `"1e-400"` = 0 all fine) | error: `null`, bool, containers, `"nan"`, `"1e999"`, `"1,000"`, `" 1"`, `"0x1"`, `"1f"`, `""`, a number token like `.5` or `00` is already a parse error |
     | `bool` | the 26 `bool` properties | literals `true`/`false` only | error for `"true"`, `1`, `null`, containers |
     | `string` | UiLanguage, ExternalEditorPath (null -> `""`) | string, or `null` | error for number/bool/container/unpaired surrogate |
     | `string?` | LastMoveToFolder, LastCopyToFolder | string or `null` (kept `None`) | as above |
     | lenient enum, type-level | InitialViewMode, JournalDurability, InstanceMode, MouseWheelAction, ImageTransition, RawFullDecode, RawPairMode, FitWidthAnchor, Operation (`FileOperationType`) of an action | W1-02 `deserialize_lenient` via `to_serde_value`: name (trim, case-insensitive, aliases), integer that is a defined member (also as a numeric string such as `"3"`); ANYTHING else (unknown, null, bool, float, array, object) = member 0, never an error | only an unpaired-surrogate string is an error |
     | lenient enum, "settings" (`Option<T>`) | LoadingMode, ImageSortMode, ScalingQuality, DecoderBackend, KineticGlideSmoothing, FitWidthAnchor2, MiddleClickAction, KeyboardZoomAnchor | W1-02 `deserialize_settings_enum`: as above but anything unreadable = `None` (C# `(T)int.MinValue`); e.g. `ImageSortMode: 6`, `"bogus"`, `null`, `{}` give `None`, `LoadingMode: 3` gives `None` | same |
     | flags | ExifInfoFields, TitleBarFields | W1-02 `deserialize_flags` (names list, number masked to defined bits; unreadable = `DEFAULT`; `1024` -> `NONE`) | only an unpaired surrogate is an error |
     | `List<string>?` | HiddenContextMenuItems | `null` -> `None`; array of strings and `null`s -> `Some`; `[]` stays `Some(vec![])` | error for `[5]`, `"Fit"`, `{}`, `[["Fit"]]`, an unpaired-surrogate element |
     | `List<ReviewAction>` | Actions | `null` -> default actions (W2-03 `FinishDeserialize` does the same; the oracle's `RustView` mirrors it); array of objects or `null`s: each `null` is DROPPED and counted in `read_notes.null_actions`; an element object starts from `ReviewAction::new(texts.action_default_name(2))` (`Shortcut` `Enter`, `Move`, `Group-2`, no confirm), applies `Name`, `Shortcut`, `Destination` (each string or `null` -> `""`, as the oracle's `RustView` does), `Operation` (`FileOperationType`, lenient, type-level, `"Delete"` = Recycle), `Confirm` (bool only; `null`, `"true"` are errors), case-sensitive names, last duplicate wins, unknown names skipped | error: non-array (`{}`, `"x"`), non-object element (`1`), `Name: 5`, object property name with an unpaired surrogate (even an unknown one) |
     | `ShortcutMappings` | Shortcuts | `null` -> `ShortcutMappings::default()`; object: each name in `ShortcutMappings::PROPERTY_NAMES` (case-sensitive) takes a string or `null` (-> `""`; no trimming, `" Return "` stays); unknown names skipped; absent names keep the default; last duplicate wins | error: `[]`, `5`, `"x"`, value `5` or `["x"]`, unpaired surrogate in a value or in ANY property name |
  5. **Undefined enums in the written file.** A `None` settings-enum is written as the STRING `"-2147483648"` (what `LenientEnumConverter.ToText`
     gives for `(T)int.MinValue`); reading that string back gives `None` again. Golden `roundtrip: undefined settings enums`.
  6. **Writing = `JsonSerializer.Serialize(settings, ctx)`.** `{` CRLF, two-space indent, every property of `JSON_KEYS` in that order, `"Name": value`
     (one space after the colon), arrays/objects expanded one element per line, empty list `[]`, `null` for `None`, CRLF between lines, closing `}`
     with NO trailing newline. `Actions` elements and `Shortcuts` print their own fields in declaration order (`Name, Shortcut, Operation,
     Destination, Confirm`; `ShortcutMappings` field order = `PROPERTY_NAMES` order, verified against the golden). Enums as W1-02 `lenient_text`
     (note `InitialViewMode` prints `Fit`, `100%`, `200%`, `400%`, then member names), flags via `serialize_flags` (`"FileName, Lens"`, `"None"`).
     `read_notes` is never written.
  7. **String escaping** (`escape_string`, default `JavaScriptEncoder`, pinned by the 150 `escape` cases for code points 0..127 (all ASCII), 128, 133,
     159, 160, 173, 233, 255, 256, 7841, 8232, 8233, 8203, 8206, 12354, 57344, 65279, 65533..65536, 128512, 1114111): output starts and ends with `"`.
     Printable ASCII is raw except `"` `&` `'` `+` `<` `>` `` ` `` which are `\u0022 \u0026 \u0027 \u002B \u003C \u003E \u0060`; `\` is `\\`; `/`
     raw; controls `\b \t \n \f \r` short, other `< 0x20` and DEL `\u00XX`; EVERY non-ASCII char is `\uXXXX` with UPPER-case hex (astral =
     surrogate pair `\uD83D\uDE00`). A string containing an unpaired surrogate cannot exist in a Rust `String`.
  8. **Doubles** (`format_f64`, pinned by 42 `double` cases): shortest round-trip digits (Rust `{:e}` gives them; do not use `{}` or `{:.*}`),
     then layout by decimal exponent `e` (value = d.ddd x 10^e): `-5 < e < 17` plain (`100000000000000`, `10000000000000000`, `0.0001`),
     otherwise `d[.ddd]E+XX` / `E-XX` (sign always, at least 2 exponent digits: `1E+17`, `1E-05`, `5E-324`, `1.7976931348623157E+308`);
     integers with an exponent below 17 print without `.0` (`12`, not `12.0`); `-0.0` is `-0`. NaN/infinity: `format_f64` returns `None` and
     `to_json` maps it to `Err` (C# `ArgumentException`). The values come from the two double properties; neither is clamped by the writer.
  9. **`ReadNotes`** is transient and never written, but it IS part of the derived `PartialEq` of `AppSettings`: a `from_json` result with a dropped
     null action differs from `from_json(to_json(..))` of it. `deep_clone` keeps it (harmless). `Normalize` (W2-02) clears it; W2-02/W2-03 must read
     `null_actions` before normalizing (see W2-02 open question).
  10. **`AppSettings.Clone`** is a JSON round trip in C#. `deep_clone` is `clone()`; they differ only for a value `to_json` cannot write
      (NaN) where C# throws. Not observable by any caller (the normalizer repairs NaN first).
- Traps checklist (docs/MULTI-AGENT-PLAN.md §7): item 1 (property names are ordinal CASE-SENSITIVE, not `OrdinalIgnoreCase`), 2 (no culture:
  every number is invariant; `to_f64`/`to_i32` take ASCII only), 3 (`i32`/`i64` ranges; stored `f64` may be infinite after an overflowing number
  token), 4 (number formatting: `format_f64` shortest round-trip, never `format!("{}")`; CRLF, no trailing newline, D9), 6 (JSON: a missing
  property keeps the default, unknown skipped, nothing dropped on round trip), 8 (`None` vs `Some("")`: the eight `Option<enum>` fields,
  `LastMoveToFolder`, `HiddenContextMenuItems`), 9 (`JsonException` = read `Err`, `ArgumentException` = write `Err`: keep them distinct in the
  harness), 10 (output order = `JSON_KEYS` order, `Vec` of entries not a map; duplicate property: last wins but every one is validated). Not in
  §7 but specific here: lone surrogates are `JsonText = None`, never replace them with U+FFFD.
- Depends on (all merged unless noted):
  - W1-01 `model::enums` (all enums incl. `FileOperationType` for `ReviewAction.operation`, `ImageSortMode::Default`), W1-02 `model::serde_ext`
    (`deserialize_lenient`, `deserialize_settings_enum`, `serialize_settings_enum`, `deserialize_flags`, `serialize_flags`, `lenient_text`), W1-03
    `model::fields::{ExifInfoFields, TitleBarFields}`, W1-04 `settings::shortcuts::{ShortcutMappings, ReviewAction, canonicalize_all}`, W1
    `settings::perf_options` constants.
  - No dependency on W2-00 (no path handling) or on any other W2 unit. W2-02 (`normalizer`), W2-03 (`store`) and W2-20 (`context_menu`: reads
    `hidden_context_menu_items` and the three legacy flags) DEPEND ON THIS unit: merge W2-01 first, with `todo!()` replaced. W2-04 (`pr-l10n`) does
    NOT depend on it and its contract does not provide a `SettingsTexts` implementation (see Open questions).
  - `serde_json` is already a normal dependency of `pr-core` (used by `to_serde_value`); dev only: `pr-test-support` `golden`.
- Done when: every test above has a Rust twin marked `// C#: <Class>.<Method>`; all 785 golden cases pass (harness below); `cargo clippy --workspace
  --all-targets -- -D warnings` clean; the stub's `#![allow(unused_variables, dead_code)]` line is removed (D11); `todo!()` count is zero; one
  hand-written test proves `apply_json_property` salvage (a mistyped `LoggingEnabled` leaves `Actions`/`Shortcuts` intact).

## Golden package
- `oracle/W2-01.cs`: command `settings-json`. Dispatch line `"settings-json" => Commands.SettingsJson(inputs),` is in the file header; copy to
  `tools/oracle-cs/Commands/W2-01.cs`, input to `tests/golden-inputs/settings-json.json`, run `tools/oracle.ps1 -Command settings-json`.
- Inputs (`golden-inputs/settings-json.json`): `read` (104 `{name,text}`), `scalars` (30 properties x tokens = 475 cases, each is
  `{"<Property>":<token>}`), `roundtrip` (13), `doubles` (42 IEEE bit patterns, written through both double properties), `escape_codepoints` (150).
- Output kinds (`golden-reference/settings-json.json`, `cases[]`, baseline `d92d6aae`):
  - `defaults`: `text` (the whole default config.json, CRLF, no trailing newline), `keys` (70), `consts` (28 constants as numbers:
    `CurrentConfigVersion` + 27; the `PerformanceOptions` consts are not in it).
    Rust: `AppSettings::default().to_json()` equals `text` byte for byte; `JSON_KEYS` equals `keys`; every const equals `consts[NAME]`
    (`CurrentConfigVersion` -> `CURRENT_CONFIG_VERSION`, etc.).
  - `read` / `scalar`: `outcome` is `ok` | `null` | `error`. Rust maps: `from_json` `Err` -> `error`; `Ok(None)` -> `null`; `Ok(Some(s))` -> `ok` with
    `null_actions = s.read_notes.null_actions` and `changed` = for each top-level segment of `s.to_json()` that differs from the default's
    segment, name -> segment text (the oracle's `W201Segments`: text after `"Name": `, lines joined with CRLF, trailing comma removed).
    NINE `scalar` cases are `outcome: error` WITH a `null_actions` field and `error_type: ArgumentException`: the read succeeded and only
    the write failed (`InfoOverlayFontSize`: `"NaN"`, `"Infinity"`, `"-Infinity"`, `1e999`, `-1e999`, `1E400`, `1.7976931348623159e308`;
    `PreloadMemoryLoadLimit`: `"NaN"`, `"Infinity"`). In Rust `from_json` is `Ok` there and `to_json` is `Err`; the harness must treat `to_json` `Err`
    as `error`. Every other error is `JsonException` = `from_json` `Err`.
  - `roundtrip`: `from_json(text)` then (the oracle's `RustView`: null Actions/Shortcuts/strings already handled by the reader) `to_json()` equals
    `text_out`.
  - `double`: `bits` is a hex string such as `0x3FE8000000000000`; set `info_overlay_font_size` and `preload_memory_load_limit` to
    `f64::from_bits(bits)`; `to_json` segments equal `text`/`text_limit`; NaN/infinity bit patterns (3 cases) must be `Err`.
  - `escape`: `last_move_to_folder = Some("a" + char + "b")`; its segment equals `json` (code points are scalar values only; no surrogates in the list).
- Case design: syntax edges (comments in every position, trailing commas, depth 63/64/65, BOM/NBSP/FF/VT, number grammar, lone surrogates in
  known/unknown values and names, escaped names, duplicates); every property's scalar grammar (integers vs string numbers, 2^31/2^63 edges,
  unicode digits, `-0`, `1e-400`, the 17-digit string, overflow); each enum with names, aliases, case, trimming, numbers, `null`, containers;
  flags; `Actions` and `Shortcuts` shapes; round trips including all shortcuts and undefined enums.

## Open questions
- Lead decisions needed (none blocks the port itself):
  (a) the `ReadNotes` field on a public struct (needed for `null_actions` and the normalizer, D10) is accepted as the replacement for C#'s "keep
  nulls and let `Normalize` remove them".
  (b) `SettingsTexts` stays a trait in `pr-core`, but NO unit currently supplies the catalog-backed implementation: W2-04 lives in `pr-l10n`, does
  not list `SettingsTexts` and has no dependency on `pr-core`. Either `pr-l10n` gains a `pr-core` dependency and W2-04 adds the impl, or the App
  composition root does; until then only `EnglishSettingsTexts` exists (enough for every golden).
  (c) `AUTO_LANGUAGE_CODE` here duplicates W2-04's `AUTO_CODE` (`pr-core` cannot see `pr-l10n`); accept the duplicate or move the constant down.
  (d) ownership of the per-property `*SettingsTests.cs` (see "UNCLAIMED" above; W2-03 expects W2-01 to take them, W2-01 cannot because they need
  `SettingsStore`/`SettingsNormalizer`). Recommended: split between W2-02 (normalizer rows) and W2-03 (store load rows); the reader half is
  covered by this golden.

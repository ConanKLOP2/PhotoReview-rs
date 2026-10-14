# W2-04: Localization core (LanguageCatalog, Localizer, LanguageLoader, BuiltInCatalog, TranslatorModes) + key generator (`build.rs`)
- Color: 🟡 | Porter: **sonnet** for `catalog.rs` + `build/keygen.rs` + `build.rs` (a hand-written `System.Text.Json` clone with exact
  edge cases, and a build-time code generator shared by two compilation units); **haiku** may take `localizer.rs` + `loader.rs`
  (data types are complete in the stub, behaviour is a direct transliteration) and escalates to sonnet after one failed gate |
  Test porter: haiku | Verifier: sonnet (mutation checks on warning texts and fall-back order)
- C# sources (baseline d92d6aae; read with `git -C C:\MyProjects\PhotoReview show d92d6aae:<path>`):
  - `src/PhotoReview.Core/Localization/LanguageCatalog.cs` (213 lines), `Localizer.cs` (156), `LanguageLoader.cs` (144),
    `BuiltInCatalog.cs` (26), `TranslatorModes.cs` (63; the Accent/padding helpers already live in `pr_l10n::template`, W1-11)
  - `src/PhotoReview.Localization.Generator/TrGenerator.cs` (456), `FlatJsonReader.cs` (345), `LocDiagnostics.cs` (39)
  - Not ported here: `LocTemplate.cs`, `LocArg.cs`, `UserFacingError.cs` (W1-11, merged); `App.Shared/Localization/*` (App wave).
- C# tests to port (all at d92d6aae, `tests/PhotoReview.Core.Tests/Localization/`; each Rust twin carries `// C#: <Class>.<Method>`):
  - `LocalizerTests.cs` (the NON-template half; the `Render_*`, `TryParse_InvalidTemplate/ValidTemplate`, `Literal_*` tests are W1-11):
    `TryParse_ValidFileWithMeta_ReadsMetaAndEntries`, `TryParse_PluralNotNone_DefaultsToOneOther`, `TryParse_NamesMissing_FallBackToNameThenCode`,
    `TryParse_MissingOrInvalidCode_ReturnsFalseWithWarning`, `TryParse_MetaNotObject_ReturnsFalseWithWarnings`,
    `TryParse_NonObjectRoot_ReturnsFalseWithWarning`, `TryParse_InvalidJson_ReturnsFalseWithWarningInsteadOfThrowing`,
    `TryParse_NonStringValues_IgnoredWithWarning`, `TryParse_UnderscoreKeys_SkippedSilently`, `TryParse_CommentsAndTrailingCommas_Accepted`,
    `TryParse_DuplicateKey_LastValueWins`, `IsValidCode_VariousCodes_MatchesRule`, `Create_*` (11 tests: EnglishOnly, Overlay, UnknownKey,
    UnknownPlaceholder, DroppingPlaceholder, Reordering, TranslationWithBadBraces, EnglishWithBadBraces, LaterOverlay, RejectedUserEntry, PriorWarnings),
    `Get_MissingKey_ReturnsKey`, `Get_TemplateWithPlaceholders_LeavesPlaceholdersVisible`, `FormatPlural_OneOther_PicksOneOnlyForCountOne`,
    `FormatPlural_PinnedCulture_LiteralExpectation` (use `format_plural_with` + an explicit `NumberCulture`), `FormatPlural_PluralNone_AlwaysUsesOther`,
    `FormatPlural_OneFormAbsent_FallsBackToOther`, `FormatPlural_NoFormsAtAll_ReturnsOtherKey`, `BuiltInEnglish_EmbeddedCatalog_IsValidAndComplete`,
    `ShippedEnglishFile_MatchesEmbeddedCatalog` (read `assets/languages/en.json` through `CARGO_MANIFEST_DIR`; compare with `EMBEDDED_ENGLISH_JSON`),
    `ShippedVietnamese_AgainstBuiltInEnglish_HasNoWarnings` (`assets/languages/vi.json`), `Current_NotReplaced_IsBuiltInEnglish`
    (`Localizer::clear_current_for_tests`), `SetCurrent_NewLocalizer_ReplacesCurrentAndRaisesCurrentChanged`.
    NOT ported: `Current_InThisAssembly_IsPinnedVietnamese` (module-initializer fixture, C# only), `SetCurrent_Null_ThrowsAndKeepsCurrent` (`Arc` cannot be null).
  - `LanguageCatalogRobustnessTests.cs`: all 5 (`LoneSurrogateEscape`, `BlankDisplayNames_FallBack`, `DisplayNames_AreCleaned`,
    `WarningText_HasNoRawLineBreaks`, `LocalizerWarning_HasNoRawLineBreaks`).
  - `LanguageLoaderTests.cs`: all but `Constructor_NullArguments_Throw` (Rust has no nulls). Use `FakeFileSystem` (UTF-8, BOM stripped,
    `fail_locked`, pattern `*.json` case-insensitive); the golden covers the "real file" variants.
  - `LocalizerPartialOverlayTests.cs` (3), `TranslatorModesTests.cs` (the 4 `CreatePseudo_*` (3) / `CreateShowKeys_*` (1) tests that need a `Localizer`;
    `Accent_*` is W1-11), `MutationGapLocalizationTests.cs`: `FormatPlural_OnlyOneForm_OneOther`, `FormatPlural_OnlyOneForm_NoneRule`
    (`Accent_MapsFirstLetter`/`CreatePseudo_PaddingLength` on a template are W1-11; the `Localizer`-level `CreatePseudo_PaddingLength` is here);
    `JsonAlias_NullArray_IsEmpty` is W2-01/02 (attribute), NOT here.
  - `FormatPluralResolutionTests.cs` (3), `LocalizerConcurrencyTests.cs` (3; `SwapWhileRendering_NeverTornOrNull` with `std::thread`, no tokio;
    `CurrentChanged_SeesNewLocalizer` with `on_current_changed`), `GeneratedTrTests.cs` (4; `tr::test_sample()` etc. are generated from
    `assets/languages/en.json` `test.*` keys; set/restore `Localizer::current` through a `serial` guard/mutex in the test file).
  - `LocalizationHardeningTests.cs`: `DuplicateKey_Warns`, `UnknownPlural_Warns` (theory), `OversizedFile_WithoutStat_Skipped`,
    `OversizedMultibyteFile_WithoutStat_Skipped` (FakeFileSystem with `fail(FsOp::Stat, ..)`). NOT here: `Describe_*` (W1-11), `Metrics_NegativeInputs_Clamped` (diagnostics unit).
  - `LocalizationFuzzTests.cs`: `Catalog_CorruptedBytes_NeverThrow` (3+ seeds, own xorshift; assert `try_parse` does not panic),
    `Overlay_Hostile_IsContained`, `FormatPlural_ExtremeCounts` (`i64::MIN/MAX`), `Catalog_Large_Loads`, `CatalogCode_TurkishI` (`to_ascii_lowercase`),
    `ShippedCatalogs_RenderEverywhere`, `Pseudo_KeepsExoticText`. NOT here: `LocTemplate_MatchesReference`, `Render_*` (W1-11).
  - `FlatJsonReaderDifferentialTests.cs` (`Valid_BothAccept`, `Soup_Agrees`, `LoneSurrogateInSkippedMeta_AgreesWithRuntime`, `Deep_And_Huge`):
    port against `keygen::read_flat_json` vs `LanguageCatalog::try_parse` (the "STJ flat verdict" of the oracle is `try_parse(..).is_some()` plus entry
    equality); soup from a test-local xorshift with the same token list, no need to reproduce the C# seeds (the golden carries the C# verdicts).
  - `LocalizationGeneratorTests.cs`: the Roslyn compile tests cannot be ported. Replace by `tests/key_generator.rs` over `keygen::plan`
    (`BrokenCatalog_ReportsDiagnostic`, `TwoCatalogs_...`, `IdentifierCollision_...`, `Placeholders_AgreeWithRuntimeTemplateParser`,
    `OtherFileIgnored_...`), and "compiles" is proven by building `pr-l10n` itself with the shipped `en.json` (`ShippedCatalog_GeneratesCompilableCode`,
    `AwkwardText_StillCompiles`: assert the rendered text for awkward English text with exact-string expectations, see Traps 9).
  - NOT here: `FormatPluralBenchmarkManualTests.cs` (manual benchmark), `App.Tests/Localization/*`, `Architecture.Tests/Localization*` (arch wave).
  - Golden tests (new, required): `tests/golden/l10n-catalog.json` (176 cases), `l10n-localizer.json` (47), `l10n-loader.json` (37),
    `l10n-generator.json` (93), `l10n-flat-json.json` (57 texts + 2 soup seeds x 1200 = 2457 cases).
- PARITY rows: P-14 (localization: shipped + user catalogs), S-20 (`UiLanguage` auto/code, `AUTO_CODE`), D-15 (user language files), I18N
  translator modes (`--i18n-pseudo`/`--i18n-keys` plumbing = `create_pseudo`/`create_show_keys`; the CLI args are App), plus a new row
  `X-l10n-keygen` ("`build.rs` + `build/keygen.rs` = C# `TrGenerator`; PRLOC001..007 are build errors"). Lead adds the row.
- Owned files (the ONLY files you may create/modify; the stub tree is `contracts/drafts-w2/W2-04/stubs/crates/pr-l10n/` until the lead moves it, it compiles clippy-clean):
    crates/pr-l10n/src/catalog.rs
    crates/pr-l10n/src/localizer.rs
    crates/pr-l10n/src/loader.rs
    crates/pr-l10n/build.rs
    crates/pr-l10n/build/keygen.rs
    crates/pr-l10n/tests/catalog.rs, localizer.rs, loader.rs, key_generator.rs, flat_json.rs, generated_tr.rs, golden_l10n.rs
  Lead-owned: `crates/pr-l10n/src/lib.rs` (`pub mod catalog; pub mod loader; pub mod localizer; pub mod keys { include!(concat!(env!("OUT_DIR"),
  "/tr_keys.rs")); } pub mod tr { #![allow(non_snake_case, clippy::too_many_arguments)] include!(concat!(env!("OUT_DIR"), "/tr_fns.rs")); }`),
  `Cargo.toml` (no new dependencies, no build-dependencies: `keygen.rs` is std-only). `crates/pr-l10n/src/template.rs` is W1-11 (read-only here).
- Read-only inputs: the five goldens above, `assets/languages/{en,vi}.json`, `pr_l10n::template`.
- Public API (stub, do NOT change signatures):
  - `catalog`: `PluralRule{OneOther,None}`, `LanguageCatalog` (`code/name/native_name/plural/native_name_declared/plural_declared/authors/entries/entry`,
    `try_parse(json, source, &mut Vec<String>) -> Option<_>`, `is_valid_code`, `safe`), `MAX_FILE_BYTES`, `META_KEY`, `MAX_DISPLAY_NAME_LENGTH`,
    `MAX_WARNING_FRAGMENT_LENGTH`.
  - `localizer`: `Localizer` (`create`, `get`, `format[_with]`, `format_plural[_with]`, `transform`, `current`, `set_current`, `keys`, `contains`,
    accessors, `clear_current_for_tests`), `on_current_changed` -> `CurrentChangedSubscription`, `number_culture/set_number_culture`,
    `create_pseudo`, `create_show_keys`, `built_in_english`, `built_in_english_localizer`, `EMBEDDED_ENGLISH_JSON`, `ENGLISH_CODE`.
  - `loader`: `LanguageLoader` (`new`, `with_native_name_comparer`, `discover_languages`, `load`, `resolve`), `LanguageInfo`, `UiCulture`
    (`invariant`, `from_name`), `current_ui_culture/set_current_ui_culture`, `AUTO_CODE`, `NativeNameComparer`.
  - `build/keygen.rs`: `read_flat_json`, `plan`, `is_valid_key`, `to_identifier`, `try_get_placeholders`, `const_name`, `fn_name`, `param_name`,
    `render_keys`, `render_tr`, the PRLOC consts and data types.

## Semantics notes (facts checked against the C# at d92d6aae and the oracle goldens)

### LanguageCatalog::try_parse
1. **Parse layer is hand-written** (not `serde_json`): grammar of `JsonDocument.Parse(CommentHandling.Skip, AllowTrailingCommas)`, max depth 64
   where the ROOT object is depth 1 (golden: `_x` holding 63 nested arrays is fine, 64 is invalid; 63/64 nested objects likewise), `//` comments end at
   LF, CR, U+2028 or U+2029 (not at U+0085), `/* */` unterminated is invalid, a comment between a name and its `:` is invalid, a leading U+FEFF
   CHARACTER in the string is invalid JSON (the loader only works because `read_all_text` strips the BOM), numbers follow JSON (no `01`, `+1`, `1.`,
   `.5`, hex, `NaN`), literals are case-sensitive, a raw control char (< U+0020) in a string is invalid, `\u` needs exactly 4 hex digits (any case).
   Duplicate names are kept in order. Any grammar failure => `"{safe(source)}: invalid JSON ({message})"` and `None`; the message text is free
   (the golden masks `(...)` as `(*)`), but keep the `({...})` shape and run it through `safe`.
2. **Lazy strings.** A lone surrogate escape (`\ud800`, `\udc00`, reversed pair) is VALID JSON; it only fails when the string is READ: every property
   name that is looked at (`property.Name` is read for every root property that is not `_meta`, and for every `_meta` property), every string
   value that is used (`code`, `name`, `nativeName`, `plural`, string items of `authors`, the value of a string entry). It is fine inside an
   `_`-prefixed root value, in a non-string value of an entry key, in a non-string `_meta` value of a known key (`code: 5`), and in non-string items
   of `authors`. The first failing read aborts the whole parse: `"{safe(source)}: unreadable text ({message})"`, `None`, and warnings added
   BEFORE the failure stay (the golden case list is exhaustive on this; implement it with a lazily-decoded token or a "has lone surrogate" flag).
3. **Root.** Not an object => `"{source}: root must be a JSON object"`. Root properties in order: name equals `_meta` (compared AFTER unescaping,
   case-sensitive: `"\u005fmeta"` is meta, `_META` is just an underscore key) => `ReadMeta`; other `_`-prefixed name => skipped silently; value not a
   string => `"{src}: '{safe(name)}' is not a string, ignored"`; duplicate => `"{src}: duplicate key '{safe(name)}', the last value is used"` and the value
   REPLACES in place (keeps the first position). Entries compare keys ordinal and case-sensitive; the empty key is an ordinary entry.
4. **ReadMeta** (called per `_meta` property, a second `_meta` merges: later `code`/`name`/... win, authors append, an earlier value stays when the
   later one is missing): not an object => `"{src}: _meta must be an object"` (and nothing else changes). Known keys only when the value has the right
   JSON type, else silently ignored: `code` string; `name`/`nativeName` string => `CleanDisplayName`; `plural` string => `plural_declared = true`, `none`
   (OrdinalIgnoreCase) => `None`, `one-other` (ignore case) => `OneOther`, anything else (also `""`) => `OneOther` + warning
   `"{src}: unknown _meta.plural '{safe(rule)}', using one-other (valid: one-other, none)"`; `authors` array => keep string items only.
5. **After the loop**: `code` null, whitespace, or `!is_valid_code` => `"{src}: _meta.code is missing or invalid"` and `None` (earlier warnings
   stay, order = emission order). Else `code = code.to_ascii_lowercase()` (invariant; the Turkish-I golden `ID` => `id`), `name = name ?? code`,
   `native_name = native ?? name ?? code`, `native_name_declared = native.is_some() || name.is_some()` (cleaned, non-blank), `plural_declared` as above.
6. **`is_valid_code`**: length 2..=16 in UTF-16 units (= chars here, an astral char fails the letter/digit test anyway), first char ASCII letter,
   every char ASCII letter/digit/`-`. Trailing `-` is valid, `a_b`/`a.b`/non-ASCII are not. Test with `code` of `" vi"` (invalid).
7. **`CleanDisplayName`**: control chars (Unicode Cc, `char::is_control`) become a SPACE, `trim()` (Unicode White_Space = .NET `char.IsWhiteSpace` for the
   golden inputs), cut to 64 UTF-16 units then `trim_end()`, blank => `None` (falls back). Trap: the cut may split a surrogate pair; C# keeps a lone
   high surrogate, the oracle's JSON writer serialises it as U+FFFD (golden `native-name-surrogate-pair-cut-at-64-utf16-units` expects 63 chars +
   `\ufffd`). A Rust `String` cannot hold it: cut by UTF-16 units and replace a split pair by U+FFFD.
8. **`safe`**: `None` => `""`; control chars (Cc) become U+FFFD (NOT space), U+0085 is Cc (replaced), U+2028/U+2029 are NOT Cc (kept as is);
   more than 120 UTF-16 units => first 120 units + `"..."` (same split-pair rule: U+FFFD). Applies to `source`, keys, rule text, parser messages.
   Note `source` is passed through `safe` in every warning (golden `source-longer-than-120-is-cut`).
9. `entries()` is a `Vec` in first-occurrence order (Trap §7.10: do not iterate the `HashMap`); `index` only for `entry`.

### Localizer
10. **`create(english, overlays, prior_warnings)`**: warnings start with `prior_warnings`. English entries in order: `LocTemplate::try_parse` ok => template;
    else warning `"en: '{key}' has invalid placeholders"` (raw key, NOT `safe`) and `LocTemplate::literal(text)`. The "English" snapshot used for validation
    is taken AFTER that (a literal fallback has no placeholders). `code/native_name/plural` start from English.
    Per overlay: `same_language = overlay.code == code` (ordinal, against the CURRENT code, which earlier overlays may have changed);
    `code = overlay.code`; `if overlay.native_name_declared || !same_language { native_name = overlay.native_name }`; same for plural. Then per entry:
    key not in English => `"{overlay.code}: unknown key '{safe(key)}' ignored"`; `try_parse` fails => `"{code}: '{safe(key)}' has unbalanced braces
    or an invalid placeholder; English used"` (skip); first translated placeholder (in translated order) not in English's names =>
    `"{code}: '{safe(key)}' uses unknown placeholder {{{name}}}; English used"` (skip, i.e. text `{name}` with literal braces); else if an English
    placeholder (English order) is missing => `"{code}: '{safe(key)}' does not use placeholder {{{name}}}"` (only the FIRST missing, and the
    template IS accepted). Accepted => replaces the value (later overlay wins; a rejected value keeps the earlier overlay's).
    Overlay entry order is the overlay's `entries()` order; warnings follow it.
11. **Lookup**: `get(key)` = `render(&[])` or the key itself (placeholders stay as `{name}`); `format` unknown key => the key; `contains`, `keys`
    (English key set; sort in tests, order is not contractual).
12. **`format_plural_with(base, count, args, culture)`**: `plurals` holds, for every key ending in `.other` (including `".other"` => base `""`), the
    pair (`base.one` if present, other). Base found: `one` only when `plural == OneOther && count == 1 && one.is_some()`, else `other`. Base NOT found
    (no `.other` key): key = `base.one` if `OneOther && count == 1` else `base.other`; if that key does not exist use `base.other`; then plain `format`
    => when `.other` is missing the RESULT IS THE TEXT `base.other` (goldens `plural-only-one-form-present`, `NoFormsAtAll_ReturnsOtherKey`).
    `count` is `i64` (`i64::MIN` ok). Built once at construction, rebuilt by `transform`.
13. **`transform(code, native_name, plural, f)`**: new templates for every key (`f(key, template)`), warnings copied, `plurals` rebuilt.
    `create_pseudo` = `transform(PSEUDO_CODE, "Pseudo", OneOther, |_, t| template::pseudo_template(t))`; `create_show_keys` =
    `transform(code, native_name, plural, |key, _| template::show_key_template(key))`. (Padding is by the UTF-16 length of the SOURCE text: W1-11.)
14. **Ambient state**: `current()` returns the installed `Arc`, installing `built_in_english_localizer()` if empty (compare-exchange: two racing
    threads end with the same Arc); `set_current` stores, then calls the handlers on the calling thread OUTSIDE every lock (copy the handler list
    first, so a handler may unsubscribe/`set_current`), a handler panic propagates like a C# exception after the value is visible.
    `CurrentChangedSubscription::drop` unregisters (C# `-=`); `on_current_changed` ids from an `AtomicU64`. `number_culture()` default
    `NumberCulture::invariant()`; the ambient number culture is NOT the UI language (C# `CurrentCulture` vs `CurrentUICulture`).
15. **BuiltInCatalog**: `OnceLock`; `try_parse(EMBEDDED_ENGLISH_JSON, "en (built-in)")`, `expect` with `"Built-in English catalog is invalid: " + warnings.join("; ")`.
    The embedded text is `assets/languages/en.json` (986 lines); a mismatch with the shipped file cannot happen (same file), keep the test anyway.

### LanguageLoader
16. **Read pipeline** (`ReadAll` = shipped folder, then user folder when `Some`; each folder: `directory_exists` false => nothing; `enumerate_files(dir,
    "*.json")` error => ONE warning `"{dir}: cannot list files ({err})"`, folder skipped; then files sorted OrdinalIgnoreCase on the full path via
    `text::cmp_ignore_case` (stable; `_` sorts after letters because .NET upper-cases first: golden `file-order-underscore-versus-letters-...`):
    skip when `get_file_name_without_extension(file)` contains `.` (`en.notes.json`, `a.b.json`; a file named just `.json` has an empty stem and IS a
    candidate); `get_file_stat` Some && `length > MAX_FILE_BYTES` => `"{file}: larger than 1048576 bytes, skipped"`; `read_all_text`; then the text's
    UTF-8 byte length `> MAX_FILE_BYTES` => same warning (stat missing or stale; exactly 1 MiB of spaces is read); any `io::Error` from stat/read =>
    `"{file}: cannot read ({err})"`; then `try_parse(json, file, warnings)` whose warnings are appended in order. Valid catalogs carry `is_user`.
    Warnings of ALL folders and files come back in folder order (shipped files, then user files).
17. **`discover_languages`** (throw-away warnings): dictionary keyed OrdinalIgnoreCase seeded with `en` => (`built_in_english` name/native_name, `false`);
    then each catalog in read order, the FIRST per code wins (so a shipped `en.json` never replaces the built-in entry and the shipped entry beats the
    user's). Result: `en` first, then stable sort by native name with `native_name_comparer` (default `cmp_ignore_case`; C# uses the current culture's
    ignore-case collation, equal names keep discovery order).
18. **`resolve(code, culture)`**: `code` non-blank and not `auto` (OrdinalIgnoreCase) => the discovered code equal ignoring case, else `"en"`;
    blank/None/`auto` => culture (`None` => `current_ui_culture()`): code equal to `culture.name` (ignore case), else equal to
    `two_letter_iso_language_name`, else `"en"`. `zh-Hant-TW` does NOT match `zh-hant` (only the exact name does, golden `script-tagged-cultures`).
    Returned strings are the catalog's lower-case code.
19. **`load(code, culture)`**: read once (`warnings` = read warnings), `resolved` as above (using the SAME read), overlays: when resolved != `en`
    (OrdinalIgnoreCase) all catalogs with that code, shipped before user (stable); when resolved == `en` only USER files with code `en`; then
    `Localizer::create(built_in_english(), overlays, warnings)`. Never fails; an unknown code is `en` + the read warnings.
20. **`UiCulture::from_name`**: `""` => invariant (`"iv"`); else the part before the first `-`, lower-cased. Exact for the cultures probed by the
    golden (`vi-VN`, `pt-BR`, `zh-*`, `fil-PH`, `haw-US`, ...); .NET maps a few three-letter languages to two letters (`ckb` => `ku`): the platform
    layer constructs `UiCulture` from `GetLocaleInfoEx` for the real app.

### Key generator (`build.rs` + `build/keygen.rs`)
21. **`plan(inputs)`** (callers have already filtered `en.json` case-insensitively): no input => `Plan{generated:false}` and no diagnostics; sort by
    ordinal path (UTF-16 order; use `encode_utf16().cmp`), every input after the first => PRLOC007 `'{path}' is an extra en.json: only '{first}' (first by
    path) is used to generate Tr`; then analyse the first: `content == None` => PRLOC001 `en.json is not a valid flat catalog: file cannot be read`,
    `generated:false`; `read_flat_json` error => PRLOC001 with the reader's message (the messages are asserted by the golden: copy the C# strings
    exactly, e.g. `unpaired surrogate in string`, `invalid escape '\x'`, `expected ',' or '}'`, `nesting too deep`), `generated:false`.
22. **Keys**: per entry in file order: `!is_valid_key` => PRLOC002 `Key '{k}' is invalid: use ASCII letters, digits and '.', start with a letter, no empty
    segment` (skipped); already seen (ordinal, case-sensitive) => PRLOC006 `Key '{k}' appears more than once` (first wins); invalid placeholder text =>
    PRLOC004 `Text of '{k}' has unbalanced braces or an invalid placeholder (use {name}, and {{ }} for literal braces)` but the key still gets a const.
    `TrKeys` consts: one per valid unique key, identifier via `to_identifier`; a clash => PRLOC003 `Key '{k}' generates identifier '{Type}.{Ident}',
    which is already used by '{owner key}'` with `Type` = `TrKeys` then `Tr` (so one colliding key reports twice, `Tr.` and `TrKeys.`), or
    `... already used by a built-in member` for the reserved names `Tr, TrKeys, Equals, ReferenceEquals, GetHashCode, GetType, ToString,
    MemberwiseClone, Finalize` (case-sensitive: the golden checks `tr`/`Tr`). Keep this C# reserved list exactly (the golden is keyed by the C# identifiers);
    ADD the Rust-only rule in 25 below.
23. **Members**: keys ending `.one`/`.other` with a non-empty base are plural-group keys (group processed once, at its first key in file order); a plain
    valid-text key is a simple member; `.one` without `.other` => PRLOC005 `Plural group '{base}' has '{base}.one' but no '{base}.other'` (no member);
    group members need both present forms to have valid text; parameters = `other`'s placeholders then `one`'s, deduplicated, without `count`; a
    plain key may have a placeholder named `count` (ordinary parameter). The group identifier is `to_identifier(base)` and competes with plain keys.
    A plural key never creates a simple member, but its const is created like any key.
24. **Placeholders** (`try_get_placeholders`): same grammar as `LocTemplate::try_parse` (`{{`/`}}` literal, `{name}` ASCII letter first then letters/digits,
    unmatched `}` or `{` invalid, `{{{`... follows left-to-right). Golden `fuzz-placeholders-seed-1/2` (2 x N random texts) asserts `generator_rejects ==
    !runtime_accepts` and equal placeholder lists: the test compares against `pr_l10n::template::LocTemplate` (dev-dependency on the crate itself).
25. **Rust naming** (decided in the stub, tested by hand, not golden): `const_name("StatusBatchDone") = STATUS_BATCH_DONE` (underscore before each
    upper-case char except the first, then upper-case all); `fn_name` same rule lower-cased; Rust keywords (strict, reserved, 2018 and 2024: `type`,
    `match`, `async`, `await`, `dyn`, `gen`, `try`, `abstract`, `become`, `box`, `do`, `final`, `macro`, `override`, `priv`, `typeof`, `unsized`,
    `virtual`, `yield`, ...) become `r#...`; `self`/`crate`/`super` cannot be raw => PRLOC003 `... already used by a built-in member`
    (key `self`, ident `Self`, no golden case). `param_name`: keywords `r#`-escaped, `self`/`Self`/`crate`/`super` => `<placeholder>_` (`self_`; a
    placeholder cannot contain `_`, so no collision). The golden's `params-rust-keywords-valid-subset` has `{type} {match} {self} {Self} {crate}
    {super} {fn} {mut} {async} {await} {dyn} {gen} {try}` all valid placeholders (and `{r#ref}` invalid): the generated function must compile with them.
26. **Rendered code**: `keys`: `pub const STATUS_BATCH_DONE: &str = "status.batchDone";\n` per const (use `{:?}`-style escaping of the key via a
    small escaper, keys are ASCII). `tr`: simple no-arg => `pub fn test_sample() -> String { Localizer::current().get(keys::TEST_SAMPLE) }`; with
    params => `pub fn test_greeting(name: impl Into<LocValue>) -> String { Localizer::current().format(keys::TEST_GREETING, &[LocArg::new("name",
    name)]) }`; plural => `pub fn test_items(count: i64, ...) -> String { Localizer::current().format_plural("test.items", count,
    &[LocArg::new("count", count), ...]) }`. The doc comment is (C# also XML-escapes `& < >`; not needed for `///`) `English: "<text>"` (`"one" / "other"` for groups) with `\n` shown as the two
    characters `\n`, `\r` dropped, U+0085/2028/2029 and other control chars replaced by a space. The generated file needs `use crate::localizer::Localizer;
    use crate::template::{LocArg, LocValue};` at its top (put it in `render_tr`, not in lib.rs, so the text is self-contained).
27. **Diagnostics ordering is not contractual**: the oracle sorts by (id, message) ordinal; sort the same way in tests. `build.rs` prints each as
    `cargo:warning` lines AND fails with `PRLOCnnn: message` (any diagnostic fails the build, as in C#).
28. **`read_flat_json`** (keygen copy of `FlatJsonReader`): flat object of string values; `_`-prefixed keys may hold any value (skipped, depth limit
    `depth > 64` with the key's value at depth 0, so 65 nested arrays PASS the reader while the runtime parser rejects 64+ in total: the golden pins this
    asymmetry, see problems); BOM char at index 0 skipped; unpaired surrogate in a KEY or entry VALUE => error, but inside a skipped `_` value it is
    fine; comment between key and `:` is an error; offsets are byte offsets (C# UTF-16, diagnostics only). A non-`_` key whose value is not a string is an ERROR here
    (`value of '{key}' must be a string`, PRLOC001) whereas the runtime only warns and ignores it. Other messages the golden pins: `expected '{'`,
    `unexpected end of file, expected '{c}'`, `unexpected content after the root object`, `unterminated string`, `unterminated comment`,
    `control character in string (escape it)`, `invalid ` + backslash + `u escape`, `invalid number`, `expected a JSON value`, `expected ',' or '}'` (`']'` inside
    an array). Re-read `FlatJsonReader.cs` for the exact wording; whitespace here is only SP/TAB/CR/LF.

## Traps checklist (docs/MULTI-AGENT-PLAN §7)
- 1 OrdinalIgnoreCase: file ordering and code compares use `pr_core::text::{cmp_ignore_case, eq_ignore_case}`; code lower-casing is ASCII
  (`to_ascii_lowercase`, valid codes are ASCII; never `to_lowercase`).
- 2 Culture: warnings and formatting never depend on the machine; `format` takes the explicit `NumberCulture`; the picker's CurrentCulture collation
  is replaced by `cmp_ignore_case` (open question 3).
- 8 Null vs empty: `Safe(null)` is `""`; `plural_declared` for `"plural": ""` is true; a blank `code` is a failure.
- 9 Exceptions: C# catches `JsonException` (invalid JSON) and `ArgumentException|InvalidOperationException` (unreadable text) separately: two distinct
  warnings; `IOException|UnauthorizedAccessException` in the loader only (other exceptions propagate; Rust maps any `io::Error`).
- 10 Order: `entries`, overlays, folder listing, discovery order (ties in native name) all stable and insertion-ordered; no `HashMap` iteration in output.
- 3 Sizes: `MAX_FILE_BYTES` is `u64`; compare byte length (`>`), exactly 1 MiB is accepted.
- Build-script trap: `build/keygen.rs` is compiled twice (`build.rs` and `tests/key_generator.rs` via `#[path]`); no `crate::` imports, no
  dependencies; `cargo:rerun-if-changed=../../assets/languages/en.json`; write BOTH output files every run.
- Warnings inside `unwrap_or_else`/`?`: every PRLOC and every catalog problem is a value (`Vec`), never a panic; `try_parse` must survive arbitrary UTF-8.

## Depends on (other units / already merged)
- W1-11 `pr_l10n::template` (`LocTemplate`, `LocArg`, `LocValue`, `NumberCulture`, `pseudo_template`, `show_key_template`, `PSEUDO_CODE`): merged.
- W0 `pr_core::abstractions::FileSystem` (`directory_exists`, `enumerate_files`, `get_file_stat`, `read_all_text`), `pr_core::text::{cmp_ignore_case,
  eq_ignore_case}`; `pr-test-support` `FakeFileSystem` (+`FsOp`, `fail`, `fail_locked`), `golden`.
- W2-00 `dotnet_path::get_file_name_without_extension` (used for the "dotted file name" rule; MUST be merged first, or the loader test for
  `a.b.json`/`.json` fails). W2-14 `PhysicalFileSystem` is NOT required (the golden's real-file cases run on `FakeFileSystem`; an optional
  `#[ignore = "native"]` twin on `PhysicalFileSystem` + `TempRoot` once W2-14 lands).
- Lead: `assets/languages/en.json` (synced, present), `crates/pr-l10n/src/lib.rs`, ADR R0002 (user folder `%LOCALAPPDATA%\PhotoReviewRs\Languages`, passed in by the App).
- Consumers: every App/UI unit calls `tr::*`; W2-01/02 (`UiLanguage`) use `AUTO_CODE`.

## Done when
All listed tests have a Rust twin with `// C#: <Class>.<Method>`; the five goldens pass; `cargo build -p pr-l10n` succeeds with the shipped `en.json`
(generated `keys::TEST_SAMPLE` and `tr::test_items(count)` exist and `generated_tr.rs` passes); `cargo fmt`, `cargo clippy -p pr-l10n --all-targets
-- -D warnings`, `cargo test -p pr-l10n` clean; the stub's `#![allow(...)]` lines and the `todo!()`/"STUB" comments are gone from all four source files.

## Golden package
- `oracle/W2-04.cs`: commands `l10n-catalog`, `l10n-localizer`, `l10n-loader` (real temp files, optional BOM/size padding, one file held open to
  provoke "cannot read"), `l10n-generator` (the real Roslyn `TrGenerator` over in-memory `AdditionalText`; extracts consts and `Tr` members with
  regexes), `l10n-flat-json` (STJ verdict vs the generator's `FlatJsonReader` through reflection, plus seeded token soup). Dispatch lines are in
  the file header; the project file needs a `Microsoft.CodeAnalysis.CSharp` reference and the generator DLL (also in the header). Every command runs
  with the invariant culture; warnings carry `<ROOT>` and `(*)` masks (`L10nMask`), so the Rust test must apply the same two replacements
  (`root` => `<ROOT>`, and regex `^(.*(?:invalid JSON|unreadable text|cannot read|cannot list files)) \(.*\)$` => `$1 (*)`) before comparing.
- `golden-inputs/l10n-{catalog,localizer,loader,generator,flat-json}.json`, outputs in `golden-reference/` (each `{generator, csharp_commit, cases}`).
- Case design: catalog 176 (code validity matrix, meta merging, display-name cleaning, plural rule, escapes, every comment/number/literal/depth
  edge, lone surrogates in every read position); localizer 47 (every warning, overlay order, plural fall-backs, pseudo/showkeys, shipped
  en/vi); loader 37 (order, size limit, BOM, lock, culture resolution, partial user overlay); generator 93 (every PRLOC, reserved names,
  collisions, plural groups, keyword placeholders, fuzz); flat-json 57 + 2400 soup cases (generator reader and runtime must agree on every one).
- Test harness hints: `{{REPEAT}}` (`repeat`/`source_repeat`), `{{DEEP:n}}`, `{{DEEPOBJ:n}}`, `{{DEEP_ARRAYS}}` (5000) are expanded by the
  test before parsing; localizer `args` objects `{"t":"double","bits":"<hex>"}|{"t":"bool"}|{"t":"int"}` map to `LocValue`; `english_asset`/`overlay_assets`/
  `files[].asset` name files under `assets/`; for the generator the Rust test maps each C# `ident` to `const_name`/`fn_name` and compares
  `(key|base, params)` only, plus the diagnostics `(id, message)` list verbatim (messages 1:1 with the C# text, `{{`/`}}` already collapsed).

## Open questions
1. **`read_all_text` and invalid UTF-8**: C# `File.ReadAllText` decodes with U+FFFD replacement and never throws for bad bytes; the Rust
   `FileSystem::read_all_text` (see `FakeFileSystem`) returns `InvalidData`, so a translation file with a stray byte becomes a `cannot read`
   warning instead of a loaded catalog. Accepted divergence (record `// DIVERGENCE:`) or make `PhysicalFileSystem` (W2-14) decode lossily? Lead decides.
2. **Shared JSON scanner**: `catalog.rs` and `keygen.rs` implement two similar scanners (one with lazy strings and depth 64 from the root, one with
   depth 64 from the value). Sharing one std-only file by `#[path]` is possible but the two differ on depth and string handling (golden pins both);
   recommended: keep two implementations and let `flat_json.rs` differential-test them (as C# does).
3. **Native-name collation**: C# sorts with `StringComparer.CurrentCultureIgnoreCase` (ICU/NLS collation: `Á` sorts next to `A`); the stub default is
   ordinal ignore case. Equal on the golden (ASCII initials, one accented name sorted after unaccented ones by chance). A platform-injected comparer
   (`with_native_name_comparer`) is the escape hatch; the App wave should wire `CompareStringEx`.
4. **Doc comments**: rustdoc cannot show the English text for a key containing `*/`-like text (not an issue for `///`), nothing to decide; the
   rendered `tr` module needs `#![allow(non_snake_case)]` for placeholders like `{X}`: contract assumes lead's lib.rs carries it (see Owned files).

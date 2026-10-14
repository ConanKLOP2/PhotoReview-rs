# W2-03: SettingsStore (load / repair / backup / save of config.json) + Rust-only first-run import
- Color: 🟡 (INV-11: a corrupt or locked config must never be overwritten or lost) | Porter: **sonnet** (one 508-line state machine with
  ordering-sensitive flags; the stub already pins every signature, so opus is not needed) | Test porter: haiku for the golden drivers and the
  mechanical C# test twins, sonnet for the fault-injection tests | Verifier: sonnet
- C# source (baseline d92d6aae): `src/PhotoReview.Core/Settings/SettingsStore.cs` (508 lines)
  (read with `git -C C:/MyProjects/PhotoReview show d92d6aae:src/PhotoReview.Core/Settings/SettingsStore.cs`).
  Plus ONE Rust-only function, `load_first_run_with_import` (ADR R0002), which has no C# source and no golden.
- C# tests to port (all at d92d6aae; `tests/PhotoReview.Core.Tests/Settings/` unless noted; files use `FakeFileSystem`/`RecordingLog` twins,
  fault injection through `FakeFileSystem::fail/fail_times/fail_locked` with `FsOp::{ReadText,Copy,WriteAtomic,CreateDirectory,EnumerateFiles,Delete}`):
  - `SettingsStoreTests.cs` (419 lines, 19 tests: `Load_WhenFileDoesNotExist_CreatesAndSavesDefault`, `Load_Inv11_*`, `Load_TransientReadFailure_IsRetried`
    (2 rows: IO and unauthorized), `Save_WritesAtomicAndRaisesChanged`, `Migrate_UpgradesV1ToCurrent`, `Load_WhenConfigHoldsUnusableValues_*`,
    `Parse_V1Fixture_*`, `Parse_V2Fixture_*` ...). `Save_SourceGeneratedJson_MatchesReflectionFormat` and `Load_SourceGeneratedJson_MatchesReflectionRead`
    compare two C# serializer back ends: Rust has one, port them as a round-trip through `to_json`/`parse_text` and say so in a comment.
  - `SettingsStoreFailureTests.cs` (280 lines, 15 tests): write failures keep file and state, `Migrate_Idempotent`, `Migrate_LanguageDefaults`, salvage
    (`Load_MistypedValue_*`, `Load_SalvageBackupFails_ConfigNeverOverwritten`), `Load_ChangedHandlerThrowsIOException_KeepsLoadedValues`
    (a handler that returns `Err(io::Error)`), `Load_UnparsableEnum_*`. `Save_Null_Throws` has no Rust twin (no null): skip and say so. (The C# tests use `InMemoryFileSystem`/`RecordingFileSystem`/`NullLog`; the
    Rust twins use `FakeFileSystem` (+ `FakeFileSystem::count(FsOp::WriteAtomic)` and the written bytes for the durable-write check) and `RecordingLog`.)
  - `SettingsStoreLegacyAndWriteFailureTests.cs` (82 lines, 6), `SettingsStoreLoadRepairPersistenceTests.cs` (277 lines, 19; the four `Build_*`
    tests exercise `SettingsLoadRepairText` and belong to W2-02: skip here), `SettingsStoreMutationGapTests.cs` (196 lines, 10),
    `SettingsStoreSaveAfterNewerVersionLoadTests.cs` (167 lines, 9).
  - `tests/PhotoReview.Core.Tests/Session/SessionWritePolicyTests.cs`: `SettingsSave_UsesDurableAtomicWrite` only (the session tests are W2-12).
  - Golden tests (new, required): `tests/golden/settings-parse-text.json` (66 cases), `tests/golden/settings-store.json` (43 scenarios).
  - Not claimed here (flag to the lead, see Open questions): `SettingsParseTextTests.cs`, `SettingsRobustnessTests.cs`, `SettingsParsePropertyTests.cs`,
    the many `*SettingsTests.cs` files that call `SettingsStore.Parse`/`ParseText` for one property each. They only need `parse`/`parse_text`
    and fit whichever unit owns the property.
- PARITY rows: S-64 (load and repair pipeline), S-65 (save), INV-11, D-01, D-02 (`.corrupt-` name), D-10 (import uses `ParseText`), C-28
  (repair notice list), plus a new row for the R0002 import (suggested: "`X-config-import` | Rust-only | first-run read-only import of the C# config").
- Owned files (the ONLY files you may create/modify):
    crates/pr-core/src/settings/store.rs          (stub: contracts-w2/W2-03/store.rs, compiles clippy-clean)
    crates/pr-core/tests/settings_store.rs
- Lead-owned edit: `crates/pr-core/src/settings/mod.rs` gets `pub mod store;`.
- Read-only inputs: the two goldens, `fixtures/config-v1.json` and `fixtures/config-v2.json` (the C# test fixtures, copy to `tests/fixtures/`).
- Public API (stub, do NOT change signatures): `SettingsStore::{new, with_startup_error_callback, with_key_name_validator, with_read_retry_sleeper,
  current, key_names, last_load_repairs, subscribe_changed, unsubscribe_changed, validate_shortcuts, load, save, parse_text, parse, migrate,
  try_salvage, load_first_run_with_import}`, `SettingsStore::{MAX_NEWER_BACKUPS, READ_RETRY_DELAYS_MS}`, `SettingsSaveError`,
  `SimpleKeyNameValidator`, `csharp_config_path`, `CONFIG_FILE_NAME`, type aliases `StartupErrorCallback`, `ChangedHandler`, `RetrySleeper`.
  The private helpers are suggestions.
- Semantics notes (checked against the C# at d92d6aae and the goldens):
  1. **Load state machine** (`path = None` is the default file `app_paths.config_file`; `Some(p)` is an explicit file). First `last_load_repairs = []`,
     `keep_corrupt_file = false`, `newer_file_on_disk = false`. If the file exists: read with retry, `parse_text`.
     a. `salvage_cause` set: `log_startup_error("config.json has unreadable values (<salvaged joined ", ">); the rest was kept", cause)`, then copy to
        `UniqueBackupPath(file)` (kind `corrupt`). Copy failure (any `io::Error`): `keep_corrupt_file = path.is_none()` and
        `log_startup_error("Could not back up config.json with unreadable values", err)`. Success sets `backed_up`.
     b. `reset_settings` not empty: `log.warn("config.json had invalid values, reset to defaults: " + ", ".join(..))`. `disabled_shortcuts` not empty:
        `log.info("Optional shortcuts disabled because their key is already bound: " + ..)`.
     c. `last_load_repairs = parsed.repairs()`; `current = loaded`; `newer_file_on_disk = path.is_none() && loaded.config_version > 3`.
     d. Repairs not empty AND `path.is_none()` AND `config_version <= 3`: if not `backed_up`, copy to `UniqueBackupPath(file, "repaired")`; on copy
        failure `keep_corrupt_file = true` and `log_startup_error("Could not back up config.json before writing repairs", err)`; then
        `try_persist_repairs` (a write failure => `log_startup_error("Could not write the repaired config.json; the repairs apply to this session only", err)`).
     e. Raise `Changed` ONCE after all file work, with `current`. A clean file is never rewritten and never backed up.
  2. **Failure branches.** Not a JSON object (or `parse_text` `Err`): `log_startup_error("Corrupt config.json detected, resetting to defaults", err)`,
     copy to `.corrupt-<stamp>`. Copy failure: `log_startup_error("Could not back up corrupt config.json, using in-memory defaults for this session", err)`,
     `keep_corrupt_file = path.is_none()`, `current = defaults`, raise `Changed`, RETURN (nothing written). Copy success: fall through to step 4.
     Any `io::Error` while reading (after the 4 retries): `log_startup_error("Config.json inaccessible, using in-memory defaults for this session", err)`,
     `keep_corrupt_file = path.is_none()`, `current = defaults`, raise `Changed`, return. The file is left untouched (a locked valid config must survive).
  3. **No file** (or file exists check false): go to step 4 as well.
  4. **Defaults path.** `settings = AppSettings::new(texts)`. `path.is_none()`: `persist(settings)`; an `io::Error` => `log_startup_error("Could not write
     default config.json, using in-memory defaults for this session", err)` and `current = settings`; then raise `Changed` exactly once. Explicit path:
     `current = settings`, raise `Changed`, write nothing. (Explicit path + missing file therefore writes nothing at all.)
  5. **`ReadAllTextWithRetry`**: up to 4 retries, pauses 50/100/200/400 ms, on ANY `io::Error` (C#: `IOException` or `UnauthorizedAccessException`);
     before each retry `log.info("config.json read failed (<IOException|UnauthorizedAccessException>), retrying: <message>")`. Map the Rust error to
     the .NET type name: `PermissionDenied` => `UnauthorizedAccessException`, everything else `IOException`. The fifth failure is returned.
  6. **`persist`**: if `keep_corrupt_file`: `log.warn("Settings kept in memory only: config.json is preserved untouched because it could not be read or
     backed up")`, `current = settings`, return `Ok` (nothing written, `save` still raises `Changed`). Otherwise `create_directory(parent)`; if
     `newer_file_on_disk`: `backup_newer_config(file)` then clear the flag; `canonicalize_all`; `config_version = 3`; `to_json`; 
     `write_all_text_atomic(file, json, durable = true)` (golden pins the bytes, CRLF line endings, no BOM); only then `current = settings`.
     On failure `current` and the old file stay as they were. The flag is cleared even if `to_json` then fails (C# order).
  7. **`save`**: `persist` then `Changed` with `current` (the saved settings, canonicalised and stamped); `Changed` is NOT raised when persist fails.
     `persist` always writes `app_paths.config_file`, so a `save` after `load(Some(p))` writes the DEFAULT file (C# behaviour; do not "fix").
     The Rust-only `SettingsSaveError::Json` (non-finite double) is what C# surfaces as `ArgumentException`: `load` never swallows it (C#'s catch
     blocks only take IO errors); in the load paths it cannot occur for defaults or normalised values, so map it to an `io::Error` of kind `InvalidData`
     only if the signature forces it.
     A handler `Err` is returned after the file was written and `current` updated.
  8. **Newer-build backup**: only when the file exists. Name `<file>.newer-<yyyyMMddHHmmss>` (UTC from the injected clock); if names with that stamp
     already exist, `-N` with N = highest counter + 1 (a bare stamp counts as 1; an unparseable counter as 0). Logs `info("config.json from a newer
     build backed up before being overwritten: <backup>")`. A copy failure (the `EnumerateFiles` listing done by `NewerBackupPath` to pick the name is in the same `try`, so its failure takes this branch too) logs `warn("Could not back up the newer-build config.json before
     overwriting it: <message>")`, prunes NOTHING and never blocks the save. Then prune: list `config.json.newer-*`, keep names starting with the
     prefix (Ordinal), order by (stamp desc Ordinal, counter desc), skip `MAX_NEWER_BACKUPS` (3), delete the rest; a delete failure
     `warn("Could not delete an old newer-build config backup: <message>")`; a listing failure `warn("Could not prune old newer-build config backups: <message>")`.
  9. **`unique_backup_path(file, kind)`**: `<file>.<kind>-<stamp>`, then `-2` ... `-99` while `file_exists(candidate)`; when all taken the LAST candidate
     (`-99`) is returned even though it exists (test `Load_CorruptConfigWithEveryBackupNameTaken_...`).
  10. **`parse_text`** (no disk access; `texts` is explicit in Rust, ambient `Tr` in C#): read the whole document with `AppSettings::from_json`
      (`Ok(None)` => `is_json_null` and defaults); on `Err`, `try_salvage(json::parse(text))` (comments and trailing commas allowed; a root that is
      not an object, or a name with an unpaired surrogate => `Err`, the C# `JsonException`; the cause is kept as `salvage_cause`). Then
      `finish_deserialize`: unversioned object (no exact-case `ConfigVersion` property) => `config_version = 1`; migrate with
      `ui_language_declared` = an object root with an exact-case `UiLanguage` property (even `null` or `""`); `shortcuts`/`actions` are never null in Rust.
      `reset_settings = salvaged ++ (normalize(..) minus salvaged, Ordinal)`; `disabled_shortcuts = disable_conflicting_optional_shortcuts(..)`.
      BOM is NOT stripped by `parse_text` (golden `BOM prefix is NOT stripped by ParseText`); only the file readers decode BOMs.
  10b. **`parse`** (`Parse`, internal test seam): `from_json` of the whole text (`Ok(None)`, i.e. `null`, => `AppSettings::new`), then `finish_deserialize`
      only. NO salvage and NO normalize: any read error is `Err`. (`parse_text` of the string `null` is `Ok`, `is_json_null = true`, defaults; it is not an error.)
  11. **`migrate_core`** order: version < 2 => default actions if missing (never in Rust); version < 3 and language not declared => `ui_language = "vi"`;
      version < 3 (or any lower than current) => version = 3; blank language => `"auto"` (keeps `AUTO` spelling when non-blank); materialise
      `hidden_context_menu_items` from the legacy flags when `None`. Public `migrate(settings)` treats blank/`auto` (OrdinalIgnoreCase) as not declared.
      `config_version` greater than 3 is never lowered by migrate (golden `newer ConfigVersion is not downgraded`).
  12. **`Changed`**: handlers run in subscription order from a snapshot taken without holding a lock (a handler may subscribe/unsubscribe or call
      `current()`/`save` without deadlock); the first `Err` stops the loop. Raised exactly once per `load`/`save`, also in the in-memory-defaults branches.
  13. **Startup errors**: if a callback is set it receives `(message, error)` INSTEAD of `log.error`; otherwise `log.error(message, Some(&err))`.
      The golden `startup_errors` strings are `message + " [" + .NET exception type name + "]"`: `JsonException` for a salvage/corrupt cause,
      `IOException`/`UnauthorizedAccessException` for IO. The Rust error types must be mapped to those names by the test helper, not in the store.
  14. **R0002 import** (`load_first_run_with_import`): own `config.json` (`app_paths.config_file`) exists or C# file missing => exactly `load(None)`.
      Otherwise reset the same state as `load` (`last_load_repairs = []`, `keep_corrupt_file = false`, `newer_file_on_disk = false`; a C# file with
      `ConfigVersion > 3` must NOT set the newer flag: there is nothing to back up at the own path) and read the C# file
      (same retry), `parse_text`, `current = result`, `log.info`, `last_load_repairs = result.repairs()`, write the canonical result to the OWN
      config (version 3) through `persist` (a write failure is handled as in step 4: `log_startup_error("Could not write default config.json, using in-memory defaults for this session", err)`,
      `current` stays the imported result), raise `Changed` once. Read failure, or not a JSON object => `log.warn` and `load(None)` (writes defaults).
      NEVER write, copy, rename or delete anything under the C# folder (no `.corrupt-`/`.repaired-` backup there): the file system calls made for the C# path
      must be reads only (`FileExists`, `ReadText`); a test asserts `FakeFileSystem::count(Copy|WriteAtomic|Delete|Move) == 0` for that path.
      `csharp_config_path(local_app_data)` = `<local_app_data>\PhotoReview\config.json` (`identity::CSHARP_APP_DIR_NAME`).
- Traps checklist (docs/MULTI-AGENT-PLAN.md §7): 1 (`OrdinalIgnoreCase` for `auto`; property names are Ordinal, so `configversion` or `uilanguage` do not
  count), 6 (JSON: comments, trailing commas, numbers as strings, duplicate property => last wins, bad FIRST duplicate still salvaged: all in the
  goldens, owned by W2-01), 8 (the explicit-path rules: no default-file write, backup next to the explicit file), 10 (`.corrupt-`/`.newer-` pruning order),
  and:
  - Flag semantics: `keep_corrupt_file` is `path.is_none()` for corrupt/salvage/IO failures but UNCONDITIONALLY true for a failed `.repaired-` backup (that branch
    is already default-file only). Do not simplify.
  - `Changed` before vs after file work: never inside the try, so a throwing handler is not mistaken for an IO failure and is invoked once (tests
    `Load_RepairedConfig_ThrowingChangedHandlerIsInvokedOnce`, `Load_FirstRun_ThrowingChangedHandlerIsInvokedOnce`). In the two early-return branches the C#
    calls it before returning.
  - Encoding comes from the file system, not the store: `FakeFileSystem::read_all_text` only strips a UTF-8 BOM and errors on invalid UTF-8, so the golden
    scenarios `UTF-16 LE BOM file...`, `UTF-16 BE BOM file...`, `binary garbage is backed up byte for byte` (and the byte-for-byte checks of every `.corrupt-`
    backup) must run on the real `PhysicalFileSystem` (W2-14, `decode_text_like_dotnet`) over a `TempRoot`; the other 40 scenarios may use either.
    Binary garbage must be a `JsonException`-class failure (corrupt + backup), not an IO failure, so the decoder must replace invalid bytes with U+FFFD.
  - `save` canonicalises a CALLER-OWNED value: take `AppSettings` by value (stub does), the caller's object is not mutated in Rust.
  - Stamp: `now.to_json()[..19]` digits only (`2026-10-14T09:08:07` => `20261014090807`); inject `FakeClock` and pin two loads in the same second
    (`-2`) and a whole-second boundary.
  - `last_load_repairs` is reset at the START of every `load`, including explicit-path loads and the early-return failure branches.
- Depends on (all must be merged before this port): W2-01 (`settings::app_settings`: `AppSettings`, `SettingsJsonError`, `SettingsTexts`, `json`,
  `from_json`, `apply_json_property`, `to_json`, `canonicalize_all`), W2-02 (`settings::normalizer`: `normalize`,
  `disable_conflicting_optional_shortcuts`, `SettingsParseResult::repairs`, `SettingsValidator`, `build_repair_text` for the golden `repair_text` field),
  W2-20 (`settings::context_menu::{legacy_hidden, to_stored}`, needed by `migrate_core`; NOT merged yet: `settings/` holds only `perf_options` and
  `shortcuts`), W2-14 for the three encoding scenarios (the rest can use `FakeFileSystem`). Already merged: W0 `abstractions::{FileSystem, Log, Clock,
  KeyNameValidator}`, `app_paths::AppPaths`, `time::DateTimeUtc`, `identity`, W1 `settings::shortcuts`, pr-test-support `FakeFileSystem`, `FakeClock`, `RecordingLog`, `TempRoot`, `golden`.
  No dependency on W2-05..W2-13. (`FileSystem` method names in Rust: `copy_file`, `read_all_text`, `write_all_text_atomic`, `enumerate_files`, `create_directory`.)
- Done when: every listed test has a Rust twin tagged `// C#: <Class>.<Method>` (the skipped ones are listed with the reason in the file header);
  both goldens pass field by field (the `changed` segments, `repairs`, `files_after` names and bytes, `startup_errors`, `log` with `<ROOT>`/`<STAMP>`
  placeholders applied by the test helper, `changed_events` per step); the R0002 tests pass (import happy path, own config wins, unreadable C# file,
  C# file with garbage leaves no backup and no write in the C# folder); `cargo clippy -p pr-core -- -D warnings` clean; the stub's `#![allow(...)]` line
  removed; PARITY rows S-64, S-65, INV-11 marked Done.

## Golden package
- `oracle/W2-03.cs` (Commands partial class, needs only the Core assembly): commands `settings-parse-text` (`SettingsStore.ParseText` per case: outcome
  ok/error, `is_json_null`, `salvaged`, `reset_settings`, `disabled_shortcuts`, `repairs`, `salvage_cause`, `config_version`, `repair_text`, and `changed` =
  the top-level JSON segments that differ from `new AppSettings()`; null `UiLanguage`/`ExternalEditorPath` are reported as `""` because Rust has no null) and
  `settings-store` (real `SettingsStore` + `PhysicalFileSystem` on a throw-away folder; steps `load`, `load_explicit`, `save`; reports per step the repairs,
  version, changed segments, thrown type and `Changed` count, then the files left in the config dir and the "other" dir with stamps replaced by `<STAMP>`,
  the startup errors and the log lines). Dispatch lines are in the file header. Texts are the built-in English catalog.
- `golden-inputs/{settings-parse-text,settings-store}.json`, outputs in `golden-reference/` (generated at csharp_commit d92d6aae; 66 + 43 cases).
- Case design: parse-text covers non-object roots, BOM, salvage of each property kind (Actions, Shortcuts, HiddenContextMenuItems, nulls, surrogates,
  duplicates), every migration branch of `UiLanguage`/`ConfigVersion` (absent, null, empty, `auto`, `AUTO`, different case key), normalizer resets,
  optional-shortcut conflicts, legacy menu flags and both fixtures. store covers first run, every corrupt-file shape, salvage/repair backups and persistence,
  newer-build file with and without a Save, explicit-path variants, encodings, unknown/duplicate fields, and save round-trips (non-finite font size throws
  and leaves the file).
- Gaps (add to the golden or cover with hand-written tests): backup-name collisions (`-2`..`-99`), more than three `.newer-` files (pruning), read retries
  and every `Copy`/`WriteAtomic`/`Delete` failure are NOT in `settings-store` (they need fault injection; the ported C# unit tests cover them with
  `FakeFileSystem`).

## Open questions
- Ownership of `SettingsParseTextTests.cs` (15 uses), `SettingsRobustnessTests.cs` and `SettingsParsePropertyTests.cs`: they test `ParseText`, whose
  steps live in this unit but whose property reading lives in W2-01 and normalization in W2-02. Recommended: this unit takes the three files above (they
  are the best executable spec of `parse_text`); the per-property `*SettingsTests.cs` stay with W2-01. Lead decides, W2-01/W2-02 contracts must not
  duplicate.
- `SettingsStore::new` takes `texts: Arc<dyn SettingsTexts>` because C# reads the ambient `Tr`. If the lead prefers a global, only `new`,
  `parse_text(text, texts)` and `try_salvage` change.
- `Changed` as a handler list (id-based `subscribe_changed`) is the recommended mapping; an `mpsc` channel would lose the "handler threw" semantics the
  tests rely on.

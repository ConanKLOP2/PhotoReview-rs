# W2-12: SessionStore (session file load/save, canonical folder key, startup sweep)
- Color: 🟡 | Porter: sonnet (every hard part is a .NET quirk: `System.Text.Json` read/write rules, `Path.GetFullPath`/`GetPathRoot`, `ToUpperInvariant`, `File.ReadAllText` decoding) | Test porter: haiku | Verifier: sonnet (run the mutations listed at the end)
- C# sources (baseline d92d6aae), under `src/PhotoReview.Core/`:
  - `Session/SessionStore.cs` (170 lines; also defines `SessionState`)
  - `Session/SessionJsonContext.cs` (13 lines; only `WriteIndented = true` and the type registration: nothing to port except the output format below)
  - read-only context: `IO/FileSystemErrors.cs`, `Abstractions/ILog.cs`, `AppPaths.cs` (all already ported)
- C# tests to port (all `tests/PhotoReview.Core.Tests/`):
  - `Session/SessionStoreTests.cs`: `SaveAndLoad_RoundTrips`, `Load_MissingFile_ReturnsDefault`, `Load_FolderWithEmbeddedNul_ReturnsNoSession`, `Load_OverlongPath_ReturnsNoSession`, `Load_CorruptJson_ReturnsDefault`, `Load_JsonWithoutFolder_AssignsFolder`, `Save_CreatesSessionsDirectory`, `GetPath_Canonicalization`, `Constructor_SweepsStaleTempFiles`, `Constructor_SweepContinuesAfterOneDeleteFails`, `Load_NullSkipped_BecomesEmptyList`, `SaveAndLoad_WhileStartupSweepStillRunning_AreUnaffected`, `GetPath_KeepsThePersistedFileNameScheme` (theory, 3 rows), `Save_UnexpectedFileSystemFailure_DoesNotThrow` (theory; Rust twin: ONE row with a file system whose `write_all_text_atomic` returns `io::ErrorKind::InvalidInput`, the Rust image of `ArgumentException`; the `NotSupportedException`/`InvalidOperationException` rows have no Rust type: say so in the report).
    `Constructor_NullValidation`: no Rust twin (`&AppPaths`/`Arc<dyn FileSystem>` cannot be null); say so.
  - `Session/SessionStoreAccessDeniedTests.cs`: `Load_ReadFails_ReturnsAnEmptySessionForTheFolder` (2 rows: `io::ErrorKind::Other` and `PermissionDenied`), `Sweep_ListingDenied_ReturnsZero`, `Sweep_DeleteDeniedForOneFile_OthersStillRemoved`.
  - `Session/SessionStoreEnvironmentRootTests.cs`: `SessionSaveAndLoad`. In Rust `std::env::set_var` is `unsafe` (edition 2024) and global: do NOT set `PHOTOREVIEW_RS_DATA_ROOT`. Port the intent as `SessionStore::new(&AppPaths::new(&root, Some(&root_string), true), Arc::new(PhysicalFileSystem), None)` over a `TempRoot` (save, load, same path/skipped); `from_environment()` itself gets one `#[ignore = "native: reads the real environment"]` smoke test that only constructs it and calls `get_path`.
  - `Session/SessionStoreLogTests.cs`: `Save_WriteFails_DoesNotThrowAndWarnsWithFolderAndReason` (assert the warning contains the folder and the injected failure text), `Load_CorruptFile_ReturnsEmptySessionAndWarnsWithThePath` (assert it contains `get_path(folder)`). The four "logger throws / logger runs out of memory" tests have NO Rust twin (a `Log` method that unwinds is a bug, R0003): say so.
  - `Session/SessionStoreMetricsTests.cs`: `SaveIsCountedLoadIsNot` (use a closure counter: `Arc::new(move || { n.fetch_add(1, SeqCst); })`), `SaveWithoutMetricsStillWorks`.
  - `Session/SessionStoreMutationGapTests.cs`: `SweepStaleTempFiles_TempExactlyOneDayOld_IsKeptAndOlderOneIsRemoved` (wait for the startup sweep FIRST, then add the fixture; `FakeFileSystem::add_file_at` + `DateTimeUtc::checked_add_ticks`), `CanonicalFolder_TrimsTrailingSeparatorsExceptOnADriveRoot` (4 rows), `GetPath_FolderWithAndWithoutTrailingSeparator_IsTheSameFileButDriveRootDiffersFromItsChild`.
  - `Session/SessionStoreSweepPerfTests.cs` (Native): `Constructor_WithManyStaleRealTempFiles_DoesNotBlockOnTheSweep` (3 rows) and `Constructor_WithEmptyDirectory_IsNearInstant`. Port ONLY the correctness half as `#[ignore = "native: real disk, 3000 files"]`: after `wait_for_startup_sweep()` exactly the 5 fresh `*.tmp` and `real-session.json` remain. The `< 50 ms` asserts are wall-clock timing and are replaced by the deterministic gate test above (`SaveAndLoad_WhileStartupSweepStillRunning...`); say so.
  - `Session/SessionWritePolicyTests.cs`: `SessionSave_UsesNonDurableAtomicWrite` (a test-local decorator that records the `durable` argument of `write_all_text_atomic`), `SessionLoad_EmptyOrPartialFile_ReturnsNoSession` (5 rows: `""`, `"   "`, a truncated object, six NUL chars, `"null"`; the row texts may need `\u{0}` escapes). `SettingsSave_UsesDurableAtomicWrite` belongs to W2-03; `PhysicalNonDurableWrite_*` belong to W2-14; `Measure_SessionSaveLatency_*` is Manual: all skipped here, say so.
  - `Json/JsonFormatCompatTests.cs`, only the three session tests: `SessionFile_Bytes_MatchPreviousBuild` (assert the exact text of the file, CRLF included, via the golden case `from test SessionFile_Bytes_MatchPreviousBuild` AND with a literal in the test), `SessionFile_LegacyText_Loads`, `SessionFile_MissingOptionalKeys_LoadsWithDefaults`.
- Golden (provided, see "Golden package"): `session-key` (102 cases), `session-json-write` (20), `session-json-read` (130). They replace most hand-written C# theory data; every golden case must be exercised by a loop test.
- PARITY rows: F-22 (session state file), D-04 (session file names and body), D-05 (the session half: atomic, not fsynced), P-07 (the sessions folder comes from `AppPaths`, already ported)
- Owned files (the ONLY files you may create/modify):
    crates/pr-core/src/session/store.rs
    crates/pr-core/tests/session_store.rs
- Lead declares (porter does not touch): `pub mod session;` in `lib.rs`; `session/mod.rs` with `pub mod store; pub mod writer;`; in `crates/pr-core/Cargo.toml` the new dependency `sha2` (see "Open questions" Q1); the golden inputs/oracle (see "Golden package").
- Read-only inputs: tests/golden/session-key.json, session-json-write.json, session-json-read.json
- Public API (already stubbed in store.rs, do NOT change signatures; the data types, `SessionWriteCounter` blanket impl, error types and their `Display` are complete):
```rust
pub const STALE_TEMP_AGE_TICKS: i64;                                   // 1 day
pub struct SessionState { pub folder: String, pub current_path: Option<String>, pub skipped: Vec<String>, pub updated_utc: JournalDateTime }
impl SessionState { pub fn new(folder: impl Into<String>) -> Self }
pub struct BlankFolder;   pub enum SessionPathError { Blank, Invalid, TooLong }   pub struct SessionJsonError;
pub trait SessionWriteCounter: Send + Sync { fn record_session_write(&self); }   // + impl for Fn()
pub struct SessionStore { .. }   // Debug (manual)
impl SessionStore {
    pub fn new(paths: &AppPaths, file_system: Arc<dyn FileSystem>, metrics: Option<Arc<dyn SessionWriteCounter>>) -> Self
    pub fn from_environment() -> Self
    pub fn with_log(self, log: Arc<dyn Log>) -> Self
    pub fn wait_for_startup_sweep(&self)
    pub fn sweep_stale_temp_files(&self, utc_now: DateTimeUtc) -> usize
    pub fn load(&self, folder: &str) -> Result<SessionState, BlankFolder>
    pub fn save(&self, state: &SessionState) -> Result<(), BlankFolder>
    pub fn get_path(&self, folder: &str) -> Result<PathBuf, SessionPathError>
    pub fn canonical_folder(folder: &str) -> Result<String, SessionPathError>
    pub fn session_key(canonical: &str) -> String
    pub fn to_upper_invariant(text: &str) -> String
    pub fn serialize_state(state: &SessionState) -> String
    pub fn deserialize_state(json: &str) -> Result<Option<SessionState>, SessionJsonError>
}
```
  Todo bodies: everything in `impl SessionStore`. Private helpers (the visitor, depth counter, sweep thread) are yours to add in the same file. The stub's private fields may be reshaped.

## Semantics notes

### 1. What a "session" is and where it lives
One file per folder: `<paths.sessions_dir>\<KEY>.json`, `KEY` = 64 UPPERCASE hex digits. The Rust app has its own sessions folder (`pr_core::app_paths`, R0002); it never reads the C# app's. Only the FORMAT must stay identical (oracle diffs it).

### 2. Identity of a folder: `canonical_folder`, `to_upper_invariant`, `session_key`, `get_path`
All pinned by golden `session-key` (102 inputs). Each case has `folder`, then either `canonical` + `upper` + `key` + `dir_ok`, or `canonical_exception` / `exception` (a .NET exception type name).
- `canonical_folder(folder)`:
  1. `full = std::path::absolute(folder)`. VERIFIED against .NET 10 (`Path.GetFullPath`) on all 102 inputs: identical output, including trailing-dot/space trimming (`C:\photos\a.` -> `C:\photos\a`), `..`/`.` segments, `/` -> `\`, `C:\\` -> `C:\`, UNC, `\\?\` verbatim (kept as typed, `..` NOT resolved) and device paths. The ONE difference: .NET expands 8.3 short names of existing paths (`C:\PROGRA~1\x` -> `C:\Program Files\x`); `std` does not. Accepted divergence (record `// DIVERGENCE:` in the test; no golden input has `~`).
  2. `std::path::absolute` errors map to .NET exceptions: a path with an embedded NUL (`InvalidInput`) or blank text -> `SessionPathError::Invalid` (blank text -> `Blank` is also fine: golden says only `ArgumentException`); a path over the OS limit (kind `InvalidFilename`, OS error 206; .NET `PathTooLongException`, golden: 40 003 chars) -> `TooLong`. Relative folders are made absolute against the current directory like .NET (the golden contains NO relative input, the result depends on the cwd).
  3. `root = get_path_root(&full)` (`file_actions::errors::get_path_root`, W1-13: the normalising `Path.GetPathRoot` port; `unwrap_or_default()`).
  4. If `full` is LONGER than `root` (compare UTF-16 lengths) the result is `full` with every trailing `\` and `/` removed (`C:\photos\\` -> `C:\photos`; `\\server\share\` -> `\\server\share`; `\\?\C:\photos\\` -> `\\?\C:\photos`); otherwise the result is `root` (`C:\`, `c:\` keep their case; `\\?\C:` stays `\\?\C:`). Blank/NUL inputs never reach this step.
- `to_upper_invariant(text)`: per char `text::ordinal_upper(c)`, EXCEPT `U+017F` (long s) which becomes `S`. VERIFIED by exhaustively comparing .NET `ToUpperInvariant` of every non-ASCII code point (1423 mappings) with `ordinal_upper`: they agree on 1422 and differ ONLY on U+017F (.NET `ToUpperInvariant` gives `S`, `OrdinalIgnoreCase` keeps `ſ`). No .NET mapping produces more than one char (`ß`, `ŉ`, `ǰ` stay as they are). Do NOT use `char::to_uppercase`/`str::to_uppercase` (`ß` -> `SS`, locale-free but different). The golden has these inputs: `ß ı İ ǆ ǅ ſ ς σ K(Kelvin) Å(Angstrom) ⅰ ǰ ŉ ᾳ Ꭰ ა 𐐨 𞤢 µ ϴ ẞ ﬁ`.
- `session_key(canonical)` = SHA-256 of the UTF-8 bytes of `to_upper_invariant(canonical)`, printed with `{:02X}` per byte.
- `get_path(folder)`: blank -> `Err(Blank)`; `canonical_folder(folder)?`; `sessions_dir.join(format!("{key}.json"))` (C# `Path.Combine`: no doubled separator when `sessions_dir` ends with one; `PathBuf::join` behaves the same). `dir_ok` in the golden = the parent folder equals `sessions_dir`.

### 3. `load(folder)` step by step
1. `folder` empty/whitespace-only (`str::trim().is_empty()`) -> `Err(BlankFolder)` (C# `ArgumentException`). This is the ONLY error.
2. `get_path(folder)`: `Err(Invalid | TooLong)` -> return `SessionState::new(folder)` ("no session"), no log. (C# RV-C05: embedded NUL / over-long path.)
3. `file_system.file_exists(&path)` false -> no session.
4. `file_system.read_all_text(&path)`: any `Err` (locked, access denied, vanished) -> no session, NO log.
5. `deserialize_state(&text)`:
   - `Ok(Some(mut state))`: if `state.folder.trim().is_empty()` set `state.folder = folder.to_owned()` (the requested spelling); return it. A non-blank stored folder is returned as stored even if it differs from the requested one (golden case `valid: other Folder is returned as stored`).
   - `Ok(None)` (the file is JSON `null`) -> no session, NO log.
   - `Err(_)` -> log ONE warning `Session file '<path>' is empty or corrupt; starting without a session.` and return no session. (ADR 0007 section 2: without fsync a power loss can leave an empty/partial file.)
6. "No session" is exactly `SessionState::new(folder)`: `folder` as given (not canonical), `current_path` None, `skipped` empty, `updated_utc` default.
Golden `session-json-read` pins steps 4-6: `warned` says whether the warning is logged (false for `null`, a missing file and valid files).

### 4. `save(state)` step by step
1. Blank `state.folder` -> `Err(BlankFolder)` (outside the best-effort region, like C#).
2. Best effort region; ANY failure below is logged as the warning `Session write failed for '<state.folder>': <reason>` and swallowed (`Ok(())` is returned):
   a. `file_system.create_directory(&sessions_dir)` FIRST (a folder string that cannot name a file still creates the sessions folder, then fails at b);
   b. `path = get_path(&state.folder)` (`SessionPathError` -> warning with its `Display`);
   c. `json = serialize_state(state)`;
   d. `file_system.write_all_text_atomic(&path, &json, false)`: atomic, NOT durable (ADR 0007 section 2; the recording-decorator test asserts `durable == false`);
   e. only after d succeeded: `metrics.record_session_write()`.
   `reason` = the `io::Error` text (`Display`); the Rust ` (os error N)` suffix may stay.
3. Never panics for I/O errors.

### 5. `serialize_state` (golden `session-json-write`: exact text and file name)
```
{CRLF
  "Folder": "<s>",CRLF
  "CurrentPath": "<s>" | null,CRLF
  "Skipped": [],CRLF                       <- when empty; otherwise:
  "Skipped": [CRLF
    "<s>",CRLF                             <- 4 spaces, items separated by ",CRLF", last without comma
    "<s>"CRLF
  ],CRLF
  "UpdatedUtc": "<ts>"CRLF
}                                          <- no final newline
```
- Member order Folder, CurrentPath, Skipped, UpdatedUtc; indent 2 spaces per level; `: ` after names; line break is CRLF (`Environment.NewLine` on Windows; System.Text.Json uses it). `CurrentPath` None is written as `null` (not omitted).
- String escaping = the default `JavaScriptEncoder` = `DotNetJsonFormatter` of W1-14: build each string literal with `serde_json::Serializer::with_formatter(Vec::new(), DotNetJsonFormatter)` + `Serialize::serialize(&str)`; `"` -> `\u0022`, `\` -> `\\`, `/` unescaped, `& ' + < > \`` and DEL -> `\u00XX`, controls -> `\b \t \n \f \r` or `\u00XX`, every non-ASCII UTF-16 unit -> `\uXXXX` (uppercase hex, astral chars as surrogate pairs). Assemble the object by hand (about 25 lines); serde's `PrettyFormatter` cannot be combined with this escaper and uses LF.
- `UpdatedUtc` = `state.updated_utc.to_json()` (`JournalDateTime`, W1-14): Utc kind `2026-09-18T10:30:00Z` (fraction only when non-zero, trailing zeros trimmed, up to 7 digits), Unspecified kind the same WITHOUT `Z`; default value `"0001-01-01T00:00:00"`.
- A VERIFIED scratch implementation of exactly this reproduced all 20 golden texts.

### 6. `deserialize_state` (golden `session-json-read`, 130 cases)
Reproduces `JsonSerializer.Deserialize<SessionState>(string)` with the default options. A VERIFIED scratch implementation built from the rules below matched all 130 golden cases (outcome, warning flag, values, date kinds/ticks), except the two documented divergences. Rules, in order:
1. Nesting: more than 64 nested `{`/`[` counted outside strings -> `Err` (64 is accepted: the root object plus 63 nested arrays; golden `depth:` cases 62/63 accepted, 64/65/66/100/200 rejected). Same helper as `journal_line::nests_too_deep` (private there: write your own).
2. `serde_json::Deserializer::from_str(json)` then `deserialize_any(visitor)` then `.end()`: exactly ONE value, only JSON whitespace (space, TAB, LF, CR) around it. Empty/blank input, comments, trailing commas, single quotes, unquoted keys, raw control characters in strings, invalid escapes, lone surrogate escapes (`\ud800`, in values and member names), leading zeros / `+1` / `.5` / `NaN` / `0x10`, a second value, trailing garbage, NUL, a U+FEFF that is not the file's first byte-order mark -> `Err`.
3. The visitor: `visit_unit` (JSON `null`) -> `Ok(None)`; `visit_map` -> the object rules below; every other root (array, string, number, bool) -> `Err` (leave serde's default `visit_*` rejections).
4. Object members are read in file order. Names match EXACTLY and CASE-SENSITIVELY after unescaping (`"folder"`/`"currentpath"` are unknown; `"Curr\u0065ntPath"` is `CurrentPath`). Unknown members: `map.next_value::<IgnoredAny>()` (any valid JSON, including `1e999` and 30-digit integers, which a `Value` parse would reject).
5. Known members: read the value as `Box<serde_json::value::RawValue>` and decode `raw.get()` yourself (as `journal_line` does):
   - `Folder`, `CurrentPath`: JSON string or `null` (None). Anything else -> `Err` for the whole file.
   - `Skipped`: `null` (-> empty list) or an array whose elements are ALL strings or `null`. A number/object/array element, or a non-array -> `Err`. DIVERGENCE: a `null` element is KEPT by C# (`["a", null]`); `Vec<String>` cannot hold it, so null elements are DROPPED (golden case `valid: null element inside Skipped...` shows `["a",null,"b"]`; the Rust test expects `["a","b"]` and says `// DIVERGENCE:`).
   - `UpdatedUtc`: a JSON string accepted by `JournalDateTime::parse_json`; `null`, numbers, garbage, `2026-09-18 10:30:00Z` (space), lowercase `z`, `+0700` (no colon), month 13, Feb 30, hour 24, second 60, year 0000, 17+ fraction digits, empty string -> `Err`. Accepted: `Z`, no zone (Unspecified), date only, no seconds, `.Z` (dot without digits), 1..16 fraction digits (digits beyond 7 are dropped), `+hh:mm` offsets. DIVERGENCE (README D5): C# turns an offset text into a `Local` DateTime; `JournalDateTime::parse_json` converts to UTC (the golden's `updated_utc_ticks` for those cases is already the UTC instant; compare ticks only and skip `updated_kind` for them).
   - A malformed occurrence rejects the file even when a later duplicate is valid (`{"Folder":5,"Folder":"x"}` -> `Err`); among valid duplicates the LAST wins (also for `Skipped`).
6. Result: `Folder` missing/null -> `""`; `CurrentPath` missing/null -> None; `Skipped` missing/null -> empty; `UpdatedUtc` missing -> `JournalDateTime::default()`.

### 7. Text decoding is the file system's job, but the goldens exercise it
The `encoding:` golden cases (UTF-8 BOM, double BOM, UTF-16 LE/BE with BOM, UTF-32 LE with BOM, invalid UTF-8 -> U+FFFD, UTF-16 without BOM = corrupt) go through `File.ReadAllText` in C#. In the Rust test decode the hex with `io::physical::decode_text_like_dotnet` (W2-14 function; see Depends on) and feed the result to `deserialize_state`; the `text` cases use `text.as_bytes()` the same way. Tests with a `FakeFileSystem` store plain UTF-8 text.

### 8. The startup sweep (R2-F-34 / R14)
- `SessionStore::new` spawns a thread named `PhotoReview.SessionSweep` that runs `sweep_stale_temp_files(DateTimeUtc::now())` and then marks "done" (also if it panics: use a drop guard). The constructor returns without waiting (measured: the sweep costs 0.25-0.3 ms per stale file, seconds for thousands, and the constructor runs on the UI thread at startup).
- `sweep_stale_temp_files(utc_now)`: `enumerate_files(&sessions_dir, "*.tmp")`; `Err` -> return 0 (a missing sessions folder is the normal first run). For each file: `get_file_stat` `Some(stat)` and `utc_now.ticks() - stat.last_write_utc.ticks() > STALE_TEMP_AGE_TICKS` (STRICT: exactly one day old is kept; a future write time is never stale) -> `delete`; `Ok` -> count it, `Err` -> skip it and go on. A vanished file (`stat` None) is skipped. Real session files (`*.json`) and fresh temp files are never touched.
- `load`/`save` never touch `*.tmp` files and never wait for the sweep (a test proves it with a gate).
- `wait_for_startup_sweep()` is the test hook (C# `StartupSweepTask`): a `Mutex<bool>` + `Condvar`, any number of callers.

### 9. Concurrency
`SessionStore` is `Send + Sync` (everything inside is `Arc`/immutable). Several threads may `save`/`load` concurrently (the writer's save thread and the UI thread's fallback save): no lock is needed, the atomic write is the synchronisation. Never hold a lock across file I/O (R0003).

### 10. Test mapping hints (what the fakes offer)
`FakeFileSystem` has no hooks like the C# `InMemoryFileSystem`; map them: `WriteHook` -> `fs.fail(FsOp::WriteAtomic, session_path, kind)` (path from `store.get_path`), `OpenReadHook` -> `fail(FsOp::ReadText, ..)`, `EnumerateFilesHook` -> `fail(FsOp::EnumerateFiles, sessions_dir, ..)`, `DeleteHook` per file -> `fail(FsOp::Delete, file, ..)`. Blocking hooks (the gate in `SaveAndLoad_WhileStartupSweepStillRunning_AreUnaffected`) need a test-local decorator `GateFs(Arc<FakeFileSystem>)` implementing `FileSystem` whose `get_file_stat` blocks on a channel until the test releases it; run `save`/`load` on a helper thread and receive their result with `recv_timeout(10 s)` so a regression fails instead of hanging. `FakeFileSystem::read_all_text` rejects invalid UTF-8 (`InvalidData`), which `load` treats as "cannot read"; that is fine for the fake and does not occur in the C# fake tests.

## Golden package (to be installed by the lead; porters never edit `tests/golden/`)
- `oracle/W2-12.cs` -> `tools/oracle-cs/Commands/W2-12.cs`, dispatch lines in its header comment -> `tools/oracle-cs/Program.cs`.
- `golden-inputs/{session-key,session-json-write,session-json-read}.json` -> `tests/golden-inputs/`; then `tools/oracle.ps1 -Command session-key,session-json-write,session-json-read` writes `tests/golden/*.json` (reference outputs from the d92d6aae build are in `golden-reference/`, identical by construction).
- Case fields: see the doc comments in `W2-12.cs`. `session-json-write` ticks are STRINGS (JS-safe 18-digit integers): parse with `str::parse::<i64>()`.
- Inputs avoid anything machine-dependent (no relative paths, no 8.3 names, no `Local` kind output).

## Traps checklist
MULTI-AGENT-PLAN section 7 items 1 (never `to_lowercase`/`to_uppercase`), 2 (invariant text), 5 (ticks/kinds), 6 (JSON: case-sensitive names, duplicates, nulls, depth), 7 (Windows paths: the canonical form), 8 (`None` vs `""` for `CurrentPath`; null `Skipped`), 9 (each `catch` is its own arm: path errors vs read errors vs JSON errors), 10 (none: `skipped` is an ordered `Vec`). R0003: no lock across I/O; the sweep thread is a named thread, panics inside it must not poison anything.

## Depends on (OTHER units whose FUNCTIONS this unit's code or tests call)
- W1-14 (merged): `JournalDateTime::{to_json, parse_json, default, utc, unspecified}`, `DotNetJsonFormatter` (escaping).
- W1-13 (merged): `file_actions::errors::get_path_root`.
- Wave 0 (merged): `text::ordinal_upper`, `time::DateTimeUtc`, `app_paths::AppPaths`, `abstractions::{FileSystem, Log, NullLog}`.
- W2-14 (this wave): `io::physical::PhysicalFileSystem` (used by `from_environment` and by the native tests; the compile-time reference exists in the stub) and `io::physical::decode_text_like_dotnet` (used by the encoding golden cases). Run those tests after W2-14 is merged; until then the rest of this unit's tests need only the fakes.
- W2-17 (this wave, `ReviewMetrics`): must implement `SessionWriteCounter` (or the composition root wraps `|| metrics.record_session_write()`); no code dependency, tests use a closure.
- W2-13 uses this unit (`SessionStore::{save, canonical_folder}`); W2-13's tests also call `SessionStore::load`.

## Done when
All listed tests pass; clippy `-D warnings` and `cargo fmt` clean; every C# test has a Rust twin with `// C#: <Class>.<Method>` (or a stated reason in the report); every golden case of the three goldens is exercised; the stub's `#![allow(unused_variables, dead_code)]` line is removed; PARITY rows F-22, D-04, D-05 updated by the lead. Verifier mutations that must make a test fail: drop the `> ` (use `>=`) in the sweep age test; replace `ordinal_upper` by `to_uppercase`; drop the `U+017F` exception; write LF instead of CRLF; make member names case-insensitive; accept a `null` `UpdatedUtc`; take the first duplicate instead of the last; skip the 64-depth check; call `metrics` before the write; make `save` fsync (`durable = true`); forget the blank-folder fallback in `load`.

## Open questions
- Q1 (new crate): `sha2 = "0.10"` for `pr-core` (SHA-256 of the session key; also needed later for cache keys). No feature flags. Alternative: a 60-line hand-written SHA-256 with the three RFC 6234 test vectors; recommended: the crate.
- Q2 (divergence, README D5 style): null elements of `Skipped` are dropped; offset `UpdatedUtc` becomes UTC instead of `Local`; `C:\PROGRA~1` short names are not expanded by `canonical_folder`. Lead: confirm and add the `Different` PARITY note.
- Q3: `SessionState.skipped` as `Vec<String>` was chosen over `Vec<Option<String>>` because nothing downstream can use a null path; say so if you disagree.
- Q4: the C# default log is the process-wide `FileLog.Default`; here the default is `NullLog` and the composition root calls `with_log`. Confirm.

# W2-08: ActionDestinationPolicy (SEC-01 containment, destination validation, picked-folder check)
- Color: 🟡 security-sensitive (a wrong answer lets a Move/Copy leave the photo folder; fails CLOSED by design) | Porter: **sonnet** | Test porter: haiku |
  Verifier: **opus** (adversarial: junction/UNC/drive-root cases) instead of sonnet
- C# source (baseline d92d6aae): `src/PhotoReview.Core/FileActions/ActionDestinationPolicy.cs` (121 lines; includes the R32 drive-root fix and `PickedFolderCheck`)
  (read with `git -C D:\MyProject\PhotoReview show d92d6aae:src/PhotoReview.Core/FileActions/ActionDestinationPolicy.cs`)
- C# tests to port (all at d92d6aae, `tests/PhotoReview.Core.Tests/FileActions/`):
  - `ActionDestinationPolicyTests.cs` (97 lines): `Validate_ClassifiesDestinations` (theory, 21 rows; the `""` and `"  "` rows are `Some("")`/`Some("  ")`),
    `ValidateNoEscapeViaReparsePoint_JunctionEscapesFolder_Rejected`, `ValidateNoEscapeViaReparsePoint_UnresolvableLink_FailsClosed` (theory: `PermissionDenied` and a generic
    io error such as `ErrorKind::Other`; the result must be `Ok(EscapesSourceFolder)`, not an `Err`), `ValidateNoEscapeViaReparsePoint_JunctionStaysInsideFolder_Allowed`,
    `ValidateNoEscapeViaReparsePoint_OrdinaryDestination_Allowed`, `ValidateNoEscapeViaReparsePoint_DestinationFileIsSymlinkOutside_Rejected`
  - `ActionDestinationRootTests.cs` (79): `ValidateNoEscapeViaReparsePoint_SourceAtRoot_DestinationInsideIsOk` (4 rows), `ValidateNoEscapeViaReparsePoint_SourceAtRoot_DestinationOnAnotherRootEscapes` (3 rows)
    (NOT `ExecuteAsync_*` there: they test `FileActionService`, W3-04)
  - `FileActionSafetyEdgeTests.cs`: `Policy_PaddedParentSegment_Escapes` (theory, 3 rows)
  - `FileActionsSmallMutationGapTests.cs`: `Validate_NameStartingWithAnInvalidCharacter_IsRejected` (theory, 5 rows)
  - `PickedFolderPolicyTests.cs` (46): `ValidatePickedFolder_Classifies` (theory, 18 rows; the `null` row is `None`), `ValidatePickedFolder_ChecksExistenceOfTheNormalizedPath`
  - NOT here (other units, listed so nobody ports them twice): `FileActionServiceTests.ExecuteAsync_RelativeDestinationThroughJunctionEscapesFolder_FailsWithoutJournalOrMove` (W3-04),
    `tests/PhotoReview.Integration.Tests/PhysicalFileSystemReparseTests.cs` `ValidateNoEscapeViaReparsePoint_ReparseLoop_FailsClosed` and `..._OrdinaryDestinationBesideLoop_StaysOk` (native, need
    `PhysicalFileSystem` of W2-14: add them to W2-14's native tests).
  - Golden test (new, required): `tests/golden/destination-policy.json` (142 cases: 88 `validate`, 28 `picked`, 26 `no_escape`).
- PARITY rows: F-05 (destination policy), C-22 (Move to... / Copy to... picked folder check). Security: SEC-01, RV-S06, R32 (journal/invariants docs).
- Owned files (the ONLY files you may create/modify):
    crates/pr-core/src/file_actions/destination_policy.rs   (stub: contracts-w2/W2-08/destination_policy.rs, compiles clippy-clean)
    crates/pr-core/tests/file_actions_destination_policy.rs
- Read-only inputs: `tests/golden/destination-policy.json`. Lead-owned: `file_actions/mod.rs` gets `pub mod destination_policy;`.
- Public API (stub, do NOT change signatures): enums `ActionDestinationCheck`, `PickedFolderCheck`; `validate(Option<&str>)`;
  `validate_no_escape_via_reparse_point(&str, &str, &dyn FileSystem) -> Result<ActionDestinationCheck, ArgumentError>`;
  `validate_picked_folder(Option<&str>, &str, &dyn Fn(&str) -> bool) -> Result<PickedFolderCheck, PathError>`.
- Semantics notes (every item is a golden case):
  1. **`validate` order matters** (the stub doc lists the 6 steps): blank => `Empty`; invalid characters (checked on the text WITHOUT a leading `\\?\`) => `InvalidChars`; then
     `!fully_qualified && contains ':'` => `InvalidChars`; then `rooted && !fully_qualified` => `InvalidChars`; then `rooted` => `Ok`; then any `..` segment => `Escapes`.
     Consequences the golden pins: `D:\Backup\..\x` is `Ok` (absolute destinations are never checked for `..`), `\\?\a\..\b` is `Ok`, `E:\a:b` is `Ok` (fully qualified, so the colon rule is skipped),
     `\\` is `Ok`, `C:` / `C:x` / `a:b` / `1:\a` / `::` are `InvalidChars`, `\photos` and `/` are `InvalidChars`, `..x`, `x..`, `...`, `a\...\b` are `Ok`, `a\..\b` and `a/..\b` escape,
     segments are `trim()`med with Unicode whitespace (`".. \x"`, `"\u{3000}.."`, `"a\\\u{a0}..\\b"` escape) but a TAB is an invalid character (`"a\\..\t\\b"` => `InvalidChars`, rule 2 runs first).
  2. **Invalid characters** are exactly `|`, U+0000..U+001F, `*`, `?`. `"`, `<`, `>` and U+007F are VALID (verified .NET 10; do not "improve" this). `\\?\` is stripped only when it is the exact
     case-sensitive prefix: `\\?\D:\Back*`, `\\?\a?b`, `\\??\x`, `\??\C:\a`, `\\?\\?\x` are `InvalidChars`, `\\?\D:\Backup` and `\\?\` are `Ok`.
  3. **Whitespace-only** inputs (`"\t"`, NBSP, U+3000, `" "`) are `Empty` (`char::is_whitespace` equals .NET `IsNullOrWhiteSpace` for these).
  4. **SEC-01** (`validate_no_escape_via_reparse_point`): resolve BOTH paths with `file_system.resolve_real_path`, `trim_ending_directory_separator` both, equal (OrdinalIgnoreCase) => `Ok`;
     else prefix = real source (+ `\` unless it already ends with a separator: drive root `C:\`, R32), `real_destination` must start with the prefix (OrdinalIgnoreCase, compare with
     `ignore_case_key` of both). ANY `io::Error` while resolving => `EscapesSourceFolder` (fail closed). `C:\photos2\x`, `C:\photos.bak`, `\\server\share2\sel`, `\\other\share\sel`, `D:\sel` are not inside.
     Both sides are resolved: a LINK AS THE SOURCE FOLDER (`C:\photos` -> `D:\real\photos`) makes `C:\photos\sub` inside and `C:\other` outside (golden `source-itself-a-link-...`).
     Blank arguments are `Err(ArgumentError::BlankPath{..})` (C# `ArgumentException`), NOT `Escapes`.
  5. **`validate_picked_folder`**: the existence callback receives the NORMALIZED text (`get_full_path` then trailing separator trimmed except a root): `E:\Backup\` is asked as `E:\Backup`, `E:/Backup` as `E:\Backup`,
     `E:\x\..\Backup` as `E:\Backup`, `E:\\` as `E:\`, `\\?\E:\Backup` as `\\?\E:\Backup` (extended paths are not resolved => `Missing` in the golden because the set lacks that spelling),
     `\\server\share\x\` as `\\server\share\x`; the callback is NOT called for `Empty`, `NotAbsolute`, `SameAsPhotoFolder`. The set test in C# is case-insensitive (`e:\backup` finds `E:\Backup`): the test's
     closure must use `ignore_case_key`.
- Traps checklist: §7 items 1, 7 (drive-relative, rooted-without-drive, `\\?\`, UNC roots: the W2-00 helpers carry the .NET rules, do not re-derive them), 8 (`None` destination vs `Some("")`),
  9 (catch arms: here every `io::Error` is one arm, fail closed).
- Depends on (functions called): W2-00 `dotnet_path::{is_path_rooted, is_path_fully_qualified, is_invalid_path_char, trim_ending_directory_separator, ends_in_directory_separator, get_full_path}` (MUST be
  merged first); `FileSystem::resolve_real_path`; W1-07 `types::ArgumentError`; `text::{eq_ignore_case, ignore_case_key}`. Tests: pr-test-support `golden`. No dependency on other W2 units.
- Test fake for reparse points (the shared `FakeFileSystem` has none; do NOT edit pr-test-support): in the test file define
  ```rust
  struct ReparseFs { links: HashMap<String, String> /* ignore_case_key(trimmed full path of link) -> target */, fail_prefix: Option<(String, io::ErrorKind)> }
  impl FileSystem for ReparseFs { fn resolve_real_path(&self, path: &Path) -> io::Result<PathBuf> { /* below */ } /* the other 13 required methods: unimplemented!() */ }
  ```
  The 13 required methods are `file_exists, directory_exists, get_file_stat, move_file, copy_file, delete, open_read_shared, open_append, write_all_text_atomic, read_all_text, enumerate_files,
  enumerate_directories, create_directory`. `resolve_real_path` (same walk as the C# test fake, which the golden oracle also uses): if `fail_prefix` matches `path` (starts_with, ignore case) return that
  error; `full = get_full_path(path)`; `root = get_path_root(full)`; no root => `full`; segments of `full[root.len()..]` split on `\`/`/` without empty parts; none => return `root` (WITH its
  separator); `current = root` trimmed of trailing separators; for each segment `candidate = current + "\" + segment` (or just the segment when `current` is empty) and
  `current = links.get(key(candidate)).unwrap_or(candidate)`; return `current`.
- Done when: every listed test has a Rust twin; the golden passes: `validate` (compare the enum name), `picked` (result AND the `asked` list, closure built from `existing` with `ignore_case_key`), `no_escape`
  (a `{"throws":"ArgumentException"}` expectation <=> `Err(ArgumentError::BlankPath{..})`, otherwise the enum name; build `ReparseFs` from `reparse`, `throw_prefix`, `throw_kind` (`io` => `ErrorKind::Other`,
  `unauthorized` => `PermissionDenied`)); clippy clean; stub's `#![allow]` line removed.

## Golden package
- `oracle/W2-08.cs`: command `destination-policy`. The C# `IFileSystem` is faked with `System.Reflection.DispatchProxy` (only `ResolveRealPath` works; the rest throws), simulating reparse points exactly like the
  test `InMemoryFileSystem`. Needs `W2-00.cs`. Dispatch line in the header.
- `golden-inputs/destination-policy.json`, `golden-reference/destination-policy.json`.
- Case design: each rejection reason alone (blank, each invalid character class, drive-relative, rooted-without-drive, `..` variants with padding/Unicode spaces), each acceptance quirk (absolute with `..`,
  `\\?\`, `\\.\`, UNC, `E:\a:b`), SEC-01: junction out / in / source-as-link / file symlink / loop / unauthorized / unresolvable source / roots / sibling-prefix names / `..` resolved by the resolver.

## Open questions
- `validate_picked_folder` returns `Result<_, PathError>` (C# lets `GetFullPath`'s exception escape for a path the OS cannot handle, e.g. > 32 767 chars). Alternative: map `Err` to `NotAbsolute` (never crashes the
  Move-to dialog). The stub follows C# literally; the caller (W8 FileActionController) decides. Same question for `validate_no_escape...` blank arguments (`Err(ArgumentError)`).

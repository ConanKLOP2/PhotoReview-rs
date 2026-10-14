# W2-14: PhysicalFileSystem + CountingFileSystem (real-disk `FileSystem`, stat-counting decorator)
- Color: 🔴 | Porter: **opus** for `physical.rs` (Win32 sharing/atomicity semantics that the file-action safety story rests on; may hand the three
  pure helpers `matches_win32_expression`, `decode_text_like_dotnet`, `new_guid_n` and all of `counting.rs` to a sonnet sub-agent) |
  Test porter: sonnet (native tests with junctions, held handles, 145 ops cases; the counting tests alone are haiku-grade) | Verifier: sonnet
  (must run the `#[ignore = "native"]` tests with `-- --ignored` on Windows and report the output)
- C# sources (baseline d92d6aae), under `src/PhotoReview.Core/IO/`:
  - `PhysicalFileSystem.cs` (356 lines)
  - `CountingFileSystem.cs` (40 lines)
  (read with `git -C C:\MyProjects\PhotoReview show d92d6aae:src/PhotoReview.Core/IO/<file>`)
  - NOT here: `Abstractions/PhysicalSourceReader.cs` (already ported in W1, `abstractions::source_reader`), `SlowLinkFileSystem` (Benchmarking, W6+),
    `FileSystemErrors.IsDestinationExists` (W1-13, `file_actions::errors::is_destination_exists`).
- C# tests to port (all at d92d6aae):
  - `tests/PhotoReview.Core.Tests/IO/CountingFileSystemTests.cs` (102 lines, 5 tests): `StatQueriesAreCounted`, `OtherOperationsAreNotCounted`,
    `ForwardedOperations_ReachTheInnerFileSystem_WithoutCounting`, `EnumerateDirectories_IsForwarded`, `TryCopyNewWithProof_IsForwardedToTheInnerOverload`
    (`InMemoryFileSystem` => `FakeFileSystem`; `ReviewMetrics` => a closure/`AtomicU64` `StatCounter`, W2-17 owns the real `ReviewMetrics`).
  - `tests/PhotoReview.Integration.Tests/FileSystemWrapperForwardingTests.cs` `EveryFileSystemDecorator_ForwardsTheCreateNewPrimitivesToTheInnerFileSystem`:
    Rust has no reflection; twin = a test-local `RecordingFs` that overrides EVERY `FileSystem` method (also the defaulted ones) and logs the method name; call
    every `CountingFileSystem` method once and assert the inner log is exactly `[<same method name>]` (so a method left on a trait default is caught:
    `try_copy_new` left default would log `try_copy_new_with_proof`). Add a comment that new trait methods must be added to the recorder.
    (C# scope is narrower: only the `Try*New` primitives, 2 overloads, and its discovery also finds `SlowLinkFileSystem`, a W6+ unit; the
    all-methods recorder is a deliberate Rust strengthening. Pass a fresh `CopyCreationProof` to `try_copy_new_with_proof` and check that the
    SAME proof object reaches the inner recorder.)
  - `tests/PhotoReview.Core.Tests/IO/ReplaceWithRetryTests.cs` (49 lines): `SharingViolation_RetriedThenSucceeds` (sleeps `[3, 6]`, 3 calls),
    `AccessDenied_RetriedThenRethrown` (8 calls, 7 sleeps), `NonTransient_SurfacesImmediately` (3 rows: `NotFound` raw 3, "path too long" raw 206, plain
    `io::ErrorKind::Other`; 1 call, 0 sleeps).
  - `PhysicalFileSystemListingInterruptedTests.cs` (98 lines): the 4 tests through `enumerate_files_with_stat_from` with a closure listing (an
    `io::Error::new(Other, "disk went away")` item after 2 entries; `reason == "disk went away"`, `path == directory`, kind `ListingInterrupted`; error before the
    first entry propagates and reports nothing; include filter applies before the interruption). The theory row `UnauthorizedAccessException` becomes
    `PermissionDenied`. `NonIoFailureMidListing_Propagates` has no Rust twin (every `io::Error` is an I/O error): note it in a comment.
    The C# tests also assert the injected enumerator was `Disposed` (also on the failing paths): the Rust twin uses a test iterator with a
    `Drop` flag and asserts it was dropped once `enumerate_files_with_stat_from` returned. Entries are real files of length 3 made with
    `TempRoot`; paths are compared with `ListedFile.full_name`.
  - `PhysicalFileSystemTryCopyNewTests.cs` (131 lines): `TryCopyNew_DestinationIsADirectory_ThrowsInsteadOfReportingFalse`,
    `TryCopyNew_ExistingFileAtDestination_ReturnsFalseAndKeepsItUntouched`, `TryCopyNew_FailsForADirectoryDestination_LeavesNoFileBehind` (real `TempRoot`,
    `#[ignore = "native"]`), `IsSwallowedAsDestinationExists_*` (rows below, pure, NOT ignored). `IsDestinationExists_*` rows belong to W1-13.
    `TryCopyNew_DestinationFolderDeniesCreatingFiles_...` (changes a real ACL via `icacls`/System.Security) is NOT ported: it needs a Deny ACE and a Win32
    ACL API; keep a `// C#: ... not ported (ACL)` comment and rely on golden-less reasoning (raw OS error 5 is returned as is).
  - `PhysicalFileSystemAtomicWriteTests.cs` (26 lines): `WriteAllTextAtomic_RenameFails_LeavesNoTempFile` (target is a directory: `Err`, no `*.tmp`
    left, the directory is still there).
  - `tests/PhotoReview.Core.Tests/Session/SessionWritePolicyTests.cs` `PhysicalNonDurableWrite_IsAtomicRoundTripAndLeavesNoTempFiles` (two
    non-durable writes through a missing `sub` folder, `"second \u00e9"` read back, the folder holds exactly the target) and
    `PhysicalNonDurableWrite_FailedRename_CleansUpTempFile` (directory as target: `PermissionDenied`, the folder holds no file). W2-12's contract
    hands these two to this unit; `Measure_SessionSaveLatency_*` is Manual (skip, say so).
  - `tests/PhotoReview.Core.Tests/IO/FileSystemContractTests.cs` (13 theories over {PhysicalFileSystem, InMemoryFileSystem}; no Rust twin exists
    yet in W0/W1): one `fn` body per C# theory taking `&dyn FileSystem`, run for `PhysicalFileSystem` (real `TempRoot`, `#[ignore = "native"]`) and
    for `FakeFileSystem` (not ignored). A failing `FakeFileSystem` row is reported to the lead: `pr-test-support` is not owned by this unit.
    Rows: file_exists (also upper-cased path), directory_exists (also upper-cased), try_delete_empty_directory, get_file_stat (None, then length
    == UTF-8 byte count and last write within 5 minutes), move, move of a missing source (`NotFound`), copy, delete (idempotent), open_read_shared
    (payload written through `open_append`), open_append sequentially, write_all_text_atomic + read_all_text (nested folders, Vietnamese text),
    enumerate_files (`*`, `*.jpg`, top directory only), enumerate_directories, try_probe_readable (Ok / Err with non-blank reason).
  - `tests/PhotoReview.Core.Tests/IO/FileSystemCollisionContractTests.cs` (7 theories over the same two systems + 4 physical-only facts):
    `Move_DestinationExists_ThrowsAndKeepsBoth`, `Move_DestinationDiffersByCaseOnly_IsACollision`, `Copy_DestinationExists_ThrowsAndKeepsBoth`,
    `TryCopyNew_DestinationExists_ReturnsFalseAndKeepsBoth`, `TryCopyNew_DestinationFree_CopiesAndReturnsTrue`,
    `Move_MissingDestinationFolder_KeepsSource` (`NotFound` raw 3), `WriteAllTextAtomic_Overwrite_ReplacesWholeContent`; physical facts:
    `LongPath_FullLifecycle` (path over 300 chars: create_directory, atomic write, copy, move, stat, delete; .NET adds the `\\?\` prefix itself.
    `std::fs` also does it internally, but the raw `MoveFileExW`/`CopyFileExW` calls of notes 3-4 do NOT: the private FFI wrapper must convert a
    path of 260+ UTF-16 units to the verbatim form (`\\?\` + `get_full_path`, `\\?\UNC\` for UNC) before the call, otherwise this test fails
    with raw 3/206), `Copy_DestinationLocked_FailsAndKeepsSource` (destination held with share None: raw 80 because existence is checked first, source
    untouched), `Move_ReadOnlySource_SameVolume_Works` (read-only flag kept; reset the attribute in cleanup), `WriteAllTextAtomic_ConcurrentWriters_
    LeaveACompleteFile` (4 threads x 25 non-durable writes of 20 000 chars to one target: no failure surfaces because the rename is retried, final
    content equals one of the texts, no `*.tmp` left; 30 s join timeout).
  - `tests/PhotoReview.Core.Tests/IO/PhysicalIoJunctionNativeTests.cs` (5 native facts: junction inside the path, junction as final segment,
    chain of two junctions, dangling junction, ordinary folder/file beside a junction). The `resolve` cases of golden `physical-fs-ops` pin the
    same facts; port the 5 facts anyway as `#[ignore = "native"]` (junction by `cmd /c mklink /J`, links removed with `remove_dir` in a guard,
    paths compared case-insensitively).
  - `tests/PhotoReview.Core.Tests/IO/PhysicalIoMutationGapTests.cs` (only the `PhysicalFileSystem`/`ResolveRealPath` facts; the two
    `PhysicalJournalCompactionFiles` facts belong to W3-02): `TryCopyNew_SourceMissing_ThrowsInsteadOfReportingAnExistingDestination` (`NotFound`
    raw 2, destination not created), `TryCopyNew_DestinationFolderMissing_Throws` (raw 3), `TryCopyNew_DestinationIsADirectory_ThrowsInsteadOfReturningFalse`,
    `TryCopyNew_DestinationFileExists_ReturnsFalseAndKeepsItsContent`, `TryProbeReadable_FileAlsoOpenForWriting_IsReadable...` (writer shares
    ReadWrite|Delete), `TryProbeReadable_FileLockedAgainstReaders_ReportsTheOsMessage`, `EnumerateReadableFilesWithStat_OneFileLocked_IsSkippedAndReported...`
    (trait default: good file listed with length 2, locked one skipped as `FileUnreadable`, same path, non-empty reason),
    `EnumerateFilesWithStat_FolderDoesNotExist_Propagates` (raw 3, nothing reported), `ResolveRealPath_PlainPath_IsReturnedUnchanged`,
    `ResolveRealPath_BareRoot_KeepsTheTrailingSeparator` (F02: `resolve_real_path` of `C:\` is `C:\`, still ends with a separator).
  - `tests/PhotoReview.Core.Tests/IO/SkippedKindTests.cs` (2 facts): a file held with share None is `FileUnreadable` with a non-blank reason through
    `enumerate_readable_files_with_stat`; the fast `enumerate_files_with_stat` lists it with no skip (AR16, no per-file open) while
    `try_probe_readable` reports it.
  - `tests/PhotoReview.Integration.Tests/PhysicalFileSystemReparseTests.cs` (native): the two RV-S06 tests call
    `ActionDestinationPolicy.ValidateNoEscapeViaReparsePoint` (W2-08). W2-08's contract hands them to this unit. Port them as `#[ignore = "native"]` once
    `file_actions::destination_policy` is merged; before that port the lower-level fact they rest on: a junction loop `a -> b -> a` makes
    `resolve_real_path(a\processed\photo.jpg)` return `Err` (no panic, no hang, no stack overflow), and a path beside the loop resolves unchanged.
  - Golden tests (new, required): `tests/golden/physical-fs-ops.json` (145 cases, real folder per case), `physical-fs-pattern.json` (56 patterns over 23 names +
    2 dirs), `physical-fs-read-text.json` (38 byte sequences), `physical-fs-resolve.json` (34 paths that exist nowhere). The two directory-heavy ones
    (`ops`, `pattern`) are `#[ignore = "native"]`; `read-text` runs `decode_text_like_dotnet` purely (not ignored) and ALSO `read_all_text` over a file in
    the ignored twin; `resolve` is pure path math and is NOT ignored on Windows (cfg(windows) only: the roots are Windows roots).
- PARITY rows: D-05 (atomic temp naming `<target>.<guidN>.tmp`, Settings fsynced, session not), F-05/SEC-01 (resolve_real_path under containment), INV-8
  (share ReadWrite|Delete + SequentialScan, others can rename/delete an open image), ADR 0007 section 2-3 (durable flag, no silent IgnoreInaccessible,
  ListingInterrupted), AR16 (stat from the directory entry, no per-file open), R01 (ownership proof only after the copy returned), RV-C05 (no temp left),
  RV-S06 (reparse loop fails closed in the caller).
- Owned files (the ONLY files you may create/modify):
    crates/pr-core/src/io/physical.rs        (stub: contracts-w2/W2-14/physical.rs, compiles clippy-clean)
    crates/pr-core/src/io/counting.rs        (stub: contracts-w2/W2-14/counting.rs)
    crates/pr-core/tests/io_physical.rs
    crates/pr-core/tests/io_counting.rs
- Lead-owned: `crates/pr-core/src/io/mod.rs` (`pub mod counting; pub mod physical;`) and `pub mod io;` in `lib.rs` (W2-12 already refers to
  `crate::io::physical::{PhysicalFileSystem, decode_text_like_dotnet}`). Read-only inputs: the four goldens. The reference files echo the whole input case
  (name, setup, op, args), so a test can run from the reference file alone.
- Public API (stub, do NOT change signatures): `PhysicalFileSystem` (unit struct, `impl FileSystem`), `ListedFile`, `OpenListing`,
  `PhysicalFileSystem::enumerate_files_with_stat_from`, `REPLACE_ATTEMPTS`, `replace_with_retry`, `is_swallowed_as_destination_exists`,
  `matches_win32_expression`, `decode_text_like_dotnet`, `new_guid_n`; `StatCounter` (+ blanket impl for `Fn()`), `CountingFileSystem::new`.
  The porter may add private helpers and a private `translate_win32_expression`; `enumerate_readable_files_with_stat` is intentionally not overridden in
  `PhysicalFileSystem` (the trait default is line-for-line the C# method: list, include, probe, skip as `FileUnreadable` with the probe reason).
- Semantics notes (every line below is pinned by a golden case or a C# test unless marked UNPINNED):
  1. **Error shape.** Errors are `io::Error`; tests compare `raw_os_error()` with the golden `win32` (the HResult low word) and the kind with the .NET class:
     `FileNotFoundException` = `NotFound` raw 2, `DirectoryNotFoundException` = `NotFound` raw 3, `UnauthorizedAccessException` = `PermissionDenied` raw 5,
     `IOException` = the raw code (32 sharing, 80 file exists, 183 already exists). Some .NET errors are synthesized by .NET before any OS call: build them with
     `io::Error::from_raw_os_error(2)` (see 3, 5). Blank/whitespace path (`ArgumentException`) = `io::ErrorKind::InvalidInput`; for the methods that return
     `bool`/`Option` (`file_exists`, `directory_exists`, `get_file_stat`, `try_delete_empty_directory`) return `false`/`None` instead of throwing
     (record `// DIVERGENCE:`; `try_probe_readable` returns `Err(reason)`). The reason of a real failure is `err.to_string()`, which in Rust
     ends with `(os error N)` and is NOT the .NET message: goldens and C# tests only require "present / non-blank", never the text.
  2. **Never use `std::fs` defaults blindly.** std opens everything with share ReadWrite|Delete and `fs::rename`/`fs::copy` OVERWRITE; every item below that
     says "share X" must be an explicit `OpenOptionsExt::share_mode(..)` (FILE_SHARE_READ 1, WRITE 2, DELETE 4) plus `custom_flags` for
     SEQUENTIAL_SCAN 0x08000000 / WRITE_THROUGH 0x80000000.
  3. **`move_file`** = C# `File.Move` (no overwrite): (a) source must be a FILE (`File.Exists`): a missing source AND a directory source give raw 2 (golden
     "a directory as source is file not found"); (b) destination existing as file, as directory, or differing only by case gives raw 183 and BOTH are kept;
     (c) missing destination folder raw 3; (d) source held without share raw 32; (e) same path is `Ok` and a no-op, same path with other case renames.
     Needs `MoveFileExW(src, dst, 0)` (flags 0 = no REPLACE_EXISTING; `std::fs::rename` is wrong: it passes REPLACE_EXISTING).
  4. **`copy_file`** = `File.Copy(src, dst)` = `CopyFileExW(.., COPY_FILE_FAIL_IF_EXISTS)`: existing destination (file OR directory, even when held open)
     raw 80 and the destination is untouched; missing source raw 2 (checked even when the destination exists); missing destination folder raw 3; directory
     source raw 5; source held without share raw 32, with share Read it copies; the copy keeps last-write time and the read-only flag (golden `copy-attrs`).
     `std::fs::copy` overwrites: do not use it. (`copy_file` of a read-only/other-case case is pinned by `copy-attrs` and the other-case rows.)
  5. **`try_copy_new_with_proof`**: `copy_file`, then on `Ok` call `proof.mark_created()` and return `Ok(true)`; on `Err(e)` return `Ok(false)` only when
     `is_swallowed_as_destination_exists(&e)` AND `!self.directory_exists(destination)`, otherwise `Err(e)`. The proof is raised ONLY after the copy
     returned (R01): a failed copy never marks it. `try_copy_new` is the trait default (fresh proof). Goldens: existing file / other-case name / destination held
     without share = `Ok(false)`; destination directory = `Err` raw 80 and no file created; missing source with an existing destination = `Err` raw 2, not
     `Ok(false)`.
  6. **`is_swallowed_as_destination_exists`** = `file_actions::errors::is_destination_exists`. Test rows (build from raw OS codes, not HResults):
     `from_raw_os_error(80)` true, `(183)` true, `ErrorKind::AlreadyExists` with no code true (Rust-only, fakes), false for `PermissionDenied`, `NotFound` raw 3,
     raw 32, raw 112, `Other` with no code. The C# theory also has HResult rows (`0x80070050`, `0x800700B7`, `0x00070050`, `0x000700B7` true, and
     false rows such as low word 80 without the Win32 facility): an `io::Error` carries no HResult, so the HResult rows are covered by W1-13's
     `file_actions::errors::hresult_is_destination_exists` (already ported, do not duplicate), not by this function. The row "InvalidOperationException
     with a matching HResult" has no twin (not an I/O error).
  7. **`delete`** = `File.Delete`: missing file in an existing folder is `Ok`; missing folder raw 3; a directory raw 5; read-only file raw 5 and kept; held
     without share (or with share Read only) raw 32 and kept. Verify `std::fs::remove_file` against these cases (std uses POSIX delete semantics where
     available and may behave differently on read-only files and held files); if any golden disagrees use `DeleteFileW` through the same private
     `extern "system"` module as 3-4.
  8. **`file_exists` / `directory_exists`** (`File.Exists` / `Directory.Exists`): attribute test on the entry ITSELF, not following links. `file_exists` =
     entry exists and is not a directory; a name with a trailing separator is false; `*`/`?` in the name is just an invalid name, false; case-insensitive;
     held without share still true. `directory_exists` = entry exists and has DIRECTORY: junction to a directory true, DANGLING junction true (golden), a
     file false, trailing separator true. `Path::is_file()/is_dir()` follow links and are wrong for the dangling junction: use
     `symlink_metadata(..).file_attributes()`.
  9. **`get_file_stat`** (`new FileInfo(path)` then `Exists`): `Some(FileStat { length, last_write_utc })` for a file, `None` for a directory or missing
     (also missing parent). `FileInfo` TRIMS a trailing separator (golden: `a.txt\` still has a stat, unlike `File.Exists`). Ticks come from the
     FILETIME of the last write (`DateTimeUtc::from_filetime(MetadataExt::last_write_time())`, 100 ns precision kept, Kind UTC); length `u64`. A file held
     without share still stats (metadata only, no open). UNPINNED: file symlinks.
  10. **`open_read_shared`**: share ReadWrite|Delete, SequentialScan; `Err` raw 2 missing, raw 32 when another handle denies Read sharing; readable while
      another handle has it open for writing with full share; another process can rename AND delete the file while the stream is open and the stream keeps
      reading (golden `openread-then-rename`, INV-8). Honour `buffer_size`: `BufReader::with_capacity(buffer_size, file)` when `buffer_size > 1`, else the
      bare `File` (`Box<dyn ReadSeek>` needs `Read + Seek + Send`; `BufReader<File>` qualifies).
  11. **`open_append`** = `FileMode.Append, FileAccess.Write, FileShare.Read`: create or open, always write at the end, share READ ONLY: a second appender,
      a delete, or a reader that does not share Write while it is open all fail raw 32 (golden); a reader with share ReadWrite|Delete succeeds;
      `durable` adds WRITE_THROUGH; missing folder raw 3; appending across two sequential handles gives `"Line 1\nLine 2\n"`. Return the raw `File`
      (unbuffered: every `write` reaches the OS, so errors surface at the call and nothing is lost by a dropped `BufWriter`).
      `// DIVERGENCE:` the C# `FileStream` has a 4096-byte buffer (data and some errors surface at `Flush`/`Dispose`); callers write whole
      lines, so the observable file content is the same.
  12. **`write_all_text_atomic`**: `dir = get_directory_name_raw(path)`; non-empty dir => `create_dir_all` first (error propagates: path under a file is raw
      183); temp = `path + "." + new_guid_n() + ".tmp"` (no extension replacement); create with `create_new`, share NONE, WRITE_THROUGH when
      `durable`; write the UTF-8 bytes WITHOUT BOM (CRLF/NUL/astral untouched); `sync_all()` when `durable`; close; then
      `replace_with_retry(|| fs::rename(temp, path), |ms| thread::sleep(Duration::from_millis(u64::from(ms))))` (`fs::rename` IS the right call here: `File.Move(overwrite: true)` is
      REPLACE_EXISTING). In every exit path (success, failure, panic-free early return) delete the temp if it still exists, ignoring the delete error, and
      return the ORIGINAL failure. Goldens: replaces the whole old content, empty text gives an empty file, read-only target raw 5 after 8 attempts,
      non-empty directory target raw 5, target held by a reader without Delete share raw 5, never a leftover `*.tmp`.
  13. **`replace_with_retry(move_op, sleep)`**: attempt `n` in `1..=8`; on `Err(e)` with `n < 8 && is_transient(e)` call `sleep(n * 3)` and loop; otherwise
      return `Err(e)` at once. Transient = `raw_os_error()` in `{32, 33}` OR (`ErrorKind::PermissionDenied` with raw code `5` or no raw code: .NET maps only
      ERROR_ACCESS_DENIED to `UnauthorizedAccessException`, while std also maps e.g. 1314 PRIVILEGE_NOT_HELD to `PermissionDenied`, which .NET
      surfaces as a plain `IOException` and does not retry). Total worst case 3+6+...+21 =
      84 ms. `sleep` is not called after the last attempt.
  14. **`read_all_text`** = `File.ReadAllText`: opens with share READ ONLY (std default would wrongly succeed): raw 32 while ANY other handle has the
      file open for writing even with full share, raw 32 when held without share, `Ok` when the other handle only reads with share Read; raw 2 missing.
      Decoding is `decode_text_like_dotnet(&bytes)` (below).
  15. **`decode_text_like_dotnet`**: detection on the leading bytes in this order: `EF BB BF` UTF-8 (drop), `FF FE 00 00` UTF-32 LE (checked BEFORE UTF-16 LE),
      `FF FE` UTF-16 LE, `FE FF` UTF-16 BE, `00 00 FE FF` UTF-32 BE; each BOM is dropped (a second BOM stays: `EFBBBFEFBBBF61` gives `[0xFEFF, 'a']`); no
      BOM = UTF-8 (UTF-16 text without BOM keeps its NULs). Invalid input never errors: UTF-8 uses maximal-subpart replacement (`String::from_utf8_lossy`
      matches all 38 golden rows including `C0AF` = 2 U+FFFD, `EDA080` = 3, `F4908080` = 4, `F888808080` = 5, `F09F9861` = U+FFFD + `a`); UTF-16
      (`String::from_utf16_lossy` over the code units): a lone surrogate or an odd trailing byte is ONE U+FFFD; UTF-32: a code point above 0x10FFFF or in
      the surrogate range, or a 1-3 byte tail, is ONE U+FFFD, astral code points are valid. Golden `utf16_units` is compared as `encode_utf16()` of the result.
  16. **`enumerate_files(directory, pattern)`** = `Directory.EnumerateFiles(directory, pattern)` (legacy overload, `EnumerationOptions.Compatible`: Hidden and
      System files ARE included, unlike W2-05's sibling listing; subfolders are not searched). Returns files only (junction/dir symlinks to directories are
      directories, so excluded). Paths are `Path::new(directory).join(name)` with the directory exactly as spelled (`"<root>\.\a.txt"`, forward slashes
      kept, a trailing separator not doubled). Missing folder raw 3. Order = directory order (callers sort; do not sort). A pattern containing `/` or
      `\` is out of scope (UNPINNED: .NET throws `ArgumentException`/`DirectoryNotFoundException`/`IOException(123)` depending on the shape): return
      `Err(InvalidInput)` and say so in a comment. `""`, `"."`, `"*"`, `"*.*"` all mean "everything" (verified with .NET 10 for `"."`).
      `// DIVERGENCE:` C# returns a lazy `IEnumerable` (a missing folder throws at the first `MoveNext`); Rust returns a `Vec`, so the error comes
      from the call itself. Same for `enumerate_directories` (20); callers enumerate immediately.
  17. **Pattern matching** (`matches_win32_expression(expression, name)`, expression = the raw user pattern). Step 1, `TranslateWin32Expression`: empty,
      `*`, `*.*` => `*` (all). Otherwise left to right: `.` that is the last char and preceded by `*` => `"` (DOS_DOT); `.` followed by `?` or `*` => `"`;
      other `.` stays; every `?` => `>` (DOS_QM); `*` followed by `.` => `<` (DOS_STAR); anything else unchanged (a user-typed `<`, `>`, `"` is already a
      DOS wildcard). Step 2, match the whole name with an NFA/backtracking matcher, characters compared case-insensitively with
      `text::ordinal_upper` (`ä.jpg` matches `Ä.jpg`): `*` = any run incl. empty; `<` = zero or more characters, where a `.` may be consumed only when it
      is NOT the last `.` of the name (`*.jpg` matches `a.b.jpg` but not `x.jpg.tmp`); `"` = one `.`, or nothing when at the end of the name; `>` = one
      character that is not `.`, or nothing when at the end of the name or when the next character is `.` (`a?.jpg` matches `a.jpg`; `a?` and `?` match `a`;
      `?a` matches nothing); every other pattern char is a literal. Pinned by 56 goldens (e.g. `*.` = `a`, `noext`; `a.????` = `a`, `a.jp`, `a.jpeg`, `a.jpg`;
      `*.j"g` and `>pg` = nothing).
  18. **`enumerate_files_with_stat`**: `std::fs::read_dir` of `get_full_path(directory)` (full names are NORMALISED: trailing separator dropped, `/` becomes
      `\`; unlike `enumerate_files`), one `DirEntry::metadata()` per entry (symlink-aware, no open of the file: a file held without share is still listed
      with its stat), skip entries whose attributes have DIRECTORY, keep Hidden/System, apply `include(&full_name)` BEFORE building the stat. `Some(FileStat)`
      always for real entries. Failure rules are those of `enumerate_files_with_stat_from`: an `Err` after at least one ENTRY WAS READ (counted before
      `include`, as `listed` in C#) keeps the entries and reports one `SkippedEntry { path: directory (as given), reason: err.to_string(), kind:
      ListingInterrupted }`; an `Err` before the first entry propagates and reports nothing (the load must fail loudly); a missing folder is `Err` raw 3
      before any entry. The production `open_listing` wraps `read_dir` and the attribute test into `ListedFile` items.
  19. **`try_probe_readable`**: one open for read, share ReadWrite|Delete, no bytes read; `Err(err.to_string())` for ANY I/O failure (missing file/folder,
      directory (raw 5), locked raw 32); readable while open for writing elsewhere, while open for reading without write share, and for a read-only file.
      The reason must be non-empty (the C# fallback is `failure ?? ""`).
  20. **`enumerate_directories`** = `Directory.EnumerateDirectories(dir)`: immediate subfolders only, junctions and directory symlinks INCLUDED (so test
      the DIRECTORY attribute of `entry.metadata()`: `file_type().is_dir()` is false for a junction), Hidden/System included, paths = `directory.join(name)`
      as spelled, missing folder raw 3.
  21. **`create_directory`** = `Directory.CreateDirectory`: `create_dir_all` (nested creates parents; existing directory is `Ok`); a FILE in the way is raw
      183 and no directory appears; a path under a file is also raw 183 (std's recursion happens to give that; the golden decides).
  22. **`try_delete_empty_directory`**: `fs::remove_dir(path)` (non-recursive, atomic refusal of a non-empty directory); `true` on success, `false` for
      non-empty, missing, a file path, or any `io::Error`; a junction is removed as a link only and its target stays (golden); blank path `false`.
  23. **`resolve_real_path`** (SEC-01): `full = dotnet_path::get_full_path(path)?` (blank/NUL = `InvalidInput`; golden: `a\..\b`, `a\.\b`, doubled and
      forward slashes, trailing `.`/space, `C:\..` => `C:\`, UNC and `\\?\`/`\\.\` roots verbatim, `\\?\` paths are NOT normalised); `root =
      full[..root_length(full)]`, empty root => `full`; `segments = full[root.len()..]` split on `\` and `/` dropping empties; no segments => `root`
      (bare `C:\` and `\\?\C:\` stay: a root keeps its separator; `\\server\share\` already becomes `\\server\share` inside `get_full_path`,
      golden `resolve`, so the segment loop never sees it); `current =
      root.trim_end(['\\', '/'])`; for each segment `candidate = if current.is_empty() { segment } else { current + "\\" + segment }` and
      `current = resolve_if_reparse_point(candidate)`. `resolve_if_reparse_point`: not an existing file or directory (existence by the entry itself, so a
      dangling junction counts as existing) => unchanged; entry has REPARSE_POINT and a link target (symlink or junction; other reparse tags such as
      cloud placeholders have no `LinkTarget` and stay unchanged) => follow the chain to the FINAL target (relative targets resolve against the link's folder)
      and return its full name even when that target does not exist (golden `dangling junction still resolves to its missing target`). Use `fs::read_link`
      in a loop with a hop cap of 63 (.NET stops there); a loop (`a -> b -> a`) or the cap returns `Err(io::Error)` (RV-S06: the caller fails closed).
      `read_link` may return `\??\C:\x`: strip the `\??\` NT prefix. Case of the input is kept (`<root>\DIR\A.TXT`), only reparse segments are replaced.
  24. **`new_guid_n`**: 32 lowercase hex chars, never repeated within or across processes; no crate: mix pid, nanoseconds, a process-wide `AtomicU64`
      counter and `RandomState` hasher output (two 64-bit hashes). Test: 10 000 calls on 8 threads are distinct, every value matches `^[0-9a-f]{32}$`.
  25. **`CountingFileSystem`**: `file_exists`, `directory_exists`, `get_file_stat` call `metrics.record_stat()` FIRST (counted even if the inner call
      fails), then forward; every other method forwards untouched with the same arguments and is never counted. Every trait method is overridden, including
      the ones with default bodies (`try_copy_new`, `try_copy_new_with_proof`, `enumerate_files_with_stat`, `try_probe_readable`,
      `enumerate_readable_files_with_stat`, `try_delete_empty_directory`, `resolve_real_path`, ...), because a left default would replace the inner atomic
      create-new copy by an exists-check-then-copy. The proof passes through untouched (the inner copy raises it).
- Traps checklist (docs/MULTI-AGENT-PLAN.md section 7): 1 (name matching through `text::ordinal_upper`, never `to_lowercase`), 3 (`length` is `u64` from the
  OS, converted by callers to `i64`; ticks arithmetic is `checked`), 5 (FILETIME to `DateTimeUtc`, Kind UTC), 7 (`get_full_path` of W2-00, not
  `std::path::absolute`; `\\?\` roots), 9 (each `catch ... when` of C# = one branch: `IsSwallowed...` + `!DirectoryExists`, transient classification,
  interrupted-listing only after the first entry, `TryDeleteEmptyDirectory` catches IO/Unauthorized/Argument/NotSupported), 10 (listing order is the OS
  order; never go through a `HashMap`). Plus the unit-specific traps: std share-mode defaults (2), `rename`/`copy` overwrite (3-4), `is_dir` follows
  links (8, 20), `FileInfo` trims the trailing separator while `File.Exists` does not (8-9), `ReadAllText` shares READ only (14), `File.Move` of a
  directory is "file not found" (3), directory listing includes Hidden/System but W2-05's does not; raw FFI paths over 259 units need the `\\?\` form (std adds it only inside std);
  `resolve` golden paths such as `\\server\share\a\b` make `Directory.Exists`-style probes hit the network (can take seconds to time out): keep them,
  but do not run them on a thread with a short test timeout.
- Depends on: W2-00 `text::dotnet_path::{get_full_path, get_directory_name_raw, root_length}` (MUST be merged first; the pure `resolve` golden needs it),
  W1-13 `file_actions::errors::is_destination_exists`, Wave 0 `abstractions::{FileSystem, FileStat, SkippedEntry, SkippedKind, CopyCreationProof, ReadSeek}`,
  `time::DateTimeUtc`, `text::ordinal_upper`; test side: `pr-test-support::{TempRoot, FakeFileSystem, golden}`. No dependency on other W2 units for `counting.rs`;
  W2-17's stub has `ReviewMetrics::record_stat(&self)` but NO `impl StatCounter` (its contract does not mention it): until the lead
  adds one, callers pass a closure `Arc::new(move || metrics.record_stat())`; `pr_core::io` is a lead-created module, W2-15 may create it first; W2-12/W2-03/W2-11 and W2-08 (reparse tests) consume `PhysicalFileSystem` after this merge.
- Done when: every listed test has a Rust twin with `// C#: <Class>.<Method>`; the four goldens pass (`ops` and `pattern` with `-- --ignored` on Windows, the
  others in the normal run); the forwarding recorder test passes; clippy clean; every `unsafe` block has a `// SAFETY:` comment; the stub `#![allow(...)]`
  lines removed; no `todo!()` left; `cargo test -p pr-core` leaves no `pr-rs-tests` folder or junction behind (junction links are removed with
  `remove_dir` before the `TempRoot` drops, as `PhysFsCleanup` does).

## Golden package
- `oracle/W2-14.cs`: commands `physical-fs-ops` (scenario interpreter over a private real temp folder: setup items file/fileHex/dir/junction(`cmd /c mklink
  /J`)/hold(FileStream with access+share)/holdAppend/deleteDir, then one op; output `ok`+`value` or `exception` class + `win32`, plus `after` probes),
  `physical-fs-pattern`, `physical-fs-read-text`, `physical-fs-resolve`. Dispatch lines are in the file header. Needs nothing from W2-00.cs.
- `golden-inputs/{physical-fs-ops,physical-fs-pattern,physical-fs-read-text,physical-fs-resolve}.json`, outputs in `golden-reference/`.
- The Rust interpreter must replay `setup` itself: `file` (UTF-8 no BOM, default text `"x"`, optional `lastWriteUtc` via `File::set_modified`, `readonly`
  via `Permissions::set_readonly`), `fileHex`, `dir`, `junction` (spawn `cmd /c mklink /J`, same as the oracle), `hold` (an `OpenOptions` with the given access
  and `share_mode`: none=0, read=1, readwrite=3, else 7), `holdAppend` (the port's own `open_append`), `deleteDir`; args are relative to the case folder
  unless they contain `{root}`/`{rootfwd}` (substituted) or start with `@` (literal after the `@`); masks `<root>`/`<rootfwd>` in string results.
- Case design: every operation x {success, missing file, missing folder, directory in place of a file, existing destination, other-case name, read-only,
  held with each share mode, junction/dangling junction}; DOS-wildcard patterns incl. translation corner cases; 38 encoding/BOM/invalid-byte sequences;
  34 path shapes (dot segments, trailing dot/space, UNC, `\\?\`, long names, blank, NUL).

## Problems found in the existing oracle/golden files (not rewritten)
- Non-reproducible values: `value.write_ticks` of `stat`/`enumstat`/`enumreadable` cases whose setup has no `lastWriteUtc` (e.g. `stat: empty file`,
  `stat: file held without share still has a stat`, all `enumstat`/`enumreadable` rows) are the creation time of that oracle run, so a regenerated
  golden changes and no Rust run can equal it. The Rust twin must compare `write_ticks` only when the setup pins `lastWriteUtc` (or `copy-attrs`), and
  otherwise compare `length` plus "ticks equal the file's own `metadata().modified()`". Suggest the lead make the oracle emit `write_ticks: null` there.
- Machine/OS-build dependent expectations are baked in (notably `copy` onto an existing directory = 80, `delete` of a directory = 5, atomic write
  over a read-only target = 5): the C# source comment says some Windows builds answer differently. They are correct for the machine that produced the
  golden; a mismatch on another build is an environment finding, not a porter bug.
- `physical-fs-resolve` has no case with a symlink, hop-cap or loop (junction cases are only in `ops`): the RV-S06 loop behaviour (note 23) is covered only
  by the ported native test.

## Open questions
- **Where do the two Win32 primitives live?** `MoveFileExW(.., 0)` (move without REPLACE_EXISTING) and `CopyFileExW(.., COPY_FILE_FAIL_IF_EXISTS)` (and maybe
  `DeleteFileW`) have no std equivalent; AGENTS.md says Windows-only code lives in `pr-platform-win` and `pr-core` takes no Windows crate. Recommended:
  a private `#[cfg(windows)] mod win32 { extern "system" { fn MoveFileExW..; fn CopyFileExW..; } }` inside `physical.rs`, three functions, each unsafe
  block with `// SAFETY:`, UTF-16 NUL-terminated buffers via `OsStrExt::encode_wide`, no new crate (kernel32 is linked by std already). Lead must approve
  because the rule text says FFI belongs elsewhere. Alternative (no FFI, worse): create the destination with `create_new` as an exclusive placeholder,
  then `fs::rename`/`fs::copy` over our own placeholder, deleting it on failure and synthesizing raw 183/80; it exposes an empty destination for a moment,
  misreports a directory destination (5 instead of 183/80) and breaks the "failed copy leaves nothing" golden unless every failure path cleans up.
- Non-Windows: the crate builds on other hosts for CI lint only? If so gate the real bodies with `#[cfg(windows)]` and leave `unimplemented!()`/`Err(Unsupported)`
  elsewhere; the pure helpers (`matches_win32_expression`, `decode_text_like_dotnet`, `new_guid_n`, `replace_with_retry`) stay portable.
- `FileInfo.Length`/stat of a FILE symlink and `enumerate_files` patterns containing a separator are not pinned by any golden (notes 9, 16): decide whether
  to add cases to the oracle before porting (cheap) or accept the documented behaviour.

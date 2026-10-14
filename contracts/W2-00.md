# W2-00: .NET `System.IO.Path` helpers (shared; PROPOSED unit, prerequisite of W2-05..W2-11)
- Color: 🟡 | Porter: **sonnet** (Windows path rules, .NET quirks) | Test porter: haiku | Verifier: sonnet
- Why this unit exists: contracts/README.md D8 says Wave 2 hoists shared helpers into a lead-owned module. Seven Wave 2 units
  (and W2-12/14 file I/O units) call `Path.GetFullPath/GetDirectoryName/GetFileName/GetFileNameWithoutExtension/
  TrimEndingDirectorySeparator/IsPathRooted/IsPathFullyQualified`. `std::path::Path::{parent,file_name,file_stem}` are NOT equivalent
  (they differ on `C:`, `C:a`, `\\?\`, UNC roots, `/` vs `\`, trailing separators), so every porter would write a different, subtly
  wrong version. One implementation + one golden removes that risk. Do not merge any W2-05..11 port before this unit.
- C# source: no single file. This is the .NET base-class `System.IO.Path` (Windows) as used by the C# units at baseline d92d6aae.
  Reference implementation (.NET 10 `Path.Windows.cs`, `PathInternal.Windows.cs`) is not in the repo; the oracle command `dotnet-path`
  (below) is the spec.
- C# tests to port: none (BCL). Rust tests: golden-driven only (below) plus the few hand-written cases in "Done when".
- PARITY rows: new row `X-dotnet-path` (suggested text: "Done | `pr_core::text::dotnet_path` | `text_dotnet_path::*` (golden `dotnet-path`) |
  .NET Path.* on Windows"). Underpins P-01, P-02, I-17, C-22, C-25, F-05, K-39.
- Owned files (the ONLY files you may create/modify):
    crates/pr-core/src/text/dotnet_path.rs      (stub: contracts-w2/W2-00/dotnet_path.rs, compiles clippy-clean)
    crates/pr-core/tests/text_dotnet_path.rs
- Lead-owned edit for this unit: `crates/pr-core/src/text/mod.rs` gets `pub mod dotnet_path;` (before `mod ordinal_casing_table;`).
- Read-only inputs: `tests/golden/dotnet-path.json` (112 cases: 72 `path` cases with 8 result fields each, 39 `full` cases, 1
  `invalid_path_chars`).
- Public API (stub, do NOT change signatures): `PathError { Argument, TooLong }`, `DIRECTORY_SEPARATOR`, `ALT_DIRECTORY_SEPARATOR`,
  `is_directory_separator`, `is_invalid_path_char`, `root_length`, `get_full_path`, `get_directory_name`, `get_directory_name_raw`,
  `get_file_name`, `get_file_name_without_extension`, `ends_in_directory_separator`, `trim_ending_directory_separator`,
  `is_path_rooted`, `is_path_fully_qualified`. Already elsewhere (do not duplicate): `catalog::types::get_extension`,
  `file_actions::errors::get_path_root`.
- Semantics notes (every statement below was checked against .NET 10.0.401 with the oracle, the algorithms were prototyped in a
  scratch crate and reproduce all 72 `path` cases on all 8 fields, and `std::path::absolute` + the two pre-checks reproduces all 39
  `full` cases of the golden):
  1. **Separators** are `\` and `/` everywhere. All indexes are BYTE offsets into the `&str` (every prefix, separator and drive char
     is ASCII, so slicing at them is safe even for non-ASCII server/file names).
  2. **`root_length`**: same rules as `get_path_root` in `file_actions/errors.rs` (read its doc, it is already verified by golden
     `path-root`) but returns the raw byte length and has NO "only spaces => None" rule and NO separator normalization.
     `device` prefixes: `\\?\`, `\??\` (exactly with backslashes), or two separators then `.`/`?` then a separator. UNC `\\srv\share`
     = through the end of the share name (a second separator ends it); `\\?\UNC\srv\share` likewise after 8 chars. Drive: ASCII letter
     + `:` (+ one separator). Examples: `C:\a` 3, `C:a` 2, `\a` 1, `\\srv\share\x` 11 (`\\srv\share`), `\\srv\` 6 (WHOLE string: a bare
     `\\srv\` is a root), `\\?\C:\a` 7, `\\?\x` 5, `1:\a` 0, `a\b` 0.
  3. **`get_directory_name`** (algorithm is in the stub doc): `None` for `""`, only spaces, or when `len <= root_length`
     (`C:\`, `C:`, `\\srv\share`, `\\?\C:\`, `/`). `Some("")` for a bare name. Result separators are NORMALIZED (`C:/a/b.jpg` =>
     `C:\a`, `/a` => `\`, `C:\a//b.jpg` => `C:\a`); `get_directory_name_raw` is the same offset WITHOUT normalization and `""`
     instead of `None` (`C:/a/b.jpg` => `C:/a`). `C:\a\..\b.jpg` => `C:\a\..` (no dot resolution).
  4. **`get_file_name`**: after the last `\`/`/` unless that separator is inside the root; `":"` is never a separator by itself
     (`C:\a\b:c.jpg` => `b:c.jpg`), but a drive-relative `C:b.jpg` => `b.jpg`. `get_file_name_without_extension`: cut at the LAST `.`
     of that name (`.jpg` => `""`, `a..jpg` => `a.`, `a.` => `a`).
  5. **`trim_ending_directory_separator`** removes ONE separator and never from a root: `C:\a\\` => `C:\a\`; `\\srv\share\` =>
     `\\srv\share`; `\\srv\` and `\\` stay; `/` stays.
  6. **`is_path_rooted`**: first char is a separator, or an ASCII letter followed by `:` (`1:\a` is NOT rooted, `a:b` is).
     **`is_path_fully_qualified`**: see the stub doc; `\\` (two chars) and `//x/y` and `\??\C:\a` are fully qualified, `\a`, `/a`, `C:a`, `C:`,
     `a:b`, `1:\a`, `..\x` are not.
  7. **`get_full_path`**: `Err(Argument)` for `""`, only U+0020 spaces (a tab or NBSP is NOT empty: it resolves against the current
     directory), or any `'\0'`. Otherwise `std::path::absolute` (it calls `GetFullPathNameW`; `\\?\...` input is returned unchanged,
     `\\.\...` is normalized). Behaviours the golden pins: trailing dots/spaces of the path are dropped (`C:\a. ` => `C:\a`), a
     trailing separator is kept, `<>|*?` are accepted, `C:\a:b` (stream syntax) is accepted, `1:\a` is accepted, `\\srv` and
     `\\srv\share` unchanged. Do NOT add golden cases that depend on the process directory or on the per-drive current directory
     (`C:`, `C:foo`, `\`, `a:b` resolve differently per machine).
  8. **`is_invalid_path_char`**: .NET 10 `Path.GetInvalidPathChars()` is ONLY `|` and U+0000..U+001F. It does NOT contain `"`, `<`,
     `>` (verified; older .NET Framework did) and not `*`, `?` (the callers add those two themselves).
- Traps: §7 items 1 (never `to_lowercase`), 7 (Windows paths: `\\?\`, UNC, volume GUID, drive-relative: the golden has a case for each),
  8 (`None` vs `Some("")` of `get_directory_name`), 3 (use `char_indices`/byte offsets, not UTF-16 positions).
- Depends on: nothing (std only). `get_extension` and `get_path_root` exist already but are not called.
- Done when: every `path` case of the golden matches all 8 fields, every `full` case matches (compare `Err(Argument)` with `{"throws":
  "ArgumentException"}`), the invalid-char code point list equals `[124, 0..=31]` as a set, clippy clean, the
  `#![allow(unused_variables, dead_code)]` line removed from the stub. Hand-written extra tests: relative `get_full_path("x\\..\\y")`
  equals `std::env::current_dir().join("y")` text; `get_full_path` of a 40 000 char plain path is `Err(TooLong)` or `Ok` depending on
  `GetFullPathNameW` (accept either, but it must not panic).

## Golden package (all files ready to copy)
- `oracle/W2-00.cs`: defines `W2Util` (helpers shared by ALL `W2-*.cs` oracle files: copy this file first) and
  `Commands.DotnetPath`. Dispatch line: `"dotnet-path" => Commands.DotnetPath(inputs),`.
- `golden-inputs/dotnet-path.json`: `{ "paths": [...], "full_paths": [...] }`.
- `golden-reference/dotnet-path.json`: the output of the oracle run at d92d6aae (built from `git archive d92d6aae`,
  `%TEMP%\pr-oracle\d92d6aae...\core`, .NET 10.0.401); the lead regenerates `tests/golden/dotnet-path.json` with `tools/oracle.ps1
  -Command dotnet-path` and it must be identical.

## Lead actions for the whole W2-05..W2-11 batch (the contracts name them again where relevant)
1. `crates/pr-core/src/text/mod.rs`: `pub mod dotnet_path;` (rustfmt order: before `mod ordinal_casing_table;`).
2. `crates/pr-core/src/catalog/mod.rs`: add `pub mod capture_groups; pub mod compare; pub mod drag_drop; pub mod review_catalog;
   pub mod snapshot_validator; pub mod sorting;` (alphabetical, `cargo fmt` fails otherwise).
3. `crates/pr-core/src/file_actions/mod.rs`: add `pub mod destination_policy; pub mod duplicates;` (alphabetical).
4. Copy every `contracts-w2/W2-*/oracle/W2-*.cs` to `tools/oracle-cs/Commands/` and add ONE block to the switch in
   `tools/oracle-cs/Program.cs` (each file header has its own lines; combined):
   ```
       // W2-00 .. W2-11 (catalog wave)
       "dotnet-path" => Commands.DotnetPath(inputs),
       "image-sort-entries" => Commands.ImageSortEntries(inputs),
       "image-sort-paths" => Commands.ImageSortPaths(inputs),
       "sibling-folders" => Commands.SiblingFolders(inputs),
       "capture-groups" => Commands.CaptureGroups(inputs),
       "compare-pair" => Commands.ComparePair(inputs),
       "numbered-pattern" => Commands.NumberedPattern(inputs),
       "duplicates" => Commands.Duplicates(inputs),
       "destination-policy" => Commands.DestinationPolicy(inputs),
       "snapshot-validator" => Commands.SnapshotValidator(inputs),
       "review-catalog" => Commands.ReviewCatalogTrace(inputs),
       "drag-drop" => Commands.DragDrop(inputs),
   ```
   Copy the `golden-inputs/*.json` into `tests/golden-inputs/` (keep `gen-review-catalog.js` next to `review-catalog.json` or in
   `tools/`; it is the generator of that input), run `./tools/oracle.ps1`, commit `tests/golden/*.json`, add the 12 rows to
   `tests/golden-inputs/README.md`. The 12 outputs must equal `contracts-w2/W2-*/golden-reference/*.json` byte for byte
   (the oracle was built with `tools/oracle-cs` of the sync worktree + these files, against the d92d6aae Core build).
5. `contracts/README.md` roster rows (see each contract for the Rust path and porter): W2-00 sonnet, W2-05 sonnet, W2-06 sonnet,
   W2-07 sonnet, W2-08 sonnet (opus verifier), W2-09 sonnet, W2-10 opus, W2-11 haiku.
6. No new crates. No change to `pr-test-support` is required (W2-08 uses a local test fake for reparse points, see its contract).

## Open questions
- Is a shared `dotnet_path` module acceptable (recommended; the alternative is six private copies)? If the lead prefers another
  location (`pr_core::path_util`), only the `use` lines in the other stubs change.
- `file_actions::errors::get_path_root` duplicates the root-length scan; after this unit merges the lead may refactor it to call
  `root_length` (golden `path-root` guards the refactor). Not required.

# Golden inputs

One JSON file per `tools/oracle-cs` command. `tools/oracle.ps1` builds the C# PhotoReview code at the baseline commit
(docs/SYNC-LEDGER.md) and writes `tests/golden/<command>.json` with the C# results for these inputs. Rust tests read
the golden file through `pr_test_support::golden`.

Rules (docs/MULTI-AGENT-PLAN.md §4):
- Only the Golden Generator role edits this folder or `tests/golden/`; porters never do.
- Add inputs that separate correct from plausible-but-wrong behaviour (boundary values, each rejection reason on its
  own: a case that fails for two reasons cannot tell which check is missing).
- After a baseline move, re-run `./tools/oracle.ps1`; a golden diff is a C# behaviour change to port.

| Command | Feeds | Notes |
|---|---|---|
| `datetime-json` | `pr_core::time` | System.Text.Json write/parse of UTC `DateTime` |
| `ordinal-ignore-case` | `pr_core::text` | `StringComparison.OrdinalIgnoreCase` equals/compare |
| `ordinal-casing-table` | `crates/pr-core/src/text/ordinal_casing_table.rs` | writes Rust source, not JSON |
| `natural-cmp` | W1-06 `ManagedNaturalComparer` | pair signs + sorted lists |
| `model-enums` | W1-01 Model enums | members, values, `[JsonAlias]`, JSON text of the 16 persisted enums |
| `lenient-enum` | W1-02 `serde_ext` lenient enums | `LenientEnumConverter` / `SettingsEnumConverter` reads and writes |
| `flag-enum-json` | W1-02 `serde_ext` flag enums | `FlagEnumJson` via ExifInfoFields / TitleBarFields |
| `flag-fields` | W1-03 flag field enums | members, `All`, `Default` of the two `[Flags]` enums |
| `shortcut-canonical` | W1-04 `ShortcutKeyCanonical` / `ShortcutMappings` | alias table, canonicalize, optional names, defaults |
| `perf-options` | W1-05 `PerformanceOptions` | every public const (name, type, value) |
| `natural-key` | W1-06 `BuildNaturalKey` | exact key strings |
| `image-file-types` | W1-07 `ImageFileTypes` | IsSupported / IsRawPath / Path.GetExtension |
| `lru-trace` | W1-08 `BoundedLruCache` | scripted eviction scenarios |
| `app-version` | W1-10 `AppVersion` | TryParse, CompareTo, operators |
| `update-url-policy` | W1-10 update URL policy | allowed / rejected update URLs |
| `loc-template` | W1-11 `LocTemplate` | parse, placeholder names, render |
| `loc-render` | W1-11 localized render | Render with arguments |
| `loc-pseudo` | W1-11 pseudo-localization | pseudo transform |
| `path-root` | W1-13 RecycleBinCapacity | Path.GetPathRoot (normalized root) |
| `journal-error-text` | W1-13 JournalErrors | IsKnown + English text per code |
| `journal-lines` | W1-14 `JournalLineParser` | journal line parsing |
| `percentile` | W1-15 `BenchmarkStatistics.Percentile` | exact IEEE bits |
| `diag-options` | W1-15 diagnostics options | option parsing |
| `vblank` | W1-22 `VBlankEstimator` | state after every Add / Reset |
| `perf-csv` | W1-16 | `perf-*.csv` reader |
| `perf-nav` | W1-19 | navigation reassembly |
| `perf-stats` | W1-17 | stats + RunFileMeta |
| `perf-rules` | W1-18 | R-* rules + number formatting |
| `perf-report` | W1-20 | summary.md + json writer |
| `perf-analyze` | W1-21 | end-to-end pipeline |
| `dotnet-path` | W2-00 `pr_core::text::dotnet_path` | System.IO.Path helpers (GetFullPath, GetDirectoryName, ...) |
| `settings-json` | W2-01 `AppSettings` JSON | System.Text.Json read/write of config.json |
| `settings-normalize` | W2-02 normalizer | normalize + disable conflicting shortcuts |
| `settings-validate` | W2-02 `SettingsValidator` | range and enum validation |
| `settings-repair-text` | W2-02 `build_repair_text` | repair report text |
| `settings-store` | W2-03 `SettingsStore` | load/save/backup scenarios |
| `settings-parse-text` | W2-03 `parse_text` | import pipeline and repairs |
| `l10n-catalog` | W2-04 `pr_l10n::catalog` | catalog JSON reader |
| `l10n-localizer` | W2-04 `pr_l10n::localizer` | lookup, fallback, pseudo, show-keys |
| `l10n-loader` | W2-04 `pr_l10n::loader` | language resolution and user folder load |
| `l10n-generator` | W2-04 key generator | PRLOC001..007 diagnostics and generated names |
| `l10n-flat-json` | W2-04 flat JSON reader | flat reader used by the generator |
| `image-sort-entries` | W2-05 `catalog::sorting` | fallback sort of entries |
| `image-sort-paths` | W2-05 `catalog::sorting` | fallback sort of paths |
| `sibling-folders` | W2-05 sibling folders | sibling folder list |
| `capture-groups` | W2-06 `capture_groups` | JPEG+RAW pairing, XMP sidecar choice |
| `compare-pair` | W2-07 `catalog::compare` | compare pair finder |
| `numbered-pattern` | W2-07 numbered pattern | numbered name matcher |
| `duplicates` | W2-07 `file_actions::duplicates` | duplicate decision |
| `destination-policy` | W2-08 `destination_policy` | destination validation |
| `snapshot-validator` | W2-09 snapshot validator | Explorer snapshot validation |
| `review-catalog` | W2-10 `ReviewCatalog` | scripted catalog scenarios (gen-review-catalog.js generates the input) |
| `drag-drop` | W2-11 `drag_drop::parse` | drop parsing |
| `session-json-read` | W2-12 `SessionStore` read | session JSON read |
| `session-json-write` | W2-12 `SessionStore` write | session JSON write |
| `session-key` | W2-12 canonical folder key | session key |
| `session-writer-sequence` | W2-13 `SessionWriter` | debounced write sequences |
| `physical-fs-ops` | W2-14 `PhysicalFileSystem` | file ops on a real disk |
| `physical-fs-pattern` | W2-14 | enumeration pattern matching |
| `physical-fs-read-text` | W2-14 | ReadAllText decoding |
| `physical-fs-resolve` | W2-14 | resolve_real_path (needs W2-00) |
| `navigation-stat-worker` | W2-15 `NavigationStatWorker` | queue and panic scenarios |
| `file-log-format` | W2-16 `FileLog` | line format |
| `file-log-backup-name` | W2-16 | rotation backup name |
| `file-log-bytes` | W2-16 | encoding bytes |
| `perf-csv-escape` | W2-17 `perf_csv` | CSV escaping |
| `perf-csv-row` | W2-17 | CSV rows |
| `perf-csv-file` | W2-17 | CSV file contents |
| `perf-path-id` | W2-17 `perf::path_id` | path id hash |
| `review-metrics` | W2-17 `ReviewMetrics` | snapshot JSON |
| `update-checker` | W2-18 `UpdateChecker` | HTTP exchange scenarios |
| `forwarded-path` | W2-19 `instance::protocol` | wire format |
| `context-menu` | W2-20 `settings::context_menu` | item ids, hidden set parsing, migration |
| `journal-stream-lines` | W3-01 `decode_stream_reader_lines` | StreamReader line decoding of journal bytes |
| `journal-ops` | W3-01 `OperationJournal` | append / dismiss / read scenarios, journal bytes |
| `journal-committed-moves` | W3-01 `read_committed_moves` | committed move read (LAST MOVE READ serialized) |
| `journal-transaction` | W3-01 `JournalTransaction` | 25 transaction cases incl. the T10 quirk |
| `journal-compaction-plan` | W3-02 `compaction::build` | compaction plan and line rules |
| `journal-tail-committed-move` | W3-02 tail reader | newest committed move from the tail window |
| `journal-compact` | W3-01 `try_compact` | compaction outcomes |
| `journal-reconcile` | W3-03 `reconcile_pending_operations` | 71 reconcile cases |
| `journal-startup-recovery` | W3-03 `startup_recovery` | 11 startup recovery cases |
| `file-action-scenarios` | W3-04 `FileActionService` | 111 single/group action scenarios, every field |
| `undo-scenarios` | W3-05 `UndoService` | 100 undo scenarios incl. journal text |
| `recovery-retry` | W3-06 `RecoveryRetryService` | 106 retry cases (6 need the stat seam) |
| `recovery-decide` | W3-07 `RecoveryFileCheck` decision | 720 verdict cases |
| `recovery-check` | W3-07 `RecoveryFileCheck` | virtual 715 cases + 20 native physical `p-*` cases |
| `partial-cleanup` | W3-08 `remove_if_partial` / `is_same_file` | 26 cases: 17 cleanup, 9 same-file |

Fixtures synced by `tools/sync-from-csharp.ps1` at `d92d6aae` (never hand-edited, P-15): `operations-legacy.jsonl`, `journal-older-build/*.jsonl`, `config-v1.json`, `config-v2.json`.

## Wave 4 (imaging)

Goldens for the merged W4 units (W4-01a, 01c, 02a, 03a, 05a/b, 06a, 07a/b, 08a/b, 09, 10a, 11a-e, 12b/c, 14a, 15a/b, 16a/b) are listed in `docs/PARITY.md` per row (name and case count). All Wave 4 units are now merged. Goldens `embedded-thumbnail-read` (native hash), `thumbnail-disk-decode` and the W4-11e native twins are only exercised by native tests that still hit `todo!()` seams (see PARITY.md "Wave 4 final status"). No pre-existing golden changed.

## Wave 5 (RAW, TurboJPEG, LibRaw)

45 commands, one oracle file per unit in `tools/oracle-cs/Commands/W5-*.cs` (names and case counts: `contracts/README.md` "Wave 5 roster"). The corpus commands (`cr2-container-corpus`, `raw-dng-corpus`, `rw2-corpus`, `raf-corpus`, `raw-exif-corpus`) and the corpus cases of `raw-arw-reader`, `raw-cr3-reader` and `libraw-decoder-native` were generated WITHOUT the RAW corpus (P-30): zero cases, `present: false` or `skipped: true`. `turbojpeg-fixtures` writes `tests/fixtures/turbojpeg/` (18 files, committed). `turbojpeg-native-decode`, `turbojpeg-decode`, `turbojpeg-read-info` and `libraw-decoder-native` record the `turbojpeg.dll`/`libraw.dll` found in the C# checkout's `native/x64` (SHA-256 in the first "environment" case). Inputs fixed while running the oracle are listed in `contracts/README.md` D56. No pre-existing golden changed.

## Wave 7 (benchmarking)

Goldens `bench-*`, `benchmark-*`, `slow-link-*`, `cli-arguments`, `cli-dataset-plan` and `cli-tool-path-guard` were merged with units W7-01a, 01b, 02a, 02b, 03a, 03b, 03c and 03d. The `cli-process` golden was not produced (needs the baseline WPF-bound Benchmark.Cli exe; D70). Units with FAIL verdicts or surviving mutants are Partial in `docs/PARITY.md`.

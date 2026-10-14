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

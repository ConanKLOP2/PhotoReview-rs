# Work-unit contracts

One file per work unit (`W<wave>-<n>.md`), written by the Contract Writer before any porter starts
(docs/MULTI-AGENT-PLAN.md §2). The stub each contract names is already in the tree with `todo!()` bodies and
compiles with the rest of the workspace; a porter fills only the files under "Owned files".

**Lead decisions below override anything a contract says.** Porters read this file first, then their contract.

## Wave 1 roster

| WU | Unit | Rust | Porter | Golden |
|---|---|---|---|---|
| W1-01 | Model enums | `pr_core::model::enums` | Haiku | `model-enums` |
| W1-02 | Lenient enum/flag JSON | `pr_core::model::serde_ext` | **Sonnet** | `lenient-enum`, `flag-enum-json` |
| W1-03 | Flag fields | `pr_core::model::fields` | Haiku | `flag-fields` |
| W1-04 | Shortcuts, ReviewAction | `pr_core::settings::shortcuts` | Haiku | `shortcut-canonical` |
| W1-05 | PerformanceOptions | `pr_core::settings::perf_options` | — (constants, complete in stub; test porter only) | `perf-options` |
| W1-06 | Natural comparer | `pr_core::catalog::natural_cmp` | Haiku | `natural-cmp`, `natural-key` |
| W1-07 | Catalog types, ExplorerReason | `pr_core::catalog::{types, explorer_reason}` | Haiku | `image-file-types` |
| W1-08 | BoundedLruCache | `pr_core::caching::lru` | Haiku | `lru-trace` |
| W1-09 | File-action records | `pr_core::file_actions::types` | Haiku | — |
| W1-10 | AppVersion, update URL policy | `pr_core::updates::version` | Haiku; `validate_update_url` **Sonnet** (D6) | `app-version`, `update-url-policy` |
| W1-11 | LocTemplate | `pr_l10n::template` | Haiku | `loc-template`, `loc-render`, `loc-pseudo` |
| W1-12 | GenerationClock | `pr_core::catalog::generation` | **Sonnet** | — |
| W1-13 | Journal/FS errors | `pr_core::file_actions::errors` | Haiku | `path-root`, `journal-error-text` |
| W1-14 | Journal line parse/write | `pr_core::file_actions::journal_line` | Haiku (escalate fast) | `journal-lines` (mandatory) |
| W1-15 | Percentile, DiagOptions | `pr_core::diagnostics::{bench_stats, diag_options}` | Haiku | `percentile`, `diag-options` |
| W1-16 | Perf CSV reader | `pr_perf::csv` | Haiku | `perf-csv` |
| W1-17 | Perf stats | `pr_perf::stats` | Haiku | `perf-stats` |
| W1-18 | Perf rules | `pr_perf::rules` | Haiku | `perf-rules` |
| W1-19 | Perf navigation | `pr_perf::nav` | Haiku | `perf-nav` |
| W1-20 | Perf report writer | `pr_perf::report` | **Sonnet** | `perf-report` |
| W1-21 | Perf analyze pipeline | `pr_perf::analyze` | **Sonnet** | `perf-analyze` |
| W1-22 | VBlankEstimator | `pr_core::display::vblank_estimator` | Haiku | `vblank` |

All units can run in parallel: every cross-unit type is already defined (complete data, `todo!()` functions) in
the stubs. A unit's tests may only call functions of OTHER units once those are merged; until then test your own
functions only.

## Lead decisions (Wave 1)

- **D1 Paths in records are `String`.** Catalog, journal, session and file-action records mirror C# `string` (journaled
  and compared as text). The file-system boundary (`pr_core::abstractions::FileSystem`) takes `&Path`; convert with
  `Path::new(&s)`. `ExplorerViewSnapshot` was changed to `String` for the same reason.
- **D2 `JournalEntry` and `JournalGroupMember` live in W1-14** (`file_actions::journal_line`, timestamps as
  `JournalDateTime`). W1-09 re-exports them and no longer defines `JournalGroupMember`; its contract text about that
  type is superseded.
- **D3 Counters keep the C# integer type.** `GenerationClock` uses `AtomicI64` with wrapping increments because C#
  uses `long`; R0003's "AtomicU64" means "an atomic of the C# width".
- **D4 Diagnostic variables use the Rust prefix** `PHOTOREVIEW_RS_DIAG_*` (R0002): they change behaviour, so a value
  set to diagnose the C# app must not leak into this one. The W1-15 golden is keyed by the C# names; its test maps
  `PHOTOREVIEW_DIAG_X` → `PHOTOREVIEW_RS_DIAG_X`. (W1-15's contract text saying "C# names on purpose" is superseded.)
- **D5 Accepted divergences** (record them in the unit's tests as `// DIVERGENCE:` comments, no golden case):
  serde_json cannot read a bare `-0` number token as an integer where System.Text.Json reads 0 (W1-02; W1-14 handles
  it with `raw_value`); W1-14 converts an offset timestamp to UTC (C# makes a machine-dependent Local value) and
  rejects invalid UTF-8 in a property name; W1-11 `describe` strips Rust's ` (os error N)` suffix so texts match .NET;
  W1-21 reads UTF-8 CSVs only, numbers like `1e400` in JSON side files are rejected; non-coded journal error text is
  `err.to_string()` (free text, never parsed).
- **D6 W1-10 `validate_update_url` goes to Sonnet** from the start (hand-written `System.Uri` subset, 127 golden cases);
  the rest of W1-10 stays Haiku.
- **D7 W1-20 and W1-21 go to Sonnet** (byte-identical JSON writer, many ordering decisions), as their contracts propose.
- **D8 Helpers are where their first unit put them** (`catalog::types::get_extension`, `file_actions::errors::
  get_path_root`, `pr_perf::rules::{fmt_fixed, fmt_double}`). Wave 2 hoists shared ones into a lead-owned module.
- **D9 Output files keep CRLF** where C# wrote them on Windows (perf report, indented JSON): byte-identical wins.
- **D10 Things deferred to later waves:** `ReviewAction` deserialization and `CanonicalizeAll(AppSettings)` (W2-01/02);
  `CreatePseudo`/`CreateShowKeys` (Localizer, W2-04); `PerfCsvListener` writer (W2-17); a Windows-locale
  `NumberCulture` lookup (W8, UI).
- **D11 Temporary `#![allow(unused_variables, dead_code)]`** at the top of a stub is allowed only while it has
  `todo!()` bodies; the porter removes it in the same commit that removes the last `todo!()`.

## Wave 2 roster (dependency order)

Merge in this order; a porter may start once its "Depends on" units are merged (stub types already compile).

| WU | Unit | Rust | Porter | Goldens |
|---|---|---|---|---|
| W2-00 | .NET `Path.*` helpers | `pr_core::text::dotnet_path` | Sonnet | `dotnet-path` |
| W2-20 | ContextMenuLayout | `pr_core::settings::context_menu` | Sonnet | `context-menu` |
| W2-01 | AppSettings + config JSON | `pr_core::settings::app_settings` | Sonnet | `settings-json` |
| W2-02 | Normalizer / validator | `pr_core::settings::normalizer` | Sonnet | `settings-normalize`, `-validate`, `-repair-text` |
| W2-03 | SettingsStore (INV-11) | `pr_core::settings::store` | Sonnet | `settings-store`, `settings-parse-text` |
| W2-04 | Localization core + keygen | `pr_l10n::{catalog,loader,localizer}`, `build.rs` | Sonnet | `l10n-*` |
| W2-05..11 | Catalog services | `pr_core::catalog::*`, `file_actions::{duplicates,destination_policy}` | Sonnet (W2-11 Haiku) | per unit |
| W2-12 | SessionStore | `pr_core::session::store` | Sonnet | `session-key`, `session-json-*` |
| W2-13 | SessionWriter | `pr_core::session::writer` | **Opus** | `session-writer-sequence` |
| W2-14 | PhysicalFileSystem, Counting | `pr_core::io::{physical,counting}` | **Opus** (`physical.rs`) | `physical-fs-*` |
| W2-15 | NavigationStatWorker | `pr_core::io::navigation_stat` | Sonnet | `navigation-stat-worker` |
| W2-16 | FileLog (INV-10) | `pr_core::diagnostics::file_log` | Sonnet | `file-log-*` |
| W2-17 | Perf diagnostics | `pr_core::diagnostics::{metrics,perf,perf_csv}` | Sonnet | `perf-*`, `review-metrics` |
| W2-18 | UpdateChecker | `pr_core::updates::checker` | Sonnet | `update-checker` |
| W2-19 | Instance forwarding | `pr_core::instance::*` | Sonnet | `forwarded-path` |

## Lead decisions (Wave 2)

- **D12 Merge order:** W2-00 -> W2-20 -> W2-01 -> W2-02/W2-03; W2-04 after W2-00. Everything else is independent.
- **D13 `SettingsTexts`** stays a `pr-core` trait; the catalog-backed implementation is supplied by the App composition
  root (not W2-04). W2-01 goldens only need the English implementation.
- **D14 Test ownership:** the per-property `*SettingsTests` that go through the normalizer belong to W2-02; those
  through store/parse belong to W2-03. W2-01 keeps reader/writer tests only.
- **D15 Module ownership:** `pr_core::io` (mod.rs) is lead-owned; W2-14 and W2-15 only fill their own files.
- **D16 Local time** (W2-16): inject a `LocalTimeSource`; `pr-core` takes no `windows-sys` dependency.
- **D17 HTTP transport** (W2-18): behind a trait, real transport lands in `pr-platform-win` later; endpoint and allow-list
  stay those of C# for parity.
- **D18 `sha2`** crate is the SHA-256 for W2-17.
- **D19 Golden determinism:** oracle output must not depend on cwd, clock or file times. Removed from goldens:
  `session-key` case with folder `"	"` (resolves against cwd); `physical-fs-ops` `write_ticks` now null (copy
  time). Equality of times is still checked by `same_write_time`.
- **D20 Accepted divergence (W2-04):** invalid UTF-8 in a language file becomes a "cannot read" warning in Rust
  (`read_all_text` returns InvalidData) where C# substitutes U+FFFD. Record `// DIVERGENCE:` and a `Different` row.

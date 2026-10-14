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

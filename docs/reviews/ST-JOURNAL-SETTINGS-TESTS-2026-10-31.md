# ST-JOURNAL-SETTINGS-TESTS (2026-10-31)

Status: done. Follow-up of ST-CSPARITY (PR #442), which listed five C# tests without a port found by name. Baseline C# `7eaee2a2`.

## Result per test

| C# test | Verdict | Rust |
|---|---|---|
| `JournalDurabilityTests.OldConfigWithoutJournalDurability_LoadsAsFast` | newly ported | `crates/pr-core/tests/settings_journal_durability.rs::old_config_without_journal_durability_loads_as_fast` |
| `JournalDurabilityTests.JournalDurability_ParsesLeniently` (3 rows) | newly ported | `...::journal_durability_parses_leniently` (loop over the same 3 rows) |
| `JournalDurabilityTests.Settings_RoundTrip_PreservesPowerLossSafe_AndProviderReadsLive` | newly ported | `...::settings_round_trip_preserves_power_loss_safe_and_provider_reads_live` (provider closure reads `SettingsStore::current()`) |
| `JournalSmallGapsTests.JournalCodedException_MessageAndInner_HasAnEmptyCodeAndKeepsBoth` | already there (different name) | `crates/pr-core/tests/file_action_errors.rs::with_inner_has_empty_code_and_keeps_the_inner_error_as_source` (empty code, message, inner as `source()`) |
| `JournalSmallGapsTests.JournalCodedException_Parameterless_HasAnEmptyCode` | deliberately not ported | `JournalCodedError` has no parameterless constructor (`code` is always passed or empty via `with_inner`); the "empty code" fact is covered by the test above. A test would need a production constructor that exists only for it. |

Grep over `crates/*/tests` and `src` found no other port of the three settings tests: the existing hits (`settings_normalizer.rs` out-of-range reset, `file_actions_journal.rs::durability_returns_the_providers_current_mode`, `read_skips_corrupt_trailing_line`) cover different behaviour (normalizer, provider-only, torn line), not the settings load/save leniency.

## Verification

- `cargo test -p pr-core --test settings_journal_durability -j 2`: 3 passed.
- `cargo fmt -p pr-core --check`: clean.
- No production code changed; no difference from C# found.

## PARITY

No rows changed by this lane; the orchestrator may drop the 4 settings/JournalCoded items from the ST-CSPARITY gap list (the parameterless constructor is an intentional skip).

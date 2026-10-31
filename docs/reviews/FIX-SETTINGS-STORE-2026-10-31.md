# FIX-SETTINGS-STORE (2026-10-31)

Source: ST-IMAGING-INV-2026-10-31.md item L2.

## Verdict: the bug is real (and inherited from C#)

`SettingsStore.cs` (baseline 7eaee2a2) uses `File.Exists`, which is false for ANY failure, not only "not found". Rust copied that
(`PhysicalFileSystem::file_exists`: "any failure is false"). In `load(None)`, "file missing" falls through to
`persist(defaults)`, which overwrites a real config without a backup. `load_first_run_with_import` had the same hole
(own config stat fails -> treated as first run -> overwritten by the C# import).

## Fix (DIVERGENCE from C#, data safety)

- `load`: when `file_exists` is false for the default path, probe with `read_all_text`: `Ok` -> the file exists (normal path);
  `NotFound` -> first run (defaults written as before); any other error -> `Config.json inaccessible` startup error,
  `keep_corrupt_file`, in-memory defaults, file untouched (same handling as an unreadable file).
- `load_first_run_with_import`: new `own_config_may_exist()` (same probe) so a real own config is never replaced by the import.
- Explicit-path `load(Some(p))` writes nothing, unchanged. Costs one extra read only when the config is absent.

## Tests (crates/pr-core/tests/settings_store.rs)

Red before the fix (2 failed: defaults loaded over a real file; no startup error), green after (122 passed):
- `load_when_stat_fails_but_config_is_readable_loads_it_and_does_not_overwrite_it`
- `load_when_stat_fails_and_read_is_denied_keeps_the_file_and_uses_in_memory_defaults`
- `first_run_import_when_own_config_stat_fails_does_not_overwrite_it_with_the_csharp_import` (not run red separately)

## Other `file_exists` sites

- `backup_newer_config` (store.rs): runs only after the file was read; a failed stat skips the backup (rare); not changed.
- `unique_backup_path`: only picks a name; a wrong "free" answer could overwrite an old backup (very rare); not changed.
- `external_editors.rs`: path resolution only, no writes.

PARITY: add a Different note on the SettingsStore row (stat-failure probe); orchestrator to do it.

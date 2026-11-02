# ADR-D-2026-11-03-settings-backup-failure: Save is refused when the backup of a newer-build config.json fails (P-66)

- **Status:** Accepted. Owner confirmed option A of `P-66` in `docs/PROPOSED-DECISIONS.md` (2026-11-01); code merged in PR #530 (`be6b278`, BACKLOG G27).
- **PARITY row:** `D-settings-newer-backup-failure` in `docs/PARITY.md`
- **Decision owner:** owner

## Context
When `config.json` was written by a newer build (`newer_file_on_disk` set at load), `SettingsStore::save` copies it to a `.newer-` backup before overwriting it with version 4, which drops the unknown fields. The C# `SettingsStore.BackupNewerConfig` is best effort: if the copy fails, C# still writes version 4 (C# test `Save_WhenBackupFails_StillWritesTheConfig`, as named in the comment above the Rust test; the C# source is not on this machine and was not re-read for this ADR).

## Decision
In `crates/pr-core/src/settings/store.rs`, `backup_newer_config` returns `io::Result<()>`. If the existence check, the backup name pick or the copy fails, it logs a warning ("Could not back up the newer-build config.json before overwriting it: ...") and returns the error; `persist` maps it to `SettingsSaveError::Io` and returns before writing. The newer file stays byte for byte on disk and `newer_file_on_disk` stays set. The flag is cleared only after the atomic write succeeds, so the next Save retries the backup. Pruning of old backups still only logs warnings and never blocks the save.

## Consequences
- Data safety over convenience: unknown fields of a newer build are never silently lost.
- Cost: while the backup keeps failing, the user cannot save settings (an error is returned and a warning is logged; nothing is lost).
- Rust differs from C# only on this failure path; the success path is unchanged.

## Alternatives not chosen
B: keep the C# best effort (unknown fields lost silently on a backup failure). C: write nothing but keep the settings in memory only (hides the failure).

## Undo
Restore the early returns in `backup_newer_config` so it returns `()` and `persist` clears the flag regardless.

## Tests
`crates/pr-core/tests/settings_store.rs`: `save_when_newer_backup_fails_refuses_to_overwrite_and_reports_the_error` (Save returns `SettingsSaveError::Io`, original bytes stay, no backup, warning logged), `save_after_failed_newer_backup_retries_the_backup_and_keeps_the_unknown_fields_there`, `save_when_backup_succeeds_but_write_fails_keeps_the_flag_for_the_next_save`.

## Sources
`docs/PROPOSED-DECISIONS.md` row `P-66`; `docs/reviews/FIX-G27-CONFIG-2026-11-01.md`; PR #530 (`be6b278`).

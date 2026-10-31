# FIX-RECYCLE-TRUNG (2026-10-31)

Status: done. Source: `docs/reviews/ST-COM-RECYCLE-2026-10-31.md`, medium items T1, T2, T4, T5 (T3 and T6 not in this lane). C# baseline `7eaee2a2` (`src/PhotoReview.Platform.Windows/Shell/RecycleBinShell.cs`, `WindowsRecycleBin.cs`). No real Recycle Bin, Shell verb or registry write was touched; tests use fakes and one read-only path that does not exist.

| Item | Class | Result |
|---|---|---|
| T5 NullLog | (a) fixed | `WindowsRecycleBin::with_log(backend, log)`; `new` kept (NullLog) but documented; `pr-shell/src/composition/platform.rs` now passes the real log. Test `with_log_routes_production_refusals_to_the_supplied_log` (production adapters, read-only queries, missing file -> "Recycle refused (size unreadable)" reaches the log; was lost before). |
| T4 dead restore_verb | (a) fixed, partly (c) | `restore_verb.rs` is now `pub mod restore_verb` in `lib.rs`; tests import it from the crate instead of `#[path]`, so they run against the compiled module. Production verb bytes now come from `restore_verb::CANONICAL_VERB_Z` (single source; test `production_verb_bytes_are_the_nul_terminated_canonical_verb`). Module doc states plainly that the localized-label helpers are not wired into production. Wiring `QueryContextMenu` + HMENU (C# path) is deferred: it needs the real Shell to prove it, and `undelete` by canonical name was the verified behaviour so far. |
| T1 DateDeleted matching | (c) deferred | C# also matches only by folder/name/size/DateModified (`RecycleCandidateSelector.IsMatch`), so Rust is equivalent; ambiguity returns `Ok(false)` (fail-safe). Design: add `System.Recycle.DateDeleted` (fmtid 9B174B33-40FF-11D2-A27E-00C04FC30871, pid 3) to `RecycleCandidate`, and pass the journal's recycle time through `RecycleBinRestoreBackend::try_restore` (trait + `UndoService` + journal change). Choosing "newest DateDeleted" without that time could restore the wrong generation when an older group is undone, so it must not be done without the caller's timestamp. Needs real-Shell proof that the property exists on bin items. |
| T2 TOCTOU / modal dialog | (b) equal to C#, product decision | C# also pre-checks only and invokes with `nShow=1`, no `NO_UI` (see FIX-UNDELETE-HANG: `NO_UI` would turn a legitimate conflict prompt into a silent failure). Rust already adds the 120 s timeout. Not changed. If the owner later wants `CMIC_MASK_FLAG_NO_UI | NOASYNC` plus a parent-folder-exists check, it must be verified on a real Shell in a controlled TempRoot. |

## Tests

`cargo test -p pr-platform-win --test recycle_bin_facade` (21 passed), `--test recycle_restore_selection` (18 passed), `cargo clippy -p pr-platform-win --all-targets -- -D warnings` clean, `cargo check -p pr-shell` ok, fmt applied.

## PARITY

No row changed. Rows for `recycle_bin_shell`/`restore_verb` should stay below Done until the native `entries()`/restore test and the T4 wiring decision exist.

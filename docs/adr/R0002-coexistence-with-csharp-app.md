# R0002: Coexistence with the C# app

**Status:** Accepted (2026-10-14)

## Context

The C# PhotoReview stays in daily use while this port is built. Both apps move/copy/delete real photos, keep an
operation journal and run startup reconcile/recovery over it (C# INV-6, ADR 0003). Two apps acting on one journal
could retry, undo or dismiss each other's operations.

## Decision

- Separate root `%LOCALAPPDATA%\PhotoReviewRs` for config, journal, sessions, logs, caches and window placement
  (`pr_core::app_paths`, port of `AppPaths.cs` with only the root and env var names changed).
- Separate env vars `PHOTOREVIEW_RS_DATA_ROOT` / `PHOTOREVIEW_RS_ISOLATE_CONFIG`.
- Separate single-instance prefix `PhotoReviewRs` and exe `PhotoReviewRs.exe`.
- File **formats** (config.json, operations.jsonl, session files) stay identical to C# so oracle tests can diff them.
- First run may import the C# `config.json` **read-only**; nothing is ever written back to the C# folder.

## Consequences

- Undo/Recovery in one app does not see the other app's actions; the user must finish pending recovery in the app
  that started it. To be stated in the release notes.
- Disk caches are not shared (more disk use, one cold start per app); accepted for safety.
- Guarded by `pr_core` tests `new_never_points_into_csharp_app_root` and `instance_prefix_differs_from_csharp_prefix`.

# Task on progress

**Updated:** 2026-10-14 (session ended for token reasons; this branch `wip/wave2-contracts` holds the handoff)

## State
- master: Wave 0 + Wave 1 (22 units, 498+ tests) + sync to C# baseline d92d6aae (#6: W1 deltas BottomThird,
  MiddleClickAction, FitWidth2/Refresh, WebP/HEIC; abstractions ShellIntegration/UiDispatcher/SettingsTarget/
  try_delete_empty_directory; assets + 31 goldens regenerated). All merged, CI green.
- This branch: `contracts/drafts-w2/` = raw Wave 2 contract drafts (20 units W2-01..20 + W2-00 shared notes) from 4
  Sonnet writers. NOT reviewed, NOT wired. The workflow may not have finished every group.

## Update 2026-10-14 (later)
- Workflow wave2-missing-contracts done: all 21 drafts (W2-00..20) now have `contract.md`, each re-verified against
  C# d92d6aae by a second agent. W2-13 still has no golden-reference (oracle/inputs only). Not yet wired/run/gated.
- Lead decisions still needed before wiring: SettingsTexts impl location (W2-01/W2-04), ownership of per-property
  *SettingsTests (W2-02 vs W2-03), merge order W2-00 -> W2-01 -> W2-20 -> W2-02/03, io/mod.rs owner (W2-14/W2-15),
  local time source for W2-16, HTTP transport for W2-18, `sha2` dependency (W2-17), oracle nondeterminism (W2-14).

## Next (lead)
1. Review each `contracts/drafts-w2/W2-xx/contract.md`: porter model, "Depends on", open questions; decide and add
   lead decisions D12+ and a Wave 2 roster (dependency order) to contracts/README.md.
2. Move contracts to `contracts/W2-xx.md`; copy stubs to their target paths; add mod lines (settings, catalog,
   file_actions, session, io, diagnostics, updates, instance; pr-l10n catalog/loader/localizer + build.rs).
3. Copy `oracle/W2-xx.cs` into tools/oracle-cs/Commands, add dispatch lines; copy golden-inputs; run
   `tools/oracle.ps1 -Commit d92d6aae`; diff against golden-reference.
4. Gate (`tools/wave.ps1 -Wave 2 -Gate`), PR, auto-merge. Then delete `contracts/drafts-w2/`.
5. Run Wave 2 porting in DEPENDENCY ORDER (lessons in docs/MULTI-AGENT-PLAN.md §10), with a .NET harness in verify.
6. Phase 0 still open: UI spikes (R0001; note C# moved to Win32 + Direct2D, ADR 0010), COM Explorer spike.

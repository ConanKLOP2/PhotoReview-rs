# G31 restore verb helper wiring (recycle-policy L1), evidence 2026-11-03

Status: checked, still unwired on master `eea07d2`. No code changed, no behaviour changed. Row P-03 stays Partial. Nothing was run natively (no real Recycle Bin, no Shell verb, no COM).

## Evidence (Rust, master eea07d2)

- `crates/pr-platform-win/src/recycle_bin_shell.rs:34` imports only `restore_verb::CANONICAL_VERB_Z as RESTORE_VERB`; `invoke_restore` (:151-166) binds `IContextMenu` and sets `lpVerb` to `undelete`. No `QueryContextMenu`, no HMENU, no verb names are read anywhere in `crates/**/src` (grep).
- `find_index`, `invoke`, `is_restore_name` of `restore_verb.rs` have no non-test caller (grep). Only `tests/recycle_restore_selection.rs` calls them.
- Difference from the L1 text: `restore_verb` is now `pub mod restore_verb` in `lib.rs:45` (FIX-RECYCLE-TRUNG, DV-16) and the tests import it from the crate. That part of L1 is stale; the wiring part is not.
- The module doc of `restore_verb.rs:1-6` already states that the localized helpers are not wired and why.

## Evidence (C#, baseline b6847767)

- `Shell/RecycleBinShell.cs:87-88`: `RestoreVerb.Invoke(verbs.Names, verbs.InvokeByIndex, verbs.InvokeCanonical)`.
- `ComRecycleVerbs` (:218-292): temporary HMENU, `IContextMenu::QueryContextMenu`, names read with `GetMenuItemInfo`, invoke by id offset, canonical `undelete` only if no name is recognised.
- So C# selects by localized label first; Rust is canonical-only. Same observable result on any Shell that accepts `undelete`; different route when a recognised label exists and canonical fails.

## Why it was not wired in this lane

Wiring is not a pure internal connection. It needs new `unsafe` COM/Win32 code (CreatePopupMenu, QueryContextMenu, GetMenuItemInfoW, DestroyMenu, id-offset invoke) and RAII for the HMENU. Its correctness (menu id range, flags, label text on a real Shell) can only be proven against the real Shell, and the allowed checks here are fakes only. Rule for this lane: do not change behaviour; propose instead.

## Proposal row (numbered from P-160; for the owner to add to docs/PROPOSED-DECISIONS.md)

| P-160 | 2026-11-03 | Open question, nothing changed (BACKLOG G31, G31 evidence `docs/reviews/G31-RESTORE-VERB-2026-11-03.md`): the Rust Recycle Bin restore (`recycle_bin_shell.rs` `invoke_restore`) invokes only the canonical verb `undelete`; the C# adapter first builds a temporary HMENU, runs `QueryContextMenu`, picks the first label recognised by `RestoreVerb` and invokes it by index, falling back to `undelete` (`RecycleBinShell.cs:87-88, 218-292`). The helper `restore_verb::{find_index, invoke}` is tested but has no production caller. Options: (A) wire it: add an `IRecycleVerbs`-like seam in `recycle_restore`/`recycle_bin_shell` with a COM implementation (HMENU + `QueryContextMenu` + `GetMenuItemInfoW`, RAII for HMENU and menu), fake-seam tests for index route, canonical route and no second attempt after a failure, then one own-item native check on the real bin (P-03 stays Partial until that check is run); (B) accept canonical-only with an ADR and a `Different` row, and either keep the helper as dead tested code or delete it and its tests; (C) leave as is (current state, row stays Partial). Recommendation: A only together with a native own-item check; otherwise C. |

## Proposed wording for the owner-edited files (not edited here)

- BACKLOG G31 line: "Checked 2026-11-03: helper still unwired; Rust is canonical-only, C# uses HMENU+QueryContextMenu; decision in P-160; evidence G31-RESTORE-VERB-2026-11-03.md."
- PARITY P-03: no status change (Partial). Optional addition: "restore verb: canonical `undelete` only, localized-label helper tested but not wired (P-160)."
- recycle-policy L1: note that `restore_verb` is in `lib.rs` since FIX-RECYCLE-TRUNG; only the missing production caller remains.

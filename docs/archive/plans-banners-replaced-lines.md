# Lines replaced by the plan-banner cleanup (2026-10-26)

Moved verbatim from the files named below by the docs cleanup lane `plans-banners` (branch `docs/cleanup-plans`, base `origin/master` 8b93fbf).
Each original line below was replaced in its file by a corrected line or heading; the original is kept here with the reason and the evidence.
Nothing else was removed from those files: the status banners, notes and "Current state" sections were added.

## `docs/PLAN.md`

Reason and evidence: UI row of the Stack table. P-06 (docs/PROPOSED-DECISIONS.md), the Phase 0 line of this same file ("Spike UI Win32 + Direct2D (P-06)") and the header of docs/adr/R0001-ui-framework.md say the direction is Win32 + Direct2D and that egui/Slint were not spiked; the row said egui was proposed and a spike would decide.

Original lines, verbatim:

```text
| UI | WPF | eframe/egui + wgpu (đề xuất), Slint là phương án khác; chốt bằng spike ([R0001](adr/R0001-ui-framework.md)) |
```

## `docs/WAVE-9-PLAN.md`

Reason and evidence: The long line says the file "carries no unit rows" although section 1 holds one row per W9 unit; the new line says what the naming guarantees (planfor never selects this file). The two headings of sections 4 and 6 gained the words "snapshot, superseded by section 0" and "superseded: docs/MULTI-MACHINE.md now assigns both W9 lanes to machine A" (ownership table rows of #232 and #241); their bodies are unchanged.

Original lines, verbatim:

```text
**The two lane plans stay the source of the unit tables.** `tools/autopilot/lib/codex-lib.py planfor` selects a plan by listing `docs/WAVE-9-PLAN-*.md` files that contain rows `| W9-<lane><n> |`, so those files must not be merged, renamed or deleted; this file is named without the dash suffix on purpose and carries no unit rows. C# spec: `7eaee2a2` (feature commit `d9623945` for lane E; `55a37986`, `9800058e`, `170a2d19` for lane R).
## 4. State of the Wave 8 dependencies (from `git log origin/master`, 2026-10-23)
## 6. Machine split (proposal for the lead; the ownership table in `docs/MULTI-MACHINE.md` is not changed here)
```

## `docs/WAVE-10-PLAN.md`

Reason and evidence: Heading of section 4 gained the words "snapshot, superseded by section 0"; the body is unchanged.

Original lines, verbatim:

```text
## 4. State of the Wave 8 dependencies (from `git log origin/master`, 2026-10-23)
```

## `docs/adr/README.md`

Reason and evidence: The list omitted three accepted records that exist in this directory (ADR-INV, ADR-I-07, ADR-S3); the new line adds them and points to the status table.

Original lines, verbatim:

```text
Accepted records: `D59-decode-error-inner-hresult.md`, `D60-raw-orientation-test-helper.md`, `R0002-coexistence-with-csharp-app.md`, `R0003-concurrency.md`; `R0001-ui-framework.md` is proposed (see its header).
```

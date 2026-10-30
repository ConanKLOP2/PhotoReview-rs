# CX-PAINTER: bounded pure-logic mutation and hidden-window evidence

Lane `feat/cx-painter`, starting HEAD `21dee8fdd20790c38747f302c6553614cdcdd791`.
Work is confined to this worktree; the orchestrator owns add/commit/push.
No production change or dependency change is planned. Results below are local
evidence, not a workspace, attended desktop, or full-port verdict.

## Read-first reconciliation

Read AGENTS.md mutation rules (lines 126-136), LESSONS.md, current status,
the six lane rows, CX-DIALOGS, CL-DLG and CX-RELEASE dated 2026-10-30.
`CL-STARTUP-2026-10-30.md` does not exist in this checkout; the available
`CX-STARTUP-2026-10-30.md` was read instead. This is recorded as a missing
requested input, not silently treated as the same report.

Do not repeat the completed CX-DIALOGS groups: `paint_scale` 5/5 caught,
`frame_rects` 9/9 caught, `border_layers` 5 caught, 1 equivalent, 1 unviable.
The equivalent `edge > 1` -> `>= 1` adds only an empty `1..1` loop.
CL-DLG covers Settings files outside this lane, not native painter mutation.

## Row map and retained boundaries

| Row | Source and covering binaries | Boundary |
|---|---|---|
| X-dialogs-painter | `dialogs/native/painter/{mod,geometry}.rs`; `dialogs_painter_geometry`, `dialogs_painter_attach` | Raw subclass/window/property operations remain glue; font/DPI pixel variation remains |
| X-chrome-painter-controls | `painter/controls.rs`; `dialogs_painter_controls` | Real-DC text, buffering and painting are glue; missing 144-DPI and empty-rectangle fill evidence remains |
| X-chrome-painter-widgets | `painter/widgets.rs`; `dialogs_painter_widgets` | Other pure layouts and raw checkbox/radio/combo/progress/trackbar painting remain separate evidence requirements |
| X-chrome-painter-frame | `painter/frame.rs`; `dialogs_painter_frame` | Other layout functions and GDI painting; empty client areas prevent native etched-horizontal/vertical pixel assertions |
| X-dialogs-native-base | `native/{template,chrome,edit_centering,list_dialog,controls/*}.rs`; `dialogs_native_template`, `dialogs_native_chrome`, `dialogs_native_edit_centering`, controls/list-dialog binaries | Full base inventory and native gallery/modal/leak scenarios are not exhausted |
| X-dialogs-native-dialog | `native/dialog/*`; `dialogs_native_dialog_{core,events,layout,close,modal,events_native,layout_native}`, lib tests | Remaining pure functions, HWND/DC glue and modal/cursor/DPI/lifetime scenarios are not exhausted |

## Covering tests

Normal target is `<worktree>/target`, with `CARGO_INCREMENTAL=0`,
`CARGO_PROFILE_DEV_DEBUG=line-tables-only`, build jobs 2. Config isolation is
`PHOTOREVIEW_RS_ISOLATE_CONFIG=1` and a data root beneath target. No real user
config, C# application data or Recycle Bin is used.

`powershell -NoProfile -ExecutionPolicy Bypass -File tools/dev/stage-native.ps1`
verified both DLL hashes against the C# baseline and staged them to this
worktree's target/debug, target/debug/deps and native/x64. Git ownership required
process-local safe.directory entries for the worktree and read-only C# checkout;
global configuration was unchanged.

The following binaries ran with `--include-ignored --test-threads=1`, under
600/900-second outer guards. Painter native fixtures use existing hidden owner
windows/memory DCs; no new visible GUI workflow was introduced.

| Binary | Passed | Failed | Ignored |
|---|---:|---:|---:|
| dialogs_painter_geometry | 17 | 0 | 0 |
| dialogs_painter_attach | 8 | 0 | 0 |
| dialogs_painter_controls | 18 | 0 | 0 |
| dialogs_painter_widgets | 17 | 0 | 0 |
| dialogs_painter_frame | 13 | 0 | 0 |
| dialogs_native_template | 11 | 0 | 0 |
| dialogs_native_chrome | 18 | 0 | 0 |
| dialogs_native_edit_centering | 3 | 0 | 0 |
| dialogs_native_dialog_core | 11 | 0 | 0 |
| dialogs_native_dialog_events | 6 | 0 | 0 |
| dialogs_native_dialog_layout | 18 | 0 | 0 |
| pr-platform-win lib (includes tests outside the lane) | 253 | 0 | 0 |

Logs: `target/painter-tests.log`, `target/base-tests.log`. The lib run is
reported separately and does not establish complete native-dialog coverage.
Modal, cursor and gallery integration binaries were not run; no attended
desktop or hardware verdict is claimed.

## Mutation method

One owned cargo-mutants process at a time, `CARGO_TARGET_DIR=<worktree>/target-mut`,
`--baseline skip --timeout 90 -j 1`, `--file` plus function `-F`, outputs
`<worktree>/mut-out-GROUP`. Normal cargo tests never use target-mut. Each file
has a 1200-second outer budget; incomplete groups must remain incomplete.

The initial `--list` inventory has **1,728 identities**, including excluded
Win32/GDI glue, across native chrome/template/edit-centering/list-dialog,
controls/*, dialog/* and painter/*.rs. This is not a count of pure candidates.
Local inventory: `target/painter-inventory.txt`. Geometry has 129 identities,
painter controls 116, widgets 266, frame 90, chrome 50. File totals include
both replacements and deletions, including unnamed constant-expression mutants. Historical completed
groups above are not counted as fresh runs.

The first shell invocation lost regex quoting through Windows PowerShell's
native argument conversion and produced no mutation verdict. Subsequent calls
use a shell script under target to preserve exact arguments. Infrastructure
attempts are not counted as tested mutants.

## Geometry bounded run and independent killer proof

`target/list-geometry-main.txt` and the run both report **58 selected**:
`is_full_paint|has_flat_border|tracks_hover|repaints_after_system_paint|control_message_action|::color$|adjust_custom_draw`.
The `::color$` alternative matches no identity in this cargo-mutants version;
the two `color` mutants are **not run**. The selected count is nevertheless
checked against the actual listing, not inferred from the intended function list.

The 1200-second guard interrupted the run. Raw outcomes report 43 entries:
40 caught, 0 missed, 3 labelled unviable, 0 test timeouts. Inspection separates
these into **40 caught, 2 genuine compile-unviable, 1 interrupted build**.
The two genuine failures are E0277: `ControlMessageAction` and
`CustomDrawAdjustment` have no `Default`. The third, `geometry.rs:398:14`
(`==` -> `!=`), ends with rustc exit **143**, so it is **not** a valid
compile-unviable or caught verdict. `outcomes.json` retains `end_time: null`.
Fifteen other selected identities were never attempted in that first run.
There are no observed survivors from that run.

The initial `w8_painter_killers.rs` has two pure tests, no clock, sleep, HWND, DC or
filesystem access. The independent C# reference is read-only
`7eaee2a2:src/PhotoReview.Shell.Win32/Dialogs/Native/NativeDialogChromePainter.cs`,
`AdjustListViewCustomDraw` (lines 340-366): preserve the dialog's flags with
`result | CdrfNewFont`; query selection only for an item prepaint.
The NEWFONT case supplies an already-set bit and unrelated flags; the second
test supplies invalid draw stages and a closure that panics if queried.
These are proactive inventory guards, not claimed historical MISSED results.

The exact `geometry.rs:413:28` mutant (`|` -> `^` in `adjust_custom_draw`)
was listed separately (**1 Found**) and run against **the killers binary alone**:
**1/1 caught**, 0 missed/unviable/timeouts, 42 seconds
(`mut-out-geometry-killer`). This adds one fresh identity from the original
15 unattempted entries. The main discovery budget was 20 minutes; the focused
proof adds 42 seconds and is reported separately.

## Push-button chunk and equivalent survivors

`push_button_draw\b`, `dialogs_painter_controls`: listing and Found both **30**,
**30/30 tested: 24 caught, 5 missed, 1 compile-unviable, 0 timeouts** in 9 minutes
(`mut-out-controls-push`). E0277 for `PushButtonDraw::default()` is the unviable.
The five raw MISSED outcomes remain in `missed.txt`; all are equivalent:

| controls.rs location | Operator | Why the result cannot change |
|---|---|---|
| 414:19 | `|` -> `^` | `DT_CENTER = 1` and `DT_VCENTER = 4` have no overlapping bits |
| 414:32 | `|` -> `^` | `(1 | 4)` and `DT_WORDBREAK = 0x10` have no overlapping bits |
| 416:19 | `|` -> `^` | Same `1` and `4` pair in the single-line branch |
| 416:32 | `|` -> `^` | `(1 | 4)` and `DT_SINGLELINE = 0x20` have no overlapping bits |
| 416:48 | `|` -> `^` | `(1 | 4 | 0x20)` and `DT_END_ELLIPSIS = 0x8000` have no overlapping bits |

Constants are defined in `crates/pr-platform-win/src/win32/dialog_native.rs:69-73`.
For disjoint bits `a ^ b == a | b`; the operands are constants, independent of
the input flags. No test can distinguish these five mutations. They are not
relabelled caught, and no production change or artificial killer is made.
The complete selected function verdict does not exhaust controls.rs: 86/116
file identities were not run fresh, including the historical `frame_rects` group
and excluded DC/GDI glue. The 144-DPI and empty-rectangle fill gaps remain.

## Widgets chunk

`progress_fill_width\b`, `dialogs_painter_widgets`: **9 listed / 9 Found / 9
caught**, 0 missed/unviable/timeouts, 2 minutes (`mut-out-widgets-progress`).
This is **9/266 fresh file identities**, not a widgets.rs-wide verdict.
`round_even`, `check_layout`, `combo_layout`, `trackbar_layout`, `tick_xs` were
not mutated in this lane; real-DC widget rendering remains glue. The native
widgets suite above passes, but neither that nor this single function closes
the remaining pure mutation inventory. X-chrome-painter-widgets is therefore
reconciled from the older Done-with-no-mutation-claim status to **Partial**.

## Frame chunk

`border_thickness\b|header_filler_rects\b`, `dialogs_painter_frame`: **22 listed /
22 Found / 22 caught**, 0 missed/unviable/timeouts, 4 minutes
(`mut-out-frame-pure`). Of these, 19 belong to `border_thickness` and 3 to
`header_filler_rects`. This is **22/90 fresh file identities**. The earlier
7-identity `border_layers` result is retained as historical evidence, not rerun.
`group_box_layout`, `etched_shape` and raw GDI painting were not mutated here;
GDI painting is excluded glue. Empty client rectangles of horizontal/vertical
etched statics still prevent those native pixel assertions. Frame stays Partial.

## Native-base chrome seams

`DarkDialogChrome.*apply_controls|theme_for_control`, `dialogs_native_chrome`:
**8 listed / 8 Found / 8 caught**, 0 missed/unviable/timeouts, 78 seconds
(`mut-out-chrome-seams`). Three replacements of `theme_for_control` and five
mutations of `DarkDialogChrome::apply_controls` cover theme selection, forwarding,
combo-list and list-view-header attachment and zero-handle refusal through the
existing recording painter/OS/theme seams. No HWND/DC is needed for these tests.
This is **8/50 fresh chrome.rs identities**, not a base-wide mutation result.
The additional DialogColors/conversion chunk is recorded below; other chrome
glue and the other native-base files remain separate inventory. The former PARITY `Stubs todo!()` note is stale:
`git grep` found no `todo!()` or `unimplemented!()` under dialogs/native. It is
replaced by the actual mutation/native boundaries, not a Done claim.

## Geometry interruption reproof

The interrupted `geometry.rs:398:14` (`==` -> `!=`) and the separate
`geometry.rs:411:14` (`&` -> `|`) were selected by exact identity (2 listed /
2 Found) and both caught by **the killers binary alone**, in 22 seconds
(`mut-out-geometry-reproof`). This resolves the initial infrastructure result
and independently proves the second stage-selection guard.

Geometry now has **45 unique valid fresh outcomes among the 58 selected**:
43 caught, 2 genuine unviable, 0 unresolved missed/test timeouts, 13 selected
identities not run. The initial exit-143 attempt is retained in its raw log but
not counted twice or as compile-unviable. The whole file has 129 identities;
**84 were not run fresh**, including the historical paint_scale group.
The 20-minute discovery guard plus focused proofs used another 64 seconds.

## Native-dialog events

Three named pure functions, 23 listed / 23 Found on every invocation:
`command_route`, `split_command_wparam`, `class_name_is`.

| Binary | Tested | Caught | Missed | Unviable | Timeout |
|---|---:|---:|---:|---:|---:|
| dialogs_native_dialog_events | 23 | 22 | 0 | 1 | 0 |
| --lib | 23 | 0 | 22 | 1 | 0 |
| w8_painter_killers alone | 23 | 22 | 0 | 1 | 0 |

Logs: `mut-out-dialog-events`, `mut-out-dialog-events-lib`,
`mut-out-dialog-events-killers`. CommandRoute has no Default (E0277), so its
body replacement is the single genuine unviable. The --lib survivors are a
binary-selection gap: the existing integration binary already catches all 22.
Three new pure oracle tables independently catch the 6 boolean routing mutants
(events.rs:58/60), 14 tuple/mask/shift mutants (:69) and 2 class-name constant
returns (:74). These are no production bug and no equivalent survivors.
NativeDialog.HandleCommand at C# 7eaee2a2 was read to confirm BN_CLICKED routing.
Only 23/47 events.rs identities were run fresh; native event glue remains out
of scope. Repeated runs of these same 23 identities count once in lane totals.

## Native-base colours, template and edit-centering

| Chunk | Listed / Found | Discovery caught | Missed | Unviable | Timeout |
|---|---:|---:|---:|---:|---:|
| DialogColors (27 identities in chrome.rs) | 27 / 27 | 25 | 2 | 0 | 0 |
| Native template functions (23 of 37 file identities) | 23 / 23 | 21 | 1 | 1 | 0 |
| padding_for_metrics and band_rect (12 of 51 file identities) | 12 / 12 | 12 | 0 | 0 | 0 |

Directories: `mut-out-chrome-colors` (4 minutes), `mut-out-native-template`
(6 minutes), `mut-out-edit-centering` (2 minutes). Template's body replacement
of `DialogItem::with_ex_style` requires a nonexistent Default (E0277).

Two template alignment mutations (`template.rs:116:25`, `%` -> `/` and `+`)
are tool-classified caught **by allocation abort**, not by a value assertion:
the mutated padding loop grows indefinitely, then allocation of 17,179,869,184
bytes fails and the test process exits with STATUS_STACK_BUFFER_OVERRUN.
Both raw logs are retained. No repeat or killers-only assertion claim is made
for these two identities. Normal production padding uses remainder and terminates.

The real colour survivors are `chrome.rs:88:20/40`, `|` -> `^` in ToColorRef.
C# `7eaee2a2:src/PhotoReview.Shell.Win32/Dialogs/Native/NativeDialogChrome.cs`
was read: channels are not clamped. Therefore these are not equivalent for
out-of-range public helper inputs. Fixed expected COLORREFs use red/green
overlap `(2,1,0) -> 0xFFFE` and green/blue overlap `(0,512,1) -> 0x1FF0000`.
The latter is necessary because Rust parses `r | g ^ b` as `r | (g ^ b)`.
The first version of the killer used red/blue overlap and missed the right-hand
mutation; its failed proof is retained, and the input was corrected.

The template formatter survivor `template.rs:88:9` is guarded by the literal
C# ArgumentException constructor message. The C# DialogTemplate.cs baseline
was read to confirm it. A focused proof intended to select those three
survivors also lists/runs `template.rs:75:16` (delete ex_style) despite the
coordinate filter: **4 listed / 4 Found**. This first proof has 2 caught and
2 missed (right-hand XOR plus ex_style). An independent builder-field test
was added for that spill-over; it does not weaken the existing template test,
which had already caught ex_style during discovery.
Final reproof follows below; these duplicate runs do not add fresh identities.

## Native-dialog DPI helpers

Eight named static helpers in dpi.rs (scale_dlu_x/y, average_char_width,
effective_dpi, fallback/header_font_height, fallback/header_log_font):
**37 listed / 37 Found / 37 caught**, 0 missed/unviable/timeouts, 6 minutes
(`mut-out-dialog-dpi`, binary `dialogs_native_dialog_layout`). The file has
71 identities; 34 were not run fresh. No inline unit tests exist in this file.
Native font creation, measured OS metrics and window DPI transitions are
separate glue/native evidence, not established by these pure font structures.

Additional chunks and final gates follow after the sequential runs.

## Restart reconciliation and final local evidence

The restart reused all completed groups above and inspected the unreported
`dialog/layout.rs` and `dialog/focus.rs` chunks. The layout selection lists and
Found agree on **15** (`button_width_for`, `button_row_total`,
`button_row_positions`, `work_area`, `initial_frame`), all caught by
`dialogs_native_dialog_layout`. The focus selection lists and Found agree on
**5**, all caught by `dialogs_native_dialog_events`. These are retained previous
session results, not fresh reruns.

The previous `widgets-rest` selection was **176** identities. Its valid results
are **62: 58 caught, 3 missed, 1 compile-unviable** (CheckLayout lacks Default).
The other **114 are infrastructure failures**, not compile-unviable: 113
cargo launches failed with -1073741502 (0xC0000142); one rustc launch in the
build failed with STATUS_DLL_INIT_FAILED. The report's earlier note saying these
functions were not mutated is superseded by this audited partial chunk.
`target/restart-outcome-audit.json` retains every raw result and deduplicates by
full mutant identity. The original `target/outcome-audit.json` is preserved.

### Restart independent proof

`mut-out-restart-killers`: **5 listed / 5 Found / 5 tested**, **4 caught,
1 missed, 0 unviable/timeouts**, 7 minutes. Only `w8_painter_killers` runs.
The four caught are chrome.rs:88:20/40 XOR and template.rs:75:16 field deletion
and :88:9 formatter replacement, resolving the prior failed proof. The fifth
is widgets.rs:230:38 (`-` -> `+`), subsequently guarded by the extreme-width
killer described below. A proposed ordinary-width checkbox killer
passed without discriminating this mutant and was removed; only the later
extreme-width case claims a discriminating proof. No production code changed or production bug was found.

A ninth retained test guards dialog PREPAINT: an existing NOTIFYITEMDRAW bit
must stay set, SKIPDEFAULT suppresses requests, selection must never be queried.
It supplements the two original custom-draw guards for the pending geometry
chunk; an eleventh test checks the missing-key magenta sentinel. Exact results
are recorded in the restart table below.

### Equivalent widgets survivors

- widgets.rs:224:20 and :224:36 (`|` -> `^`): DT_VCENTER=4,
  DT_SINGLELINE=0x20 and DT_END_ELLIPSIS=0x8000 are disjoint constants, so XOR
  and OR produce identical flags for every input.
- widgets.rs:230:38 (`-` -> `+`) is NOT equivalent over the public helper's
  representable inputs. Normal-sized rectangles clip the changed intermediate,
  but width=i32::MAX, left=19 and measured width=12 keeps all original
  arithmetic in range while the mutant's right+left overflows. The added killer
  requires the literal focus rectangle (18,8,33,22). In debug mutation builds,
  only the mutant panics. No real window or large allocation is used.
  C# baseline `NativeDialogChromePainter.cs:725-746` was read to confirm
  subtraction before Math.Min and the outer clipping formula.

The five push-button XOR equivalents above remain raw MISSED. Equivalents
are not counted as caught. Any later frame survivor and the checkbox proof are reconciled in the restart table below.

### Audited combined lane inventory

This combines retained previous-session runs and the restart, counting each
identity once. It does not include completed CX-DIALOGS history as fresh work.
The full inventory contains Win32/DC/window glue and is not a pure-candidate
denominator. Infrastructure-only identities are NOT successful tests.

**352/1728 unique identities have valid outcomes**: 337 caught, 8 raw missed, 7 compile-unviable, 0 test timeouts. **1376 identities have no valid verdict** (including 115 infrastructure-only identities); none is called caught or unviable.

| File | Inventory | Valid unique | Caught | Raw missed | Compile-unviable | No valid verdict |
|---|---:|---:|---:|---:|---:|---:|
| `chrome.rs` | 50 | 35 | 35 | 0 | 0 | 15 |
| `dialog/dpi.rs` | 71 | 37 | 37 | 0 | 0 | 34 |
| `dialog/events.rs` | 47 | 23 | 22 | 0 | 1 | 24 |
| `dialog/focus.rs` | 20 | 5 | 5 | 0 | 0 | 15 |
| `dialog/layout.rs` | 208 | 15 | 15 | 0 | 0 | 193 |
| `edit_centering.rs` | 51 | 12 | 12 | 0 | 0 | 39 |
| `painter/controls.rs` | 116 | 30 | 24 | 5 | 1 | 86 |
| `painter/frame.rs` | 90 | 41 | 39 | 1 | 1 | 49 |
| `painter/geometry.rs` | 129 | 60 | 58 | 0 | 2 | 69 |
| `painter/widgets.rs` | 266 | 71 | 68 | 2 | 1 | 195 |
| `template.rs` | 37 | 23 | 22 | 0 | 1 | 14 |

All other files in the 1,728 inventory have no valid fresh result in this lane.
This includes controls/*, list_dialog.rs and the remaining dialog/* raw
window/COM/DC glue. In painter/mod.rs, attach/property/subclass/OS lifetime
operations and is_list_view's window-class query are native glue, not pure
predicate targets. Existing hidden-window attach evidence above is retained;
no visible/modal/cursor-driven test was launched during the restart.
Remaining pure candidates include kind_for_style/from_value/ui_state_from_flags/
scaled in geometry; widgets layout/tick functions after the interrupted
selection; and unselected base/dialog helpers. They are explicitly unverified,
not silently classified as glue. The scoped restart does not exhaust this inventory.

### Restart chunks (not counted as distinct identities when rerun)

| Group | Listed / Found | Valid outcomes | Caught | Raw missed | Compile-unviable | Test timeout | Infrastructure |
|---|---:|---:|---:|---:|---:|---:|---:|
| `mut-out-restart-killers` | 5 / 5 | 5 | 4 | 1 | 0 | 0 | 0 |
| `mut-out-restart-frame` | 36 / 36 | 19 | 17 | 1 | 1 | 0 | 1 |
| `mut-out-restart-geometry` | 16 / 16 | 16 | 16 | 0 | 0 | 0 | 0 |

Frame used a 900-second outer guard, in addition to its earlier four-minute chunk.
GroupBoxLayout has no Default (E0277). The interrupted frame build (exit 143) is
infrastructure, not compile-unviable. Frame :159:53 XOR is equivalent:
DT_SINGLELINE, DT_END_ELLIPSIS and the optional DT_HIDEPREFIX have disjoint bits.
The same reasoning applies to either OR in this constant flag expression.
Remaining frame candidates are unrun after the guard; no complete
group_box_layout/etched_shape verdict.

### Closed selected geometry group and remaining limits

The original 58-identity selected geometry group now has **58/58 valid outcomes:
56 caught, 2 compile-unviable, 0 unresolved missed/test timeouts**. The two colour
identities previously missed by the filter are additionally caught. Thus the
geometry file has **60/129 valid lane outcomes**, with **69 not run in this lane**
(including historical paint_scale evidence retained separately). The final
restart group lists and Found both equal 16: these 15 geometry identities plus
the checkbox overflow survivor, all caught by the killers binary alone, in
9 minutes. No inline unit tests exist in geometry.rs or widgets.rs requiring
a second --lib run.

All eight final raw MISSED identities are explained disjoint-bit XOR equivalents:
five push-button, two checkbox and one group-box flag expression. The checkbox
subtraction survivor is **caught**, not equivalent; its prior MISSED attempts
remain in raw logs. Every other observed viable survivor from completed chunks
has an independent killers-only caught result. No production change or bug fix
was needed.

Native evidence above was read back from target/painter-tests.log and
target/base-tests.log during the restart and reused, not rerun. Stage-native
had already verified/staged both pinned DLLs before those runs. No modal/cursor/
visible GUI test was launched in the restart; no new native evidence is claimed.
Raw DC/GDI/window painting remains glue and every lane row stays **Partial**.
Remaining pure candidates in the table are not declared complete, nor reclassified
as glue. Geometry/frame discovery budgets were consumed as recorded; widgets
partial discovery plus focused proof is retained without restarting its
infrastructure-failed tail. Other unselected base/dialog candidates were not
scheduled in this bounded restart.

### Final gates and handoff

- `cargo fmt -p pr-platform-win --check`: pass (target/restart-fmt.log).
- `cargo clippy -p pr-platform-win --all-targets --locked -- -D warnings`: pass on the final source (target/restart-clippy-final.log).
- `cargo test -p pr-platform-win --locked --test w8_painter_killers -- --test-threads=1`: **11 passed**, 0 failed/ignored (target/restart-killers11.log). No timing/wall-clock assertions, so a timing-related 20-repeat gate is not applicable.
- `python -I tools/dev/doc-links.py --quiet`: pass, 497 files, 0 errors; 172 proposed and 42 archive_unchecked are reported separately. No workspace/full-native/full-file mutation gate is claimed.

The orchestrator owns add/commit/push. Tests and evidence are left unstaged in
feat/cx-painter. task_on_progress.md retains one CX-PAINTER line and stays below
8 KiB; inherited verbatim archive edits were preserved. No unsafe code or
unsafe-audit baseline was changed.


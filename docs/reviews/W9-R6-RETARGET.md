# W9-R6 rename view retarget

## Scope and source

C# baseline `7eaee2a2`: `ImagePresenter.RetargetPath` and `IsPresentInFlight`,
`ZoomDetailLoader.RetargetPath`, and MainViewModel's Rename command and
`OnFilesRenamed`. R10 owns the wider dedicated test inventories, and R7/R8 own
prompt/menu/native UI integration. This report does not close those units.

MainViewModel queues Rename through the existing file-action gate and Explorer
order wait. The sink recorder snapshots the synchronous keep decision and
replays the rename map in call order. Session current/skipped paths persist
before the presentation bridge is re-keyed, old cache paths are evicted, the
catalog is notified, and the presenter is retargeted. Rename and Undo completions
use the same sink route.

The presenter re-keys displayed identity before deciding whether the frame can
be kept. Compare-visible and suspended-present paths return false; Compare is
fully cleared with the current format context. A keepable image updates path,
photo-info names and status suffix without stat, decode or another image event.
The suffix boundary follows the oracle's ordinal casing table, including
case-equivalent characters with different UTF-8 byte lengths. An empty rename
map still re-arms zoom detail when the frame can be kept.

Zoom retarget cancels the old request, changes target identity, preserves held
original pixels and a latched failure, and restarts only when zoom requires it.
Stale old-path pixels are rejected. Fake rename fixtures preserve file metadata
so source-identity validation exercises a rename rather than an edited file.

## Suspended present lifecycle

A single current Navigation slot did not match C#: an older suspended present
still counts after a newer present finishes. Retired state retains event
identities without decoded payloads. Thumbnail/preview uses the first relevant
completion; compare preview/hash phases need both remaining sides. Stats,
dimensions and post-frame yields match their specific completion purpose.
Nested retry/missing presents retain parent links, retired iteratively.
Standalone missing removal does not itself count as a PresentAsync call.

Queued stat cancellation formerly dropped the closure without a presenter
completion. A closure-owned guard now reports abandonment without reading the
file, spawning a thread, or labeling it missing. Normal completion disarms the
guard and sends the normal stat result. Panic abandonment is distinguished and
logged. The matching active/retired continuation is retired on the UI pump;
existing `is_idle` semantics are unchanged.

## Evidence and limits

- The 16 named contract scenarios remain: 3 presenter, all 10 dedicated zoom
  retarget scenarios, and 3 MainViewModel rename scenarios.
- Four additional integration regressions cover superseded PostFrame lifetime,
  canceled queued stat with zero filesystem calls, present-owned missing work,
  and standalone missing work. The PostFrame test proves the newer present
  finished through the presented-image metric before releasing the old yield.
- Four lifecycle unit tests cover Any/All waits, exact purposes and a 20,000-link
  iterative parent chain. Five suffix unit tests cover exact and Unicode case,
  mismatch, prefix preservation and empty suffix.
- Owner focused checks: 20 rename + 50 presenter A + 58 presenter B + 14 compare
  + 4 lifecycle tests passed (146). After the narrow suffix correction, 5 suffix
  and 20 rename tests passed; package fmt and locked all-target Clippy passed.

Lead final source gates at 35859e1c, containing master cf325668: format passed through the 12-package Windows command-length fallback; locked workspace all-target Clippy passed; locked workspace tests passed with 10041 passed, 0 failed, 1336 ignored (571 result lines). Pre-pr passed; its downstream workspace Clippy skip is covered by the independent full workspace invocation. Only evidence/status documentation changed after these source gates. Selected postmerge pure-logic mutation remains pending. No native window, GPU, NAS,
RAW-corpus, attended measurement, performance or full mutation verdict is
claimed. Temporary files belong to each fixture and the bin is fake.

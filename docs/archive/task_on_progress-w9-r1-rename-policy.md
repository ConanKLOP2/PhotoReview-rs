# R1 rename policy history

Moved verbatim from the R1 branch on 2026-10-26. History only; current state is in [task_on_progress.md](../../task_on_progress.md).

## Wave 9 R1 verified root gate, machine A, 2026-10-26 13:02

- Runner17764 completed exit0 on integrated source8161e88: fmt-all Windows206 with all12package checks PASS, workspace all-target locked Clippy-Dwarnings PASS, workspace9904passed/0failed/1328ignored across566resultlines. This is authoritative completion of that runner; earlier active-runner notes are historical.
- E9 PR256 subsequently merged82b8418 and master079a28d adds tooling/docs only. Native-input fixture21ff633 completed9862/0/1329 separately, then integrated079a28d at74109b6 and is running final gates/native shared-fixture regression. Finish its PR first, then integrate merged master into R1 and run final head gates once before PR; no stacked PR. W9-R1 A remains claimed and no R2 edit/claim before R1 merges.
- R1 has53focused G/F functions,44unchanged oracle cases, shared PathKeyMap and ADR-R-12. No native filesystem claim. After merge, time-box narrow pure-policy cargo-mutants with-j1 in a clean checkout and an outside unique target; untracked target-w9-e1 must not be copied into the mutation tree. No full mutation verdict yet.

## Wave 9 R1 integrated root repair, machine A, 2026-10-26 09:35

- Final pre-root head4edc4dd completed fmt12/Clippy/workspace:9903passed/0failed/1328ignored across566result lines, exit0. Existing53pure policy/map/group functions include44unchanged oracle cases. No native filesystem or full mutation claim.
- Integrated merged root repair aa119f4 after terminal runner51008; retained both task histories and PARITY rows, including D-rename-blank-source/ADR-R-12. Rerun final integrated gates before PR. W9-R1 claim stays held until merge and selected mutation follow-up; R2 must not start before R1 is on master.

**Updated:** 2026-10-23 (older entries moved to `docs/archive/`; the sections below are the latest ones; where two sections disagree the later date wins, not the later position)

## Wave 9 R1 rename policy implementation, machine A, 2026-10-26

- Claim W9-R1 held A; starts from merged roster/master57ec6e9. Replaced policy/plan stubs using C#7eaee2a2, preserving UTF-16 units, exact extension spelling, first invalid unit, reserved devices, conditional path budget and XMP ownership. No filesystem effects.
- Added one shared PathKeyMap with .NET OrdinalIgnoreCase lookup, first key spelling/order retention and indexer replacement. Propagated its type through downstream lead-owned signatures without implementing their units. ADR-R-12 records blank-source rejection instead of a user-data panic; the null group branch is excluded by the Rust reference type.
- Evidence: initial plan tests failed at the original todo; after implementation, both oracle tests match all44cases (33single/11group). Final focused suite passes53test functions (34policy/map,19group), including all44oracle cases, individual named flag cases, UTF-16 supplementary limits and RAW/XMP-only rejections. C# null group is type-excluded. Merged master75ceca6 including E5 in finalsourcef34c36f. All12package fmt PASS (fmt-all Windows206); final workspace Clippy PASS; workspace tests on sourcef34c36f exited0:9898passed/0failed/1325ignored across565resultlines, TEMP/w9-r1-final-test-*. That gate predates new E8/E9 and current tooling master; integrate merged E-lane changes and verify final head before R1PR. E8PR254 merged0fee72c and is integrated into R1 with both task sections/PARITY rows retained; E9 has selected native4/4rows and four new wiring tests, its workspace gate is active in its own root. Postmerge time-boxed cargo-mutants remains pending. No full-port/native/corpus verdict.
---

End of preserved sections.

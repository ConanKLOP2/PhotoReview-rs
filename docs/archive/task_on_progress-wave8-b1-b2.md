# task_on_progress archive: Wave 8 batches B1 and B2 contracts

Moved verbatim from `task_on_progress.md` (DOC-PROGRESS-TRIM, machine C). Wave 8 B1 (2026-10-18) and B2 (2026-10-19) contract PR entries.
The text below is historical: states, paths and next steps are those of the date in each heading, not current.
Current state: `../../task_on_progress.md`.

## Wave 8 batch B1 contracts PR (2026-10-18, branch `wave8/b1-contracts`, based on master `cf9888b`)
- Wired: 35 contracts (`contracts/W8-01a.md` .. `W8-16e.md`), stubs in `crates/pr-app/src/` (`input`, `viewport`, `menus`, `viewmodels`, `windowing`, `localization`, `services`, `misc`, `coordinators`, `presenters`, `overlay`, `viewing`) and `crates/pr-platform-win/src/shell_viewing/`, 22 oracle files (`tools/oracle-cs/Commands/W8-*.cs`) with 49 dispatch lines plus `render-geometry`, 50 goldens (`tests/golden/`, inputs in `tests/golden-inputs/`), roster and decisions D-W8B1-1..12 in `contracts/README.md`, PARITY rows, proposal P-39.
- Oracle: `tools/oracle.ps1 -Commit d92d6aae` now also builds `PhotoReview.App.Shared`, `Shell.Interop`, `Shell.Rendering` and `Shell.Win32` from the baseline archive; `OracleCs.csproj` references them and compiles the internal baseline files via `-p:CSharpSrcDir`. All 50 commands ran twice; the two runs are byte-identical; no pre-existing golden changed; no command was dropped. Input and oracle bugs fixed on the way are listed in D-W8B1-7.
- Cross-unit reconciliation done by the lead: overlay element API (15c/15d retargeted to 15a's `ElementNode`), `FullscreenControl` trait deleted, `viewing` split between `pr_app` and `pr_platform_win` (cargo cycle avoided), `KeyId` from 05b, shared type homes (D-W8B1-2, -3, -5).
- Verified on the final head: `cargo fmt --all`, `cargo clippy --workspace --all-targets --locked -- -D warnings` clean, `cargo test --workspace --locked` exit 0 (5465 passed, 0 failed, 431 ignored).
- NOT run: any Rust test of the new units (all stub bodies are `todo!()`; no golden test is written yet, porters add them); `cargo-mutants`; the hand-derived tables in the contracts (for example W8-16e layouts) were not compared with the goldens; the layering PR of D-W8B1-1 (needed before W8-12b); CI result pending at PR time.
- Next: port per P-28 once the direct dependencies merge (start with W8-01a, 05b, 04d, 10c, 16a; then 01b, 01c, 05a, 05c, 02b); add the missing plan edges of D-W8B1-9; do the layering PR before B3 contracts.

## Wave 8 batch B2 contracts PR (2026-10-19, branch `wave8/b2-contracts`, based on master `724394f`)
- Wired: 26 contracts (`contracts/W8-05g.md` .. `W8-10i.md`), stubs in `crates/pr-app/src/` (`input/pointer`, `viewmodels/{compare,main}`, `coordinators/{file_action_gate,sibling_navigator,folder_load,file_actions,duplicate_cleanup,zoom_detail,presenter}`, `composition`, `presenters/{settings,recovery_window,benchmark}`), roster and decisions D-W8B2-1..8 in `contracts/README.md`, PARITY rows, proposal P-41. All Codex drafts were rewritten (async or invented API); `contracts/drafts-w8b2` removed.
- Verified on the final head: `cargo fmt --all`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked --no-fail-fast` (results in the PR body).
- NOT run: any Rust test of the new units (all stub bodies are `todo!()`); `cargo-mutants`; the C# claims in the contracts were checked by sub-agents reading the baseline source, not by running the C# tests; CI result pending at PR time.
- Open: B1 stub changes of D-W8B2-6, duplicate concepts and image payload type (D-W8B2-4/5).

//! Slice S0 of unit W8-20b (`contracts/W8-20b.md`): the guard of the PARITY row to test map (`common/ui_parity_table.rs`). PURE: no window; once ported, none of its tests is ignored. It reads `docs/PARITY.md`
//! and the test sources of the repository through `pr_test_support::golden::repo_root()` (the one allowed way to find the repo; AGENTS.md "Tests").
//!
//! The map exists so that a `K-*`/`C-*`/`W-*` row can never again be marked `Done` with no named test, and so that a test that is renamed or deleted breaks a named row. STUB: the bodies are
//! `todo!()` and the tests carry a stub `#[ignore]` so the workspace gate stays green; the red-test step writes them and removes the ignore, and they stay RED until the ports of the units named in the
//! table have merged (that is the point of putting this slice last).
#![cfg(windows)]
#![allow(dead_code, unused_imports)] // stub

#[path = "common/ui_parity_table.rs"]
mod ui_parity_table;

use ui_parity_table::{Gate, PARITY_ROWS, ParityRow};

/// Every `K-nn`, `C-nn`, `W-nn` id found in the first column of the tables of sections 1, 2 and 4 of `docs/PARITY.md` (the lines `| K-01 | Next image...`), plus `K-43`, has exactly one row in
/// [`PARITY_ROWS`]; the message lists the missing ids. Rows of other sections (`S-*`, `F-*`, `P-*`, `I-*`, `X-*`) are ignored. Ids of the status table (top of the file) are NOT collected: the
/// inventory sections are the authority, because `C-01` also names a pixel-buffer status row there (the NO-WPF numbering collides with the inventory, D-W8B6-49).
#[test]
#[ignore = "stub: W8-20b red test not written yet"]
fn every_parity_row_of_sections_1_2_and_4_has_a_map_entry() {
    todo!("W8-20b S0: contracts/W8-20b.md")
}

/// No row of [`PARITY_ROWS`] names an id that is not in those sections (the one exception is `K-43`, which lives in the Wave 7 open items, not in section 1), and no id appears twice.
#[test]
#[ignore = "stub: W8-20b red test not written yet"]
fn no_map_entry_names_a_row_that_does_not_exist_or_repeats() {
    todo!("W8-20b S0: contracts/W8-20b.md")
}

/// Every `proofs` entry `"<crate>:<stem>"` names a file `crates/<crate>/tests/<stem>.rs` (or `.../<stem>/mod.rs`) that exists; the message lists `row id -> missing file`.
#[test]
#[ignore = "stub: W8-20b red test not written yet"]
fn every_named_proof_file_exists() {
    todo!("W8-20b S0: contracts/W8-20b.md")
}

/// Every `smoke` entry `"<stem>::<fn>"` names a `fn <fn>` that is declared in `crates/pr-shell/tests/<stem>.rs` with a `#[test]` attribute (checked by scanning the source text: the line
/// before the `fn` or the attribute block above it contains `#[test]`).
#[test]
#[ignore = "stub: W8-20b red test not written yet"]
fn every_named_smoke_test_exists() {
    todo!("W8-20b S0: contracts/W8-20b.md")
}

/// Gate consistency: every row has at least one proof; a `Gate::Pure` row has no proof file whose stem ends in `_native` or `_e2e` (a pure row must not secretly need a window); a `Gate::Native`
/// row has either a `smoke` entry or a proof file whose stem ends in `_native`, `_e2e`, or that lives in `pr-platform-win`/`pr-shell` (it is proven on a window somewhere); `Different`,
/// `Retired` and `Pending` rows carry a non-empty reason.
#[test]
#[ignore = "stub: W8-20b red test not written yet"]
fn gates_are_consistent_with_the_proofs() {
    todo!("W8-20b S0: contracts/W8-20b.md")
}

/// The count guard: `PARITY_ROWS` has 85 rows (K-01..K-43 = 43, C-01..C-28 = 28, W-01..W-14 = 14). A new inventory row added to `docs/PARITY.md` fails the first test; this one makes the
/// failure message say which number to update.
#[test]
#[ignore = "stub: W8-20b red test not written yet"]
fn the_map_has_one_row_per_inventory_row() {
    todo!("W8-20b S0: contracts/W8-20b.md")
}

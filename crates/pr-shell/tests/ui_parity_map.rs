//! Slice S0 of unit W8-20b (`contracts/W8-20b.md`): the guard of the PARITY row to test map (`common/ui_parity_table.rs`). PURE: no window; once ported, none of its tests is ignored. It reads `docs/PARITY.md`
//! and the test sources of the repository through `pr_test_support::golden::repo_root()` (the one allowed way to find the repo; AGENTS.md "Tests").
//!
//! The guards check inventory coverage and source declarations; desktop evidence remains a separate gate.
#![cfg(windows)]
use std::collections::BTreeSet;

#[path = "common/ui_parity_table.rs"]
mod ui_parity_table;

use ui_parity_table::{Gate, PARITY_ROWS};

/// Every `K-nn`, `C-nn`, `W-nn` id found in the first column of the tables of sections 1, 2 and 4 of `docs/PARITY.md` (the lines `| K-01 | Next image...`), plus `K-43`, has exactly one row in
/// [`PARITY_ROWS`]; the message lists the missing ids. Rows of other sections (`S-*`, `F-*`, `P-*`, `I-*`, `X-*`) are ignored. Ids of the status table (top of the file) are NOT collected: the
/// inventory sections are the authority, because `C-01` also names a pixel-buffer status row there (the NO-WPF numbering collides with the inventory, D-W8B6-49).
#[test]
fn every_parity_row_of_sections_1_2_and_4_has_a_map_entry() {
    let inventory = inventory_ids();
    let mapped: BTreeSet<_> = PARITY_ROWS.iter().map(|r| r.id.to_owned()).collect();
    let missing: Vec<_> = inventory.difference(&mapped).collect();
    assert!(missing.is_empty(), "inventory ids without proofs: {missing:?}");
}

/// No row of [`PARITY_ROWS`] names an id that is not in those sections (the one exception is `K-43`, which lives in the Wave 7 open items, not in section 1), and no id appears twice.
#[test]
fn no_map_entry_names_a_row_that_does_not_exist_or_repeats() {
    let inventory = inventory_ids();
    let mut seen = BTreeSet::new();
    for row in PARITY_ROWS {
        assert!(inventory.contains(row.id), "unexpected {} ({})", row.id, row.item);
        assert!(seen.insert(row.id), "duplicate {}", row.id);
    }
}

/// Every `proofs` entry `"<crate>:<stem>"` names a file `crates/<crate>/tests/<stem>.rs` (or `.../<stem>/mod.rs`) that exists; the message lists `row id -> missing file`.
#[test]
fn every_named_proof_file_exists() {
    let root = pr_test_support::golden::repo_root();
    let mut missing = Vec::new();
    for row in PARITY_ROWS {
        for proof in row.proofs {
            let (krate, stem) = proof.split_once(':').expect("crate:test proof");
            let tests = root.join("crates").join(krate).join("tests");
            if !tests.join(format!("{stem}.rs")).is_file() && !tests.join(stem).join("mod.rs").is_file() {
                missing.push(format!("{} -> {proof}", row.id));
            }
        }
    }
    assert!(missing.is_empty(), "missing proof files: {missing:#?}");
}

/// Every `smoke` entry `"<stem>::<fn>"` names a `fn <fn>` that is declared in `crates/pr-shell/tests/<stem>.rs` with a `#[test]` attribute (checked by scanning the source text: the line
/// before the `fn` or the attribute block above it contains `#[test]`).
#[test]
fn every_named_smoke_test_exists() {
    let root = pr_test_support::golden::repo_root();
    for row in PARITY_ROWS {
        for smoke in row.smoke {
            let (stem, name) = smoke.split_once("::").expect("stem::test");
            let source =
                std::fs::read_to_string(root.join("crates/pr-shell/tests").join(format!("{stem}.rs"))).unwrap();
            let mut attributes = String::new();
            let mut found = false;
            for line in source.lines().map(str::trim) {
                if line.starts_with("#[") {
                    attributes.push_str(line);
                } else if line.starts_with("fn ") {
                    if line.starts_with(&format!("fn {name}(")) && attributes.contains("#[test]") {
                        found = true;
                    }
                    attributes.clear();
                } else if !line.is_empty() && !line.starts_with("//") {
                    attributes.clear();
                }
            }
            assert!(found, "{} -> {smoke} is not a test", row.id);
        }
    }
}

/// Gate consistency: every row has at least one proof; a `Gate::Pure` row has no proof file whose stem ends in `_native` or `_e2e` (a pure row must not secretly need a window); a `Gate::Native`
/// row has either a `smoke` entry or a proof file whose stem ends in `_native`, `_e2e`, or that lives in `pr-platform-win`/`pr-shell` (it is proven on a window somewhere); `Different`,
/// `Retired` and `Pending` rows carry a non-empty reason.
#[test]
fn gates_are_consistent_with_the_proofs() {
    for row in PARITY_ROWS {
        assert!(!row.proofs.is_empty(), "{} has no proofs", row.id);
        match row.gate {
            Gate::Pure => assert!(
                row.proofs
                    .iter()
                    .all(|p| !p.ends_with("_native") && !p.ends_with("_e2e")),
                "{} pure gate has native proof",
                row.id
            ),
            Gate::Native => assert!(
                !row.smoke.is_empty()
                    || row.proofs.iter().any(|p| p.ends_with("_native")
                        || p.ends_with("_e2e")
                        || p.starts_with("pr-platform-win:")
                        || p.starts_with("pr-shell:")),
                "{} native gate has no window proof",
                row.id
            ),
            Gate::Different(reason) | Gate::Retired(reason) | Gate::Pending(reason) => {
                assert!(!reason.trim().is_empty(), "{} needs a reason", row.id)
            }
        }
    }
}

/// The count guard: `PARITY_ROWS` has 85 rows (K-01..K-43 = 43, C-01..C-28 = 28, W-01..W-14 = 14). A new inventory row added to `docs/PARITY.md` fails the first test; this one makes the
/// failure message say which number to update.
#[test]
fn the_map_has_one_row_per_inventory_row() {
    assert_eq!(PARITY_ROWS.len(), 85, "update inventory count when adding a row");
    assert_eq!(inventory_ids().len(), PARITY_ROWS.len());
}

fn inventory_ids() -> BTreeSet<String> {
    let source = std::fs::read_to_string(pr_test_support::golden::repo_root().join("docs/PARITY.md")).unwrap();
    let mut active = false;
    let mut ids = BTreeSet::new();
    for line in source.lines() {
        if line.starts_with("## ") {
            active = ["## 1.", "## 2.", "## 4."].iter().any(|p| line.starts_with(p));
        }
        if active && line.starts_with('|') {
            let id = line.split('|').nth(1).unwrap_or("").trim().trim_matches('`');
            if ["K-", "C-", "W-"].iter().any(|p| id.starts_with(p)) {
                assert!(ids.insert(id.to_owned()), "duplicate inventory id {id}");
            }
        }
    }
    ids.insert("K-43".to_owned());
    ids
}

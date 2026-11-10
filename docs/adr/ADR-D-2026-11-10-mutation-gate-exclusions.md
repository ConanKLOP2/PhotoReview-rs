# ADR-D-2026-11-10-mutation-gate-exclusions: COM glue and Recycle Bin code are outside the mutation gate; two WIC mutants stay open (P-403, P-402)

- **Status:** Accepted. Owner chose option A of `P-403` and option A of `P-402` in `docs/PROPOSED-DECISIONS.md` (2026-11-10).
- **PARITY row:** none (policy); touches the Partial rows of the WIC and Recycle Bin items, no Status cell change.
- **Decision owner:** owner

## Context
Owner rule B: only a caught or unviable mutant, or a whole-domain run proof, closes a mutant. Three groups of code cannot meet it in an unattended mutation run:
- `crates/pr-platform-win/src/wic/interop.rs` is raw COM glue; its mutants are observable only against real WIC.
- `recycle_bin*.rs` act on the shell Recycle Bin. `AGENTS.md` (Recycle Bin rule) forbids running code that deletes from, sweeps or empties the user's real Recycle Bin, including mutation checks of such code (C# incident 2026-09-24).
- `wic/cache_image.rs` 679:13 and 680:13 (delete PROPBAG2 `dwType` / `vt`) are equivalent only by a differential run against the real WIC JPEG encoder (`docs/reviews/P6-CACHE-IMAGE-MUT-2026-11-06.md`, whose own caveat says a strict reader keeps them open).

## Decision
1. **P-403:** `wic/interop.rs` and `recycle_bin*.rs` are excluded from the mutation gate. Their evidence is the native tests (`#[ignore]` with the allowed prefixes), run by hand with `tools/dev/native-tests.sh` or by `.github/workflows/native-nightly.yml`. Native Recycle Bin tests remove only their own items. Survivors in these files are not reported as open mutants; the file list and the native test run are the record.
2. **P-402:** `cache_image.rs` 679:13 and 680:13 stay open with a recorded reason: equivalent only by a differential run against real WIC, and a production change to the image encoder to remove them is not worth the risk. They are not closed and no row is promoted because of them.

## Consequences
- Rows that depend on these files cannot reach Done through mutation evidence; they need a recorded native run instead.
- A mutation run that names these files is out of policy; do not start one. Other files of the same crate stay in the gate.
- If the owner later wants these mutants closed, a differential or native proof run by hand must be written down per mutant (rule B).

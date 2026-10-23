# ADR-D-actions-unknown-operation: config.json load: an unknown action Operation drops the whole actions list (C# turns it into Move)

- **Status:** Proposed (draft written by DOC-ADR-DRAFTS, machine C, 2026-10-23; not accepted by the lead)
- **PARITY row:** `D-actions-unknown-operation` in `docs/PARITY.md`, listed in `docs/PARITY-CLOSEOUT.md` ("Different")
- **Decision owner:** project lead

## Context
C# #533 (`7eaee2a2`) makes an unknown `Operation` in a typed or imported action list an error. For `config.json` load, C# `LenientEnumConverter` still turns it into `Move` so one typo never discards the file. Rust rejects it in typed/imported lists too, but on `config.json` load drops the whole actions list and keeps the previous ones. The PARITY row says the behaviour was not changed because it needs a product decision.

## Proposed decision
Keep the Rust behaviour for now: an unknown or undefined `Operation` never becomes `Move` (a wrong guess would move files); the actions list is left as it was. Revisit if the lead wants the C# lenient load; copying it would reintroduce the silent-Move risk that #533 fixed for the other paths.

## Consequences
A config with a typo in one action loses the whole list instead of keeping the other entries as C# does. No file is ever moved by a misread operation. A second gap exists for numeric strings (C# `TryReadStrict` accepts a defined number encoded as a string; Rust's action-list reader does not), see `docs/SYNC-LEDGER.md`.

## Alternatives not chosen
Copy C#: lenient load to `Move` (parity of the config file, silent wrong operation). Keep the other actions and drop only the bad entry (neither C# nor current Rust; would need its own golden).

## Sources
`docs/PARITY.md` row `D-actions-unknown-operation`; `docs/SYNC-LEDGER.md` (#533 row and the oracle-diff section); tests `settings_field_map::*`, `action_list_json::deserialize_undefined_operation_number_is_malformed`, `deserialize_unknown_operation_name_is_malformed`.

## Not verified
The C# `LenientEnumConverter` code at `7eaee2a2` was not re-read for this draft (the row and the ledger are the sources).

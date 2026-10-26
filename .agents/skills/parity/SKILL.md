---
name: parity
description: Use when the owner asks about the progress of the Rust port, the status of a PARITY.md row or unit, or which PARITY rows are stale.
---

Run the matching command from the repo root and show the result:

- Overall progress: `bash tools/dev/parity.sh stats`
- One unit (for example W8-18o): `bash tools/dev/parity.sh unit W8-18o`
- Stale rows: `bash tools/dev/parity.sh reconcile` (it only prints proposals)
- Status edit: `bash tools/dev/parity.sh set <ROW-ID> <status> "<evidence>"` is a dry-run; run it with `--apply` only after the owner agrees.

Report in Vietnamese: the counts per status, the rows and commits found, and for reconcile the rows that look stale.
Docs-only commits also count as merged in reconcile, so say that the proposals need a check against the proving test.
Never edit docs/PARITY.md by hand when `set` can do it.

Run every command from the repository root.

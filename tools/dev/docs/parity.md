# parity.sh

Reads `docs/PARITY.md` as data (tables with a `Status` header column; headerless rows after a bullet reuse column 2).
Recognized statuses: Done, Ported, Superseded, Partial, Todo, Different, Contract, Doing, Implementation, Retired, Context.

```bash
bash tools/dev/parity.sh stats                       # rows per status, percentages, unrecognized rows
bash tools/dev/parity.sh unit W8-18o                 # rows citing the unit + git log subjects (ranges expanded)
bash tools/dev/parity.sh reconcile                   # open rows whose cited units are all merged (print only)
bash tools/dev/parity.sh set X-row Done "PR #231"    # dry-run; add --apply to write
```

Env: `PY_BIN`, `GIT_BIN`, `PARITY_DOC` (file), `PARITY_REF` (default `origin/master`), `CODEX_LIB` (path of codex-lib.py).
Unit ids are expanded with `expand_unit_ids` of `tools/autopilot/lib/codex-lib.py` (runpy), with a built-in fallback.

Safety: only `set --apply` writes, and only the status cell of one row (new status word, old detail kept, `; evidence` appended).
Unknown status, unknown or ambiguous row, and evidence containing `|` are refused. Docs commits also count as merged in `reconcile`: verify.
Selftest: `bash tools/dev/test/parity-selftest.sh`.
